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

pub fn detect_local_agents() -> Vec<AgentInfo> {
    let mut agents = Vec::new();

    if let Some(version) = detect_cli_version("claude", &["--version"]) {
        agents.push(AgentInfo { id: "claude-code".into(), name: "Claude Code".into(), agent_type: "cli".into(), command: "claude".into(), version: Some(version) });
    }
    if which("hermes").is_some() {
        agents.push(AgentInfo { id: "hermes".into(), name: "Hermes".into(), agent_type: "cli".into(), command: "hermes".into(), version: None });
    }
    if let Some(version) = detect_cli_version("aider", &["--version"]) {
        agents.push(AgentInfo { id: "aider".into(), name: "Aider".into(), agent_type: "cli".into(), command: "aider".into(), version: Some(version) });
    }
    if which("openclaw").is_some() {
        agents.push(AgentInfo { id: "openclaw".into(), name: "OpenClaw".into(), agent_type: "cli".into(), command: "openclaw".into(), version: None });
    }
    if let Some(models) = detect_ollama_models() {
        for model in models {
            agents.push(AgentInfo { id: format!("ollama:{model}"), name: format!("Ollama ({model})"), agent_type: "api".into(), command: format!("ollama run {model}"), version: None });
        }
    }
    agents
}

fn which(name: &str) -> Option<String> {
    Command::new("which").arg(name).output().ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn detect_cli_version(cmd: &str, args: &[&str]) -> Option<String> {
    which(cmd)?;
    Command::new(cmd).args(args).output().ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or("").trim().to_string())
}

fn detect_ollama_models() -> Option<Vec<String>> {
    let output = Command::new("curl").args(["-s", "--max-time", "2", "http://localhost:11434/api/tags"]).output().ok().filter(|o| o.status.success())?;
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    Some(json["models"].as_array()?.iter().filter_map(|m| m["name"].as_str().map(|s| s.split(':').next().unwrap_or(s).to_string())).collect())
}
