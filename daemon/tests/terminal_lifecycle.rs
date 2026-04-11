use serde_json::{Value, json};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;

static NEXT_PORT: AtomicU16 = AtomicU16::new(0);

fn next_port() -> u16 {
    let seeded = NEXT_PORT.load(Ordering::SeqCst);
    if seeded != 0 {
        return NEXT_PORT.fetch_add(1, Ordering::SeqCst);
    }

    // Seed each test process into a different high port range so repeated cargo
    // test runs are much less likely to collide with stale daemons from an
    // earlier failed run.
    let pid = std::process::id() as u16;
    let base = 20_000 + (pid % 20_000);

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

async fn with_daemon<F, Fut>(test: F)
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let port = next_port();
    let base = format!("http://127.0.0.1:{port}");
    let temp_db = std::env::temp_dir().join(format!(
        "ghost_protocol_test_{}_{}.db",
        std::process::id(),
        port
    ));

    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_ghost-protocol-daemon"))
        .args([
            "--bind-host",
            "127.0.0.1",
            "--bind-port",
            &port.to_string(),
            "--db-path",
            temp_db.to_str().unwrap(),
        ])
        .env("GHOST_PROTOCOL_ALLOWED_CIDRS", "127.0.0.1/32")
        .kill_on_drop(true)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn daemon");

    // Wait for daemon to be ready
    let client = reqwest::Client::new();
    let mut ready = false;
    // Give the daemon a bit more room on slower machines and under release-prep load.
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

    // Run the test
    test(base).await;

    // Cleanup
    child.kill().await.ok();
    let _ = child.wait().await;
    let _ = std::fs::remove_file(&temp_db);
    // Also remove WAL/SHM files that SQLite may create
    let _ = std::fs::remove_file(temp_db.with_extension("db-wal"));
    let _ = std::fs::remove_file(temp_db.with_extension("db-shm"));
}

#[tokio::test]
async fn test_create_list_terminate_session() {
    with_daemon(|base| async move {
        let client = reqwest::Client::new();

        // Create session
        let resp = client
            .post(format!("{base}/api/terminal/sessions"))
            .json(&json!({"mode": "rescue_shell", "name": "test-session"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201, "expected 201 Created");
        let body: Value = resp.json().await.unwrap();
        let session_id = body["id"].as_str().unwrap().to_string();
        assert_eq!(body["mode"], "rescue_shell");
        // Status may be "created" or "running" depending on timing
        assert!(
            body["status"] == "running" || body["status"] == "created",
            "unexpected status: {}",
            body["status"]
        );

        // Give the session a moment to fully start
        tokio::time::sleep(Duration::from_millis(300)).await;

        // List sessions
        let resp = client
            .get(format!("{base}/api/terminal/sessions"))
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success());
        let sessions: Vec<Value> = resp.json().await.unwrap();
        assert!(!sessions.is_empty(), "expected at least one session");

        // Terminate
        let resp = client
            .post(format!(
                "{base}/api/terminal/sessions/{session_id}/terminate"
            ))
            .send()
            .await
            .unwrap();
        assert!(
            resp.status().is_success(),
            "terminate returned {}",
            resp.status()
        );
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["status"], "terminated");
    })
    .await;
}

#[tokio::test]
async fn test_health_and_system_status() {
    with_daemon(|base| async move {
        let client = reqwest::Client::new();

        let resp = client.get(format!("{base}/health")).send().await.unwrap();
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["ok"], true);

        let resp = client
            .get(format!("{base}/api/system/status"))
            .send()
            .await
            .unwrap();
        let body: Value = resp.json().await.unwrap();
        assert!(
            body["connection"]["bindHost"].is_string(),
            "expected bindHost to be a string, got: {}",
            body
        );
    })
    .await;
}

#[tokio::test]
async fn test_session_exit_code_captured() {
    with_daemon(|base| async move {
        let client = reqwest::Client::new();

        // Create a session (starts a default shell)
        let resp = client
            .post(format!("{base}/api/terminal/sessions"))
            .json(&json!({
                "mode": "local",
                "name": "exit-test"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        let body: Value = resp.json().await.unwrap();
        let session_id = body["id"].as_str().unwrap().to_string();

        // Give the shell a moment to start, then send an exit command
        tokio::time::sleep(Duration::from_millis(500)).await;
        let resp = client
            .post(format!("{base}/api/terminal/sessions/{session_id}/input"))
            .json(&json!({ "input": "exit 42" }))
            .send()
            .await
            .unwrap();
        assert!(
            resp.status().is_success(),
            "send_input returned {}",
            resp.status()
        );

        // Wait for the exit monitor to detect the exit (polls every 1s)
        tokio::time::sleep(Duration::from_secs(4)).await;

        // Check session status and exit code
        let resp = client
            .get(format!("{base}/api/terminal/sessions"))
            .send()
            .await
            .unwrap();
        let sessions: Vec<Value> = resp.json().await.unwrap();
        let session = sessions
            .iter()
            .find(|s| s["id"] == session_id)
            .expect("session should still exist in listing");

        assert_eq!(
            session["status"], "error",
            "non-zero exit should set status to error"
        );
        assert_eq!(session["exitCode"], 42, "exit code should be captured");
        assert!(
            session["finishedAt"].is_string(),
            "finishedAt should be set"
        );

        // Check that an outcome was logged
        let resp = client
            .get(format!("{base}/api/outcomes?category=terminal"))
            .send()
            .await
            .unwrap();
        let outcomes: Vec<Value> = resp.json().await.unwrap();
        let exit_outcome = outcomes
            .iter()
            .find(|o| o["action"] == "session_exited")
            .expect("session_exited outcome should exist");

        assert_eq!(exit_outcome["exitCode"], 42);
        assert_eq!(exit_outcome["status"], "failed");
        assert!(exit_outcome["durationSecs"].is_number());
    })
    .await;
}

#[tokio::test]
async fn test_session_clean_exit_code_zero() {
    with_daemon(|base| async move {
        let client = reqwest::Client::new();

        // Create a session (starts a default shell)
        let resp = client
            .post(format!("{base}/api/terminal/sessions"))
            .json(&json!({
                "mode": "local",
                "name": "clean-exit-test"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        let body: Value = resp.json().await.unwrap();
        let session_id = body["id"].as_str().unwrap().to_string();

        // Give the shell a moment to start, then send an exit command
        tokio::time::sleep(Duration::from_millis(500)).await;
        let resp = client
            .post(format!("{base}/api/terminal/sessions/{session_id}/input"))
            .json(&json!({ "input": "exit 0" }))
            .send()
            .await
            .unwrap();
        assert!(
            resp.status().is_success(),
            "send_input returned {}",
            resp.status()
        );

        // Wait for exit detection
        tokio::time::sleep(Duration::from_secs(4)).await;

        let resp = client
            .get(format!("{base}/api/terminal/sessions"))
            .send()
            .await
            .unwrap();
        let sessions: Vec<Value> = resp.json().await.unwrap();
        let session = sessions
            .iter()
            .find(|s| s["id"] == session_id)
            .expect("session should exist");

        assert_eq!(session["status"], "exited");
        assert_eq!(session["exitCode"], 0);

        // Outcome should report success
        let resp = client
            .get(format!("{base}/api/outcomes?category=terminal"))
            .send()
            .await
            .unwrap();
        let outcomes: Vec<Value> = resp.json().await.unwrap();
        let exit_outcome = outcomes
            .iter()
            .find(|o| o["action"] == "session_exited")
            .expect("outcome should exist");

        assert_eq!(exit_outcome["status"], "success");
        assert_eq!(exit_outcome["exitCode"], 0);
    })
    .await;
}

#[tokio::test]
async fn test_hardware_endpoints() {
    with_daemon(|base| async move {
        let client = reqwest::Client::new();

        // GET /api/system/hardware
        let resp = client
            .get(format!("{base}/api/system/hardware"))
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success());
        let body: Value = resp.json().await.unwrap();
        assert!(body["hostname"].is_string());
        assert!(body["cpu"]["cores"].is_number());
        assert!(body["ramGb"].is_number());

        // GET /api/system/hardware/status
        let resp = client
            .get(format!("{base}/api/system/hardware/status"))
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success());
        let body: Value = resp.json().await.unwrap();
        assert!(body["ramTotalGb"].is_number());
        assert!(body["ramUsedGb"].is_number());
    })
    .await;
}
