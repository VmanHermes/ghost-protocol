use serde::{Deserialize, Serialize};
use std::env;

fn default_max_lessons() -> usize {
    3
}

fn default_processing_transcript_limit() -> usize {
    8000
}

fn default_min_session_duration() -> u64 {
    10
}

fn default_min_session_chunks() -> usize {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntelligenceConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub embedding_provider: Option<String>,
    #[serde(default)]
    pub embedding_model: Option<String>,
    #[serde(default = "default_max_lessons")]
    pub max_lessons: usize,
    #[serde(default = "default_processing_transcript_limit")]
    pub processing_transcript_limit: usize,
    #[serde(default = "default_min_session_duration")]
    pub min_session_duration: u64,
    #[serde(default = "default_min_session_chunks")]
    pub min_session_chunks: usize,
}

impl Default for IntelligenceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: None,
            model: None,
            api_key_env: None,
            embedding_provider: None,
            embedding_model: None,
            max_lessons: default_max_lessons(),
            processing_transcript_limit: default_processing_transcript_limit(),
            min_session_duration: default_min_session_duration(),
            min_session_chunks: default_min_session_chunks(),
        }
    }
}

impl IntelligenceConfig {
    /// Resolve config: tries project config JSON first, then daemon-level config file,
    /// then returns default (disabled).
    pub fn resolve(project_config_json: Option<&str>) -> Self {
        // 1. Try project config JSON — look for "intelligence" block
        if let Some(json) = project_config_json {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(json) {
                if let Some(intel_val) = val.get("intelligence") {
                    if let Ok(cfg) = serde_json::from_value::<IntelligenceConfig>(intel_val.clone())
                    {
                        return cfg;
                    }
                }
            }
        }

        // 2. Try daemon-level config file
        if let Some(cfg) = Self::load_daemon_config() {
            return cfg;
        }

        // 3. Return default (disabled)
        Self::default()
    }

    /// Load config from ~/.config/ghost-protocol/intelligence.toml (respects XDG_CONFIG_HOME).
    /// Parses [intelligence] and [intelligence.embedding] sections line-by-line.
    pub fn load_daemon_config() -> Option<Self> {
        let config_home = env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| {
                let home = env::var("HOME").unwrap_or_else(|_| String::from("/root"));
                format!("{}/.config", home)
            });

        let path = format!("{}/ghost-protocol/intelligence.toml", config_home);
        let content = std::fs::read_to_string(&path).ok()?;

        let mut cfg = Self::default();
        let mut in_intelligence = false;
        let mut in_embedding = false;
        let mut found_any = false;

        for line in content.lines() {
            let line = line.trim();

            // Skip comments and empty lines
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Section headers
            if line == "[intelligence]" {
                in_intelligence = true;
                in_embedding = false;
                continue;
            }
            if line == "[intelligence.embedding]" {
                in_intelligence = true;
                in_embedding = true;
                continue;
            }
            // Any other section header — stop processing intelligence sections
            if line.starts_with('[') {
                in_intelligence = false;
                in_embedding = false;
                continue;
            }

            if !in_intelligence {
                continue;
            }

            // Parse key = value (value may be quoted or unquoted)
            let Some((key, raw_val)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let raw_val = raw_val.trim();
            // Strip surrounding quotes if present
            let val = if (raw_val.starts_with('"') && raw_val.ends_with('"'))
                || (raw_val.starts_with('\'') && raw_val.ends_with('\''))
            {
                raw_val[1..raw_val.len() - 1].to_string()
            } else {
                raw_val.to_string()
            };

            found_any = true;

            if in_embedding {
                match key {
                    "provider" => cfg.embedding_provider = Some(val),
                    "model" => cfg.embedding_model = Some(val),
                    _ => {}
                }
            } else {
                match key {
                    "enabled" => cfg.enabled = val == "true",
                    "provider" => cfg.provider = Some(val),
                    "model" => cfg.model = Some(val),
                    "api_key_env" => cfg.api_key_env = Some(val),
                    "max_lessons" => {
                        if let Ok(n) = val.parse() {
                            cfg.max_lessons = n;
                        }
                    }
                    "processing_transcript_limit" => {
                        if let Ok(n) = val.parse() {
                            cfg.processing_transcript_limit = n;
                        }
                    }
                    "min_session_duration" => {
                        if let Ok(n) = val.parse() {
                            cfg.min_session_duration = n;
                        }
                    }
                    "min_session_chunks" => {
                        if let Ok(n) = val.parse() {
                            cfg.min_session_chunks = n;
                        }
                    }
                    _ => {}
                }
            }
        }

        if found_any { Some(cfg) } else { None }
    }

    /// Resolve the API key. Checks api_key_env first, then well-known vars by provider.
    pub fn resolve_api_key(&self) -> Option<String> {
        // 1. Check the configured env var name
        if let Some(env_var) = &self.api_key_env {
            if let Ok(key) = env::var(env_var) {
                if !key.is_empty() {
                    return Some(key);
                }
            }
        }

        // 2. Fall back to well-known vars by provider
        match self.provider.as_deref() {
            Some("api") => {
                // Try Anthropic first, then OpenAI
                if let Ok(key) = env::var("ANTHROPIC_API_KEY") {
                    if !key.is_empty() {
                        return Some(key);
                    }
                }
                if let Ok(key) = env::var("OPENAI_API_KEY") {
                    if !key.is_empty() {
                        return Some(key);
                    }
                }
                None
            }
            Some("ollama") => None, // ollama doesn't need an API key
            _ => None,
        }
    }

    /// Returns true if intelligence is enabled and a provider is configured.
    pub fn is_active(&self) -> bool {
        self.enabled && self.provider.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_disabled() {
        let cfg = IntelligenceConfig::default();
        assert!(!cfg.enabled);
        assert!(cfg.provider.is_none());
        assert!(!cfg.is_active());
        assert_eq!(cfg.max_lessons, 3);
        assert_eq!(cfg.processing_transcript_limit, 8000);
        assert_eq!(cfg.min_session_duration, 10);
        assert_eq!(cfg.min_session_chunks, 5);
    }

    #[test]
    fn resolve_from_project_json() {
        let json = r#"{
            "intelligence": {
                "enabled": true,
                "provider": "api",
                "model": "claude-3-5-sonnet-20241022",
                "apiKeyEnv": "MY_API_KEY",
                "maxLessons": 5
            }
        }"#;

        let cfg = IntelligenceConfig::resolve(Some(json));
        assert!(cfg.enabled);
        assert_eq!(cfg.provider.as_deref(), Some("api"));
        assert_eq!(cfg.model.as_deref(), Some("claude-3-5-sonnet-20241022"));
        assert_eq!(cfg.api_key_env.as_deref(), Some("MY_API_KEY"));
        assert_eq!(cfg.max_lessons, 5);
        assert!(cfg.is_active());
    }

    #[test]
    fn resolve_disabled_when_no_provider() {
        let json = r#"{"intelligence": {"enabled": true}}"#;
        let cfg = IntelligenceConfig::resolve(Some(json));
        assert!(cfg.enabled);
        assert!(cfg.provider.is_none());
        assert!(!cfg.is_active());
    }

    #[test]
    fn resolve_falls_back_to_default_when_no_intelligence_block() {
        let json = r#"{"someOtherKey": "value", "projectName": "my-project"}"#;
        let cfg = IntelligenceConfig::resolve(Some(json));
        // Should fall back — if no daemon config file exists in test env, gets default
        // The key assertion is that it doesn't panic and returns a valid config
        // In a clean test env without the daemon config, it should be the default
        assert_eq!(cfg.max_lessons, 3);
        assert_eq!(cfg.processing_transcript_limit, 8000);
    }
}
