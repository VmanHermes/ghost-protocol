use dialoguer::{Input, Password, Select};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagedClaudeConfig {
    enabled: bool,
    api_key: Option<String>,
    auth_token: Option<String>,
    base_url: Option<String>,
}

pub async fn run_claude() -> Result<(), String> {
    println!("Configuring Ghost-managed Claude Code for this machine...\n");

    let options = [
        "Anthropic API key (recommended)",
        "Custom bearer token",
        "Disable managed Claude Code for this machine",
    ];
    let choice = Select::new()
        .with_prompt("How should Ghost authenticate managed Claude Code sessions?")
        .items(&options)
        .default(0)
        .interact()
        .map_err(|e| format!("setup error: {e}"))?;

    let path = managed_claude_config_path();

    match choice {
        0 => {
            let api_key = Password::new()
                .with_prompt("Anthropic API key")
                .with_confirmation("Confirm API key", "Keys did not match")
                .interact()
                .map_err(|e| format!("input error: {e}"))?;
            if api_key.trim().is_empty() {
                return Err("API key cannot be empty".into());
            }
            let base_url = prompt_optional("Custom Anthropic base URL (optional)")?;
            write_managed_claude_config(
                &path,
                &ManagedClaudeConfig {
                    enabled: true,
                    api_key: Some(api_key),
                    auth_token: None,
                    base_url,
                },
            )?;
        }
        1 => {
            let auth_token = Password::new()
                .with_prompt("Bearer auth token")
                .with_confirmation("Confirm bearer token", "Tokens did not match")
                .interact()
                .map_err(|e| format!("input error: {e}"))?;
            if auth_token.trim().is_empty() {
                return Err("Bearer token cannot be empty".into());
            }
            let base_url = prompt_optional("Custom Anthropic base URL (optional)")?;
            write_managed_claude_config(
                &path,
                &ManagedClaudeConfig {
                    enabled: true,
                    api_key: None,
                    auth_token: Some(auth_token),
                    base_url,
                },
            )?;
        }
        2 => {
            disable_managed_claude(&path)?;
            println!("Managed Claude Code disabled for this machine.");
            println!(
                "If you also export {} in the daemon environment, unset it before restarting the daemon.",
                "GHOST_ENABLE_MANAGED_CLAUDE"
            );
            return Ok(());
        }
        _ => unreachable!("unexpected setup selection"),
    }

    println!("\nSaved machine-local Claude auth at {}", path.display());
    println!("This file is separate from project config and is intended for the daemon host only.");
    println!("Restart `ghost-protocol-daemon` so managed Claude sessions pick up the new settings.");
    Ok(())
}

fn prompt_optional(prompt: &str) -> Result<Option<String>, String> {
    let value: String = Input::new()
        .with_prompt(prompt)
        .allow_empty(true)
        .interact_text()
        .map_err(|e| format!("input error: {e}"))?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

fn write_managed_claude_config(path: &PathBuf, config: &ManagedClaudeConfig) -> Result<(), String> {
    let dir = path
        .parent()
        .ok_or_else(|| format!("invalid config path: {}", path.display()))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
    std::fs::write(
        path,
        serde_json::to_string_pretty(config).map_err(|e| format!("serialize error: {e}"))?,
    )
    .map_err(|e| format!("failed to write {}: {e}", path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)
            .map_err(|e| format!("failed to stat {}: {e}", path.display()))?
            .permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(path, perms)
            .map_err(|e| format!("failed to secure {}: {e}", path.display()))?;
    }

    Ok(())
}

fn disable_managed_claude(path: &PathBuf) -> Result<(), String> {
    if path.exists() {
        std::fs::remove_file(path)
            .map_err(|e| format!("failed to remove {}: {e}", path.display()))?;
    }
    Ok(())
}

fn managed_claude_config_path() -> PathBuf {
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
