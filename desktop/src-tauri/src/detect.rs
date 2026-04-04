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
pub fn detect_python() -> Result<String, String> {
    Err("not_implemented".to_string())
}

#[tauri::command]
pub fn detect_tmux() -> Result<String, String> {
    Err("not_implemented".to_string())
}

#[tauri::command]
pub fn detect_tailscale() -> Result<String, String> {
    Err("not_implemented".to_string())
}

#[tauri::command]
pub fn detect_daemon() -> Result<String, String> {
    Err("not_implemented".to_string())
}
