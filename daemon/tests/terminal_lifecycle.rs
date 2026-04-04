use serde_json::{json, Value};
use std::time::Duration;

const BASE: &str = "http://127.0.0.1:18787";

async fn with_daemon<F, Fut>(test: F)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let temp_db = std::env::temp_dir().join(format!(
        "ghost_protocol_test_{}.db",
        std::process::id()
    ));

    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_ghost-protocol-daemon"))
        .args([
            "--bind-host",
            "127.0.0.1",
            "--bind-port",
            "18787",
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
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Ok(resp) = client.get(format!("{BASE}/health")).send().await {
            if resp.status().is_success() {
                ready = true;
                break;
            }
        }
    }
    assert!(ready, "daemon did not become ready within 3 seconds");

    // Run the test
    test().await;

    // Cleanup
    child.kill().await.ok();
    let _ = std::fs::remove_file(&temp_db);
    // Also remove WAL/SHM files that SQLite may create
    let _ = std::fs::remove_file(temp_db.with_extension("db-wal"));
    let _ = std::fs::remove_file(temp_db.with_extension("db-shm"));
}

#[tokio::test]
async fn test_create_list_terminate_session() {
    with_daemon(|| async {
        let client = reqwest::Client::new();

        // Create session
        let resp = client
            .post(format!("{BASE}/api/terminal/sessions"))
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
            .get(format!("{BASE}/api/terminal/sessions"))
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success());
        let sessions: Vec<Value> = resp.json().await.unwrap();
        assert!(!sessions.is_empty(), "expected at least one session");

        // Terminate
        let resp = client
            .post(format!(
                "{BASE}/api/terminal/sessions/{session_id}/terminate"
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
    with_daemon(|| async {
        let client = reqwest::Client::new();

        let resp = client
            .get(format!("{BASE}/health"))
            .send()
            .await
            .unwrap();
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["ok"], true);

        let resp = client
            .get(format!("{BASE}/api/system/status"))
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
async fn test_hardware_endpoints() {
    with_daemon(|| async {
        let client = reqwest::Client::new();

        // GET /api/system/hardware
        let resp = client
            .get(format!("{BASE}/api/system/hardware"))
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
            .get(format!("{BASE}/api/system/hardware/status"))
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
