mod pty;
mod detect;

use pty::PtyManager;
use std::sync::Mutex;
use tauri::{Manager, State};
use tauri_plugin_shell::process::CommandChild;
use tauri_plugin_shell::ShellExt;

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
            pty_spawn, pty_write, pty_resize, pty_kill,
            detect::detect_tmux,
            detect::detect_tailscale,
            detect::detect_daemon,
            detect::detect_platform,
            detect::detect_package_manager,
            detect::detect_tailscale_ip,
        ])
        .setup(|app| {
            // Skip sidecar in dev mode or if explicitly disabled
            if cfg!(debug_assertions) || std::env::var("GHOST_NO_SIDECAR").is_ok() {
                eprintln!("[daemon] sidecar disabled (dev mode or GHOST_NO_SIDECAR set)");
                return Ok(());
            }

            // Try to spawn, but don't fail if it doesn't work
            match app.shell().sidecar("binaries/ghost-protocol-daemon") {
                Ok(sidecar) => {
                    match sidecar.spawn() {
                        Ok((mut rx, child)) => {
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
                        Err(e) => eprintln!("[daemon] failed to spawn sidecar: {e}"),
                    }
                }
                Err(e) => eprintln!("[daemon] sidecar not available: {e}"),
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
