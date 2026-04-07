use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

#[derive(Clone, Serialize)]
pub struct PtyChunk {
    pub session_id: String,
    pub data: String,
}

#[derive(Clone, Serialize)]
pub struct PtyStatus {
    pub session_id: String,
    pub status: String,
    pub exit_code: Option<i32>,
}

pub struct PtySession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
}

pub struct PtyManager {
    sessions: Arc<Mutex<HashMap<String, PtySession>>>,
}

fn default_home_dir() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string())
}

fn expand_workdir(input: &str) -> String {
    let trimmed = input.trim();

    if trimmed.is_empty() || trimmed == "~" {
        return default_home_dir();
    }

    if let Some(rest) = trimmed.strip_prefix("~/") {
        return PathBuf::from(default_home_dir())
            .join(rest)
            .to_string_lossy()
            .into_owned();
    }

    trimmed.to_string()
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn spawn(
        &self,
        app: AppHandle,
        cols: u16,
        rows: u16,
        workdir: Option<String>,
    ) -> Result<String, String> {
        let pty_system = native_pty_system();

        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pair = pty_system
            .openpty(size)
            .map_err(|e| format!("Failed to open PTY: {e}"))?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let mut cmd = CommandBuilder::new(&shell);
        cmd.arg("-l");
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("GHOST_PROTOCOL_LOCAL", "1");

        if let Some(ref dir) = workdir {
            cmd.cwd(expand_workdir(dir));
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("Failed to spawn shell: {e}"))?;

        // Drop slave — we only need the master side
        drop(pair.slave);

        let session_id = Uuid::new_v4().to_string();
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("Failed to take writer: {e}"))?;

        let child = Arc::new(Mutex::new(child));

        // Reader thread
        let reader_session_id = session_id.clone();
        let reader_app = app.clone();
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("Failed to clone reader: {e}"))?;

        std::thread::spawn(move || {
            // Brief pause to let the frontend attach its event listener.
            // Without this, the welcome text and early shell output are lost
            // because Tauri's listen() is async and resolves after pty_spawn returns.
            std::thread::sleep(std::time::Duration::from_millis(100));

            // Emit welcome text as the very first chunk
            let hostname = std::process::Command::new("hostname")
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let welcome = format!(
                "\x1b[2mGhost Protocol — {hostname}\r\n\r\n\
Commands:\r\n\
  ghost init          Set up a project in this directory\r\n\
  ghost status        Mesh overview (machines, sessions)\r\n\
  ghost agents        Available agents across the mesh\r\n\
  ghost chat <agent>  Start a chat with an agent\r\n\
  ghost help          Full command reference\r\n\
\x1b[0m\r\n"
            );
            let _ = reader_app.emit(
                "pty:chunk",
                PtyChunk {
                    session_id: reader_session_id.clone(),
                    data: welcome,
                },
            );

            // Now read shell output normally
            let mut buf = [0u8; 16384];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = String::from_utf8_lossy(&buf[..n]).to_string();
                        let _ = reader_app.emit(
                            "pty:chunk",
                            PtyChunk {
                                session_id: reader_session_id.clone(),
                                data,
                            },
                        );
                    }
                    Err(_) => break,
                }
            }
        });

        // Waiter thread
        let waiter_session_id = session_id.clone();
        let waiter_child = Arc::clone(&child);
        let waiter_sessions = Arc::clone(&self.sessions);
        let waiter_app = app;

        std::thread::spawn(move || {
            loop {
                let status = {
                    let mut child_lock = waiter_child.lock().unwrap();
                    child_lock.try_wait()
                };
                match status {
                    Ok(Some(exit_status)) => {
                        let code = i32::try_from(exit_status.exit_code()).unwrap_or(-1);
                        let _ = waiter_app.emit(
                            "pty:status",
                            PtyStatus {
                                session_id: waiter_session_id.clone(),
                                status: "exited".to_string(),
                                exit_code: Some(code),
                            },
                        );
                        waiter_sessions.lock().unwrap().remove(&waiter_session_id);
                        break;
                    }
                    Ok(None) => {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    Err(_) => {
                        let _ = waiter_app.emit(
                            "pty:status",
                            PtyStatus {
                                session_id: waiter_session_id.clone(),
                                status: "error".to_string(),
                                exit_code: None,
                            },
                        );
                        waiter_sessions.lock().unwrap().remove(&waiter_session_id);
                        break;
                    }
                }
            }
        });

        let session = PtySession {
            master: pair.master,
            writer,
            child,
        };

        self.sessions
            .lock()
            .unwrap()
            .insert(session_id.clone(), session);

        Ok(session_id)
    }

    pub fn write_input(&self, session_id: &str, data: &[u8]) -> Result<(), String> {
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| format!("Session not found: {session_id}"))?;
        session
            .writer
            .write_all(data)
            .map_err(|e| format!("Write failed: {e}"))?;
        session
            .writer
            .flush()
            .map_err(|e| format!("Flush failed: {e}"))?;
        Ok(())
    }

    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let sessions = self.sessions.lock().unwrap();
        let session = sessions
            .get(session_id)
            .ok_or_else(|| format!("Session not found: {session_id}"))?;
        session
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("Resize failed: {e}"))
    }

    pub fn kill(&self, session_id: &str) -> Result<(), String> {
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions
            .remove(session_id)
            .ok_or_else(|| format!("Session not found: {session_id}"))?;
        let mut child = session.child.lock().unwrap();
        child.kill().map_err(|e| format!("Kill failed: {e}"))
    }
}
