use serde_json::json;
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
async fn test_ghost_tools_check_mesh() {
    with_daemon(|base_url| async move {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{base_url}/api/ghost/tools/check_mesh"))
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert!(
            body["result"].is_string(),
            "check_mesh should return a string result"
        );
    })
    .await;
}

#[tokio::test]
async fn test_ghost_tools_list_machines() {
    with_daemon(|base_url| async move {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{base_url}/api/ghost/tools/list_machines"))
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert!(
            body["result"].is_string(),
            "list_machines should return a string result"
        );
    })
    .await;
}

#[tokio::test]
async fn test_ghost_tools_unknown_tool_returns_404() {
    with_daemon(|base_url| async move {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{base_url}/api/ghost/tools/nonexistent"))
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);
    })
    .await;
}
