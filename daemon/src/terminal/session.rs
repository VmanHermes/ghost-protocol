use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::process::Stdio;
use std::sync::Arc;

use tokio::process::Child;
use tracing::{debug, error, warn};

use crate::store::Store;
use crate::terminal::broadcaster::SessionBroadcaster;
use crate::terminal::tmux;

const READ_BUF_SIZE: usize = 16 * 1024;

pub struct ManagedSession {
    pub session_id: String,
    pub master_fd: OwnedFd,
    pub attach_process: Child,
    pub broadcaster: Arc<SessionBroadcaster>,
}

/// Sets the terminal window size on a file descriptor via ioctl.
fn set_window_size(fd: i32, cols: u16, rows: u16) {
    let ws = nix::pty::Winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    unsafe {
        libc::ioctl(fd, libc::TIOCSWINSZ, &ws as *const _);
    }
}

/// Spawns a `tmux attach-session` process connected via a PTY pair.
///
/// A reader thread continuously reads output from the master PTY fd and forwards
/// it through the store (SQLite) and broadcaster (broadcast channel) for real-time
/// streaming to connected clients.
pub fn spawn_attach(
    session_id: String,
    store: Store,
    broadcaster: Arc<SessionBroadcaster>,
) -> Result<ManagedSession, String> {
    // 1. Open PTY pair
    let pty = nix::pty::openpty(None, None)
        .map_err(|e| format!("openpty failed: {e}"))?;

    // 2. Set initial window size (120x32)
    set_window_size(pty.master.as_raw_fd(), 120, 32);

    // 3. Get attach command
    let cmd = tmux::attach_command(&session_id);
    debug!(session_id, ?cmd, "spawning tmux attach");

    // 4. Spawn attach process with slave fd for stdin/stdout/stderr
    //    We dup the slave fd for each stdio handle so each Stdio owns its own fd.
    //    The original pty.slave OwnedFd is dropped after spawning.
    let slave_raw = pty.slave.as_raw_fd();
    let child = {
        // SAFETY: slave_raw is a valid fd from openpty.
        let stdin_fd = unsafe { libc::dup(slave_raw) };
        if stdin_fd < 0 { return Err("dup slave fd for stdin failed".into()); }
        let stdout_fd = unsafe { libc::dup(slave_raw) };
        if stdout_fd < 0 { return Err("dup slave fd for stdout failed".into()); }
        let stderr_fd = unsafe { libc::dup(slave_raw) };
        if stderr_fd < 0 { return Err("dup slave fd for stderr failed".into()); }

        // SAFETY: each fd was just created by dup and is valid.
        let stdin = unsafe { Stdio::from_raw_fd(stdin_fd) };
        let stdout = unsafe { Stdio::from_raw_fd(stdout_fd) };
        let stderr = unsafe { Stdio::from_raw_fd(stderr_fd) };

        let mut command = tokio::process::Command::new(&cmd[0]);
        command
            .args(&cmd[1..])
            .stdin(stdin)
            .stdout(stdout)
            .stderr(stderr)
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .env("GHOST_PROTOCOL_REMOTE_SESSION", "1")
            .process_group(0)
            .kill_on_drop(true);

        command
            .spawn()
            .map_err(|e| format!("failed to spawn tmux attach: {e}"))?
    };

    // 5. Drop slave fd — only master is needed from this point.
    //    The duped fds are now owned by the child process's Stdio handles.
    drop(pty.slave);

    // 6. Spawn reader thread: reads from master fd, sends text over channel
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let read_fd = pty.master.as_raw_fd();

    std::thread::Builder::new()
        .name(format!("pty-reader-{}", &session_id[..8.min(session_id.len())]))
        .spawn(move || {
            let mut buf = [0u8; READ_BUF_SIZE];
            loop {
                match nix::unistd::read(read_fd, &mut buf) {
                    Ok(0) => {
                        debug!("pty reader: EOF");
                        break;
                    }
                    Ok(n) => {
                        let text = String::from_utf8_lossy(&buf[..n]).into_owned();
                        if tx.send(text).is_err() {
                            debug!("pty reader: channel closed");
                            break;
                        }
                    }
                    Err(e) => {
                        // EIO is expected when the slave side closes
                        if e != nix::errno::Errno::EIO {
                            warn!(error = %e, "pty reader: read error");
                        }
                        break;
                    }
                }
            }
            debug!("pty reader thread exiting");
        })
        .map_err(|e| format!("failed to spawn reader thread: {e}"))?;

    // 7. Spawn tokio task: receives strings, persists to DB, broadcasts
    let bc = Arc::clone(&broadcaster);
    let sid = session_id.clone();
    tokio::spawn(async move {
        while let Some(text) = rx.recv().await {
            match store.append_terminal_chunk(&sid, "stdout", &text) {
                Ok(chunk_record) => {
                    bc.send(chunk_record);
                }
                Err(e) => {
                    error!(session_id = %sid, error = %e, "failed to persist terminal chunk");
                }
            }
        }
        debug!(session_id = %sid, "terminal output forwarding task exiting");
    });

    Ok(ManagedSession {
        session_id,
        master_fd: pty.master,
        attach_process: child,
        broadcaster,
    })
}

impl ManagedSession {
    /// Writes raw input bytes to the terminal (master PTY fd).
    pub fn write_input(&self, data: &[u8]) -> Result<(), String> {
        nix::unistd::write(&self.master_fd, data)
            .map_err(|e| format!("write to pty failed: {e}"))?;
        Ok(())
    }

    /// Resizes the terminal to the given dimensions.
    pub fn resize(&self, cols: u16, rows: u16) {
        set_window_size(self.master_fd.as_raw_fd(), cols, rows);
    }

    /// Sends Ctrl+C (interrupt signal) to the terminal.
    pub fn interrupt(&self) -> Result<(), String> {
        self.write_input(&[0x03])
    }
}
