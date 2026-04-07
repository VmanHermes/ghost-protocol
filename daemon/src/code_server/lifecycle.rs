use std::fs;
use std::net::TcpListener;
use std::process::{Child, Stdio};

use chrono::Utc;

use crate::code_server::CodeServerInfo;
use crate::store::Store;
use crate::store::sessions::{CreateWorkSessionParams, TerminalSessionRecord};
use crate::supervisor::{self, DRIVER_CODE_SERVER};

// ─── helpers ────────────────────────────────────────────────────────────────

/// Returns the last path component (filename/dirname) of a path string.
pub fn short_path(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
}

/// Extracts the port number from a `--bind-addr` argument list.
/// Handles both `--bind-addr host:port` (two separate args) and
/// `--bind-addr=host:port` (single arg with `=`).
pub fn extract_port(args: &[&str]) -> Option<u16> {
    extract_bind_addr(args).map(|(_, port)| port)
}

fn extract_bind_host(args: &[&str]) -> Option<String> {
    extract_bind_addr(args).map(|(host, _)| host)
}

fn extract_bind_addr(args: &[&str]) -> Option<(String, u16)> {
    let mut iter = args.iter().peekable();
    while let Some(&arg) = iter.next() {
        if arg == "--bind-addr" {
            if let Some(&next) = iter.peek() {
                return parse_bind_addr(next);
            }
        } else if let Some(rest) = arg.strip_prefix("--bind-addr=") {
            return parse_bind_addr(rest);
        }
    }
    None
}

fn parse_bind_addr(s: &str) -> Option<(String, u16)> {
    if let Some(rest) = s.strip_prefix('[') {
        let end = rest.find(']')?;
        let host = &rest[..end];
        let port = rest[end + 1..].strip_prefix(':')?.parse::<u16>().ok()?;
        return Some((host.to_string(), port));
    }

    let idx = s.rfind(':')?;
    let host = &s[..idx];
    let port = s[idx + 1..].parse::<u16>().ok()?;
    Some((host.to_string(), port))
}

fn is_localhost_bind_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

fn is_launcher_entry(args: &[&str], index: usize, arg: &str) -> bool {
    if index == 0 {
        return true;
    }

    if index == 1 {
        let launcher = args.first().copied().unwrap_or_default();
        let launcher_name = launcher.rsplit('/').next().unwrap_or(launcher);
        let looks_like_node = launcher_name == "node" || launcher_name == "nodejs";
        if looks_like_node && arg.contains("code-server") {
            return true;
        }
    }

    false
}

/// Extracts the working directory from an argument list.
/// Prefers `--user-data-dir <path>` / `--user-data-dir=<path>`,
/// then falls back to the last positional arg starting with `/`.
pub fn extract_workdir(args: &[&str]) -> Option<String> {
    let mut iter = args.iter().peekable();
    while let Some(&arg) = iter.next() {
        if arg == "--user-data-dir" {
            if let Some(&&next) = iter.peek() {
                return Some(next.to_string());
            }
        } else if let Some(rest) = arg.strip_prefix("--user-data-dir=") {
            return Some(rest.to_string());
        }
    }

    // Positional: last non-flag arg starting with '/'
    let mut workdir: Option<String> = None;
    let mut skip_next = false;
    for (index, &arg) in args.iter().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }
        if is_launcher_entry(args, index, arg) {
            continue;
        }
        // flags that consume their following arg
        if arg == "--bind-addr"
            || arg == "--user-data-dir"
            || arg == "--auth"
            || arg == "--cert"
            || arg == "--extensions-dir"
            || arg == "--config"
        {
            skip_next = true;
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        if arg.starts_with('/') && !arg.is_empty() {
            workdir = Some(arg.to_string());
        }
    }
    workdir
}

// ─── public API ─────────────────────────────────────────────────────────────

/// Scans `/proc` for running code-server processes and returns info about each.
pub fn scan_running_code_servers() -> Vec<CodeServerInfo> {
    let mut result = Vec::new();

    let proc_dir = match fs::read_dir("/proc") {
        Ok(d) => d,
        Err(_) => return result,
    };

    for entry in proc_dir.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Only numeric PID directories
        let pid: u32 = match name_str.parse() {
            Ok(p) => p,
            Err(_) => continue,
        };

        let cmdline_path = format!("/proc/{}/cmdline", pid);
        let raw = match fs::read(&cmdline_path) {
            Ok(b) => b,
            Err(_) => continue,
        };

        if raw.is_empty() {
            continue;
        }

        // Split on null bytes
        let parts: Vec<&str> = raw
            .split(|&b| b == 0)
            .filter_map(|s| std::str::from_utf8(s).ok())
            .collect();

        // Must contain "code-server" but not "grep"
        let has_code_server = parts.iter().any(|s| s.contains("code-server"));
        let is_grep = parts.iter().any(|s| s.contains("grep"));
        if !has_code_server || is_grep {
            continue;
        }

        let args: Vec<&str> = parts.iter().map(|s| s.as_ref()).collect();
        let bind_host = extract_bind_host(&args);

        if bind_host
            .as_deref()
            .map(is_localhost_bind_host)
            .unwrap_or(true)
        {
            continue;
        }

        let port = extract_port(&args).unwrap_or(8080);

        let workdir = match extract_workdir(&args) {
            Some(w) => w,
            None => continue,
        };

        result.push(CodeServerInfo { pid, port, workdir });
    }

    result
}

/// Tries ports 8400–8499 and returns the first one that is free to bind.
pub fn find_available_port() -> Option<u16> {
    for port in 8400u16..=8499 {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Some(port);
        }
    }
    None
}

/// Spawns a new code-server process and registers it in the store.
pub fn spawn_code_server(
    store: &Store,
    workdir: &str,
    project_id: Option<&str>,
    host_ip: &str,
) -> Result<(TerminalSessionRecord, Child), String> {
    let workdir = crate::workdir::expand_workdir(workdir);
    let port = find_available_port().ok_or("no available port in range 8400-8499")?;

    let id = uuid::Uuid::new_v4().to_string();
    let capabilities = supervisor::driver_capabilities(DRIVER_CODE_SERVER, false, false);
    let bind_addr = format!("0.0.0.0:{}", port);
    let command = vec![
        "code-server".to_string(),
        "--bind-addr".to_string(),
        bind_addr.clone(),
        "--auth".to_string(),
        "none".to_string(),
        workdir.clone(),
    ];
    let url = format!("http://{}:{}/?folder={}", host_ip, port, workdir);
    let name = short_path(&workdir);

    let record = store
        .create_work_session(CreateWorkSessionParams {
            id: &id,
            mode: "project",
            name: Some(name),
            workdir: &workdir,
            command: &command,
            session_type: "code_server",
            project_id,
            parent_session_id: None,
            root_session_id: None,
            host_id: None,
            host_name: None,
            agent_id: None,
            driver_kind: DRIVER_CODE_SERVER,
            capabilities: &capabilities,
            port: Some(port as i64),
            url: Some(&url),
            adopted: false,
        })
        .map_err(|e| format!("failed to create session record: {e}"))?;

    let child = std::process::Command::new("code-server")
        .arg("--bind-addr")
        .arg(&bind_addr)
        .arg("--auth")
        .arg("none")
        .arg(&workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to spawn code-server: {e}"))?;

    // Update the record with the PID and running status
    let pid = child.id() as i64;
    store
        .update_code_server_session(&id, "running", Some(pid), Some(&url))
        .map_err(|e| format!("failed to update session with PID: {e}"))?;

    Ok((record, child))
}

/// Adopts an already-running code-server process into the store.
pub fn adopt_code_server(
    store: &Store,
    info: &CodeServerInfo,
    host_ip: &str,
) -> Result<TerminalSessionRecord, String> {
    // Verify the process is still alive
    let proc_path = format!("/proc/{}", info.pid);
    if !std::path::Path::new(&proc_path).exists() {
        return Err(format!("process {} no longer exists", info.pid));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let capabilities = supervisor::driver_capabilities(DRIVER_CODE_SERVER, false, false);
    let url = format!("http://{}:{}/?folder={}", host_ip, info.port, info.workdir);
    let name = short_path(&info.workdir);
    let command = vec![
        "code-server".to_string(),
        format!("--bind-addr=0.0.0.0:{}", info.port),
        "--auth".to_string(),
        "none".to_string(),
        info.workdir.clone(),
    ];

    let record = store
        .create_work_session(CreateWorkSessionParams {
            id: &id,
            mode: "project",
            name: Some(name),
            workdir: &info.workdir,
            command: &command,
            session_type: "code_server",
            project_id: None,
            parent_session_id: None,
            root_session_id: None,
            host_id: None,
            host_name: None,
            agent_id: None,
            driver_kind: DRIVER_CODE_SERVER,
            capabilities: &capabilities,
            port: Some(info.port as i64),
            url: Some(&url),
            adopted: true,
        })
        .map_err(|e| format!("failed to create adopted session record: {e}"))?;

    store
        .update_code_server_session(&id, "running", Some(info.pid as i64), Some(&url))
        .map_err(|e| format!("failed to update adopted session: {e}"))?;

    Ok(record)
}

/// Terminates a code-server session by PID and marks it terminated in the store.
pub fn terminate_code_server(store: &Store, session_id: &str) -> Result<(), String> {
    let session = store
        .get_terminal_session(session_id)
        .map_err(|e| format!("store error: {e}"))?
        .ok_or_else(|| format!("session {} not found", session_id))?;

    if session.session_type != "code_server" {
        return Err(format!(
            "session {} is not a code_server session (type: {})",
            session_id, session.session_type
        ));
    }

    if let Some(pid) = session.pid {
        if pid > 0 {
            // SAFETY: pid is a positive integer from the database; SIGTERM is safe.
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGTERM);
            }
        }
    }

    let now = Utc::now().to_rfc3339();
    let conn = store.conn();
    conn.execute(
        "UPDATE terminal_sessions SET status = 'terminated', finished_at = ?1 WHERE id = ?2",
        rusqlite::params![now, session_id],
    )
    .map_err(|e| format!("failed to update session status: {e}"))?;

    Ok(())
}

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_port_flag() {
        let args = vec![
            "node",
            "/usr/bin/code-server",
            "--bind-addr",
            "0.0.0.0:8443",
            "/home/user",
        ];
        assert_eq!(extract_port(&args), Some(8443));
    }

    #[test]
    fn test_extract_port_equals() {
        let args = vec!["code-server", "--bind-addr=127.0.0.1:9000"];
        assert_eq!(extract_port(&args), Some(9000));
    }

    #[test]
    fn test_extract_port_missing() {
        let args = vec!["code-server", "/home/user"];
        assert_eq!(extract_port(&args), None);
    }

    #[test]
    fn test_extract_workdir_positional() {
        let args = vec![
            "node",
            "/usr/bin/code-server",
            "--bind-addr",
            "0.0.0.0:8443",
            "/home/user/projects/foo",
            "",
        ];
        assert_eq!(
            extract_workdir(&args),
            Some("/home/user/projects/foo".to_string())
        );
    }

    #[test]
    fn test_extract_workdir_user_data_dir() {
        let args = vec!["code-server", "--user-data-dir", "/home/user/mydir"];
        assert_eq!(extract_workdir(&args), Some("/home/user/mydir".to_string()));
    }

    #[test]
    fn test_extract_workdir_none() {
        let args = vec!["code-server", "--auth", "none", ""];
        assert_eq!(extract_workdir(&args), None);
    }

    #[test]
    fn test_extract_workdir_ignores_launcher_entry() {
        let args = vec![
            "node",
            "/usr/lib/code-server/out/node/entry",
            "--bind-addr",
            "0.0.0.0:8080",
            "--auth",
            "none",
        ];
        assert_eq!(extract_workdir(&args), None);
    }

    #[test]
    fn test_extract_bind_host_flag() {
        let args = vec!["code-server", "--bind-addr", "0.0.0.0:8443", "/home/user"];
        assert_eq!(extract_bind_host(&args).as_deref(), Some("0.0.0.0"));
    }

    #[test]
    fn test_extract_bind_host_equals() {
        let args = vec!["code-server", "--bind-addr=127.0.0.1:9000"];
        assert_eq!(extract_bind_host(&args).as_deref(), Some("127.0.0.1"));
    }

    #[test]
    fn test_find_available_port() {
        let port = find_available_port();
        assert!(port.is_some());
        let p = port.unwrap();
        assert!(p >= 8400 && p <= 8499);
    }
}
