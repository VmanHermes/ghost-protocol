use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub agent_type: String,
    pub command: String,
    pub version: Option<String>,
}

pub fn detect_agents() -> Vec<AgentInfo> {
    let mut agents = Vec::new();

    // Claude Code — runs as interactive TUI in tmux
    if let Some(version) = detect_cli_version("claude", &["--version"]) {
        agents.push(AgentInfo {
            id: "claude-code".into(),
            name: "Claude Code".into(),
            agent_type: "cli".into(),
            command: "claude".into(),
            version: Some(version),
        });
    }

    // Hermes
    if which("hermes").is_some() {
        agents.push(AgentInfo {
            id: "hermes".into(),
            name: "Hermes".into(),
            agent_type: "cli".into(),
            command: "hermes".into(),
            version: None,
        });
    }

    // Aider
    if let Some(version) = detect_cli_version("aider", &["--version"]) {
        agents.push(AgentInfo {
            id: "aider".into(),
            name: "Aider".into(),
            agent_type: "cli".into(),
            command: "aider".into(),
            version: Some(version),
        });
    }

    // OpenClaw
    if which("openclaw").is_some() {
        agents.push(AgentInfo {
            id: "openclaw".into(),
            name: "OpenClaw".into(),
            agent_type: "cli".into(),
            command: "openclaw".into(),
            version: None,
        });
    }

    // Ollama models
    if let Some(models) = detect_ollama_models() {
        for model in models {
            agents.push(AgentInfo {
                id: format!("ollama:{model}"),
                name: format!("Ollama ({model})"),
                agent_type: "api".into(),
                command: format!("ollama run {model}"),
                version: None,
            });
        }
    }

    // Custom agents from config
    if let Some(custom) = load_custom_agents() {
        agents.extend(custom);
    }

    agents
}

fn which(name: &str) -> Option<String> {
    Command::new("which")
        .arg(name)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn detect_cli_version(cmd: &str, args: &[&str]) -> Option<String> {
    which(cmd)?;
    Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        })
}

fn detect_ollama_models() -> Option<Vec<String>> {
    let output = Command::new("curl")
        .args(["-s", "--max-time", "2", "http://localhost:11434/api/tags"])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let models = json["models"].as_array()?;
    Some(
        models
            .iter()
            .filter_map(|m| {
                m["name"]
                    .as_str()
                    .map(|s| s.split(':').next().unwrap_or(s).to_string())
            })
            .collect(),
    )
}

fn load_custom_agents() -> Option<Vec<AgentInfo>> {
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            std::path::PathBuf::from(home).join(".config")
        });
    let path = config_dir.join("ghost-protocol").join("agents.json");
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}
