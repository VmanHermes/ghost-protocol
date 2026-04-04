use serde::Serialize;
use std::fs;

#[derive(Debug, Clone, Serialize)]
pub struct CpuInfo {
    pub cores: usize,
    pub model: String,
}

pub fn detect_cpu() -> CpuInfo {
    let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();

    let model = cpuinfo
        .lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split(':').nth(1))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let cores = cpuinfo
        .lines()
        .filter(|l| l.starts_with("processor"))
        .count();

    CpuInfo {
        cores: cores.max(1),
        model,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_info_returns_valid_data() {
        let info = detect_cpu();
        assert!(info.cores > 0, "expected at least 1 core");
        assert!(!info.model.is_empty(), "expected non-empty CPU model");
    }
}
