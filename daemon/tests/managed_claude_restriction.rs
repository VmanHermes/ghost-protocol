use serde_json::{Value, json};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;

static NEXT_PORT: AtomicU16 = AtomicU16::new(0);

fn next_port() -> u16 {
    let seeded = NEXT_PORT.load(Ordering::SeqCst);
    if seeded != 0 {
        return NEXT_PORT.fetch_add(1, Ordering::SeqCst);
    }

    let pid = std::process::id() as u16;
    let base = 30_000 + (pid % 20_000);

    match NEXT_PORT.compare_exchange(0, base + 1, Ordering::SeqCst, Ordering::SeqCst) {
        Ok(_) => base,
        Err(existing) => {
            if existing == 0 {
                unreachable!("NEXT_PORT should be initialized by compare_exchange");
            }
            NEXT_PORT.fetch_add(1, Ordering::SeqCst)
        }
    }
}

fn write_fake_claude(bin_dir: &std::path::Path) {
    let path = bin_dir.join("claude");
    std::fs::write(
        &path,
        r#"#!/usr/bin/env bash
if [[ "${1:-}" == "--version" ]]; then
  echo "claude 0.0-test"
  exit 0
fi
printf '{"type":"result","usage":{"input_tokens":1,"output_tokens":1}}\n'
exit 0
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
    }
}

struct DaemonHarness {
    base: String,
    child: tokio::process::Child,
    temp_db: std::path::PathBuf,
    temp_root: std::path::PathBuf,
}

impl DaemonHarness {
    async fn spawn(extra_env: &[(&str, &str)], managed_config: Option<Value>) -> Self {
        let port = next_port();
        let base = format!("http://127.0.0.1:{port}");
        let temp_root = std::env::temp_dir().join(format!(
            "ghost_protocol_managed_claude_{}_{}",
            std::process::id(),
            port
        ));
        let bin_dir = temp_root.join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        write_fake_claude(&bin_dir);

        let temp_db = temp_root.join("ghost_protocol.db");
        let config_home = temp_root.join("config");
        let home = temp_root.join("home");
        std::fs::create_dir_all(&config_home).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        if let Some(config) = managed_config {
            let ghost_config_dir = config_home.join("ghost-protocol");
            std::fs::create_dir_all(&ghost_config_dir).unwrap();
            std::fs::write(
                ghost_config_dir.join("managed-claude.json"),
                serde_json::to_vec_pretty(&config).unwrap(),
            )
            .unwrap();
        }

        let mut path_parts = vec![bin_dir.to_string_lossy().to_string()];
        if let Ok(existing) = std::env::var("PATH") {
            path_parts.push(existing);
        }
        let path_value = path_parts.join(":");

        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_ghost-protocol-daemon"));
        command
            .args([
                "--bind-host",
                "127.0.0.1",
                "--bind-port",
                &port.to_string(),
                "--db-path",
                temp_db.to_str().unwrap(),
            ])
            .env("GHOST_PROTOCOL_ALLOWED_CIDRS", "127.0.0.1/32")
            .env("PATH", path_value)
            .env("XDG_CONFIG_HOME", &config_home)
            .env("HOME", &home)
            .kill_on_drop(true)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        for (key, value) in extra_env {
            command.env(key, value);
        }

        let mut child = command.spawn().expect("failed to spawn daemon");
        let client = reqwest::Client::new();
        let mut ready = false;
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if let Ok(resp) = client.get(format!("{base}/health")).send().await {
                if resp.status().is_success() {
                    ready = true;
                    break;
                }
            }
        }

        if !ready {
            let _ = child.kill().await;
            let output = child
                .wait_with_output()
                .await
                .expect("failed to collect daemon output after timeout");
            panic!(
                "daemon did not become ready within 10 seconds\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Self {
            base,
            child,
            temp_db,
            temp_root,
        }
    }

    async fn cleanup(mut self) {
        self.child.kill().await.ok();
        let _ = self.child.wait().await;
        let _ = std::fs::remove_file(&self.temp_db);
        let _ = std::fs::remove_file(self.temp_db.with_extension("db-wal"));
        let _ = std::fs::remove_file(self.temp_db.with_extension("db-shm"));
        let _ = std::fs::remove_dir_all(&self.temp_root);
    }
}

#[tokio::test]
async fn blocks_managed_claude_without_explicit_daemon_auth() {
    let harness = DaemonHarness::spawn(&[], None).await;
    let client = reqwest::Client::new();

    let agents_resp = client
        .get(format!("{}/api/agents", harness.base))
        .send()
        .await
        .unwrap();
    assert_eq!(agents_resp.status(), reqwest::StatusCode::OK);
    let agents: Vec<Value> = agents_resp.json().await.unwrap();
    let claude = agents
        .iter()
        .find(|agent| agent["id"] == "claude-code")
        .expect("claude-code should be detected");
    assert_eq!(claude["launchSupported"], false);
    assert!(
        claude["launchNote"]
            .as_str()
            .unwrap_or("")
            .contains("GHOST_ENABLE_MANAGED_CLAUDE=1")
    );

    let response = client
        .post(format!("{}/api/chat/sessions", harness.base))
        .json(&json!({
            "agentId": "claude-code",
            "workdir": "~"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let body: Value = response.json().await.unwrap();
    let error = body["error"].as_str().unwrap_or("");
    assert!(error.contains("claude-code"));
    assert!(error.contains("unavailable"));
    assert!(error.contains("GHOST_ENABLE_MANAGED_CLAUDE=1"));

    harness.cleanup().await;
}

#[tokio::test]
async fn allows_managed_claude_with_daemon_api_key_opt_in() {
    let harness = DaemonHarness::spawn(
        &[
            ("GHOST_ENABLE_MANAGED_CLAUDE", "1"),
            ("ANTHROPIC_API_KEY", "test-api-key"),
        ],
        None,
    )
    .await;
    let client = reqwest::Client::new();

    let agents_resp = client
        .get(format!("{}/api/agents", harness.base))
        .send()
        .await
        .unwrap();
    assert_eq!(agents_resp.status(), reqwest::StatusCode::OK);
    let agents: Vec<Value> = agents_resp.json().await.unwrap();
    let claude = agents
        .iter()
        .find(|agent| agent["id"] == "claude-code")
        .expect("claude-code should be detected");
    assert_eq!(claude["launchSupported"], true);
    assert!(
        claude["launchNote"]
            .as_str()
            .unwrap_or("")
            .contains("daemon-supplied API or cloud auth")
    );

    let response = client
        .post(format!("{}/api/chat/sessions", harness.base))
        .json(&json!({
            "agentId": "claude-code",
            "workdir": "~"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["agent"]["id"], "claude-code");
    assert_eq!(body["agent"]["launchSupported"], true);

    harness.cleanup().await;
}

#[tokio::test]
async fn allows_managed_claude_with_machine_config_file() {
    let harness = DaemonHarness::spawn(
        &[],
        Some(json!({
            "enabled": true,
            "apiKey": "configured-api-key"
        })),
    )
    .await;
    let client = reqwest::Client::new();

    let agents_resp = client
        .get(format!("{}/api/agents", harness.base))
        .send()
        .await
        .unwrap();
    assert_eq!(agents_resp.status(), reqwest::StatusCode::OK);
    let agents: Vec<Value> = agents_resp.json().await.unwrap();
    let claude = agents
        .iter()
        .find(|agent| agent["id"] == "claude-code")
        .expect("claude-code should be detected");
    assert_eq!(claude["launchSupported"], true);

    let response = client
        .post(format!("{}/api/chat/sessions", harness.base))
        .json(&json!({
            "agentId": "claude-code",
            "workdir": "~"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["agent"]["id"], "claude-code");
    assert_eq!(body["agent"]["launchSupported"], true);

    harness.cleanup().await;
}
