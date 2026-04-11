use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

fn default_launch_supported() -> bool {
    true
}

const GHOST_ENABLE_MANAGED_CLAUDE: &str = "GHOST_ENABLE_MANAGED_CLAUDE";

#[derive(Debug, Clone, Default)]
pub struct ManagedClaudeLaunch {
    pub launch_supported: bool,
    pub launch_note: Option<String>,
    pub env: HashMap<String, String>,
}

impl ManagedClaudeLaunch {
    fn disabled(note: impl Into<String>) -> Self {
        Self {
            launch_supported: false,
            launch_note: Some(note.into()),
            env: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ManagedClaudeConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub auth_token: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub agent_type: String,
    pub command: String,
    pub version: Option<String>,
    #[serde(default)]
    pub persistent: bool,
    #[serde(default)]
    pub supports_mcp: bool,
    #[serde(default = "default_launch_supported")]
    pub launch_supported: bool,
    #[serde(default)]
    pub launch_note: Option<String>,
}

impl AgentInfo {
    /// Whether this agent speaks Claude Code's stream-JSON protocol
    /// (--input-format stream-json, --mcp-config, --allowedTools, etc.)
    pub fn uses_claude_protocol(&self) -> bool {
        self.id == "claude-code" || self.id.starts_with("claude")
    }
}

pub fn is_claude_protocol_agent(agent_id: &str) -> bool {
    agent_id == "claude-code" || agent_id.starts_with("claude")
}

pub fn resolve_managed_claude_launch() -> ManagedClaudeLaunch {
    let persisted = match load_managed_claude_config() {
        Ok(config) => config,
        Err(error) => {
            return ManagedClaudeLaunch::disabled(format!(
                "Ghost-managed Claude Code config could not be read: {error}"
            ));
        }
    };

    let enabled = truthy_env(GHOST_ENABLE_MANAGED_CLAUDE)
        || persisted.as_ref().map(|config| config.enabled).unwrap_or(false);

    if !enabled {
        return ManagedClaudeLaunch::disabled(
            "Ghost-managed Claude Code stays manual-only unless the daemon host sets GHOST_ENABLE_MANAGED_CLAUDE=1 or runs `ghost setup claude`, then provides API or cloud auth. Run Claude Code directly and attach Ghost through MCP instead.",
        );
    }

    let mut env = HashMap::new();
    copy_env_if_set("ANTHROPIC_API_KEY", &mut env);
    copy_env_if_set("ANTHROPIC_AUTH_TOKEN", &mut env);
    copy_env_if_set("ANTHROPIC_BASE_URL", &mut env);
    copy_config_if_missing(
        "ANTHROPIC_API_KEY",
        persisted.as_ref().and_then(|config| config.api_key.clone()),
        &mut env,
    );
    copy_config_if_missing(
        "ANTHROPIC_AUTH_TOKEN",
        persisted.as_ref().and_then(|config| config.auth_token.clone()),
        &mut env,
    );
    copy_config_if_missing(
        "ANTHROPIC_BASE_URL",
        persisted.as_ref().and_then(|config| config.base_url.clone()),
        &mut env,
    );

    let bedrock = truthy_env("CLAUDE_CODE_USE_BEDROCK");
    let vertex = truthy_env("CLAUDE_CODE_USE_VERTEX");
    let foundry = truthy_env("CLAUDE_CODE_USE_FOUNDRY");

    if bedrock {
        env.insert("CLAUDE_CODE_USE_BEDROCK".into(), "1".into());
    }
    if vertex {
        env.insert("CLAUDE_CODE_USE_VERTEX".into(), "1".into());
    }
    if foundry {
        env.insert("CLAUDE_CODE_USE_FOUNDRY".into(), "1".into());
    }

    let has_supported_auth = env.contains_key("ANTHROPIC_API_KEY")
        || env.contains_key("ANTHROPIC_AUTH_TOKEN")
        || bedrock
        || vertex
        || foundry;

    if !has_supported_auth {
        return ManagedClaudeLaunch::disabled(
            "Ghost-managed Claude Code requires daemon-supplied API or cloud auth. Set ANTHROPIC_API_KEY, ANTHROPIC_AUTH_TOKEN, or enable Claude cloud auth (Bedrock, Vertex, or Foundry) on the daemon host, or store an API key with `ghost setup claude`.",
        );
    }

    let config_dir = managed_claude_config_dir();
    if let Err(error) = std::fs::create_dir_all(&config_dir) {
        return ManagedClaudeLaunch::disabled(format!(
            "Ghost-managed Claude Code could not prepare its dedicated config dir at {}: {error}",
            config_dir.display()
        ));
    }

    env.insert(
        "CLAUDE_CONFIG_DIR".into(),
        config_dir.to_string_lossy().to_string(),
    );

    ManagedClaudeLaunch {
        launch_supported: true,
        launch_note: Some(
            "Ghost-managed Claude Code is enabled with daemon-supplied API or cloud auth and a dedicated Claude config dir.".into(),
        ),
        env,
    }
}

pub fn detect_agents() -> Vec<AgentInfo> {
    let mut agents = Vec::new();

    // Claude Code — runs as interactive TUI in tmux
    if let Some(version) = detect_cli_version("claude", &["--version"]) {
        let managed = resolve_managed_claude_launch();
        agents.push(AgentInfo {
            id: "claude-code".into(),
            name: "Claude Code".into(),
            agent_type: "cli".into(),
            command: "claude".into(),
            version: Some(version),
            persistent: true,
            supports_mcp: true,
            launch_supported: managed.launch_supported,
            launch_note: managed.launch_note,
        });
    }

    // Hermes — needs `chat -Q` for quiet/programmatic piped I/O
    if let Some(version) = detect_cli_version("hermes", &["version"]) {
        agents.push(AgentInfo {
            id: "hermes".into(),
            name: "Hermes".into(),
            agent_type: "cli".into(),
            command: "hermes chat -Q".into(),
            version: Some(version),
            persistent: false,
            supports_mcp: false,
            launch_supported: true,
            launch_note: None,
        });
    } else if which("hermes").is_some() {
        agents.push(AgentInfo {
            id: "hermes".into(),
            name: "Hermes".into(),
            agent_type: "cli".into(),
            command: "hermes chat -Q".into(),
            version: None,
            persistent: false,
            supports_mcp: false,
            launch_supported: true,
            launch_note: None,
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
            persistent: false,
            supports_mcp: false,
            launch_supported: true,
            launch_note: None,
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
            persistent: false,
            supports_mcp: false,
            launch_supported: true,
            launch_note: None,
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
                persistent: false,
                supports_mcp: false,
                launch_supported: true,
                launch_note: None,
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

fn truthy_env(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            let trimmed = value.trim();
            !trimmed.is_empty()
                && trimmed != "0"
                && !trimmed.eq_ignore_ascii_case("false")
                && !trimmed.eq_ignore_ascii_case("no")
        })
        .unwrap_or(false)
}

fn copy_env_if_set(name: &str, env: &mut HashMap<String, String>) {
    if let Ok(value) = std::env::var(name) {
        if !value.trim().is_empty() {
            env.insert(name.to_string(), value);
        }
    }
}

fn copy_config_if_missing(name: &str, value: Option<String>, env: &mut HashMap<String, String>) {
    if env.contains_key(name) {
        return;
    }
    if let Some(value) = value {
        if !value.trim().is_empty() {
            env.insert(name.to_string(), value);
        }
    }
}

pub(crate) fn load_managed_claude_config() -> Result<Option<ManagedClaudeConfig>, String> {
    let path = managed_claude_config_path();
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!("failed to read {}: {error}", path.display()));
        }
    };

    serde_json::from_str(&content)
        .map(Some)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

pub(crate) fn managed_claude_config_path() -> PathBuf {
    ghost_config_dir().join("managed-claude.json")
}

fn ghost_config_dir() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".config")
        })
        .join("ghost-protocol")
}

fn managed_claude_config_dir() -> PathBuf {
    ghost_config_dir().join("claude-managed")
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
