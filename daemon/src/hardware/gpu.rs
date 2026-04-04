use serde::Serialize;
use std::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct GpuInfo {
    pub model: String,
    pub vram_gb: f64,
    pub driver: String,
    pub utilization_percent: Option<u8>,
    pub vram_used_gb: Option<f64>,
}

pub fn detect_gpu() -> Option<GpuInfo> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total,driver_version,utilization.gpu,memory.used",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next()?;
    parse_nvidia_smi_line(first_line)
}

fn parse_nvidia_smi_line(line: &str) -> Option<GpuInfo> {
    let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
    if parts.len() < 3 {
        return None;
    }

    let model = parts[0].to_string();
    let vram_mb: f64 = parts[1].parse().ok()?;
    let driver = parts[2].to_string();

    let utilization_percent = parts.get(3).and_then(|s| s.parse::<u8>().ok());
    let vram_used_gb = parts.get(4).and_then(|s| {
        s.parse::<f64>()
            .ok()
            .map(|mb| (mb / 1024.0 * 10.0).round() / 10.0)
    });

    Some(GpuInfo {
        model,
        vram_gb: (vram_mb / 1024.0 * 10.0).round() / 10.0,
        driver,
        utilization_percent,
        vram_used_gb,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_gpu_does_not_panic() {
        let info = detect_gpu();
        if let Some(gpu) = &info {
            assert!(!gpu.model.is_empty());
            assert!(gpu.vram_gb > 0.0);
        }
    }

    #[test]
    fn test_parse_nvidia_smi_output() {
        let output = "NVIDIA GeForce RTX 4090, 24564 MiB, 570.86.16, 45, 8200 MiB";
        let info = parse_nvidia_smi_line(output);
        assert!(info.is_some());
        let gpu = info.unwrap();
        assert_eq!(gpu.model, "NVIDIA GeForce RTX 4090");
        assert!((gpu.vram_gb - 24.0).abs() < 0.5);
        assert_eq!(gpu.driver, "570.86.16");
        assert_eq!(gpu.utilization_percent, Some(45));
        assert!((gpu.vram_used_gb.unwrap() - 8.0).abs() < 0.1);
    }

    #[test]
    fn test_parse_nvidia_smi_bad_output() {
        assert!(parse_nvidia_smi_line("garbage data").is_none());
        assert!(parse_nvidia_smi_line("").is_none());
    }
}
