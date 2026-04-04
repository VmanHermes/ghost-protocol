use std::path::PathBuf;
use std::process::Command;

/// Returns ~/.local/share/ghost-protocol/daemon-venv
fn daemon_venv_dir() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".local/share")
        });
    base.join("ghost-protocol/daemon-venv")
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
pub fn detect_python() -> Result<String, String> {
    let output = Command::new("python3")
        .arg("--version")
        .output()
        .map_err(|_| "not_found".to_string())?;
    if !output.status.success() {
        return Err("not_found".to_string());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout.trim().strip_prefix("Python ").unwrap_or(stdout.trim());
    if version_gte(version, "3.10") {
        Ok(version.to_string())
    } else {
        Err(format!("version_too_old:{}", version))
    }
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
    let venv_dir = daemon_venv_dir();
    let venv_python = venv_dir.join("bin/python3");

    // Check if already installed in venv
    if venv_python.exists() {
        let check = Command::new(&venv_python)
            .args(["-c", "import ghost_protocol_daemon"])
            .output();
        if let Ok(output) = check {
            if output.status.success() {
                return Ok("already_installed".to_string());
            }
        }
    }

    // Create venv if it doesn't exist (python3 -m venv always has pip)
    if !venv_python.exists() {
        let result = Command::new("python3")
            .args(["-m", "venv", venv_dir.to_str().unwrap()])
            .output()
            .map_err(|e| format!("install_failed:venv:{}", e))?;
        if !result.status.success() {
            let stderr = String::from_utf8_lossy(&result.stderr);
            return Err(format!("install_failed:venv:{}", stderr.chars().take(200).collect::<String>()));
        }
    }

    // Install into the venv
    let output = Command::new(&venv_python)
        .args(["-m", "pip", "install",
               "git+https://github.com/VmanHermes/ghost-protocol.git#subdirectory=backend"])
        .output()
        .map_err(|e| format!("install_failed:{}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("install_failed:{}", stderr.chars().take(200).collect::<String>()));
    }
    Ok("installed".to_string())
}

#[tauri::command]
pub fn start_daemon(bind_host: String, port: u16) -> Result<String, String> {
    let venv_python = daemon_venv_dir().join("bin/python3");
    let python = if venv_python.exists() {
        venv_python.to_str().unwrap().to_string()
    } else {
        "python3".to_string()
    };

    // Spawn daemon as a detached process via setsid
    let bind = format!("{},127.0.0.1", bind_host);
    let cidrs = "100.64.0.0/10,fd7a:115c:a1e0::/48,127.0.0.1/32";

    let result = Command::new("setsid")
        .args([&python, "-m", "ghost_protocol_daemon"])
        .env("GHOST_PROTOCOL_BIND_HOST", &bind)
        .env("GHOST_PROTOCOL_BIND_PORT", port.to_string())
        .env("GHOST_PROTOCOL_ALLOWED_CIDRS", cidrs)
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
        .args(["-f", "python.*ghost_protocol_daemon"])
        .output()
        .map_err(|e| format!("not_running:{}", e))?;
    if result.status.success() {
        Ok("stopped".to_string())
    } else {
        Err("not_running".to_string())
    }
}
