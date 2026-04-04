# Rust Daemon Rewrite — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Python `ghost_protocol_daemon` with a Rust binary that provides terminal multiplexing and host capabilities over HTTP + WebSocket, with the same API contract so the React frontend works unchanged.

**Architecture:** Single Rust binary using axum + tokio for async HTTP/WS, rusqlite for SQLite persistence, nix crate for PTY management, and tmux for session persistence. The daemon manages terminal sessions (create, stream, resize, terminate) with multi-client broadcasting and chunk replay. Agent-specific features (runs, approvals, artifacts) are dropped — agents run inside terminal sessions.

**Tech Stack:** Rust, axum, tokio, rusqlite, nix, serde/serde_json, tracing, clap, uuid

**Spec:** `docs/superpowers/specs/2026-04-04-rust-daemon-rewrite-design.md`

---

## File Structure

```
daemon/
├── Cargo.toml
├── migrations/
│   └── 001_initial.sql
└── src/
    ├── main.rs              — CLI args (clap), config loading, server startup
    ├── config.rs            — Settings struct, env var loading
    ├── server.rs            — axum router assembly, middleware, startup/shutdown hooks
    ├── terminal/
    │   ├── mod.rs           — pub mod declarations, re-exports
    │   ├── manager.rs       — TerminalManager: owns sessions, create/terminate/recover
    │   ├── session.rs       — ManagedSession: PTY attach/detach, reader task, idle timeout
    │   ├── broadcaster.rs   — SessionBroadcaster: tokio::broadcast, subscriber tracking
    │   └── tmux.rs          — Tmux: CLI wrapper (new_session, attach, kill, has_session, list)
    ├── transport/
    │   ├── mod.rs
    │   ├── http.rs          — REST route handlers
    │   └── ws.rs            — WebSocket handler (subscribe, input, resize, interrupt, terminate)
    ├── store/
    │   ├── mod.rs            — Database pool init, migration runner
    │   ├── sessions.rs       — Terminal session CRUD
    │   └── chunks.rs         — Terminal chunk append + replay queries
    ├── middleware/
    │   ├── mod.rs
    │   ├── tailscale.rs      — CIDR allowlist layer
    │   └── cors.rs           — CORS layer
    └── host/
        ├── mod.rs
        ├── detect.rs         — Tailscale IP, system info
        └── logs.rs           — In-memory ring buffer
```

---

## Task 1: Project Scaffold & Config

**Files:**
- Create: `daemon/Cargo.toml`
- Create: `daemon/src/main.rs`
- Create: `daemon/src/config.rs`

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "ghost-protocol-daemon"
version = "0.1.0"
edition = "2024"

[dependencies]
axum = { version = "0.8", features = ["ws"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rusqlite = { version = "0.32", features = ["bundled"] }
nix = { version = "0.29", features = ["pty", "signal", "term", "process"] }
uuid = { version = "1", features = ["v4"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
clap = { version = "4", features = ["derive"] }
chrono = { version = "0.4", features = ["serde"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["cors"] }
ipnet = "2"

[dev-dependencies]
reqwest = { version = "0.12", features = ["json"] }
tokio-tungstenite = "0.24"
tempfile = "3"
```

- [ ] **Step 2: Create config.rs**

```rust
// daemon/src/config.rs
use std::net::IpAddr;
use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name = "ghost-protocol-daemon", about = "Ghost Protocol terminal daemon")]
pub struct Cli {
    /// Bind address (comma-separated for multiple interfaces)
    #[arg(long, env = "GHOST_PROTOCOL_BIND_HOST", default_value = "127.0.0.1")]
    pub bind_host: String,

    /// Bind port
    #[arg(long, env = "GHOST_PROTOCOL_BIND_PORT", default_value_t = 8787)]
    pub bind_port: u16,

    /// Allowed CIDRs (comma-separated)
    #[arg(
        long,
        env = "GHOST_PROTOCOL_ALLOWED_CIDRS",
        default_value = "100.64.0.0/10,fd7a:115c:a1e0::/48,127.0.0.1/32"
    )]
    pub allowed_cidrs: String,

    /// Database path
    #[arg(long, env = "GHOST_PROTOCOL_DB", default_value = "./data/ghost_protocol.db")]
    pub db_path: PathBuf,

    /// Log directory
    #[arg(long, env = "GHOST_PROTOCOL_LOG_DIR")]
    pub log_dir: Option<PathBuf>,
}

pub struct Settings {
    pub bind_hosts: Vec<String>,
    pub bind_port: u16,
    pub allowed_cidrs: Vec<ipnet::IpNet>,
    pub db_path: PathBuf,
    pub log_dir: PathBuf,
}

impl Settings {
    pub fn from_cli(cli: Cli) -> Result<Self, String> {
        let bind_hosts: Vec<String> = cli
            .bind_host
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();

        let allowed_cidrs: Vec<ipnet::IpNet> = cli
            .allowed_cidrs
            .split(',')
            .map(|s| {
                s.trim()
                    .parse::<ipnet::IpNet>()
                    .map_err(|e| format!("invalid CIDR '{}': {}", s.trim(), e))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let log_dir = cli
            .log_dir
            .unwrap_or_else(|| cli.db_path.parent().unwrap_or(&PathBuf::from(".")).join("logs"));

        Ok(Settings {
            bind_hosts,
            bind_port: cli.bind_port,
            allowed_cidrs,
            db_path: cli.db_path,
            log_dir,
        })
    }

    pub fn is_ip_allowed(&self, ip: IpAddr) -> bool {
        self.allowed_cidrs.iter().any(|net| net.contains(&ip))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_is_ip_allowed_tailscale() {
        let settings = Settings::from_cli(Cli {
            bind_host: "127.0.0.1".into(),
            bind_port: 8787,
            allowed_cidrs: "100.64.0.0/10,127.0.0.1/32".into(),
            db_path: PathBuf::from("./data/test.db"),
            log_dir: None,
        })
        .unwrap();

        assert!(settings.is_ip_allowed(IpAddr::V4(Ipv4Addr::new(100, 100, 1, 1))));
        assert!(settings.is_ip_allowed(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(!settings.is_ip_allowed(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
    }

    #[test]
    fn test_multiple_bind_hosts() {
        let settings = Settings::from_cli(Cli {
            bind_host: "100.64.1.1,127.0.0.1".into(),
            bind_port: 8787,
            allowed_cidrs: "127.0.0.1/32".into(),
            db_path: PathBuf::from("./data/test.db"),
            log_dir: None,
        })
        .unwrap();

        assert_eq!(settings.bind_hosts, vec!["100.64.1.1", "127.0.0.1"]);
    }
}
```

- [ ] **Step 3: Create main.rs (minimal skeleton)**

```rust
// daemon/src/main.rs
mod config;

use clap::Parser;
use config::{Cli, Settings};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ghost_protocol_daemon=info".into()),
        )
        .init();

    let cli = Cli::parse();
    let settings = Settings::from_cli(cli).expect("invalid configuration");

    tracing::info!(
        bind = ?settings.bind_hosts,
        port = settings.bind_port,
        "starting ghost-protocol-daemon"
    );

    // Server startup will be added in Task 9
    tracing::info!("daemon ready");
}
```

- [ ] **Step 4: Verify it compiles and config tests pass**

Run: `cd daemon && cargo test -- config`
Expected: 2 tests pass

Run: `cd daemon && cargo build`
Expected: Compiles successfully

- [ ] **Step 5: Commit**

```bash
git add daemon/
git commit -m "feat(daemon): scaffold Rust daemon with config and CLI parsing"
```

---

## Task 2: Database Schema & Store Foundation

**Files:**
- Create: `daemon/migrations/001_initial.sql`
- Create: `daemon/src/store/mod.rs`
- Create: `daemon/src/store/sessions.rs`
- Create: `daemon/src/store/chunks.rs`

- [ ] **Step 1: Create migration SQL**

```sql
-- daemon/migrations/001_initial.sql
CREATE TABLE IF NOT EXISTS terminal_sessions (
    id TEXT PRIMARY KEY,
    mode TEXT NOT NULL,
    status TEXT NOT NULL,
    name TEXT,
    workdir TEXT NOT NULL,
    command_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT,
    last_chunk_at TEXT,
    pid INTEGER,
    exit_code INTEGER
);

CREATE TABLE IF NOT EXISTS terminal_chunks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    stream TEXT NOT NULL,
    chunk TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(session_id) REFERENCES terminal_sessions(id)
);

CREATE INDEX IF NOT EXISTS idx_chunks_session_id ON terminal_chunks(session_id, id);
```

- [ ] **Step 2: Create store/mod.rs with DB init**

```rust
// daemon/src/store/mod.rs
pub mod sessions;
pub mod chunks;

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

impl Store {
    pub fn open(db_path: &Path) -> Result<Self, rusqlite::Error> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

        let migration = include_str!("../../migrations/001_initial.sql");
        conn.execute_batch(migration)?;

        Ok(Store {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("db lock poisoned")
    }
}

impl Clone for Store {
    fn clone(&self) -> Self {
        Store {
            conn: Arc::clone(&self.conn),
        }
    }
}

#[cfg(test)]
pub fn test_store() -> Store {
    Store::open(Path::new(":memory:")).expect("in-memory db")
}
```

- [ ] **Step 3: Create store/sessions.rs**

```rust
// daemon/src/store/sessions.rs
use serde::Serialize;

use super::Store;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSessionRecord {
    pub id: String,
    pub mode: String,
    pub status: String,
    pub name: Option<String>,
    pub workdir: String,
    pub command: Vec<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub last_chunk_at: Option<String>,
    pub pid: Option<i64>,
    pub exit_code: Option<i32>,
}

impl Store {
    pub fn create_terminal_session(
        &self,
        id: &str,
        mode: &str,
        name: Option<&str>,
        workdir: &str,
        command: &[String],
    ) -> Result<TerminalSessionRecord, rusqlite::Error> {
        let now = chrono::Utc::now().to_rfc3339();
        let command_json = serde_json::to_string(command).unwrap_or_else(|_| "[]".into());

        let conn = self.conn();
        conn.execute(
            "INSERT INTO terminal_sessions (id, mode, status, name, workdir, command_json, created_at)
             VALUES (?1, ?2, 'created', ?3, ?4, ?5, ?6)",
            rusqlite::params![id, mode, name, workdir, command_json, now],
        )?;

        Ok(TerminalSessionRecord {
            id: id.to_string(),
            mode: mode.to_string(),
            status: "created".to_string(),
            name: name.map(|s| s.to_string()),
            workdir: workdir.to_string(),
            command: command.to_vec(),
            created_at: now,
            started_at: None,
            finished_at: None,
            last_chunk_at: None,
            pid: None,
            exit_code: None,
        })
    }

    pub fn update_terminal_session(
        &self,
        session_id: &str,
        status: Option<&str>,
        started_at: Option<&str>,
        finished_at: Option<&str>,
        last_chunk_at: Option<&str>,
        pid: Option<i64>,
        exit_code: Option<i32>,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        if let Some(v) = status {
            conn.execute(
                "UPDATE terminal_sessions SET status = ?1 WHERE id = ?2",
                rusqlite::params![v, session_id],
            )?;
        }
        if let Some(v) = started_at {
            conn.execute(
                "UPDATE terminal_sessions SET started_at = ?1 WHERE id = ?2",
                rusqlite::params![v, session_id],
            )?;
        }
        if let Some(v) = finished_at {
            conn.execute(
                "UPDATE terminal_sessions SET finished_at = ?1 WHERE id = ?2",
                rusqlite::params![v, session_id],
            )?;
        }
        if let Some(v) = last_chunk_at {
            conn.execute(
                "UPDATE terminal_sessions SET last_chunk_at = ?1 WHERE id = ?2",
                rusqlite::params![v, session_id],
            )?;
        }
        if let Some(v) = pid {
            conn.execute(
                "UPDATE terminal_sessions SET pid = ?1 WHERE id = ?2",
                rusqlite::params![v, session_id],
            )?;
        }
        if let Some(v) = exit_code {
            conn.execute(
                "UPDATE terminal_sessions SET exit_code = ?1 WHERE id = ?2",
                rusqlite::params![v, session_id],
            )?;
        }
        Ok(())
    }

    pub fn get_terminal_session(
        &self,
        session_id: &str,
    ) -> Result<Option<TerminalSessionRecord>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, mode, status, name, workdir, command_json, created_at,
                    started_at, finished_at, last_chunk_at, pid, exit_code
             FROM terminal_sessions WHERE id = ?1",
        )?;

        let mut rows = stmt.query(rusqlite::params![session_id])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_session(row)?)),
            None => Ok(None),
        }
    }

    pub fn list_terminal_sessions(&self) -> Result<Vec<TerminalSessionRecord>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, mode, status, name, workdir, command_json, created_at,
                    started_at, finished_at, last_chunk_at, pid, exit_code
             FROM terminal_sessions ORDER BY created_at DESC, id ASC",
        )?;

        let rows = stmt.query_map([], |row| row_to_session(row))?;
        rows.collect()
    }

    pub fn terminate_incomplete_sessions(&self) -> Result<usize, rusqlite::Error> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn();
        let count = conn.execute(
            "UPDATE terminal_sessions SET status = 'terminated', finished_at = ?1
             WHERE status IN ('created', 'running')",
            rusqlite::params![now],
        )?;
        Ok(count)
    }
}

fn row_to_session(row: &rusqlite::Row) -> Result<TerminalSessionRecord, rusqlite::Error> {
    let command_json: String = row.get(5)?;
    let command: Vec<String> =
        serde_json::from_str(&command_json).unwrap_or_default();

    Ok(TerminalSessionRecord {
        id: row.get(0)?,
        mode: row.get(1)?,
        status: row.get(2)?,
        name: row.get(3)?,
        workdir: row.get(4)?,
        command,
        created_at: row.get(6)?,
        started_at: row.get(7)?,
        finished_at: row.get(8)?,
        last_chunk_at: row.get(9)?,
        pid: row.get(10)?,
        exit_code: row.get(11)?,
    })
}

#[cfg(test)]
mod tests {
    use super::super::test_store;

    #[test]
    fn test_create_and_get_session() {
        let store = test_store();
        let session = store
            .create_terminal_session("s1", "project", Some("test"), "/tmp", &["bash".into()])
            .unwrap();

        assert_eq!(session.id, "s1");
        assert_eq!(session.status, "created");
        assert_eq!(session.mode, "project");

        let fetched = store.get_terminal_session("s1").unwrap().unwrap();
        assert_eq!(fetched.id, "s1");
        assert_eq!(fetched.name, Some("test".into()));
    }

    #[test]
    fn test_update_session_status() {
        let store = test_store();
        store
            .create_terminal_session("s1", "agent", None, "/tmp", &["bash".into()])
            .unwrap();

        store
            .update_terminal_session("s1", Some("running"), Some("2026-01-01T00:00:00Z"), None, None, Some(1234), None)
            .unwrap();

        let session = store.get_terminal_session("s1").unwrap().unwrap();
        assert_eq!(session.status, "running");
        assert_eq!(session.pid, Some(1234));
    }

    #[test]
    fn test_list_sessions() {
        let store = test_store();
        store.create_terminal_session("s1", "agent", None, "/tmp", &["bash".into()]).unwrap();
        store.create_terminal_session("s2", "project", None, "/home", &["zsh".into()]).unwrap();

        let sessions = store.list_terminal_sessions().unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn test_terminate_incomplete() {
        let store = test_store();
        store.create_terminal_session("s1", "agent", None, "/tmp", &["bash".into()]).unwrap();
        store.update_terminal_session("s1", Some("running"), None, None, None, None, None).unwrap();
        store.create_terminal_session("s2", "project", None, "/tmp", &["bash".into()]).unwrap();

        let count = store.terminate_incomplete_sessions().unwrap();
        assert_eq!(count, 2);

        let s1 = store.get_terminal_session("s1").unwrap().unwrap();
        assert_eq!(s1.status, "terminated");
        assert!(s1.finished_at.is_some());
    }
}
```

- [ ] **Step 4: Create store/chunks.rs**

```rust
// daemon/src/store/chunks.rs
use serde::Serialize;

use super::Store;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalChunkRecord {
    pub id: i64,
    pub session_id: String,
    pub stream: String,
    pub chunk: String,
    pub created_at: String,
}

impl Store {
    pub fn append_terminal_chunk(
        &self,
        session_id: &str,
        stream: &str,
        chunk: &str,
    ) -> Result<TerminalChunkRecord, rusqlite::Error> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn();

        conn.execute(
            "INSERT INTO terminal_chunks (session_id, stream, chunk, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![session_id, stream, chunk, now],
        )?;

        let id = conn.last_insert_rowid();

        // Update last_chunk_at on the session
        conn.execute(
            "UPDATE terminal_sessions SET last_chunk_at = ?1 WHERE id = ?2",
            rusqlite::params![now, session_id],
        )?;

        Ok(TerminalChunkRecord {
            id,
            session_id: session_id.to_string(),
            stream: stream.to_string(),
            chunk: chunk.to_string(),
            created_at: now,
        })
    }

    pub fn list_terminal_chunks(
        &self,
        session_id: &str,
        after_chunk_id: i64,
        limit: i64,
    ) -> Result<Vec<TerminalChunkRecord>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, stream, chunk, created_at
             FROM terminal_chunks
             WHERE session_id = ?1 AND id > ?2
             ORDER BY id ASC
             LIMIT ?3",
        )?;

        let rows = stmt.query_map(rusqlite::params![session_id, after_chunk_id, limit], |row| {
            Ok(TerminalChunkRecord {
                id: row.get(0)?,
                session_id: row.get(1)?,
                stream: row.get(2)?,
                chunk: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;

        rows.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_store;

    #[test]
    fn test_append_and_list_chunks() {
        let store = test_store();
        store
            .create_terminal_session("s1", "agent", None, "/tmp", &["bash".into()])
            .unwrap();

        let c1 = store.append_terminal_chunk("s1", "stdout", "hello ").unwrap();
        let c2 = store.append_terminal_chunk("s1", "stdout", "world\n").unwrap();

        assert_eq!(c1.id, 1);
        assert_eq!(c2.id, 2);

        let chunks = store.list_terminal_chunks("s1", 0, 500).unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].chunk, "hello ");
        assert_eq!(chunks[1].chunk, "world\n");
    }

    #[test]
    fn test_list_chunks_after_id() {
        let store = test_store();
        store
            .create_terminal_session("s1", "agent", None, "/tmp", &["bash".into()])
            .unwrap();

        store.append_terminal_chunk("s1", "stdout", "a").unwrap();
        store.append_terminal_chunk("s1", "stdout", "b").unwrap();
        store.append_terminal_chunk("s1", "stdout", "c").unwrap();

        let chunks = store.list_terminal_chunks("s1", 1, 500).unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].chunk, "b");
        assert_eq!(chunks[1].chunk, "c");
    }

    #[test]
    fn test_append_updates_last_chunk_at() {
        let store = test_store();
        store
            .create_terminal_session("s1", "agent", None, "/tmp", &["bash".into()])
            .unwrap();

        let before = store.get_terminal_session("s1").unwrap().unwrap();
        assert!(before.last_chunk_at.is_none());

        store.append_terminal_chunk("s1", "stdout", "data").unwrap();

        let after = store.get_terminal_session("s1").unwrap().unwrap();
        assert!(after.last_chunk_at.is_some());
    }
}
```

- [ ] **Step 5: Wire modules into main.rs and verify tests pass**

Update `daemon/src/main.rs` to add module declarations:

```rust
// Add at top of main.rs
mod config;
mod store;
```

Run: `cd daemon && cargo test -- store`
Expected: All 7 store tests pass

- [ ] **Step 6: Commit**

```bash
git add daemon/
git commit -m "feat(daemon): add SQLite store with session and chunk CRUD"
```

---

## Task 3: Tmux Wrapper

**Files:**
- Create: `daemon/src/terminal/mod.rs`
- Create: `daemon/src/terminal/tmux.rs`

- [ ] **Step 1: Create terminal/mod.rs**

```rust
// daemon/src/terminal/mod.rs
pub mod tmux;
pub mod broadcaster;
pub mod session;
pub mod manager;
```

- [ ] **Step 2: Create terminal/tmux.rs**

```rust
// daemon/src/terminal/tmux.rs
use std::process::Command;

use tracing::{debug, warn};

const SESSION_PREFIX: &str = "ghost-";

pub fn session_name(session_id: &str) -> String {
    format!("{}{}", SESSION_PREFIX, session_id.replace('-', ""))
}

pub fn new_session(session_id: &str, workdir: &str, shell: &str) -> Result<(), String> {
    let name = session_name(session_id);
    let output = Command::new("tmux")
        .args([
            "new-session",
            "-d",
            "-s", &name,
            "-x", "120",
            "-y", "32",
            "-c", workdir,
            shell,
        ])
        .output()
        .map_err(|e| format!("failed to spawn tmux: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("tmux new-session failed: {}", stderr));
    }

    // Configure session: disable status bar, pane borders, mouse
    let _ = Command::new("tmux")
        .args([
            "set-option", "-t", &name, "status", "off", ";",
            "set-option", "-t", &name, "pane-border-status", "off", ";",
            "set-option", "-t", &name, "mouse", "off",
        ])
        .output();

    debug!(session_id, tmux_name = %name, "created tmux session");
    Ok(())
}

pub fn attach_command(session_id: &str) -> Vec<String> {
    vec![
        "tmux".into(),
        "attach-session".into(),
        "-t".into(),
        session_name(session_id),
    ]
}

pub fn has_session(session_id: &str) -> bool {
    let name = session_name(session_id);
    Command::new("tmux")
        .args(["has-session", "-t", &name])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn kill_session(session_id: &str) -> bool {
    let name = session_name(session_id);
    let result = Command::new("tmux")
        .args(["kill-session", "-t", &name])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if result {
        debug!(session_id, "killed tmux session");
    } else {
        warn!(session_id, "tmux kill-session failed or session not found");
    }
    result
}

/// List all ghost-protocol tmux sessions. Returns session names (without prefix).
pub fn list_ghost_sessions() -> Vec<String> {
    let output = Command::new("tmux")
        .args(["list-sessions", "-F", "#{session_name}"])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter(|line| line.starts_with(SESSION_PREFIX))
                .map(|line| line.to_string())
                .collect()
        }
        _ => Vec::new(),
    }
}

pub fn is_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_name_format() {
        assert_eq!(session_name("abc-def-123"), "ghost-abcdef123");
        assert_eq!(session_name("simple"), "ghost-simple");
    }

    #[test]
    fn test_attach_command() {
        let cmd = attach_command("test-id");
        assert_eq!(cmd, vec!["tmux", "attach-session", "-t", "ghost-testid"]);
    }
}
```

- [ ] **Step 3: Wire terminal module into main.rs**

Add to `daemon/src/main.rs`:
```rust
mod terminal;
```

- [ ] **Step 4: Run tests**

Run: `cd daemon && cargo test -- tmux`
Expected: 2 tests pass

- [ ] **Step 5: Commit**

```bash
git add daemon/src/terminal/
git commit -m "feat(daemon): add tmux CLI wrapper"
```

---

## Task 4: Session Broadcaster

**Files:**
- Create: `daemon/src/terminal/broadcaster.rs`

- [ ] **Step 1: Create broadcaster.rs**

```rust
// daemon/src/terminal/broadcaster.rs
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::sync::broadcast;

use crate::store::chunks::TerminalChunkRecord;

const BROADCAST_CAPACITY: usize = 256;

pub struct SessionBroadcaster {
    sender: broadcast::Sender<TerminalChunkRecord>,
    subscriber_count: AtomicUsize,
}

impl SessionBroadcaster {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(BROADCAST_CAPACITY);
        SessionBroadcaster {
            sender,
            subscriber_count: AtomicUsize::new(0),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TerminalChunkRecord> {
        self.subscriber_count.fetch_add(1, Ordering::Relaxed);
        self.sender.subscribe()
    }

    pub fn unsubscribe(&self) {
        self.subscriber_count.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn send(&self, chunk: TerminalChunkRecord) {
        // Ignore send errors — means no active receivers
        let _ = self.sender.send(chunk);
    }

    pub fn subscriber_count(&self) -> usize {
        self.subscriber_count.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_broadcast_to_multiple_subscribers() {
        let broadcaster = SessionBroadcaster::new();
        let mut rx1 = broadcaster.subscribe();
        let mut rx2 = broadcaster.subscribe();
        assert_eq!(broadcaster.subscriber_count(), 2);

        let chunk = TerminalChunkRecord {
            id: 1,
            session_id: "s1".into(),
            stream: "stdout".into(),
            chunk: "hello".into(),
            created_at: "now".into(),
        };

        broadcaster.send(chunk.clone());

        let received1 = rx1.recv().await.unwrap();
        let received2 = rx2.recv().await.unwrap();
        assert_eq!(received1.chunk, "hello");
        assert_eq!(received2.chunk, "hello");
    }

    #[test]
    fn test_send_with_no_subscribers() {
        let broadcaster = SessionBroadcaster::new();
        let chunk = TerminalChunkRecord {
            id: 1,
            session_id: "s1".into(),
            stream: "stdout".into(),
            chunk: "orphan".into(),
            created_at: "now".into(),
        };
        // Should not panic
        broadcaster.send(chunk);
    }

    #[test]
    fn test_subscriber_count_tracking() {
        let broadcaster = SessionBroadcaster::new();
        assert_eq!(broadcaster.subscriber_count(), 0);

        let _rx1 = broadcaster.subscribe();
        assert_eq!(broadcaster.subscriber_count(), 1);

        let _rx2 = broadcaster.subscribe();
        assert_eq!(broadcaster.subscriber_count(), 2);

        broadcaster.unsubscribe();
        assert_eq!(broadcaster.subscriber_count(), 1);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cd daemon && cargo test -- broadcaster`
Expected: 3 tests pass

- [ ] **Step 3: Commit**

```bash
git add daemon/src/terminal/broadcaster.rs
git commit -m "feat(daemon): add session broadcaster for multi-client sync"
```

---

## Task 5: Terminal Session Manager

**Files:**
- Create: `daemon/src/terminal/session.rs`
- Create: `daemon/src/terminal/manager.rs`

This is the most complex task — it handles PTY lifecycle, reader tasks, idle timeouts, and recovery.

- [ ] **Step 1: Create session.rs — ManagedSession struct and PTY attach logic**

```rust
// daemon/src/terminal/session.rs
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::process::Stdio;

use nix::pty::openpty;
use nix::sys::termios;
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tracing::{debug, warn};

use super::broadcaster::SessionBroadcaster;
use super::tmux;
use crate::store::Store;

pub struct ManagedSession {
    pub session_id: String,
    pub master_fd: OwnedFd,
    pub attach_process: Child,
    pub broadcaster: SessionBroadcaster,
    reader_shutdown: Option<mpsc::Sender<()>>,
}

impl ManagedSession {
    /// Spawn a tmux attach process connected via a PTY pair.
    /// Starts a reader task that reads from the PTY master and broadcasts chunks.
    pub async fn attach(
        session_id: String,
        store: Store,
    ) -> Result<Self, String> {
        // Open PTY pair
        let pty = openpty(None, None).map_err(|e| format!("openpty failed: {}", e))?;

        // Set initial window size
        set_window_size(pty.master.as_raw_fd(), 120, 32);

        let cmd_parts = tmux::attach_command(&session_id);

        // Spawn tmux attach on the slave FD
        let slave_raw = pty.slave.as_raw_fd();
        let attach_process = unsafe {
            Command::new(&cmd_parts[0])
                .args(&cmd_parts[1..])
                .env("TERM", "xterm-256color")
                .env("COLORTERM", "truecolor")
                .env("GHOST_PROTOCOL_REMOTE_SESSION", "1")
                .stdin(Stdio::from_raw_fd(slave_raw))
                .stdout(Stdio::from_raw_fd(slave_raw))
                .stderr(Stdio::from_raw_fd(slave_raw))
                .process_group(0)
                .kill_on_drop(true)
                .spawn()
                .map_err(|e| format!("failed to spawn tmux attach: {}", e))?
        };

        // Drop slave — we only need the master side
        drop(pty.slave);

        let broadcaster = SessionBroadcaster::new();

        // Start reader task
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>(1);
        let reader_fd = pty.master.as_raw_fd();
        let reader_store = store.clone();
        let reader_broadcaster = broadcaster.subscribe(); // We'll use the sender directly
        let reader_session_id = session_id.clone();

        // Clone broadcaster's sender for the reader task
        drop(reader_broadcaster); // We don't need the receiver
        let reader_broadcast_ref = &broadcaster as *const SessionBroadcaster;
        // Safety: broadcaster lives as long as ManagedSession
        let broadcaster_ptr = reader_broadcast_ref as usize;

        // Actually, let's use a channel-based approach instead of raw pointers
        let (chunk_tx, mut chunk_rx) = mpsc::unbounded_channel::<String>();

        // Spawn blocking reader thread
        let read_fd = pty.master.as_raw_fd();
        let reader_sid = session_id.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 16384];
            loop {
                let n = match nix::unistd::read(read_fd, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(_) => break,
                };
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                if chunk_tx.send(text).is_err() {
                    break;
                }
            }
            debug!(session_id = %reader_sid, "reader thread exited");
        });

        // Spawn async task to persist chunks and broadcast
        let persist_store = store.clone();
        let persist_session_id = session_id.clone();
        // We need to return the broadcaster in the struct, so we use a channel to bridge
        // The manager will call start_broadcast_task after construction
        let mut managed = ManagedSession {
            session_id,
            master_fd: pty.master,
            attach_process,
            broadcaster,
            reader_shutdown: Some(shutdown_tx),
        };

        // Start the persist+broadcast task
        let sid = managed.session_id.clone();
        let store_clone = store;
        // We need a reference to the broadcaster, but it's owned by the struct.
        // Use an unbounded channel to decouple: reader → chunk_rx → persist+broadcast task
        tokio::spawn(async move {
            while let Some(text) = chunk_rx.recv().await {
                match store_clone.append_terminal_chunk(&sid, "stdout", &text) {
                    Ok(record) => {
                        // Broadcasting is handled by the manager via a separate mechanism
                        // For now, we'll handle this in the manager
                    }
                    Err(e) => {
                        warn!(session_id = %sid, error = %e, "failed to persist chunk");
                    }
                }
            }
        });

        Ok(managed)
    }

    pub fn write_input(&self, data: &[u8]) -> Result<(), String> {
        nix::unistd::write(&self.master_fd, data)
            .map_err(|e| format!("write to PTY failed: {}", e))?;
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        set_window_size(self.master_fd.as_raw_fd(), cols, rows);
    }

    pub fn interrupt(&self) -> Result<(), String> {
        // Send Ctrl+C
        self.write_input(&[0x03])
    }
}

impl Drop for ManagedSession {
    fn drop(&mut self) {
        debug!(session_id = %self.session_id, "dropping managed session");
    }
}

fn set_window_size(fd: i32, cols: u16, rows: u16) {
    let ws = nix::pty::Winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // TIOCSWINSZ
    unsafe {
        libc::ioctl(fd, libc::TIOCSWINSZ, &ws as *const _);
    }
}
```

**Note:** The session.rs above has a design issue — the reader thread and broadcaster need to be connected differently. Let me revise in Step 2.

- [ ] **Step 2: Revise session.rs with clean reader→broadcast architecture**

Replace the full `daemon/src/terminal/session.rs` with:

```rust
// daemon/src/terminal/session.rs
use std::os::fd::{AsRawFd, OwnedFd};
use std::process::Stdio;
use std::sync::Arc;

use nix::pty::openpty;
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tracing::{debug, warn};

use super::broadcaster::SessionBroadcaster;
use super::tmux;
use crate::store::Store;

pub struct ManagedSession {
    pub session_id: String,
    pub master_fd: OwnedFd,
    pub attach_process: Child,
    pub broadcaster: Arc<SessionBroadcaster>,
}

/// Spawn a tmux attach, start a reader thread that persists chunks and broadcasts them.
/// Returns the ManagedSession.
pub fn spawn_attach(
    session_id: String,
    store: Store,
    broadcaster: Arc<SessionBroadcaster>,
) -> Result<ManagedSession, String> {
    let pty = openpty(None, None).map_err(|e| format!("openpty failed: {e}"))?;
    set_window_size(pty.master.as_raw_fd(), 120, 32);

    let cmd_parts = tmux::attach_command(&session_id);
    let slave_fd = pty.slave.as_raw_fd();

    // Safety: slave_fd is valid and owned by pty.slave which we drop after spawn
    let attach_process = unsafe {
        Command::new(&cmd_parts[0])
            .args(&cmd_parts[1..])
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .env("GHOST_PROTOCOL_REMOTE_SESSION", "1")
            .stdin(Stdio::from_raw_fd(slave_fd))
            .stdout(Stdio::from_raw_fd(slave_fd))
            .stderr(Stdio::from_raw_fd(slave_fd))
            .process_group(0)
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("failed to spawn tmux attach: {e}"))?
    };
    drop(pty.slave);

    // Reader thread: reads PTY master → sends text over channel
    let read_fd = pty.master.as_raw_fd();
    let (chunk_tx, mut chunk_rx) = mpsc::unbounded_channel::<String>();
    let reader_sid = session_id.clone();

    std::thread::spawn(move || {
        let mut buf = [0u8; 16384];
        loop {
            match nix::unistd::read(read_fd, &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let text = String::from_utf8_lossy(&buf[..n]).to_string();
                    if chunk_tx.send(text).is_err() {
                        break;
                    }
                }
            }
        }
        debug!(session_id = %reader_sid, "PTY reader thread exited");
    });

    // Async task: persists chunks to DB and broadcasts to subscribers
    let persist_store = store;
    let persist_sid = session_id.clone();
    let persist_broadcaster = Arc::clone(&broadcaster);

    tokio::spawn(async move {
        while let Some(text) = chunk_rx.recv().await {
            match persist_store.append_terminal_chunk(&persist_sid, "stdout", &text) {
                Ok(record) => persist_broadcaster.send(record),
                Err(e) => warn!(session_id = %persist_sid, error = %e, "failed to persist chunk"),
            }
        }
        debug!(session_id = %persist_sid, "chunk persist task exited");
    });

    Ok(ManagedSession {
        session_id,
        master_fd: pty.master,
        attach_process,
        broadcaster,
    })
}

impl ManagedSession {
    pub fn write_input(&self, data: &[u8]) -> Result<(), String> {
        nix::unistd::write(&self.master_fd, data)
            .map_err(|e| format!("PTY write failed: {e}"))?;
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        set_window_size(self.master_fd.as_raw_fd(), cols, rows);
    }

    pub fn interrupt(&self) -> Result<(), String> {
        self.write_input(&[0x03])
    }
}

fn set_window_size(fd: i32, cols: u16, rows: u16) {
    let ws = nix::pty::Winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    unsafe {
        libc::ioctl(fd, libc::TIOCSWINSZ, &ws as *const _);
    }
}
```

- [ ] **Step 3: Create manager.rs — TerminalManager**

```rust
// daemon/src/terminal/manager.rs
use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use super::broadcaster::SessionBroadcaster;
use super::session::{self, ManagedSession};
use super::tmux;
use crate::store::Store;
use crate::store::sessions::TerminalSessionRecord;

const IDLE_TIMEOUT_SECS: u64 = 120;

pub struct TerminalManager {
    sessions: Arc<Mutex<HashMap<String, ManagedSession>>>,
    broadcasters: Arc<Mutex<HashMap<String, Arc<SessionBroadcaster>>>>,
    store: Store,
}

impl TerminalManager {
    pub fn new(store: Store) -> Self {
        TerminalManager {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            broadcasters: Arc::new(Mutex::new(HashMap::new())),
            store,
        }
    }

    /// Create a new terminal session backed by tmux.
    pub async fn create_session(
        &self,
        mode: &str,
        name: Option<&str>,
        workdir: Option<&str>,
    ) -> Result<TerminalSessionRecord, String> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let workdir = workdir.unwrap_or("/tmp");
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        let command = vec![shell.clone()];

        // Create DB record
        let record = self
            .store
            .create_terminal_session(&session_id, mode, name, workdir, &command)
            .map_err(|e| format!("DB error: {e}"))?;

        // Create tmux session
        tmux::new_session(&session_id, workdir, &shell)?;

        // Update status to running
        let now = chrono::Utc::now().to_rfc3339();
        self.store
            .update_terminal_session(&session_id, Some("running"), Some(&now), None, None, None, None)
            .map_err(|e| format!("DB error: {e}"))?;

        // Create broadcaster for this session
        let broadcaster = Arc::new(SessionBroadcaster::new());
        self.broadcasters.lock().await.insert(session_id.clone(), Arc::clone(&broadcaster));

        // Attach PTY
        let managed = session::spawn_attach(session_id.clone(), self.store.clone(), broadcaster)?;
        self.sessions.lock().await.insert(session_id.clone(), managed);

        // Re-fetch to get updated status
        self.store
            .get_terminal_session(&session_id)
            .map_err(|e| format!("DB error: {e}"))?
            .ok_or_else(|| "session vanished after creation".into())
    }

    /// Ensure a session has an active PTY attachment. Re-attaches if needed.
    pub async fn ensure_attached(&self, session_id: &str) -> Result<(), String> {
        let sessions = self.sessions.lock().await;
        if sessions.contains_key(session_id) {
            return Ok(());
        }
        drop(sessions);

        // Check if tmux session still exists
        if !tmux::has_session(session_id) {
            return Err(format!("tmux session for {} not found", session_id));
        }

        // Re-attach
        let broadcaster = {
            let mut broadcasters = self.broadcasters.lock().await;
            broadcasters
                .entry(session_id.to_string())
                .or_insert_with(|| Arc::new(SessionBroadcaster::new()))
                .clone()
        };

        let managed = session::spawn_attach(session_id.to_string(), self.store.clone(), broadcaster)?;
        self.sessions.lock().await.insert(session_id.to_string(), managed);

        debug!(session_id, "re-attached to existing tmux session");
        Ok(())
    }

    /// Send input to a session's PTY.
    pub async fn send_input(
        &self,
        session_id: &str,
        data: &str,
        append_newline: bool,
    ) -> Result<(), String> {
        self.ensure_attached(session_id).await?;

        let sessions = self.sessions.lock().await;
        let managed = sessions
            .get(session_id)
            .ok_or_else(|| format!("session {} not found", session_id))?;

        let mut raw = data.to_string();
        if append_newline {
            raw.push('\n');
        }
        managed.write_input(raw.as_bytes())
    }

    /// Resize a session's PTY.
    pub async fn resize_session(
        &self,
        session_id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<TerminalSessionRecord, String> {
        self.ensure_attached(session_id).await?;

        let sessions = self.sessions.lock().await;
        if let Some(managed) = sessions.get(session_id) {
            managed.resize(cols, rows);
        }
        drop(sessions);

        self.store
            .get_terminal_session(session_id)
            .map_err(|e| format!("DB error: {e}"))?
            .ok_or_else(|| format!("session {} not found", session_id))
    }

    /// Send Ctrl+C to a session.
    pub async fn interrupt_session(&self, session_id: &str) -> Result<(), String> {
        let sessions = self.sessions.lock().await;
        if let Some(managed) = sessions.get(session_id) {
            managed.interrupt()?;
        }
        Ok(())
    }

    /// Called when a WebSocket subscriber disconnects. If no subscribers remain,
    /// starts an idle timer. After 120s with no new subscribers, detaches PTY
    /// (but keeps tmux alive).
    pub async fn on_unsubscribe(&self, session_id: &str) {
        let broadcaster = self.broadcasters.lock().await.get(session_id).cloned();
        if let Some(bc) = broadcaster {
            if bc.subscriber_count() == 0 {
                let mgr = self.clone();
                let sid = session_id.to_string();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(IDLE_TIMEOUT_SECS)).await;
                    // Re-check subscriber count after timeout
                    if let Some(bc) = mgr.broadcasters.lock().await.get(&sid) {
                        if bc.subscriber_count() == 0 {
                            // Detach PTY but keep tmux alive
                            mgr.sessions.lock().await.remove(&sid);
                            debug!(session_id = %sid, "detached idle session (tmux still alive)");
                        }
                    }
                });
            }
        }
    }

    /// Terminate a session: kill tmux, clean up PTY, update DB.
    pub async fn terminate_session(
        &self,
        session_id: &str,
    ) -> Result<TerminalSessionRecord, String> {
        // Remove from active sessions (drops PTY fd, kills attach process)
        self.sessions.lock().await.remove(session_id);
        self.broadcasters.lock().await.remove(session_id);

        // Kill tmux session
        tmux::kill_session(session_id);

        // Update DB
        let now = chrono::Utc::now().to_rfc3339();
        self.store
            .update_terminal_session(session_id, Some("terminated"), None, Some(&now), None, None, None)
            .map_err(|e| format!("DB error: {e}"))?;

        self.store
            .get_terminal_session(session_id)
            .map_err(|e| format!("DB error: {e}"))?
            .ok_or_else(|| format!("session {} not found", session_id))
    }

    /// Get the broadcaster for a session (for WebSocket subscriptions).
    pub async fn get_broadcaster(&self, session_id: &str) -> Option<Arc<SessionBroadcaster>> {
        self.broadcasters.lock().await.get(session_id).cloned()
    }

    /// Recover existing tmux sessions on startup.
    pub async fn recover(&self) {
        if !tmux::is_available() {
            warn!("tmux not available, marking all incomplete sessions as terminated");
            if let Err(e) = self.store.terminate_incomplete_sessions() {
                warn!(error = %e, "failed to terminate incomplete sessions");
            }
            return;
        }

        let live_tmux = tmux::list_ghost_sessions();
        let db_sessions = self.store.list_terminal_sessions().unwrap_or_default();

        // Find DB sessions that have a matching live tmux session
        for record in &db_sessions {
            if record.status == "terminated" || record.status == "error" {
                continue;
            }
            let expected_name = tmux::session_name(&record.id);
            if live_tmux.contains(&expected_name) {
                info!(session_id = %record.id, "recovered tmux session");
                // Create broadcaster — actual PTY attach happens on first subscribe
                let broadcaster = Arc::new(SessionBroadcaster::new());
                self.broadcasters.lock().await.insert(record.id.clone(), broadcaster);
            } else {
                // Tmux session gone, mark as terminated
                let now = chrono::Utc::now().to_rfc3339();
                let _ = self.store.update_terminal_session(
                    &record.id, Some("terminated"), None, Some(&now), None, None, None,
                );
                info!(session_id = %record.id, "marked orphaned DB session as terminated");
            }
        }

        // Kill tmux sessions that have no DB record
        let db_ids: std::collections::HashSet<String> = db_sessions
            .iter()
            .map(|r| tmux::session_name(&r.id))
            .collect();

        for tmux_name in &live_tmux {
            if !db_ids.contains(tmux_name) {
                let _ = std::process::Command::new("tmux")
                    .args(["kill-session", "-t", tmux_name])
                    .output();
                info!(tmux_session = %tmux_name, "killed orphaned tmux session");
            }
        }
    }
}

impl Clone for TerminalManager {
    fn clone(&self) -> Self {
        TerminalManager {
            sessions: Arc::clone(&self.sessions),
            broadcasters: Arc::clone(&self.broadcasters),
            store: self.store.clone(),
        }
    }
}
```

- [ ] **Step 4: Add libc dependency for ioctl in Cargo.toml**

Add to `[dependencies]` in `daemon/Cargo.toml`:
```toml
libc = "0.2"
```

- [ ] **Step 5: Verify compilation**

Run: `cd daemon && cargo check`
Expected: Compiles with no errors (there may be warnings about unused imports — those are fine, they'll be used by later tasks)

- [ ] **Step 6: Commit**

```bash
git add daemon/
git commit -m "feat(daemon): add terminal session manager with PTY attach and recovery"
```

---

## Task 6: In-Memory Log Buffer

**Files:**
- Create: `daemon/src/host/mod.rs`
- Create: `daemon/src/host/logs.rs`
- Create: `daemon/src/host/detect.rs`

- [ ] **Step 1: Create host/mod.rs**

```rust
// daemon/src/host/mod.rs
pub mod logs;
pub mod detect;
```

- [ ] **Step 2: Create host/logs.rs — ring buffer for log entries**

```rust
// daemon/src/host/logs.rs
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde::Serialize;

const DEFAULT_CAPACITY: usize = 1000;

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub level: String,
    pub message: String,
    pub timestamp: String,
    pub source: String,
}

pub struct LogBuffer {
    entries: Arc<Mutex<VecDeque<LogEntry>>>,
    capacity: usize,
}

impl LogBuffer {
    pub fn new() -> Self {
        LogBuffer {
            entries: Arc::new(Mutex::new(VecDeque::with_capacity(DEFAULT_CAPACITY))),
            capacity: DEFAULT_CAPACITY,
        }
    }

    pub fn push(&self, entry: LogEntry) {
        let mut entries = self.entries.lock().unwrap();
        if entries.len() >= self.capacity {
            entries.pop_front();
        }
        entries.push_back(entry);
    }

    pub fn entries(&self, limit: usize, level: Option<&str>) -> Vec<LogEntry> {
        let entries = self.entries.lock().unwrap();
        entries
            .iter()
            .filter(|e| match level {
                Some(l) => e.level.eq_ignore_ascii_case(l),
                None => true,
            })
            .rev()
            .take(limit)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }
}

impl Clone for LogBuffer {
    fn clone(&self) -> Self {
        LogBuffer {
            entries: Arc::clone(&self.entries),
            capacity: self.capacity,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_and_retrieve() {
        let buf = LogBuffer::new();
        buf.push(LogEntry {
            level: "INFO".into(),
            message: "hello".into(),
            timestamp: "t1".into(),
            source: "daemon".into(),
        });
        buf.push(LogEntry {
            level: "ERROR".into(),
            message: "oops".into(),
            timestamp: "t2".into(),
            source: "daemon".into(),
        });

        let all = buf.entries(200, None);
        assert_eq!(all.len(), 2);

        let errors = buf.entries(200, Some("ERROR"));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "oops");
    }

    #[test]
    fn test_capacity_eviction() {
        let buf = LogBuffer {
            entries: Arc::new(Mutex::new(VecDeque::with_capacity(3))),
            capacity: 3,
        };

        for i in 0..5 {
            buf.push(LogEntry {
                level: "INFO".into(),
                message: format!("msg{}", i),
                timestamp: format!("t{}", i),
                source: "daemon".into(),
            });
        }

        let entries = buf.entries(10, None);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].message, "msg2");
        assert_eq!(entries[2].message, "msg4");
    }
}
```

- [ ] **Step 3: Create host/detect.rs — system detection**

```rust
// daemon/src/host/detect.rs
use std::process::Command;

use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub tailscale_ip: Option<String>,
    pub hostname: String,
    pub ssh_available: bool,
}

pub fn get_system_info() -> SystemInfo {
    SystemInfo {
        tailscale_ip: get_tailscale_ip(),
        hostname: hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".into()),
        ssh_available: is_ssh_available(),
    }
}

pub fn get_tailscale_ip() -> Option<String> {
    let output = Command::new("tailscale")
        .args(["ip", "-4"])
        .output()
        .ok()?;

    if output.status.success() {
        let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !ip.is_empty() { Some(ip) } else { None }
    } else {
        None
    }
}

fn is_ssh_available() -> bool {
    Command::new("ssh")
        .arg("-V")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
```

- [ ] **Step 4: Add hostname crate to Cargo.toml**

Add to `[dependencies]`:
```toml
hostname = "0.4"
```

- [ ] **Step 5: Wire host module into main.rs**

Add to `daemon/src/main.rs`:
```rust
mod host;
```

- [ ] **Step 6: Run tests**

Run: `cd daemon && cargo test -- logs`
Expected: 2 tests pass

- [ ] **Step 7: Commit**

```bash
git add daemon/
git commit -m "feat(daemon): add in-memory log buffer and system detection"
```

---

## Task 7: Middleware — CORS & Tailscale CIDR

**Files:**
- Create: `daemon/src/middleware/mod.rs`
- Create: `daemon/src/middleware/cors.rs`
- Create: `daemon/src/middleware/tailscale.rs`

- [ ] **Step 1: Create middleware/mod.rs**

```rust
// daemon/src/middleware/mod.rs
pub mod cors;
pub mod tailscale;
```

- [ ] **Step 2: Create middleware/cors.rs**

```rust
// daemon/src/middleware/cors.rs
use axum::http::{HeaderValue, Method, StatusCode};
use axum::response::Response;
use axum::middleware::Next;
use axum::extract::Request;

pub async fn cors_layer(request: Request, next: Next) -> Response {
    let origin = request
        .headers()
        .get("origin")
        .cloned();

    if request.method() == Method::OPTIONS {
        let mut response = Response::new(axum::body::Body::empty());
        *response.status_mut() = StatusCode::NO_CONTENT;
        apply_cors(&mut response, origin.as_ref());
        return response;
    }

    let mut response = next.run(request).await;
    apply_cors(&mut response, origin.as_ref());
    response
}

fn apply_cors(response: &mut Response, origin: Option<&HeaderValue>) {
    let headers = response.headers_mut();
    if let Some(origin) = origin {
        headers.insert("access-control-allow-origin", origin.clone());
    }
    headers.insert("vary", HeaderValue::from_static("Origin"));
    headers.insert(
        "access-control-allow-headers",
        HeaderValue::from_static("Content-Type, Authorization"),
    );
    headers.insert(
        "access-control-allow-methods",
        HeaderValue::from_static("GET, POST, OPTIONS"),
    );
}
```

- [ ] **Step 3: Create middleware/tailscale.rs**

```rust
// daemon/src/middleware/tailscale.rs
use std::net::IpAddr;
use std::sync::Arc;

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Json, Response};
use serde_json::json;

use crate::config::Settings;

pub async fn tailscale_guard(
    request: Request,
    next: Next,
) -> Response {
    let settings = request
        .extensions()
        .get::<Arc<Settings>>()
        .cloned();

    let Some(settings) = settings else {
        return next.run(request).await;
    };

    let client_ip = extract_client_ip(&request);

    match client_ip {
        Some(ip) if settings.is_ip_allowed(ip) => next.run(request).await,
        Some(ip) => {
            let body = json!({
                "error": "forbidden",
                "message": format!("client {} is not in the configured private allowlist", ip),
            });
            (StatusCode::FORBIDDEN, Json(body)).into_response()
        }
        None => {
            let body = json!({
                "error": "forbidden",
                "message": "could not determine client IP",
            });
            (StatusCode::FORBIDDEN, Json(body)).into_response()
        }
    }
}

fn extract_client_ip(request: &Request) -> Option<IpAddr> {
    request
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0.ip())
}
```

- [ ] **Step 4: Wire middleware module into main.rs**

Add to `daemon/src/main.rs`:
```rust
mod middleware;
```

- [ ] **Step 5: Verify compilation**

Run: `cd daemon && cargo check`
Expected: Compiles

- [ ] **Step 6: Commit**

```bash
git add daemon/src/middleware/
git commit -m "feat(daemon): add CORS and Tailscale CIDR middleware"
```

---

## Task 8: HTTP Route Handlers

**Files:**
- Create: `daemon/src/transport/mod.rs`
- Create: `daemon/src/transport/http.rs`

- [ ] **Step 1: Create transport/mod.rs**

```rust
// daemon/src/transport/mod.rs
pub mod http;
pub mod ws;
```

- [ ] **Step 2: Create transport/http.rs**

```rust
// daemon/src/transport/http.rs
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use serde::Deserialize;
use serde_json::json;

use crate::host::logs::LogBuffer;
use crate::store::Store;
use crate::terminal::manager::TerminalManager;

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub manager: TerminalManager,
    pub log_buffer: LogBuffer,
    pub bind_address: String,
    pub allowed_cidrs: Vec<String>,
}

// GET /health
pub async fn health() -> Json<serde_json::Value> {
    Json(json!({"ok": true}))
}

// GET /api/system/status
pub async fn system_status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let sessions = state.store.list_terminal_sessions().unwrap_or_default();
    let active_count = sessions.iter().filter(|s| s.status == "running").count();

    Json(json!({
        "activeTerminalSessions": active_count,
        "connection": {
            "bindHost": state.bind_address,
            "allowedCidrs": state.allowed_cidrs,
        }
    }))
}

#[derive(Deserialize)]
pub struct LogQuery {
    limit: Option<usize>,
    level: Option<String>,
}

// GET /api/system/logs
pub async fn system_logs(
    State(state): State<AppState>,
    Query(params): Query<LogQuery>,
) -> Json<Vec<crate::host::logs::LogEntry>> {
    let limit = params.limit.unwrap_or(200);
    Json(state.log_buffer.entries(limit, params.level.as_deref()))
}

// GET /api/terminal/sessions
pub async fn list_sessions(State(state): State<AppState>) -> Json<serde_json::Value> {
    match state.store.list_terminal_sessions() {
        Ok(sessions) => Json(json!(sessions)),
        Err(e) => Json(json!({"error": e.to_string()})),
    }
}

#[derive(Deserialize)]
pub struct CreateSessionBody {
    mode: Option<String>,
    name: Option<String>,
    workdir: Option<String>,
}

// POST /api/terminal/sessions
pub async fn create_session(
    State(state): State<AppState>,
    Json(body): Json<CreateSessionBody>,
) -> impl IntoResponse {
    let mode = body.mode.as_deref().unwrap_or("agent");
    match state.manager.create_session(mode, body.name.as_deref(), body.workdir.as_deref()).await {
        Ok(session) => (StatusCode::CREATED, Json(json!(session))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
    }
}

// GET /api/terminal/sessions/:id
pub async fn get_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    // Ensure attached so session is ready
    let _ = state.manager.ensure_attached(&session_id).await;

    match state.store.get_terminal_session(&session_id) {
        Ok(Some(session)) => {
            let chunks = state.store.list_terminal_chunks(&session_id, 0, 500).unwrap_or_default();
            Json(json!({"session": session, "chunks": chunks})).into_response()
        }
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({"error": "terminal session not found"}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
pub struct InputBody {
    input: String,
    #[serde(default = "default_true")]
    #[serde(rename = "appendNewline")]
    append_newline: bool,
}

fn default_true() -> bool { true }

// POST /api/terminal/sessions/:id/input
pub async fn post_input(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(body): Json<InputBody>,
) -> impl IntoResponse {
    match state.manager.send_input(&session_id, &body.input, body.append_newline).await {
        Ok(()) => (StatusCode::NO_CONTENT, ()).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
    }
}

#[derive(Deserialize)]
pub struct ResizeBody {
    cols: u16,
    rows: u16,
}

// POST /api/terminal/sessions/:id/resize
pub async fn resize_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(body): Json<ResizeBody>,
) -> impl IntoResponse {
    match state.manager.resize_session(&session_id, body.cols, body.rows).await {
        Ok(session) => Json(json!(session)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
    }
}

// POST /api/terminal/sessions/:id/terminate
pub async fn terminate_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match state.manager.terminate_session(&session_id).await {
        Ok(session) => Json(json!(session)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
    }
}
```

- [ ] **Step 3: Verify compilation**

Run: `cd daemon && cargo check`
Expected: Compiles

- [ ] **Step 4: Commit**

```bash
git add daemon/src/transport/
git commit -m "feat(daemon): add HTTP route handlers for terminal sessions"
```

---

## Task 9: WebSocket Handler

**Files:**
- Create: `daemon/src/transport/ws.rs`

- [ ] **Step 1: Create transport/ws.rs**

```rust
// daemon/src/transport/ws.rs
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::broadcast;
use tracing::{debug, warn};

use super::http::AppState;
use crate::store::chunks::TerminalChunkRecord;

#[derive(Deserialize)]
struct WsMessage {
    op: String,
    #[serde(default)]
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    #[serde(default)]
    #[serde(rename = "afterChunkId")]
    after_chunk_id: Option<i64>,
    #[serde(default)]
    input: Option<String>,
    #[serde(default)]
    #[serde(rename = "appendNewline")]
    append_newline: Option<bool>,
    #[serde(default)]
    cols: Option<u16>,
    #[serde(default)]
    rows: Option<u16>,
    #[serde(default)]
    ts: Option<String>,
}

pub async fn ws_upgrade(
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(mut socket: WebSocket, state: AppState) {
    let mut terminal_forward_task: Option<tokio::task::JoinHandle<()>> = None;
    let mut subscribed_session: Option<String> = None;

    // Heartbeat: send ping every 20 seconds
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(20));
    heartbeat.tick().await; // skip first immediate tick

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                let msg = json!({"op": "heartbeat", "ts": chrono::Utc::now().to_rfc3339()});
                if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                    break;
                }
            }

            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let parsed: WsMessage = match serde_json::from_str(&text) {
                            Ok(m) => m,
                            Err(e) => {
                                let _ = send_error(&mut socket, &format!("invalid message: {}", e)).await;
                                continue;
                            }
                        };

                        match parsed.op.as_str() {
                            "ping" => {
                                let reply = json!({"op": "heartbeat", "ts": parsed.ts.unwrap_or_default()});
                                let _ = socket.send(Message::Text(reply.to_string().into())).await;
                            }

                            "subscribe_terminal" => {
                                let Some(session_id) = parsed.session_id.as_deref() else {
                                    let _ = send_error(&mut socket, "sessionId required").await;
                                    continue;
                                };

                                // Cancel previous subscription
                                if let Some(task) = terminal_forward_task.take() {
                                    task.abort();
                                }
                                if let Some(prev_sid) = subscribed_session.take() {
                                    if let Some(bc) = state.manager.get_broadcaster(&prev_sid).await {
                                        bc.unsubscribe();
                                    }
                                }

                                // Ensure session is attached
                                if let Err(e) = state.manager.ensure_attached(session_id).await {
                                    let _ = send_error(&mut socket, &e).await;
                                    continue;
                                }

                                // Get session record for acknowledgment
                                let session = state.store.get_terminal_session(session_id).ok().flatten();

                                // Replay chunks from DB
                                let after_id = parsed.after_chunk_id.unwrap_or(0);
                                let chunks = state.store.list_terminal_chunks(session_id, after_id, 5000).unwrap_or_default();
                                for chunk in &chunks {
                                    let msg = json!({"op": "terminal_chunk", "chunk": chunk});
                                    if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                                        break;
                                    }
                                }

                                // Send subscribed acknowledgment
                                let ack = json!({"op": "subscribed_terminal", "session": session});
                                let _ = socket.send(Message::Text(ack.to_string().into())).await;

                                // Subscribe to live broadcast
                                if let Some(broadcaster) = state.manager.get_broadcaster(session_id).await {
                                    let mut rx = broadcaster.subscribe();
                                    subscribed_session = Some(session_id.to_string());

                                    // The forward task needs its own socket sender.
                                    // We split the socket — but axum doesn't support split easily.
                                    // Instead, use a channel to bridge.
                                    let (fwd_tx, mut fwd_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

                                    terminal_forward_task = Some(tokio::spawn(async move {
                                        loop {
                                            match rx.recv().await {
                                                Ok(chunk) => {
                                                    let msg = json!({"op": "terminal_chunk", "chunk": chunk});
                                                    if fwd_tx.send(msg.to_string()).is_err() {
                                                        break;
                                                    }
                                                }
                                                Err(broadcast::error::RecvError::Lagged(n)) => {
                                                    warn!(lagged = n, "broadcast subscriber lagged");
                                                }
                                                Err(broadcast::error::RecvError::Closed) => break,
                                            }
                                        }
                                    }));

                                    // Spawn a task to forward from channel to socket
                                    // Actually, we handle fwd_rx in the main select loop
                                    // This is a bit tricky — we need to integrate fwd_rx into the loop.
                                    // Let's use a different approach: store fwd_rx in the loop state.

                                    // For simplicity, we'll restructure: see revised loop below.
                                    // For now, let's just recv from the channel in the select.
                                    // But we need fwd_rx accessible in the select...
                                    // We'll restructure by making fwd_rx optional in the outer scope.

                                    // Actually, let me just use a simpler pattern.
                                    // Drop this forward task approach and use fwd_rx directly.
                                    terminal_forward_task.take().map(|t| t.abort());

                                    // We'll handle the broadcast recv directly in the select loop.
                                    // Store the broadcast receiver.
                                    // This requires refactoring — see Step 2.
                                }
                            }

                            "terminal_input" => {
                                if let Some(sid) = parsed.session_id.as_deref() {
                                    let input = parsed.input.unwrap_or_default();
                                    let newline = parsed.append_newline.unwrap_or(false);
                                    let _ = state.manager.send_input(sid, &input, newline).await;
                                }
                            }

                            "resize_terminal" => {
                                if let Some(sid) = parsed.session_id.as_deref() {
                                    if let (Some(cols), Some(rows)) = (parsed.cols, parsed.rows) {
                                        if let Ok(session) = state.manager.resize_session(sid, cols, rows).await {
                                            let msg = json!({"op": "terminal_status", "session": session});
                                            let _ = socket.send(Message::Text(msg.to_string().into())).await;
                                        }
                                    }
                                }
                            }

                            "interrupt_terminal" => {
                                if let Some(sid) = parsed.session_id.as_deref() {
                                    let _ = state.manager.interrupt_session(sid).await;
                                }
                            }

                            "terminate_terminal" => {
                                if let Some(sid) = parsed.session_id.as_deref() {
                                    if let Ok(session) = state.manager.terminate_session(sid).await {
                                        let msg = json!({"op": "terminal_status", "session": session});
                                        let _ = socket.send(Message::Text(msg.to_string().into())).await;
                                    }
                                }
                            }

                            other => {
                                let _ = send_error(&mut socket, &format!("unknown op: {}", other)).await;
                            }
                        }
                    }

                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {} // ignore binary, ping, pong
                }
            }
        }
    }

    // Cleanup: unsubscribe from broadcaster
    if let Some(task) = terminal_forward_task.take() {
        task.abort();
    }
    if let Some(sid) = subscribed_session.take() {
        if let Some(bc) = state.manager.get_broadcaster(&sid).await {
            bc.unsubscribe();
        }
    }
    debug!("WebSocket connection closed");
}

async fn send_error(socket: &mut WebSocket, message: &str) -> Result<(), axum::Error> {
    let msg = json!({"op": "error", "message": message});
    socket.send(Message::Text(msg.to_string().into())).await
}
```

- [ ] **Step 2: Refactor ws.rs to handle broadcast recv in the select loop**

The above has a design issue with integrating the broadcast receiver into the select loop. Replace the full file with this cleaner version:

```rust
// daemon/src/transport/ws.rs
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::broadcast;
use tracing::{debug, warn};

use super::http::AppState;
use crate::store::chunks::TerminalChunkRecord;

#[derive(Deserialize)]
struct WsMessage {
    op: String,
    #[serde(default, rename = "sessionId")]
    session_id: Option<String>,
    #[serde(default, rename = "afterChunkId")]
    after_chunk_id: Option<i64>,
    #[serde(default)]
    input: Option<String>,
    #[serde(default, rename = "appendNewline")]
    append_newline: Option<bool>,
    #[serde(default)]
    cols: Option<u16>,
    #[serde(default)]
    rows: Option<u16>,
    #[serde(default)]
    ts: Option<String>,
}

pub async fn ws_upgrade(
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(mut socket: WebSocket, state: AppState) {
    let mut broadcast_rx: Option<broadcast::Receiver<TerminalChunkRecord>> = None;
    let mut subscribed_session: Option<String> = None;

    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(20));
    heartbeat.tick().await;

    loop {
        tokio::select! {
            biased;

            // Forward broadcast chunks to WebSocket (highest priority)
            chunk = async {
                match broadcast_rx.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                match chunk {
                    Ok(record) => {
                        let msg = json!({"op": "terminal_chunk", "chunk": record});
                        if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(lagged = n, "broadcast subscriber lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        broadcast_rx = None;
                    }
                }
            }

            // Heartbeat
            _ = heartbeat.tick() => {
                let msg = json!({"op": "heartbeat", "ts": chrono::Utc::now().to_rfc3339()});
                if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                    break;
                }
            }

            // Client messages
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let parsed: WsMessage = match serde_json::from_str(&text) {
                            Ok(m) => m,
                            Err(e) => {
                                let _ = send_error(&mut socket, &format!("invalid message: {e}")).await;
                                continue;
                            }
                        };

                        match parsed.op.as_str() {
                            "ping" => {
                                let reply = json!({"op": "heartbeat", "ts": parsed.ts.unwrap_or_default()});
                                let _ = socket.send(Message::Text(reply.to_string().into())).await;
                            }

                            "subscribe_terminal" => {
                                let Some(session_id) = parsed.session_id.as_deref() else {
                                    let _ = send_error(&mut socket, "sessionId required").await;
                                    continue;
                                };

                                // Unsubscribe from previous
                                if let Some(prev_sid) = subscribed_session.take() {
                                    if let Some(bc) = state.manager.get_broadcaster(&prev_sid).await {
                                        bc.unsubscribe();
                                    }
                                }
                                broadcast_rx = None;

                                // Ensure attached
                                if let Err(e) = state.manager.ensure_attached(session_id).await {
                                    let _ = send_error(&mut socket, &e).await;
                                    continue;
                                }

                                // Replay chunks
                                let after_id = parsed.after_chunk_id.unwrap_or(0);
                                let chunks = state.store.list_terminal_chunks(session_id, after_id, 5000).unwrap_or_default();
                                for chunk in &chunks {
                                    let msg = json!({"op": "terminal_chunk", "chunk": chunk});
                                    if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                                        return;
                                    }
                                }

                                // Send ack
                                let session = state.store.get_terminal_session(session_id).ok().flatten();
                                let ack = json!({"op": "subscribed_terminal", "session": session});
                                let _ = socket.send(Message::Text(ack.to_string().into())).await;

                                // Subscribe to live broadcast
                                if let Some(broadcaster) = state.manager.get_broadcaster(session_id).await {
                                    broadcast_rx = Some(broadcaster.subscribe());
                                    subscribed_session = Some(session_id.to_string());
                                }
                            }

                            "terminal_input" => {
                                if let Some(sid) = parsed.session_id.as_deref() {
                                    let input = parsed.input.unwrap_or_default();
                                    let newline = parsed.append_newline.unwrap_or(false);
                                    let _ = state.manager.send_input(sid, &input, newline).await;
                                }
                            }

                            "resize_terminal" => {
                                if let Some(sid) = parsed.session_id.as_deref() {
                                    if let (Some(cols), Some(rows)) = (parsed.cols, parsed.rows) {
                                        if let Ok(session) = state.manager.resize_session(sid, cols, rows).await {
                                            let msg = json!({"op": "terminal_status", "session": session});
                                            let _ = socket.send(Message::Text(msg.to_string().into())).await;
                                        }
                                    }
                                }
                            }

                            "interrupt_terminal" => {
                                if let Some(sid) = parsed.session_id.as_deref() {
                                    let _ = state.manager.interrupt_session(sid).await;
                                }
                            }

                            "terminate_terminal" => {
                                if let Some(sid) = parsed.session_id.as_deref() {
                                    if let Ok(session) = state.manager.terminate_session(sid).await {
                                        let msg = json!({"op": "terminal_status", "session": session});
                                        let _ = socket.send(Message::Text(msg.to_string().into())).await;
                                    }
                                }
                            }

                            other => {
                                let _ = send_error(&mut socket, &format!("unknown op: {other}")).await;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }

    // Cleanup: unsubscribe and trigger idle timeout check
    if let Some(sid) = subscribed_session {
        if let Some(bc) = state.manager.get_broadcaster(&sid).await {
            bc.unsubscribe();
        }
        state.manager.on_unsubscribe(&sid).await;
    }
    debug!("WebSocket connection closed");
}

async fn send_error(socket: &mut WebSocket, message: &str) -> Result<(), axum::Error> {
    let msg = json!({"op": "error", "message": message});
    socket.send(Message::Text(msg.to_string().into())).await
}
```

- [ ] **Step 3: Verify compilation**

Run: `cd daemon && cargo check`
Expected: Compiles

- [ ] **Step 4: Commit**

```bash
git add daemon/src/transport/ws.rs
git commit -m "feat(daemon): add WebSocket handler with terminal subscription and broadcast"
```

---

## Task 10: Server Assembly & Main

**Files:**
- Create: `daemon/src/server.rs`
- Modify: `daemon/src/main.rs`

- [ ] **Step 1: Create server.rs — router assembly and startup**

```rust
// daemon/src/server.rs
use std::net::SocketAddr;
use std::sync::Arc;

use axum::middleware as axum_mw;
use axum::routing::{get, post};
use axum::Router;
use tracing::info;

use crate::config::Settings;
use crate::host::logs::LogBuffer;
use crate::middleware::{cors, tailscale};
use crate::store::Store;
use crate::terminal::manager::TerminalManager;
use crate::transport::http::{self, AppState};
use crate::transport::ws;

pub async fn run(settings: Settings) -> Result<(), Box<dyn std::error::Error>> {
    let store = Store::open(&settings.db_path)?;
    let manager = TerminalManager::new(store.clone());
    let log_buffer = LogBuffer::new();

    // Recover existing tmux sessions
    manager.recover().await;

    let state = AppState {
        store,
        manager,
        log_buffer,
        bind_address: format!("{}:{}", settings.bind_hosts.join(","), settings.bind_port),
        allowed_cidrs: settings.allowed_cidrs.iter().map(|c| c.to_string()).collect(),
    };

    let app = Router::new()
        .route("/health", get(http::health))
        .route("/api/system/status", get(http::system_status))
        .route("/api/system/logs", get(http::system_logs))
        .route("/api/terminal/sessions", get(http::list_sessions).post(http::create_session))
        .route("/api/terminal/sessions/{id}", get(http::get_session))
        .route("/api/terminal/sessions/{id}/input", post(http::post_input))
        .route("/api/terminal/sessions/{id}/resize", post(http::resize_session))
        .route("/api/terminal/sessions/{id}/terminate", post(http::terminate_session))
        .route("/ws", get(ws::ws_upgrade))
        .layer(axum_mw::from_fn(cors::cors_layer))
        .layer(axum_mw::from_fn(tailscale::tailscale_guard))
        .layer(axum::Extension(Arc::new(settings.clone())))
        .with_state(state);

    // Bind to all configured hosts
    let mut handles = Vec::new();
    for host in &settings.bind_hosts {
        let addr: SocketAddr = format!("{}:{}", host, settings.bind_port)
            .parse()
            .map_err(|e| format!("invalid bind address '{}:{}': {}", host, settings.bind_port, e))?;

        let app = app.clone();
        info!(%addr, "listening");

        let listener = tokio::net::TcpListener::bind(addr).await?;
        handles.push(tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .expect("server error");
        }));
    }

    // Wait for all listeners (they run forever until process exits)
    for handle in handles {
        handle.await?;
    }

    Ok(())
}
```

- [ ] **Step 2: Update main.rs to wire everything together**

Replace `daemon/src/main.rs` with:

```rust
// daemon/src/main.rs
mod config;
mod host;
mod middleware;
mod server;
mod store;
mod terminal;
mod transport;

use clap::Parser;
use config::{Cli, Settings};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ghost_protocol_daemon=info".into()),
        )
        .init();

    let cli = Cli::parse();
    let settings = Settings::from_cli(cli).expect("invalid configuration");

    tracing::info!(
        bind = ?settings.bind_hosts,
        port = settings.bind_port,
        "starting ghost-protocol-daemon"
    );

    if let Err(e) = server::run(settings).await {
        tracing::error!(error = %e, "daemon exited with error");
        std::process::exit(1);
    }
}
```

- [ ] **Step 3: Make Settings Clone-able**

Add `Clone` derive to `Settings` in `config.rs`. Update the struct:

In `daemon/src/config.rs`, add `#[derive(Clone)]` above `pub struct Settings`:

```rust
#[derive(Clone)]
pub struct Settings {
    // ... (fields unchanged)
}
```

- [ ] **Step 4: Build and verify**

Run: `cd daemon && cargo build`
Expected: Compiles successfully

Run: `cd daemon && cargo test`
Expected: All tests pass (config: 2, store: 7, broadcaster: 3, tmux: 2, logs: 2 = ~16 tests)

- [ ] **Step 5: Smoke test — start the daemon**

Run: `cd daemon && cargo run -- --bind-host 127.0.0.1 --db-path /tmp/ghost-test.db`
Expected: Logs show "starting ghost-protocol-daemon" and "listening" on 127.0.0.1:8787

In another terminal, verify:
```bash
curl http://127.0.0.1:8787/health
```
Expected: `{"ok":true}`

```bash
curl http://127.0.0.1:8787/api/terminal/sessions
```
Expected: `[]`

Kill the daemon with Ctrl+C.

- [ ] **Step 6: Commit**

```bash
git add daemon/
git commit -m "feat(daemon): assemble server with all routes, middleware, and startup"
```

---

## Task 11: Integration Test — Terminal Session Lifecycle

**Files:**
- Create: `daemon/tests/terminal_lifecycle.rs`

- [ ] **Step 1: Write integration test**

```rust
// daemon/tests/terminal_lifecycle.rs
//! Integration test: requires tmux to be installed.

use std::time::Duration;

use serde_json::{json, Value};
use tokio::time::sleep;

const BASE: &str = "http://127.0.0.1:18787";

/// Starts the daemon on a test port, runs the test, then kills the daemon.
async fn with_daemon<F, Fut>(test: F)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let db_path = format!("/tmp/ghost-test-{}.db", std::process::id());

    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_ghost-protocol-daemon"))
        .args(["--bind-host", "127.0.0.1", "--bind-port", "18787", "--db-path", &db_path])
        .env("GHOST_PROTOCOL_ALLOWED_CIDRS", "127.0.0.1/32")
        .kill_on_drop(true)
        .spawn()
        .expect("failed to start daemon");

    // Wait for daemon to be ready
    let client = reqwest::Client::new();
    for _ in 0..30 {
        if client.get(format!("{BASE}/health")).send().await.is_ok() {
            break;
        }
        sleep(Duration::from_millis(100)).await;
    }

    test().await;

    child.kill().await.ok();
    std::fs::remove_file(&db_path).ok();
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
        assert_eq!(resp.status(), 201);
        let body: Value = resp.json().await.unwrap();
        let session_id = body["id"].as_str().unwrap().to_string();
        assert_eq!(body["mode"], "rescue_shell");
        assert_eq!(body["status"], "running");

        // List sessions
        let resp = client
            .get(format!("{BASE}/api/terminal/sessions"))
            .send()
            .await
            .unwrap();
        let sessions: Vec<Value> = resp.json().await.unwrap();
        assert!(!sessions.is_empty());

        // Terminate session
        let resp = client
            .post(format!("{BASE}/api/terminal/sessions/{session_id}/terminate"))
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success());
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["status"], "terminated");
    })
    .await;
}

#[tokio::test]
async fn test_health_and_system_status() {
    with_daemon(|| async {
        let client = reqwest::Client::new();

        let resp = client.get(format!("{BASE}/health")).send().await.unwrap();
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["ok"], true);

        let resp = client
            .get(format!("{BASE}/api/system/status"))
            .send()
            .await
            .unwrap();
        let body: Value = resp.json().await.unwrap();
        assert!(body["connection"]["bindHost"].is_string());
    })
    .await;
}
```

- [ ] **Step 2: Run integration tests**

Run: `cd daemon && cargo test --test terminal_lifecycle -- --test-threads=1`
Expected: 2 tests pass (requires tmux to be installed)

- [ ] **Step 3: Commit**

```bash
git add daemon/tests/
git commit -m "test(daemon): add integration tests for terminal lifecycle"
```

---

## Task 12: Frontend — Update detect.rs for Rust Binary

**Files:**
- Modify: `desktop/src-tauri/src/detect.rs`

- [ ] **Step 1: Read current detect.rs**

Read `desktop/src-tauri/src/detect.rs` to understand the current `install_daemon()` and `start_daemon()` functions.

- [ ] **Step 2: Update install_daemon to use Rust binary**

Replace the Python venv install logic with Rust binary detection/copy:

```rust
// In detect.rs, replace the install_daemon function:

fn daemon_bin_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("/usr/local/bin"))
        .join("ghost-protocol")
        .join("ghost-protocol-daemon")
}

#[tauri::command]
pub fn install_daemon() -> Result<String, String> {
    let bin_path = daemon_bin_path();

    // Check if already installed
    if bin_path.exists() {
        let check = Command::new(&bin_path)
            .arg("--help")
            .output();
        if let Ok(output) = check {
            if output.status.success() {
                return Ok("already_installed".to_string());
            }
        }
    }

    // Create directory
    if let Some(parent) = bin_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("install_failed:mkdir:{}", e))?;
    }

    // Try to find the binary bundled with the app
    // During development: look in daemon/target/release/
    // In packaged release: look alongside the app binary
    let bundled_candidates = vec![
        // Packaged release: same directory as the app
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("ghost-protocol-daemon"))),
        // Development: built from source
        Some(PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../daemon/target/release/ghost-protocol-daemon"))),
    ];

    for candidate in bundled_candidates.into_iter().flatten() {
        if candidate.exists() {
            std::fs::copy(&candidate, &bin_path)
                .map_err(|e| format!("install_failed:copy:{}", e))?;

            // Make executable
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&bin_path, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| format!("install_failed:chmod:{}", e))?;
            }

            return Ok("installed".to_string());
        }
    }

    Err("install_failed:binary_not_found".to_string())
}
```

- [ ] **Step 3: Update start_daemon to use Rust binary**

```rust
// Replace start_daemon in detect.rs:

#[tauri::command]
pub fn start_daemon(bind_host: String, port: u16) -> Result<String, String> {
    let bin_path = daemon_bin_path();

    if !bin_path.exists() {
        return Err("daemon_not_installed".to_string());
    }

    let bind = format!("{},127.0.0.1", bind_host);
    let cidrs = "100.64.0.0/10,fd7a:115c:a1e0::/48,127.0.0.1/32";

    Command::new("setsid")
        .arg(&bin_path)
        .args(["--bind-host", &bind])
        .args(["--bind-port", &port.to_string()])
        .args(["--allowed-cidrs", cidrs])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("start_failed:{}", e))?;

    Ok("spawned".to_string())
}
```

- [ ] **Step 4: Update stop_daemon**

```rust
// Replace stop_daemon in detect.rs:

#[tauri::command]
pub fn stop_daemon() -> Result<String, String> {
    Command::new("pkill")
        .args(["-f", "ghost-protocol-daemon"])
        .output()
        .map_err(|e| format!("stop_failed:{}", e))?;

    Ok("stopped".to_string())
}
```

- [ ] **Step 5: Remove old Python venv references**

Remove the `daemon_venv_dir()` function and any `python`/`venv`-related code from detect.rs.

- [ ] **Step 6: Verify Tauri build**

Run: `cd desktop/src-tauri && cargo check`
Expected: Compiles

- [ ] **Step 7: Commit**

```bash
git add desktop/src-tauri/src/detect.rs
git commit -m "feat(desktop): update detect.rs to install/start Rust daemon binary"
```

---

## Task 13: Frontend — Update SetupChecklist

**Files:**
- Modify: `desktop/src/components/SetupChecklist.tsx`

- [ ] **Step 1: Read current SetupChecklist.tsx**

Read `desktop/src/components/SetupChecklist.tsx` to understand the current checklist items.

- [ ] **Step 2: Remove Python from checklist**

In `SetupChecklist.tsx`, remove the Python check item from `INITIAL_CHECKS`. The checklist should now be:

```typescript
const INITIAL_CHECKS: CheckItem[] = [
  { name: "tmux", key: "tmux", minVersion: "3.0" },
  { name: "Tailscale", key: "tailscale", minVersion: "1.0" },
  { name: "Tailscale mesh", key: "tailscale_mesh", minVersion: "" },
];
```

Remove any calls to `detect_python` from the checklist polling logic.

- [ ] **Step 3: Verify frontend builds**

Run: `cd desktop && npx tsc --noEmit`
Expected: No type errors

- [ ] **Step 4: Commit**

```bash
git add desktop/src/components/SetupChecklist.tsx
git commit -m "feat(desktop): remove Python from setup checklist (Rust daemon has no Python dep)"
```

---

## Task 14: Frontend — Hide Agent-Specific UI

**Files:**
- Modify: `desktop/src/App.tsx`
- Modify: `desktop/src/types.ts`

- [ ] **Step 1: Read App.tsx and identify agent-specific UI**

Read `desktop/src/App.tsx` and identify sections that reference runs, approvals, agents, or conversations that won't have a backend.

- [ ] **Step 2: Hide/remove agent-specific UI from App.tsx**

- Remove or comment out the ChatView rendering (no backend for it yet)
- Remove InspectorPanel sections that show runs/agents/approvals
- Keep the terminal workspace, sidebar, and logs views as-is

Specifics depend on what's found in the file — the goal is to hide UI that would make broken API calls to endpoints the Rust daemon doesn't serve.

- [ ] **Step 3: Clean up types.ts**

Remove or mark as optional the agent-related types: `RunRecord`, `ApprovalRecord`, `AgentRecord`, etc. Keep `TerminalSession`, `SavedHost`, `HostConnection`, and other types that are still used.

- [ ] **Step 4: Verify frontend builds**

Run: `cd desktop && npx tsc --noEmit`
Expected: No type errors

- [ ] **Step 5: Commit**

```bash
git add desktop/src/
git commit -m "feat(desktop): hide agent-specific UI for Rust daemon v1"
```

---

## Task 15: End-to-End Verification

- [ ] **Step 1: Build Rust daemon**

```bash
cd daemon && cargo build --release
```
Expected: Binary at `daemon/target/release/ghost-protocol-daemon`

- [ ] **Step 2: Start daemon manually**

```bash
./daemon/target/release/ghost-protocol-daemon --bind-host 127.0.0.1
```
Expected: Daemon starts, logs "listening" on 127.0.0.1:8787

- [ ] **Step 3: Test HTTP API**

```bash
curl http://127.0.0.1:8787/health
# → {"ok":true}

curl -X POST http://127.0.0.1:8787/api/terminal/sessions \
  -H 'Content-Type: application/json' \
  -d '{"mode":"rescue_shell","name":"e2e-test"}'
# → {"id":"...","mode":"rescue_shell","status":"running",...}

curl http://127.0.0.1:8787/api/terminal/sessions
# → [{"id":"...","mode":"rescue_shell","status":"running",...}]
```

- [ ] **Step 4: Test with Tauri app**

```bash
cd desktop && npm run tauri dev
```

- Open the app
- Verify the setup checklist no longer shows Python
- Click "Host a connection" or manually add `http://127.0.0.1:8787` as a host
- Create a remote terminal session from the terminal workspace
- Type commands, verify output streams back
- Verify resize works (resize the window)
- Close and reopen the app — verify reconnect replays chunks

- [ ] **Step 5: Test multi-client (if a second machine is available)**

On Machine A: start daemon bound to Tailscale IP
On Machine B: add Machine A's Tailscale IP as a host in the app
Create/view terminal sessions across machines

- [ ] **Step 6: Final commit if any fixes were needed**

```bash
git add -A
git commit -m "fix(daemon): address issues found during e2e verification"
```

---

## Task 16: Remove Python Backend

Only after all verification passes.

- [ ] **Step 1: Remove backend/ directory**

```bash
git rm -r backend/
```

- [ ] **Step 2: Update README.md**

Update the README to reflect:
- `daemon/` replaces `backend/`
- Installation no longer requires Python for hosting
- Development setup: `cd daemon && cargo build` instead of Python venv
- Useful commands: update backend type-check to `cd daemon && cargo check`

- [ ] **Step 3: Update docs/project-plan.md**

Add a note about the Rust daemon rewrite completion.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore: remove Python backend, replaced by Rust daemon"
```
