use serde::Serialize;
use std::fs;

#[derive(Debug, Clone, Serialize)]
pub struct RamInfo {
    pub total_gb: f64,
    pub used_gb: f64,
}

pub fn detect_ram() -> RamInfo {
    let meminfo = fs::read_to_string("/proc/meminfo").unwrap_or_default();

    let total_kb = parse_meminfo_field(&meminfo, "MemTotal:");
    let available_kb = parse_meminfo_field(&meminfo, "MemAvailable:");

    let total_gb = total_kb as f64 / 1_048_576.0;
    let used_gb = (total_kb.saturating_sub(available_kb)) as f64 / 1_048_576.0;

    RamInfo {
        total_gb: (total_gb * 10.0).round() / 10.0,
        used_gb: (used_gb * 10.0).round() / 10.0,
    }
}

fn parse_meminfo_field(meminfo: &str, field: &str) -> u64 {
    meminfo
        .lines()
        .find(|l| l.starts_with(field))
        .and_then(|l| {
            l.split_whitespace()
                .nth(1)
                .and_then(|v| v.parse::<u64>().ok())
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ram_info_returns_valid_data() {
        let info = detect_ram();
        assert!(info.total_gb > 0.0, "expected positive total RAM");
        assert!(info.used_gb >= 0.0, "expected non-negative used RAM");
        assert!(info.used_gb <= info.total_gb, "used cannot exceed total");
    }

    #[test]
    fn test_parse_meminfo_field() {
        let meminfo = "MemTotal:       16384000 kB\nMemAvailable:    8192000 kB\n";
        assert_eq!(parse_meminfo_field(meminfo, "MemTotal:"), 16384000);
        assert_eq!(parse_meminfo_field(meminfo, "MemAvailable:"), 8192000);
        assert_eq!(parse_meminfo_field(meminfo, "Missing:"), 0);
    }
}
