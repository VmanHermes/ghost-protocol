use serde::Serialize;
use std::fs;

const NOTABLE_NAMES: &[&str] = &[
    "ollama",
    "hermes",
    "vllm",
    "llama-server",
    "code-server",
    "node",
    "cargo",
    "rustc",
    "python",
    "ghost-protocol-daemon",
];

#[derive(Debug, Clone, Serialize)]
pub struct NotableProcess {
    pub name: String,
    pub pid: u32,
    pub cpu_percent: Option<f64>,
}

pub fn scan_notable_processes() -> Vec<NotableProcess> {
    let mut result = Vec::new();

    let Ok(entries) = fs::read_dir("/proc") else {
        return result;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Only numeric directories are PIDs
        let Ok(pid) = name_str.parse::<u32>() else {
            continue;
        };

        let comm_path = entry.path().join("comm");
        let Ok(comm) = fs::read_to_string(&comm_path) else {
            continue;
        };
        let comm = comm.trim();

        if is_notable(comm) {
            result.push(NotableProcess {
                name: comm.to_string(),
                pid,
                cpu_percent: None,
            });
        }
    }

    result
}

pub fn is_notable(name: &str) -> bool {
    NOTABLE_NAMES.iter().any(|n| name.contains(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_notable_processes_returns_list() {
        let procs = scan_notable_processes();
        assert!(procs.len() <= 50, "sanity: shouldn't return hundreds");
    }

    #[test]
    fn test_is_notable() {
        assert!(is_notable("ollama"));
        assert!(is_notable("hermes"));
        assert!(is_notable("vllm"));
        assert!(!is_notable("bash"));
        assert!(!is_notable("systemd"));
    }
}
