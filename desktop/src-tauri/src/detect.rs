use std::path::PathBuf;
use std::process::Command;

/// Returns the install path for the Rust daemon binary.
/// ~/.local/share/ghost-protocol/ghost-protocol-daemon
fn daemon_bin_path() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".local/share")
        });
    base.join("ghost-protocol").join("ghost-protocol-daemon")
}

/// Compare "major.minor" strings. Returns true if actual >= minimum.
fn version_gte(actual: &str, minimum: &str) -> bool {
    let parse = |s: &str| -> (u32, u32) {
        let parts: Vec<&str> = s.split('.').collect();
        let major = parts.first().and_then(|p| p.parse().ok()).unwrap_or(0);
        let minor = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(0);
        (major, minor)
    };
    let (a_maj, a_min) = parse(actual);
    let (m_maj, m_min) = parse(minimum);
    (a_maj, a_min) >= (m_maj, m_min)
}

#[tauri::command]
pub fn detect_platform() -> String {
    std::env::consts::OS.to_string()
}

#[tauri::command]
pub fn detect_package_manager() -> Result<String, String> {
    for name in ["apt", "dnf", "pacman", "brew"] {
        let result = Command::new("which").arg(name).output();
        if let Ok(output) = result {
            if output.status.success() {
                return Ok(name.to_string());
            }
        }
    }
    Err("unknown".to_string())
}

#[tauri::command]
pub fn detect_tmux() -> Result<String, String> {
    let output = Command::new("tmux")
        .arg("-V")
        .output()
        .map_err(|_| "not_found".to_string())?;
    if !output.status.success() {
        return Err("not_found".to_string());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout.trim().strip_prefix("tmux ").unwrap_or(stdout.trim());
    if version_gte(version, "3.0") {
        Ok(version.to_string())
    } else {
        Err(format!("version_too_old:{}", version))
    }
}

#[tauri::command]
pub fn detect_tailscale() -> Result<String, String> {
    let output = Command::new("tailscale")
        .arg("version")
        .output()
        .map_err(|_| "not_found".to_string())?;
    if !output.status.success() {
        return Err("not_found".to_string());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout.lines().next().unwrap_or("").trim().to_string();
    if version.is_empty() {
        return Err("not_found".to_string());
    }
    if version_gte(&version, "1.0") {
        Ok(version)
    } else {
        Err(format!("version_too_old:{}", version))
    }
}

#[tauri::command]
pub fn detect_daemon() -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .map_err(|e| format!("not_running:{}", e))?;
    let resp = client
        .get("http://127.0.0.1:8787/health")
        .send()
        .map_err(|_| "not_running".to_string())?;
    if resp.status().is_success() {
        Ok("running".to_string())
    } else {
        Err("not_running".to_string())
    }
}

#[tauri::command]
pub fn detect_tailscale_ip() -> Result<String, String> {
    let output = Command::new("tailscale")
        .args(["ip", "-4"])
        .output()
        .map_err(|_| "not_connected".to_string())?;
    if !output.status.success() {
        return Err("not_connected".to_string());
    }
    let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if ip.is_empty() {
        return Err("not_connected".to_string());
    }
    Ok(ip)
}

#[tauri::command]
pub fn install_daemon() -> Result<String, String> {
    let bin_path = daemon_bin_path();

    // Check if already installed and working
    if bin_path.exists() {
        let check = Command::new(&bin_path).arg("--help").output();
        if let Ok(output) = check {
            if output.status.success() {
                return Ok("already_installed".to_string());
            }
        }
    }

    // Look for bundled binary candidates
    let candidates: Vec<PathBuf> = vec![
        // Same directory as the app executable
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("ghost-protocol-daemon")))
            .unwrap_or_default(),
        // Development path: relative to the Tauri src dir
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| {
                d.join("../../daemon/target/release/ghost-protocol-daemon")
            }))
            .unwrap_or_default(),
        // Also try from the project root (common dev layout)
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../daemon/target/release/ghost-protocol-daemon"),
    ];

    let source = candidates.iter().find(|p| {
        !p.as_os_str().is_empty() && p.exists()
    });

    let source = match source {
        Some(p) => p,
        None => return Err("install_failed:binary_not_found".to_string()),
    };

    // Create parent directory
    if let Some(parent) = bin_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("install_failed:mkdir:{}", e))?;
    }

    // Copy binary to install path
    std::fs::copy(source, &bin_path)
        .map_err(|e| format!("install_failed:copy:{}", e))?;

    // chmod +x
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&bin_path, perms)
            .map_err(|e| format!("install_failed:chmod:{}", e))?;
    }

    Ok("installed".to_string())
}

#[tauri::command]
pub fn start_daemon(bind_host: String, port: u16) -> Result<String, String> {
    let bin_path = daemon_bin_path();
    let bin = if bin_path.exists() {
        bin_path.to_str().unwrap().to_string()
    } else {
        // Fallback: try PATH
        "ghost-protocol-daemon".to_string()
    };

    let bind = format!("{},127.0.0.1", bind_host);
    let cidrs = "100.64.0.0/10,fd7a:115c:a1e0::/48,127.0.0.1/32";

    let result = Command::new("setsid")
        .args([
            &bin,
            "--bind-host", &bind,
            "--bind-port", &port.to_string(),
            "--allowed-cidrs", cidrs,
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    match result {
        Ok(_) => Ok("spawned".to_string()),
        Err(e) => Err(format!("spawn_failed:{}", e)),
    }
}

#[tauri::command]
pub fn stop_daemon() -> Result<String, String> {
    let result = Command::new("pkill")
        .args(["-f", "ghost-protocol-daemon"])
        .output()
        .map_err(|e| format!("not_running:{}", e))?;
    if result.status.success() {
        Ok("stopped".to_string())
    } else {
        Err("not_running".to_string())
    }
}
