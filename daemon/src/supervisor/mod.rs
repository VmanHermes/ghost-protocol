use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const DRIVER_TERMINAL: &str = "terminal_driver";
pub const DRIVER_STRUCTURED_CHAT: &str = "structured_chat_driver";
pub const DRIVER_API: &str = "api_driver";
pub const DRIVER_IDE: &str = "ide_driver";
pub const DRIVER_CODE_SERVER: &str = "code_server_driver";

pub const CAP_CHAT_VIEW: &str = "supports_chat_view";
pub const CAP_TERMINAL_VIEW: &str = "supports_terminal_view";
pub const CAP_RESUME: &str = "supports_resume";
pub const CAP_SAFE_MODE_SWITCH: &str = "supports_safe_mode_switch";
pub const CAP_STRUCTURED_EVENTS: &str = "supports_structured_events";
pub const CAP_DELEGATION: &str = "supports_delegation";
pub const CAP_MAILBOX: &str = "supports_mailbox";
pub const CAP_BROWSER_VIEW: &str = "supports_browser_view";
pub const CAP_CODE_SERVER: &str = "supports_code_server";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    #[serde(default)]
    pub experimental_multi_agent: bool,
    #[serde(default = "default_allowed_driver_kinds")]
    pub allowed_driver_kinds: Vec<String>,
    #[serde(default)]
    pub default_skill_set: Vec<String>,
    #[serde(default)]
    pub delegation_limits: DelegationLimits,
    #[serde(default = "default_communication_policy")]
    pub communication_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegationLimits {
    #[serde(default = "default_max_depth")]
    pub max_depth: u32,
    #[serde(default = "default_max_children")]
    pub max_children: u32,
    pub budget_tokens: Option<u64>,
    #[serde(default = "default_budget_secs")]
    pub budget_secs: f64,
}

impl Default for DelegationLimits {
    fn default() -> Self {
        Self {
            max_depth: default_max_depth(),
            max_children: default_max_children(),
            budget_tokens: None,
            budget_secs: default_budget_secs(),
        }
    }
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            experimental_multi_agent: false,
            allowed_driver_kinds: default_allowed_driver_kinds(),
            default_skill_set: Vec::new(),
            delegation_limits: DelegationLimits::default(),
            communication_policy: default_communication_policy(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupervisorEvent {
    pub event_type: String,
    pub session_id: Option<String>,
    pub contract_id: Option<String>,
    pub ts: String,
    pub payload: Value,
}

pub fn default_allowed_driver_kinds() -> Vec<String> {
    vec![
        DRIVER_TERMINAL.to_string(),
        DRIVER_STRUCTURED_CHAT.to_string(),
        DRIVER_API.to_string(),
    ]
}

pub fn default_communication_policy() -> String {
    "supervisor_mailbox".to_string()
}

fn default_max_depth() -> u32 {
    2
}

fn default_max_children() -> u32 {
    4
}

fn default_budget_secs() -> f64 {
    900.0
}

pub fn normalize_project_config(input: &Value) -> Value {
    let mut config = serde_json::from_value::<ProjectConfig>(input.clone()).unwrap_or_default();

    if config.allowed_driver_kinds.is_empty() {
        config.allowed_driver_kinds = default_allowed_driver_kinds();
    }
    if config.communication_policy.is_empty() {
        config.communication_policy = default_communication_policy();
    }

    serde_json::to_value(config).unwrap_or_else(|_| json!({}))
}

pub fn parse_project_config(config_json: &str) -> ProjectConfig {
    serde_json::from_str::<ProjectConfig>(config_json).unwrap_or_default()
}

pub fn driver_capabilities(driver_kind: &str, persistent: bool, agent_present: bool) -> Vec<String> {
    let mut caps: Vec<&str> = match driver_kind {
        DRIVER_STRUCTURED_CHAT | DRIVER_API => vec![
            CAP_CHAT_VIEW,
            CAP_STRUCTURED_EVENTS,
            CAP_DELEGATION,
            CAP_MAILBOX,
        ],
        DRIVER_IDE => vec![CAP_RESUME],
        DRIVER_CODE_SERVER => vec![CAP_BROWSER_VIEW, CAP_CODE_SERVER],
        _ => vec![CAP_TERMINAL_VIEW, CAP_RESUME],
    };

    if persistent {
        caps.push(CAP_RESUME);
    }
    if agent_present && !caps.contains(&CAP_DELEGATION) {
        caps.push(CAP_DELEGATION);
    }
    if agent_present && !caps.contains(&CAP_MAILBOX) {
        caps.push(CAP_MAILBOX);
    }

    caps.sort_unstable();
    caps.dedup();
    caps.into_iter().map(|s| s.to_string()).collect()
}

pub fn supports_capability(capabilities: &[String], capability: &str) -> bool {
    capabilities.iter().any(|c| c == capability)
}
