use std::process::Command;

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
