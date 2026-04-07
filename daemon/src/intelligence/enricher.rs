use crate::intelligence::config::IntelligenceConfig;
use crate::store::Store;

/// Result of enriching a session with contextual information.
#[derive(Debug, Clone)]
pub struct EnrichmentResult {
    pub system_prompt: String,
}

/// Project-level commands parsed from config JSON.
#[derive(Debug, Clone, Default)]
pub struct ProjectCommands {
    pub build: Option<String>,
    pub test: Option<String>,
    pub lint: Option<String>,
    pub deploy: Option<String>,
}

impl ProjectCommands {
    /// Parse from a project config JSON blob. Looks for a `commands` object with
    /// `build`, `test`, `lint`, and `deploy` string fields.
    pub fn from_config_json(config_json: &str) -> Self {
        let Ok(val) = serde_json::from_str::<serde_json::Value>(config_json) else {
            return Self::default();
        };
        let Some(cmds) = val.get("commands") else {
            return Self::default();
        };
        Self {
            build: cmds
                .get("build")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            test: cmds
                .get("test")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            lint: cmds
                .get("lint")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            deploy: cmds
                .get("deploy")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        }
    }
}

/// Build a minimal (~200 token) system prompt for a new session.
///
/// No LLM calls — purely a store query + string assembly.
pub fn enrich_session(
    store: &Store,
    config: &IntelligenceConfig,
    project_id: Option<&str>,
    project_name: Option<&str>,
    machine_name: &str,
    commands: Option<&ProjectCommands>,
) -> EnrichmentResult {
    let mut parts: Vec<String> = Vec::new();

    // 1. Ghost Protocol intro (3 lines)
    parts.push(
        "You are running inside Ghost Protocol, a mesh control plane that connects\n\
         your session to other machines and agents on the network. Use the Ghost\n\
         Protocol MCP tools to search memory, report outcomes, and check mesh state."
            .to_string(),
    );

    // 2. Project / machine line
    let context_line = match project_name {
        Some(name) => format!("Project: {} on {}", name, machine_name),
        None => format!("Machine: {}", machine_name),
    };
    parts.push(context_line);

    // 3. Commands line (only if commands provided and at least one is set)
    if let Some(cmds) = commands {
        let mut cmd_parts: Vec<String> = Vec::new();
        if let Some(build) = &cmds.build {
            cmd_parts.push(format!("build={}", build));
        }
        if let Some(test) = &cmds.test {
            cmd_parts.push(format!("test={}", test));
        }
        if let Some(lint) = &cmds.lint {
            cmd_parts.push(format!("lint={}", lint));
        }
        if let Some(deploy) = &cmds.deploy {
            cmd_parts.push(format!("deploy={}", deploy));
        }
        if !cmd_parts.is_empty() {
            parts.push(format!("Commands: {}", cmd_parts.join(", ")));
        }
    }

    // 4. Key lessons (top N by importance, only if any exist)
    let limit = config.max_lessons;
    if let Ok(lessons) = store.get_top_lessons(project_id, limit) {
        if !lessons.is_empty() {
            let mut lesson_lines = vec!["Key lessons:".to_string()];
            for record in &lessons {
                if let Some(lesson) = &record.lesson {
                    lesson_lines.push(format!("- {}", lesson));
                }
            }
            parts.push(lesson_lines.join("\n"));
        }
    }

    EnrichmentResult {
        system_prompt: parts.join("\n\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intelligence::config::IntelligenceConfig;
    use crate::store::test_store;

    #[test]
    fn enrichment_without_lessons() {
        let store = test_store();
        let config = IntelligenceConfig::default();

        let result = enrich_session(
            &store,
            &config,
            Some("proj-1"),
            Some("my-app"),
            "dev-machine",
            None,
        );

        let prompt = &result.system_prompt;
        assert!(
            prompt.contains("Ghost Protocol"),
            "should contain Ghost Protocol intro"
        );
        assert!(
            prompt.contains("Project: my-app on dev-machine"),
            "should contain project line"
        );
        assert!(
            !prompt.contains("Key lessons"),
            "should not contain Key lessons section when no memories"
        );
    }

    #[test]
    fn enrichment_with_lessons() {
        let store = test_store();
        store
            .create_project("proj-1", "my-app", "/tmp/my-app", "{}")
            .unwrap();
        store
            .create_memory(
                "m1",
                Some("proj-1"),
                None,
                "error",
                "Auth issue",
                "Tokens expired",
                Some("Always refresh tokens before they expire"),
                "{}",
                0.9,
            )
            .unwrap();

        let config = IntelligenceConfig::default();
        let result = enrich_session(
            &store,
            &config,
            Some("proj-1"),
            Some("my-app"),
            "dev-machine",
            None,
        );

        let prompt = &result.system_prompt;
        assert!(
            prompt.contains("Key lessons:"),
            "should contain Key lessons section"
        );
        assert!(
            prompt.contains("Always refresh tokens before they expire"),
            "should contain the lesson text"
        );
    }

    #[test]
    fn enrichment_with_commands() {
        let store = test_store();
        let config = IntelligenceConfig::default();

        let cmds = ProjectCommands {
            build: Some("cargo build".to_string()),
            test: Some("cargo test".to_string()),
            lint: None,
            deploy: None,
        };

        let result = enrich_session(
            &store,
            &config,
            None,
            Some("my-app"),
            "dev-machine",
            Some(&cmds),
        );

        let prompt = &result.system_prompt;
        assert!(
            prompt.contains("build=cargo build"),
            "should contain build command"
        );
        assert!(
            prompt.contains("test=cargo test"),
            "should contain test command"
        );
    }

    #[test]
    fn project_commands_from_json() {
        let json = r#"{
            "commands": {
                "build": "cargo build --release",
                "test": "cargo test",
                "lint": "cargo clippy",
                "deploy": "fly deploy"
            }
        }"#;

        let cmds = ProjectCommands::from_config_json(json);
        assert_eq!(cmds.build.as_deref(), Some("cargo build --release"));
        assert_eq!(cmds.test.as_deref(), Some("cargo test"));
        assert_eq!(cmds.lint.as_deref(), Some("cargo clippy"));
        assert_eq!(cmds.deploy.as_deref(), Some("fly deploy"));
    }
}
