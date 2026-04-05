use std::process::Command;
use tracing::{debug, warn};

/// Formats a session name as `ghost-{id_without_dashes}`.
pub fn session_name(session_id: &str) -> String {
    format!("ghost-{}", session_id.replace('-', ""))
}

/// Creates a new detached tmux session with the given working directory and shell,
/// then configures it (status off, pane-border-status off, mouse off).
pub fn new_session(session_id: &str, workdir: &str, shell: &str) -> Result<(), String> {
    let name = session_name(session_id);

    debug!(session = %name, workdir, shell, "creating tmux session");

    let mut cmd = Command::new("tmux");
    cmd.args([
        "new-session",
        "-d",
        "-s", &name,
        "-x", "120",
        "-y", "32",
        "-c", workdir,
    ]);

    // Multi-word commands (e.g., "ollama run llama3") need bash -c wrapping
    if shell.contains(' ') {
        cmd.args(["bash", "-c", shell]);
    } else {
        cmd.arg(shell);
    }

    let output = cmd
        .output()
        .map_err(|e| format!("failed to spawn tmux: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(session = %name, %stderr, "tmux new-session failed");
        return Err(format!("tmux new-session failed: {stderr}"));
    }

    // Configure the session
    for (option, value) in [
        ("status", "off"),
        ("pane-border-status", "off"),
        ("mouse", "off"),
    ] {
        let set_output = Command::new("tmux")
            .args(["set-option", "-t", &name, option, value])
            .output()
            .map_err(|e| format!("failed to set tmux option {option}: {e}"))?;

        if !set_output.status.success() {
            let stderr = String::from_utf8_lossy(&set_output.stderr);
            warn!(session = %name, option, %stderr, "tmux set-option failed");
        }
    }

    debug!(session = %name, "tmux session created and configured");
    Ok(())
}

/// Returns the command vector to attach to a tmux session.
pub fn attach_command(session_id: &str) -> Vec<String> {
    let name = session_name(session_id);
    vec![
        "tmux".to_string(),
        "attach-session".to_string(),
        "-t".to_string(),
        name,
    ]
}

/// Checks whether a tmux session exists.
pub fn has_session(session_id: &str) -> bool {
    let name = session_name(session_id);

    let output = Command::new("tmux")
        .args(["has-session", "-t", &name])
        .output();

    match output {
        Ok(o) => o.status.success(),
        Err(e) => {
            warn!(session = %name, error = %e, "failed to check tmux session");
            false
        }
    }
}

/// Kills a tmux session. Returns true if the session was successfully killed.
pub fn kill_session(session_id: &str) -> bool {
    let name = session_name(session_id);

    debug!(session = %name, "killing tmux session");

    let output = Command::new("tmux")
        .args(["kill-session", "-t", &name])
        .output();

    match output {
        Ok(o) => {
            if !o.status.success() {
                let stderr = String::from_utf8_lossy(&o.stderr);
                warn!(session = %name, %stderr, "tmux kill-session failed");
            }
            o.status.success()
        }
        Err(e) => {
            warn!(session = %name, error = %e, "failed to kill tmux session");
            false
        }
    }
}

/// Lists all ghost-protocol tmux sessions (those with a `ghost-` prefix).
pub fn list_ghost_sessions() -> Vec<String> {
    let output = Command::new("tmux")
        .args(["list-sessions", "-F", "#{session_name}"])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout
                .lines()
                .filter(|line| line.starts_with("ghost-"))
                .map(|line| line.to_string())
                .collect()
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            debug!(%stderr, "tmux list-sessions returned non-zero");
            Vec::new()
        }
        Err(e) => {
            warn!(error = %e, "failed to list tmux sessions");
            Vec::new()
        }
    }
}

/// Checks whether tmux is available on the system.
pub fn is_available() -> bool {
    let output = Command::new("tmux")
        .arg("-V")
        .output();

    match output {
        Ok(o) => {
            debug!(
                version = %String::from_utf8_lossy(&o.stdout).trim(),
                "tmux availability check"
            );
            o.status.success()
        }
        Err(e) => {
            warn!(error = %e, "tmux is not available");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_name_format() {
        let id = "550e8400-e29b-41d4-a716-446655440000";
        let name = session_name(id);
        assert_eq!(name, "ghost-550e8400e29b41d4a716446655440000");
        assert!(name.starts_with("ghost-"));
        // The UUID portion must have no dashes.
        assert!(!name["ghost-".len()..].contains('-'));
    }

    #[test]
    fn test_attach_command() {
        let id = "550e8400-e29b-41d4-a716-446655440000";
        let cmd = attach_command(id);
        assert_eq!(cmd, vec![
            "tmux",
            "attach-session",
            "-t",
            "ghost-550e8400e29b41d4a716446655440000",
        ]);
    }
}
