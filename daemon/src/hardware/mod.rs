pub mod agents;
pub mod cpu;
pub mod gpu;
pub mod processes;
pub mod ram;

use serde::Serialize;

use crate::host::detect;

#[derive(Debug, Clone, Serialize)]
pub struct ToolsInfo {
    pub tmux: Option<String>,
    pub hermes: Option<String>,
    pub ollama: Option<String>,
    pub ssh_user: String,
    pub agents: Vec<agents::AgentInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineInfo {
    pub hostname: String,
    pub tailscale_ip: Option<String>,
    pub daemon_version: String,
    pub os: String,
    pub cpu: cpu::CpuInfo,
    pub ram_gb: f64,
    pub gpu: Option<gpu::GpuInfo>,
    pub tools: ToolsInfo,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineStatus {
    pub cpu_percent: Option<f64>,
    pub ram_used_gb: f64,
    pub ram_total_gb: f64,
    pub gpu_percent: Option<u8>,
    pub gpu_vram_used_gb: Option<f64>,
    pub active_sessions: usize,
    pub uptime_hours: f64,
    pub notable_processes: Vec<processes::NotableProcess>,
}

pub fn collect_machine_info() -> MachineInfo {
    let cpu_info = cpu::detect_cpu();
    let ram_info = ram::detect_ram();
    let gpu_info = gpu::detect_gpu();
    let sys_info = detect::get_system_info();

    let tmux_version = std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    let hermes_path = which_command("hermes");
    let ollama_endpoint = std::process::Command::new("curl")
        .args(["-s", "--max-time", "1", "http://localhost:11434/api/tags"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|_| "http://localhost:11434".to_string());

    let ssh_user = std::env::var("USER").unwrap_or_else(|_| "unknown".to_string());
    let detected_agents = agents::detect_agents();

    MachineInfo {
        hostname: sys_info.hostname,
        tailscale_ip: sys_info.tailscale_ip,
        daemon_version: env!("CARGO_PKG_VERSION").to_string(),
        os: read_os_release(),
        cpu: cpu_info,
        ram_gb: ram_info.total_gb,
        gpu: gpu_info,
        tools: ToolsInfo {
            tmux: tmux_version,
            hermes: hermes_path,
            ollama: ollama_endpoint,
            ssh_user,
            agents: detected_agents,
        },
    }
}

pub fn collect_machine_status(active_sessions: usize) -> MachineStatus {
    let ram_info = ram::detect_ram();
    let gpu_info = gpu::detect_gpu();
    let procs = processes::scan_notable_processes();

    let uptime_hours = std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
        .map(|secs| (secs / 3600.0 * 10.0).round() / 10.0)
        .unwrap_or(0.0);

    MachineStatus {
        cpu_percent: None,
        ram_used_gb: ram_info.used_gb,
        ram_total_gb: ram_info.total_gb,
        gpu_percent: gpu_info.as_ref().and_then(|g| g.utilization_percent),
        gpu_vram_used_gb: gpu_info.as_ref().and_then(|g| g.vram_used_gb),
        active_sessions,
        uptime_hours,
        notable_processes: procs,
    }
}

fn read_os_release() -> String {
    std::fs::read_to_string("/proc/version")
        .ok()
        .and_then(|v| v.split_whitespace().take(3).collect::<Vec<_>>().join(" ").into())
        .unwrap_or_else(|| "Linux unknown".to_string())
}

fn which_command(name: &str) -> Option<String> {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_machine_info_populates_fields() {
        let info = collect_machine_info();
        assert!(!info.hostname.is_empty());
        assert!(!info.daemon_version.is_empty());
        assert!(!info.os.is_empty());
        assert!(info.cpu.cores > 0);
        assert!(info.ram_gb > 0.0);
    }

    #[test]
    fn test_machine_status_populates_fields() {
        let status = collect_machine_status(0);
        assert!(status.ram_total_gb > 0.0);
        assert!(status.ram_used_gb >= 0.0);
    }
}
