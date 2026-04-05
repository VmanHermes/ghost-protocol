use std::process::Command;

use serde::Serialize;
use serde_json;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub tailscale_ip: Option<String>,
    pub hostname: String,
    pub ssh_available: bool,
}

pub fn get_system_info() -> SystemInfo {
    SystemInfo {
        tailscale_ip: get_tailscale_ip(),
        hostname: hostname::get()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "unknown".to_string()),
        ssh_available: is_ssh_available(),
    }
}

pub fn get_tailscale_ip() -> Option<String> {
    Command::new("tailscale")
        .args(["ip", "-4"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

pub fn is_ssh_available() -> bool {
    Command::new("ssh")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TailscalePeer {
    pub name: String,
    pub ip: String,
    pub online: bool,
}

pub fn list_tailscale_peers() -> Vec<TailscalePeer> {
    let output = match Command::new("tailscale")
        .args(["status", "--json"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return vec![],
    };

    let json: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    let peers = match json.get("Peer").and_then(|p| p.as_object()) {
        Some(p) => p,
        None => return vec![],
    };

    let mut result = Vec::new();
    for (_key, peer) in peers {
        let name = peer["HostName"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();
        let online = peer["Online"].as_bool().unwrap_or(false);

        let ip = peer["TailscaleIPs"]
            .as_array()
            .and_then(|ips| {
                ips.iter()
                    .filter_map(|v| v.as_str())
                    .find(|s| !s.contains(':'))
                    .map(|s| s.to_string())
            });

        if let Some(ip) = ip {
            result.push(TailscalePeer { name, ip, online });
        }
    }
    result
}
