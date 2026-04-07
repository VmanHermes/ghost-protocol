mod detect;
mod pty;

use pty::PtyManager;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{Manager, State};
use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::Command;
use tauri_plugin_shell::process::CommandChild;

const DEV_DAEMON_DB_RELATIVE_PATH: &str = "data/dev/ghost_protocol-dev.db";
const RELEASE_DAEMON_DB_FILENAME: &str = "ghost_protocol.db";

#[tauri::command]
fn pty_spawn(
    app: tauri::AppHandle,
    state: State<'_, PtyManager>,
    cols: u16,
    rows: u16,
    workdir: Option<String>,
) -> Result<String, String> {
    state.spawn(app, cols, rows, workdir)
}

#[tauri::command]
fn pty_write(state: State<'_, PtyManager>, session_id: String, data: String) -> Result<(), String> {
    state.write_input(&session_id, data.as_bytes())
}

#[tauri::command]
fn pty_resize(
    state: State<'_, PtyManager>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    state.resize(&session_id, cols, rows)
}

#[tauri::command]
fn pty_kill(state: State<'_, PtyManager>, session_id: String) -> Result<(), String> {
    state.kill(&session_id)
}

fn resolve_daemon_bind_hosts(
    configured_bind_hosts: Option<&str>,
    detected_tailscale_ip: Option<&str>,
) -> Option<String> {
    if configured_bind_hosts
        .map(str::trim)
        .map(|value| !value.is_empty())
        .unwrap_or(false)
    {
        return None;
    }

    detected_tailscale_ip
        .and_then(|value| value.lines().map(str::trim).find(|value| !value.is_empty()))
        .map(|ip| format!("{ip},127.0.0.1"))
}

fn default_daemon_bind_hosts() -> Option<String> {
    let configured_bind_hosts = std::env::var("GHOST_PROTOCOL_BIND_HOST").ok();
    let detected_tailscale_ip = detect::detect_tailscale_ip().ok();

    resolve_daemon_bind_hosts(
        configured_bind_hosts.as_deref(),
        detected_tailscale_ip.as_deref(),
    )
}

fn configured_daemon_db_path(configured_db_path: Option<&str>) -> Option<PathBuf> {
    configured_db_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn default_dev_daemon_db_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(DEV_DAEMON_DB_RELATIVE_PATH)
}

fn default_release_daemon_db_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join(RELEASE_DAEMON_DB_FILENAME))
}

fn resolve_daemon_db_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    configured_daemon_db_path(std::env::var("GHOST_PROTOCOL_DB").ok().as_deref()).or_else(|| {
        if cfg!(debug_assertions) {
            Some(default_dev_daemon_db_path())
        } else {
            default_release_daemon_db_path(app)
        }
    })
}

fn apply_daemon_startup_args(
    mut command: Command,
    bind_hosts: Option<&str>,
    db_path: Option<&Path>,
) -> Command {
    if let Some(bind_hosts) = bind_hosts {
        command = command.args(["--bind-host", bind_hosts]);
    }
    if let Some(db_path) = db_path {
        command = command.arg("--db-path").arg(db_path.as_os_str());
    }
    command
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Disable DMA-BUF renderer on Linux to avoid Wayland/WebKit crashes
    // on certain GPU/driver combinations (e.g. NVIDIA, older Intel).
    #[cfg(target_os = "linux")]
    if std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").is_err() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .manage(PtyManager::new())
        .invoke_handler(tauri::generate_handler![
            pty_spawn,
            pty_write,
            pty_resize,
            pty_kill,
            detect::detect_tmux,
            detect::detect_tailscale,
            detect::detect_daemon,
            detect::detect_platform,
            detect::detect_package_manager,
            detect::detect_tailscale_ip,
        ])
        .setup(|app| {
            // In dev builds, append "- Dev" to the window title
            if cfg!(debug_assertions) {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.set_title("Ghost Protocol - Dev");
                }
            }

            // Skip sidecar if explicitly disabled
            if std::env::var("GHOST_NO_SIDECAR").is_ok() {
                eprintln!("[daemon] sidecar disabled (GHOST_NO_SIDECAR set)");
                return Ok(());
            }

            let app_handle = app.handle().clone();
            let bind_hosts = default_daemon_bind_hosts();
            let db_path = resolve_daemon_db_path(&app_handle);
            if let Some(bind_hosts) = bind_hosts.as_deref() {
                eprintln!("[daemon] binding daemon to {bind_hosts}");
            }
            if let Some(db_path) = db_path.as_deref() {
                eprintln!("[daemon] using db path {}", db_path.display());
            }

            // Try bundled sidecar first, then fall back to system-installed daemon
            let spawned = match app.shell().sidecar("binaries/ghost-protocol-daemon") {
                Ok(sidecar) => match apply_daemon_startup_args(
                    sidecar,
                    bind_hosts.as_deref(),
                    db_path.as_deref(),
                )
                .spawn()
                {
                    Ok(result) => {
                        eprintln!("[daemon] started bundled sidecar");
                        Some(result)
                    }
                    Err(e) => {
                        eprintln!("[daemon] bundled sidecar failed: {e}, trying system PATH...");
                        None
                    }
                },
                Err(e) => {
                    eprintln!("[daemon] sidecar not available: {e}, trying system PATH...");
                    None
                }
            };

            // Fall back to system-installed ghost-protocol-daemon
            let spawned = spawned.or_else(|| {
                match apply_daemon_startup_args(
                    app.shell().command("ghost-protocol-daemon"),
                    bind_hosts.as_deref(),
                    db_path.as_deref(),
                )
                .spawn()
                {
                    Ok(result) => {
                        eprintln!("[daemon] started system-installed daemon from PATH");
                        Some(result)
                    }
                    Err(e) => {
                        eprintln!("[daemon] system daemon also failed: {e}");
                        None
                    }
                }
            });

            if let Some((mut rx, child)) = spawned {
                app.manage(Mutex::new(Some(child)));

                // Log daemon output in background
                tauri::async_runtime::spawn(async move {
                    use tauri_plugin_shell::process::CommandEvent;
                    while let Some(event) = rx.recv().await {
                        match event {
                            CommandEvent::Stdout(line) => {
                                let line = String::from_utf8_lossy(&line);
                                eprintln!("[daemon stdout] {}", line);
                            }
                            CommandEvent::Stderr(line) => {
                                let line = String::from_utf8_lossy(&line);
                                eprintln!("[daemon stderr] {}", line);
                            }
                            CommandEvent::Terminated(status) => {
                                eprintln!("[daemon] exited: {:?}", status);
                                break;
                            }
                            _ => {}
                        }
                    }
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Kill the daemon when the last window is destroyed
            if let tauri::WindowEvent::Destroyed = event {
                let app = window.app_handle();
                if let Some(child_state) = app.try_state::<Mutex<Option<CommandChild>>>() {
                    if let Ok(mut guard) = child_state.lock() {
                        if let Some(child) = guard.take() {
                            let _ = child.kill();
                            eprintln!("[daemon] killed daemon sidecar on window close");
                        }
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::{
        DEV_DAEMON_DB_RELATIVE_PATH, configured_daemon_db_path, default_dev_daemon_db_path,
        resolve_daemon_bind_hosts,
    };
    use std::path::{Path, PathBuf};

    #[test]
    fn respects_explicit_bind_host_override() {
        let bind_hosts = resolve_daemon_bind_hosts(Some("127.0.0.1"), Some("100.64.0.10"));
        assert_eq!(bind_hosts, None);
    }

    #[test]
    fn uses_tailscale_ip_when_no_override_is_configured() {
        let bind_hosts = resolve_daemon_bind_hosts(None, Some("100.64.0.10"));
        assert_eq!(bind_hosts.as_deref(), Some("100.64.0.10,127.0.0.1"));
    }

    #[test]
    fn ignores_blank_values_when_deriving_bind_hosts() {
        let bind_hosts = resolve_daemon_bind_hosts(Some("   "), Some("  \n100.64.0.10\n"));
        assert_eq!(bind_hosts.as_deref(), Some("100.64.0.10,127.0.0.1"));
    }

    #[test]
    fn returns_none_without_override_or_tailscale_ip() {
        let bind_hosts = resolve_daemon_bind_hosts(None, None);
        assert_eq!(bind_hosts, None);
    }

    #[test]
    fn trims_explicit_db_path_override() {
        let db_path = configured_daemon_db_path(Some(" /tmp/ghost.db "));
        assert_eq!(db_path, Some(PathBuf::from("/tmp/ghost.db")));
    }

    #[test]
    fn ignores_blank_db_path_override() {
        let db_path = configured_daemon_db_path(Some("   "));
        assert_eq!(db_path, None);
    }

    #[test]
    fn dev_db_path_points_to_repo_dev_data_dir() {
        assert!(default_dev_daemon_db_path().ends_with(Path::new(DEV_DAEMON_DB_RELATIVE_PATH)));
    }
}
