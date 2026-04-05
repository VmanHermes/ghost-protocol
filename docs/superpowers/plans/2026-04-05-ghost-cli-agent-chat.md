# Ghost CLI, Project System & Agent Chat Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `ghost` CLI with project initialization, agent discovery, and agent chat sessions that wrap terminal sessions with a message layer.

**Architecture:** New `cli/` crate communicates with the daemon via HTTP. The daemon gains agent detection (probing for CLIs and APIs), a project registry (SQLite), and chat sessions that wrap terminal sessions with agent-specific output parsers. The session model gets a `session_type` column to unify terminal/chat/code-server under one concept.

**Tech Stack:** Rust (clap, reqwest, serde, tokio), SQLite, axum

**Spec:** `docs/superpowers/specs/2026-04-05-agent-discovery-design.md`

---

## File Structure

### New Files

| File | Responsibility |
|---|---|
| `cli/Cargo.toml` | Ghost CLI crate dependencies |
| `cli/src/main.rs` | Entry point, clap subcommand dispatch |
| `cli/src/detect.rs` | Agent detection probes (claude, hermes, ollama, aider, openclaw) |
| `cli/src/init.rs` | `ghost init` — interactive project setup |
| `cli/src/commands.rs` | `ghost status`, `agents`, `projects`, `help` |
| `cli/src/chat.rs` | `ghost chat <agent>` — thin client for chat sessions |
| `daemon/src/hardware/agents.rs` | Daemon-side agent detection (reuses detection logic) |
| `daemon/migrations/006_projects_and_chat.sql` | projects table, session_type column, chat_messages table |
| `daemon/src/store/projects.rs` | CRUD for projects |
| `daemon/src/store/chat.rs` | CRUD for chat_messages |
| `daemon/src/chat/mod.rs` | Chat session management |
| `daemon/src/chat/adapters/mod.rs` | Adapter trait + registry |
| `daemon/src/chat/adapters/generic.rs` | Generic delimiter-based adapter |
| `daemon/src/chat/adapters/claude.rs` | Claude Code output parser |
| `daemon/src/chat/adapters/ollama.rs` | Ollama output parser |

### Modified Files

| File | Change |
|---|---|
| `daemon/src/store/mod.rs` | Register projects, chat modules, run migration 006 |
| `daemon/src/store/sessions.rs` | Add session_type + project_id to TerminalSessionRecord |
| `daemon/src/store/hosts.rs` | Update HostCapabilities (agents vec replaces hermes/ollama bools) |
| `daemon/src/hardware/mod.rs` | Add agents module, agents field to ToolsInfo/MachineInfo |
| `daemon/src/server.rs` | Register project/agent/chat routes, spawn agent detection task |
| `daemon/src/transport/http.rs` | Add project CRUD, agent list, chat session endpoints |
| `daemon/src/transport/ws.rs` | Add subscribe_chat, chat_message, send_chat_message ops |
| `daemon/src/terminal/manager.rs` | Support creating chat-type sessions |
| `daemon/src/mcp/resources.rs` | Add ghost://agents/available, ghost_list_agents tool, briefing |
| `daemon/src/mcp/transport.rs` | Register new resource + tool |
| `desktop/src/types.ts` | Add AgentInfo, Project, ChatMessage types |
| `desktop/src/api.ts` | Add project/agent/chat API functions |
| `desktop/src/components/ChatView.tsx` | Revive with agent/machine picker |
| `desktop/src/components/Sidebar.tsx` | Show agents per connection |
| `desktop/src/App.tsx` | Wire up chat view + project state |
| `scripts/package-linux.sh` | Include ghost CLI binary in release |

---

## Task 1: Database Migration — Projects, Session Types, Chat Messages

**Files:**
- Create: `daemon/migrations/006_projects_and_chat.sql`
- Modify: `daemon/src/store/mod.rs`
- Modify: `daemon/src/store/sessions.rs`

- [ ] **Step 1: Write the migration SQL**

Create `daemon/migrations/006_projects_and_chat.sql`:

```sql
CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    workdir TEXT NOT NULL UNIQUE,
    config_json TEXT NOT NULL,
    registered_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS chat_messages (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES terminal_sessions(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_chat_messages_session ON chat_messages(session_id);
```

Note: SQLite doesn't support `ALTER TABLE ADD COLUMN` with `NOT NULL` without a default. The `session_type` and `project_id` columns:

```sql
-- session_type defaults to 'terminal' for all existing sessions
-- Cannot use ALTER TABLE for these in SQLite with NOT NULL constraint easily,
-- so we handle it in Rust by checking if column exists and adding if not
```

- [ ] **Step 2: Register migration in store/mod.rs**

Add `pub mod projects;` and `pub mod chat;` after `pub mod outcomes;`.

After migration_005 block, add:
```rust
let migration_006 = include_str!("../../migrations/006_projects_and_chat.sql");
conn.execute_batch(migration_006)?;

// Add session_type column if not present (ALTER TABLE is idempotent-safe this way)
conn.execute_batch(
    "ALTER TABLE terminal_sessions ADD COLUMN session_type TEXT NOT NULL DEFAULT 'terminal';"
).ok(); // ignore error if column already exists
conn.execute_batch(
    "ALTER TABLE terminal_sessions ADD COLUMN project_id TEXT REFERENCES projects(id);"
).ok();
```

- [ ] **Step 3: Add session_type and project_id to TerminalSessionRecord**

In `daemon/src/store/sessions.rs`, add to the struct (after `exit_code`):
```rust
pub session_type: String,
pub project_id: Option<String>,
```

Update `create_terminal_session` to accept and store `session_type` (default `"terminal"`) and `project_id` (default `None`). Update all `query_map` SELECT statements to include the new columns.

- [ ] **Step 4: Run tests**

Run: `cd daemon && cargo test`
Expected: All existing tests pass (session_type defaults to "terminal", backward compatible)

- [ ] **Step 5: Commit**

```bash
git add daemon/migrations/006_projects_and_chat.sql daemon/src/store/mod.rs daemon/src/store/sessions.rs
git commit -m "feat(daemon): add projects table, chat_messages table, session_type column"
```

---

## Task 2: Projects Store

**Files:**
- Create: `daemon/src/store/projects.rs`

**Depends on:** Task 1

- [ ] **Step 1: Write types and tests**

Create `daemon/src/store/projects.rs`:

```rust
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::Store;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRecord {
    pub id: String,
    pub name: String,
    pub workdir: String,
    pub config_json: String,
    pub registered_at: String,
    pub updated_at: String,
}
```

**Store methods:**
- `create_project(id, name, workdir, config_json)` — INSERT, returns ProjectRecord
- `list_projects()` — SELECT all, ORDER BY name
- `get_project(id)` — SELECT by id, returns Option
- `get_project_by_workdir(workdir)` — SELECT by workdir, returns Option
- `update_project(id, config_json)` — UPDATE config_json and updated_at
- `remove_project(id)` — DELETE

**Tests (4):**
- test_create_and_get_project
- test_get_project_by_workdir
- test_list_projects
- test_update_and_remove_project

- [ ] **Step 2: Implement and run tests**

Run: `cd daemon && cargo test store::projects`
Expected: All 4 tests PASS

- [ ] **Step 3: Commit**

```bash
git add daemon/src/store/projects.rs
git commit -m "feat(daemon): add projects store with CRUD"
```

---

## Task 3: Chat Messages Store

**Files:**
- Create: `daemon/src/store/chat.rs`

**Depends on:** Task 1

- [ ] **Step 1: Write types and tests**

Create `daemon/src/store/chat.rs`:

```rust
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::Store;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub session_id: String,
    pub role: String,    // "user", "assistant", "system"
    pub content: String,
    pub created_at: String,
}
```

**Store methods:**
- `create_chat_message(id, session_id, role, content)` — INSERT, returns ChatMessage
- `list_chat_messages(session_id, after_id: Option<&str>, limit: usize)` — SELECT by session, optional cursor, ORDER BY created_at ASC
- `get_chat_message(id)` — SELECT by id, returns Option

**Tests (3):**
- test_create_and_get_chat_message
- test_list_messages_for_session (create 3, retrieve, verify order)
- test_list_messages_with_cursor (create 5, get after id 2, verify only 3 returned)

- [ ] **Step 2: Implement and run tests**

Run: `cd daemon && cargo test store::chat`
Expected: All 3 tests PASS

- [ ] **Step 3: Commit**

```bash
git add daemon/src/store/chat.rs
git commit -m "feat(daemon): add chat_messages store"
```

---

## Task 4: Agent Detection Module

**Files:**
- Create: `daemon/src/hardware/agents.rs`
- Modify: `daemon/src/hardware/mod.rs`
- Modify: `daemon/src/store/hosts.rs`

**Depends on:** None

- [ ] **Step 1: Create agent detection module**

Create `daemon/src/hardware/agents.rs`:

```rust
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub agent_type: String,
    pub command: String,
    pub version: Option<String>,
}

pub fn detect_agents() -> Vec<AgentInfo> {
    let mut agents = Vec::new();

    // Claude Code
    if let Some(version) = detect_cli_version("claude", &["--version"]) {
        agents.push(AgentInfo {
            id: "claude-code".to_string(),
            name: "Claude Code".to_string(),
            agent_type: "cli".to_string(),
            command: "claude".to_string(),
            version: Some(version),
        });
    }

    // Hermes
    if which("hermes").is_some() {
        agents.push(AgentInfo {
            id: "hermes".to_string(),
            name: "Hermes".to_string(),
            agent_type: "cli".to_string(),
            command: "hermes".to_string(),
            version: None,
        });
    }

    // Aider
    if let Some(version) = detect_cli_version("aider", &["--version"]) {
        agents.push(AgentInfo {
            id: "aider".to_string(),
            name: "Aider".to_string(),
            agent_type: "cli".to_string(),
            command: "aider".to_string(),
            version: Some(version),
        });
    }

    // OpenClaw
    if which("openclaw").is_some() {
        agents.push(AgentInfo {
            id: "openclaw".to_string(),
            name: "OpenClaw".to_string(),
            agent_type: "cli".to_string(),
            command: "openclaw".to_string(),
            version: None,
        });
    }

    // Ollama models
    if let Some(models) = detect_ollama_models() {
        for model in models {
            agents.push(AgentInfo {
                id: format!("ollama:{model}"),
                name: format!("Ollama ({model})"),
                agent_type: "api".to_string(),
                command: format!("ollama run {model}"),
                version: None,
            });
        }
    }

    // Custom agents from config
    if let Some(custom) = load_custom_agents() {
        agents.extend(custom);
    }

    agents
}

fn which(name: &str) -> Option<String> {
    Command::new("which")
        .arg(name)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn detect_cli_version(cmd: &str, args: &[&str]) -> Option<String> {
    which(cmd)?; // ensure binary exists first
    Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        })
}

fn detect_ollama_models() -> Option<Vec<String>> {
    let output = Command::new("curl")
        .args(["-s", "--max-time", "2", "http://localhost:11434/api/tags"])
        .output()
        .ok()
        .filter(|o| o.status.success())?;

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let models = json["models"].as_array()?;

    Some(
        models
            .iter()
            .filter_map(|m| m["name"].as_str().map(|s| s.split(':').next().unwrap_or(s).to_string()))
            .collect(),
    )
}

fn load_custom_agents() -> Option<Vec<AgentInfo>> {
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            std::path::PathBuf::from(home).join(".config")
        });
    let path = config_dir.join("ghost-protocol").join("agents.json");
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}
```

- [ ] **Step 2: Integrate into hardware/mod.rs**

Add `pub mod agents;` at the top of `daemon/src/hardware/mod.rs`.

Add `agents` field to `ToolsInfo`:
```rust
pub struct ToolsInfo {
    pub tmux: Option<String>,
    pub hermes: Option<String>,
    pub ollama: Option<String>,
    pub ssh_user: String,
    pub agents: Vec<agents::AgentInfo>,
}
```

In `collect_machine_info()`, add agent detection:
```rust
let detected_agents = agents::detect_agents();
```

And include in the ToolsInfo construction:
```rust
tools: ToolsInfo {
    tmux: tmux_version,
    hermes: hermes_path,
    ollama: ollama_endpoint,
    ssh_user,
    agents: detected_agents,
},
```

- [ ] **Step 3: Update HostCapabilities to include agents**

In `daemon/src/store/hosts.rs`, add to `HostCapabilities`:
```rust
pub struct HostCapabilities {
    pub gpu: Option<String>,
    pub ram_gb: Option<f64>,
    pub hermes: bool,
    pub ollama: bool,
    pub agents: Option<Vec<crate::hardware::agents::AgentInfo>>,
}
```

Keep `hermes` and `ollama` bools for backward compatibility with existing data. Add `agents` as `Option` so old capability JSON still deserializes.

Update the health poller in `server.rs` to parse agents from peer hardware responses:
```rust
let agents: Option<Vec<_>> = v["tools"]["agents"].as_array().map(|arr| {
    arr.iter().filter_map(|a| serde_json::from_value(a.clone()).ok()).collect()
});
```

- [ ] **Step 4: Verify compilation**

Run: `cd daemon && cargo build`

- [ ] **Step 5: Commit**

```bash
git add daemon/src/hardware/agents.rs daemon/src/hardware/mod.rs daemon/src/store/hosts.rs daemon/src/server.rs
git commit -m "feat(daemon): add agent detection module with 5 built-in detectors + custom config"
```

---

## Task 5: Project & Agent HTTP Endpoints

**Files:**
- Modify: `daemon/src/transport/http.rs`
- Modify: `daemon/src/server.rs`

**Depends on:** Task 2, Task 4

- [ ] **Step 1: Add project CRUD handlers**

Add to `daemon/src/transport/http.rs`:

```rust
// POST /api/projects
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectBody {
    pub name: String,
    pub workdir: String,
    pub config: serde_json::Value,
}

pub async fn create_project(
    _guard: RequireLocalhostOnly,
    State(state): State<AppState>,
    Json(body): Json<CreateProjectBody>,
) -> Result<(StatusCode, Json<crate::store::projects::ProjectRecord>), (StatusCode, Json<serde_json::Value>)> {
    let id = uuid::Uuid::new_v4().to_string();
    let config_json = serde_json::to_string(&body.config).unwrap_or_default();
    state.store.create_project(&id, &body.name, &body.workdir, &config_json)
        .map(|p| (StatusCode::CREATED, Json(p)))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") }))))
}

// GET /api/projects
pub async fn list_projects(
    _guard: RequireLocalhostOnly,
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::store::projects::ProjectRecord>>, (StatusCode, Json<serde_json::Value>)> {
    state.store.list_projects().map(Json).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") })))
    })
}

// GET /api/projects/{id}
pub async fn get_project(
    _guard: RequireLocalhostOnly,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<crate::store::projects::ProjectRecord>, (StatusCode, Json<serde_json::Value>)> {
    state.store.get_project(&id).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") })))
    })?.ok_or_else(|| (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "project not found" })))).map(Json)
}

// PUT /api/projects/{id}
#[derive(Deserialize)]
pub struct UpdateProjectBody {
    pub config: serde_json::Value,
}

pub async fn update_project(
    _guard: RequireLocalhostOnly,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateProjectBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let config_json = serde_json::to_string(&body.config).unwrap_or_default();
    state.store.update_project(&id, &config_json).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") })))
    })?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// DELETE /api/projects/{id}
pub async fn remove_project(
    _guard: RequireLocalhostOnly,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    state.store.remove_project(&id).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") })))
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// GET /api/agents
pub async fn list_agents(
    _guard: RequireLocalhostOnly,
) -> Json<Vec<crate::hardware::agents::AgentInfo>> {
    Json(crate::hardware::agents::detect_agents())
}
```

- [ ] **Step 2: Register routes in server.rs**

```rust
.route("/api/projects", get(http::list_projects).post(http::create_project))
.route("/api/projects/{id}", get(http::get_project).put(http::update_project).delete(http::remove_project))
.route("/api/agents", get(http::list_agents))
```

- [ ] **Step 3: Add agent detection background task in server.rs**

After the approval expiry task, add a 5-minute agent detection cycle that caches results in an Arc:

```rust
// Spawn agent detection task (5 minute interval)
{
    tokio::spawn(async move {
        loop {
            // Detection happens on-demand via the /api/agents endpoint
            // and in collect_machine_info(). No background caching needed
            // since detection is fast (<2s) and called infrequently.
            tokio::time::sleep(std::time::Duration::from_secs(300)).await;
        }
    });
}
```

Actually, agent detection is fast and called via `/api/agents` and `/api/system/hardware` on demand. No background task needed — remove this step.

- [ ] **Step 4: Verify compilation**

Run: `cd daemon && cargo build`

- [ ] **Step 5: Commit**

```bash
git add daemon/src/transport/http.rs daemon/src/server.rs
git commit -m "feat(daemon): add project CRUD and agent list HTTP endpoints"
```

---

## Task 6: Ghost CLI Crate — Init & Commands

**Files:**
- Create: `cli/Cargo.toml`
- Create: `cli/src/main.rs`
- Create: `cli/src/detect.rs`
- Create: `cli/src/init.rs`
- Create: `cli/src/commands.rs`

**Depends on:** Task 5 (needs project API)

- [ ] **Step 1: Create cli/Cargo.toml**

```toml
[package]
name = "ghost"
version = "0.2.1"
edition = "2024"

[[bin]]
name = "ghost"
path = "src/main.rs"

[dependencies]
clap = { version = "4", features = ["derive"] }
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls", "blocking"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
uuid = { version = "1", features = ["v4"] }
dialoguer = "0.11"
```

- [ ] **Step 2: Create main.rs with clap subcommands**

```rust
use clap::{Parser, Subcommand};

mod commands;
mod detect;
mod init;

#[derive(Parser)]
#[command(name = "ghost", about = "Ghost Protocol CLI")]
struct Cli {
    #[arg(long, env = "GHOST_DAEMON_URL", default_value = "http://127.0.0.1:8787")]
    daemon_url: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a Ghost Protocol project in the current directory
    Init,
    /// Show mesh status
    Status,
    /// List available agents
    Agents,
    /// List registered projects
    Projects,
    /// Start a chat with an agent
    Chat {
        /// Agent ID (e.g., claude-code, ollama:llama3)
        agent: String,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init => init::run(&cli.daemon_url).await,
        Commands::Status => commands::status(&cli.daemon_url).await,
        Commands::Agents => commands::agents(&cli.daemon_url).await,
        Commands::Projects => commands::projects(&cli.daemon_url).await,
        Commands::Chat { agent } => commands::chat(&cli.daemon_url, &agent).await,
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
```

- [ ] **Step 3: Create detect.rs**

Reuse the detection logic from daemon (copy the core functions — `which`, `detect_cli_version`, `detect_ollama_models`):

```rust
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub agent_type: String,
    pub command: String,
    pub version: Option<String>,
}

pub fn detect_local_agents() -> Vec<AgentInfo> {
    // Same detection logic as daemon/src/hardware/agents.rs::detect_agents()
    // Duplicated here because cli/ and daemon/ are separate crates
    // ... (full implementation same as Task 4)
}
```

- [ ] **Step 4: Create init.rs**

```rust
use std::path::PathBuf;
use dialoguer::{Input, MultiSelect};

use crate::detect;

pub async fn run(daemon_url: &str) -> Result<(), String> {
    let workdir = std::env::current_dir()
        .map_err(|e| format!("failed to get current directory: {e}"))?;
    let dir_name = workdir.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project")
        .to_string();

    println!("Initializing Ghost Protocol project...\n");

    // Project name
    let name: String = Input::new()
        .with_prompt("Project name")
        .default(dir_name)
        .interact_text()
        .map_err(|e| format!("input error: {e}"))?;

    // Detect agents
    println!("\nDetecting available agents...");
    let agents = detect::detect_local_agents();
    if agents.is_empty() {
        println!("  No agents detected. You can add them to .ghost/config.json later.");
    } else {
        for agent in &agents {
            let ver = agent.version.as_deref().unwrap_or("");
            println!("  ✓ {} {}", agent.name, ver);
        }
    }

    // Select agents
    let selected = if !agents.is_empty() {
        let labels: Vec<String> = agents.iter().map(|a| a.name.clone()).collect();
        let selections = MultiSelect::new()
            .with_prompt("Select agents for this project")
            .items(&labels)
            .defaults(&vec![true; labels.len()])
            .interact()
            .map_err(|e| format!("selection error: {e}"))?;
        selections.into_iter().map(|i| agents[i].clone()).collect::<Vec<_>>()
    } else {
        vec![]
    };

    // Build config
    let config = serde_json::json!({
        "name": name,
        "workdir": workdir.to_string_lossy(),
        "agents": selected.iter().map(|a| serde_json::json!({
            "id": a.id,
            "enabled": true,
            "preferredMachine": null
        })).collect::<Vec<_>>(),
        "machines": {},
        "commands": {
            "build": null,
            "test": null,
            "lint": null,
            "deploy": null
        },
        "environment": {}
    });

    // Write .ghost/config.json
    let ghost_dir = workdir.join(".ghost");
    std::fs::create_dir_all(&ghost_dir)
        .map_err(|e| format!("failed to create .ghost/: {e}"))?;
    let config_path = ghost_dir.join("config.json");
    std::fs::write(&config_path, serde_json::to_string_pretty(&config).unwrap())
        .map_err(|e| format!("failed to write config: {e}"))?;

    println!("\nCreated .ghost/config.json");

    // Register with daemon
    let client = reqwest::Client::new();
    match client
        .post(format!("{daemon_url}/api/projects"))
        .json(&serde_json::json!({
            "name": name,
            "workdir": workdir.to_string_lossy(),
            "config": config
        }))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            println!("Registered project with daemon.");
        }
        Ok(resp) => {
            let text = resp.text().await.unwrap_or_default();
            println!("Warning: failed to register with daemon: {text}");
            println!("(Daemon may not be running. Project config saved locally.)");
        }
        Err(_) => {
            println!("Warning: daemon not reachable. Project config saved locally.");
        }
    }

    println!("\nRun 'ghost chat <agent>' to start working.");
    Ok(())
}
```

- [ ] **Step 5: Create commands.rs**

```rust
pub async fn status(daemon_url: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let hardware: serde_json::Value = client
        .get(format!("{daemon_url}/api/system/hardware"))
        .send().await.map_err(|e| format!("daemon unreachable: {e}"))?
        .json().await.map_err(|e| format!("parse error: {e}"))?;

    let hosts: Vec<serde_json::Value> = client
        .get(format!("{daemon_url}/api/hosts"))
        .send().await.map_err(|e| format!("daemon unreachable: {e}"))?
        .json().await.map_err(|e| format!("parse error: {e}"))?;

    let hostname = hardware["hostname"].as_str().unwrap_or("?");
    let ip = hardware["tailscaleIp"].as_str().unwrap_or("?");
    let online_count = hosts.iter().filter(|h| h["status"].as_str() == Some("online")).count();

    println!("Ghost Protocol — {hostname} ({ip})");
    println!("Mesh: {} machine(s), {} online", hosts.len() + 1, online_count + 1);

    for host in &hosts {
        let name = host["name"].as_str().unwrap_or("?");
        let status = host["status"].as_str().unwrap_or("?");
        let dot = if status == "online" { "●" } else { "○" };
        println!("  {dot} {name} [{status}]");
    }
    Ok(())
}

pub async fn agents(daemon_url: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let agents: Vec<crate::detect::AgentInfo> = client
        .get(format!("{daemon_url}/api/agents"))
        .send().await.map_err(|e| format!("daemon unreachable: {e}"))?
        .json().await.map_err(|e| format!("parse error: {e}"))?;

    if agents.is_empty() {
        println!("No agents detected on this machine.");
    } else {
        println!("Available agents:");
        for agent in &agents {
            let ver = agent.version.as_deref().map(|v| format!(" v{v}")).unwrap_or_default();
            println!("  {} ({}){}", agent.name, agent.id, ver);
        }
    }
    Ok(())
}

pub async fn projects(daemon_url: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let projects: Vec<serde_json::Value> = client
        .get(format!("{daemon_url}/api/projects"))
        .send().await.map_err(|e| format!("daemon unreachable: {e}"))?
        .json().await.map_err(|e| format!("parse error: {e}"))?;

    if projects.is_empty() {
        println!("No registered projects. Run 'ghost init' in a project directory.");
    } else {
        println!("Registered projects:");
        for p in &projects {
            let name = p["name"].as_str().unwrap_or("?");
            let workdir = p["workdir"].as_str().unwrap_or("?");
            println!("  {name} ({workdir})");
        }
    }
    Ok(())
}

pub async fn chat(daemon_url: &str, agent: &str) -> Result<(), String> {
    // Placeholder — will be implemented in Task 9
    println!("Starting chat with {agent}...");
    println!("(Chat session support coming in the next task)");
    Ok(())
}
```

- [ ] **Step 6: Build the CLI**

Run: `cd cli && cargo build`
Expected: Compiles

- [ ] **Step 7: Test manually**

```bash
cd cli && cargo run -- help
cargo run -- agents
cargo run -- status
```

- [ ] **Step 8: Commit**

```bash
git add cli/
git commit -m "feat(cli): add ghost CLI with init, status, agents, projects commands"
```

---

## Task 7: Chat Adapters (Output Parsers)

**Files:**
- Create: `daemon/src/chat/mod.rs`
- Create: `daemon/src/chat/adapters/mod.rs`
- Create: `daemon/src/chat/adapters/generic.rs`
- Create: `daemon/src/chat/adapters/claude.rs`
- Create: `daemon/src/chat/adapters/ollama.rs`

**Depends on:** Task 3

- [ ] **Step 1: Create the adapter trait**

Create `daemon/src/chat/mod.rs`:
```rust
pub mod adapters;
```

Create `daemon/src/chat/adapters/mod.rs`:
```rust
pub mod generic;
pub mod claude;
pub mod ollama;

/// Parsed message from agent output
pub struct ParsedMessage {
    pub role: String,    // "assistant" or "system"
    pub content: String,
}

/// Trait for agent-specific output parsers
pub trait ChatAdapter: Send + Sync {
    /// Feed raw text from the agent's PTY output. Returns any complete messages parsed.
    fn feed(&mut self, text: &str) -> Vec<ParsedMessage>;

    /// Signal that the agent has finished (EOF). Flush any buffered content.
    fn flush(&mut self) -> Vec<ParsedMessage>;
}

pub fn adapter_for_agent(agent_id: &str) -> Box<dyn ChatAdapter> {
    if agent_id == "claude-code" || agent_id.starts_with("claude") {
        Box::new(claude::ClaudeAdapter::new())
    } else if agent_id.starts_with("ollama:") {
        Box::new(ollama::OllamaAdapter::new())
    } else {
        Box::new(generic::GenericAdapter::new())
    }
}
```

- [ ] **Step 2: Implement GenericAdapter**

Create `daemon/src/chat/adapters/generic.rs`:
```rust
use super::{ChatAdapter, ParsedMessage};

/// Simple adapter: buffers all output between user inputs as one assistant message.
pub struct GenericAdapter {
    buffer: String,
}

impl GenericAdapter {
    pub fn new() -> Self {
        Self { buffer: String::new() }
    }
}

impl ChatAdapter for GenericAdapter {
    fn feed(&mut self, text: &str) -> Vec<ParsedMessage> {
        self.buffer.push_str(text);
        // Don't emit messages incrementally — wait for flush or explicit boundary
        vec![]
    }

    fn flush(&mut self) -> Vec<ParsedMessage> {
        if self.buffer.trim().is_empty() {
            self.buffer.clear();
            return vec![];
        }
        let content = std::mem::take(&mut self.buffer).trim().to_string();
        vec![ParsedMessage { role: "assistant".to_string(), content }]
    }
}
```

- [ ] **Step 3: Implement ClaudeAdapter**

Create `daemon/src/chat/adapters/claude.rs`:
```rust
use super::{ChatAdapter, ParsedMessage};

/// Adapter for Claude Code CLI output.
/// Claude outputs markdown with tool call blocks. We accumulate until
/// we see a clear response boundary (prompt marker or EOF).
pub struct ClaudeAdapter {
    buffer: String,
}

impl ClaudeAdapter {
    pub fn new() -> Self {
        Self { buffer: String::new() }
    }
}

impl ChatAdapter for ClaudeAdapter {
    fn feed(&mut self, text: &str) -> Vec<ParsedMessage> {
        self.buffer.push_str(text);
        // Claude Code streams output continuously.
        // We'll emit on flush (when user sends next message or EOF).
        vec![]
    }

    fn flush(&mut self) -> Vec<ParsedMessage> {
        if self.buffer.trim().is_empty() {
            self.buffer.clear();
            return vec![];
        }
        let content = std::mem::take(&mut self.buffer).trim().to_string();
        vec![ParsedMessage { role: "assistant".to_string(), content }]
    }
}
```

- [ ] **Step 4: Implement OllamaAdapter**

Create `daemon/src/chat/adapters/ollama.rs`:
```rust
use super::{ChatAdapter, ParsedMessage};

/// Adapter for Ollama CLI (`ollama run <model>`).
/// Ollama outputs a response followed by a `>>> ` prompt.
pub struct OllamaAdapter {
    buffer: String,
}

impl OllamaAdapter {
    pub fn new() -> Self {
        Self { buffer: String::new() }
    }
}

impl ChatAdapter for OllamaAdapter {
    fn feed(&mut self, text: &str) -> Vec<ParsedMessage> {
        self.buffer.push_str(text);

        // Check if we see the Ollama prompt marker ">>> "
        let mut messages = vec![];
        while let Some(idx) = self.buffer.find(">>> ") {
            let content = self.buffer[..idx].trim().to_string();
            self.buffer = self.buffer[idx + 4..].to_string();
            if !content.is_empty() {
                messages.push(ParsedMessage { role: "assistant".to_string(), content });
            }
        }
        messages
    }

    fn flush(&mut self) -> Vec<ParsedMessage> {
        if self.buffer.trim().is_empty() {
            self.buffer.clear();
            return vec![];
        }
        let content = std::mem::take(&mut self.buffer).trim().to_string();
        vec![ParsedMessage { role: "assistant".to_string(), content }]
    }
}
```

- [ ] **Step 5: Register chat module in daemon**

Add `pub mod chat;` to `daemon/src/main.rs` (or wherever modules are registered — check the existing module declarations).

- [ ] **Step 6: Verify compilation**

Run: `cd daemon && cargo build`

- [ ] **Step 7: Commit**

```bash
git add daemon/src/chat/
git commit -m "feat(daemon): add chat adapter trait with claude, ollama, generic parsers"
```

---

## Task 8: Chat Session Endpoints & WebSocket

**Files:**
- Modify: `daemon/src/transport/http.rs`
- Modify: `daemon/src/transport/ws.rs`
- Modify: `daemon/src/server.rs`

**Depends on:** Task 1, Task 3, Task 7

- [ ] **Step 1: Add chat session creation endpoint**

In `http.rs`, add:

```rust
// POST /api/chat/sessions
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateChatSessionBody {
    pub agent_id: String,
    pub project_id: Option<String>,
    pub workdir: Option<String>,
}

pub async fn create_chat_session(
    _tier: RequireFullAccess,
    needs_approval: OptionalNeedsApproval,
    client_ip: ClientIp,
    State(state): State<AppState>,
    Json(body): Json<CreateChatSessionBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    // Check approval
    if needs_approval.0 {
        let host_id = state.store.resolve_host_id_by_ip(&client_ip.0).ok().flatten().unwrap_or_default();
        let id = uuid::Uuid::new_v4().to_string();
        let expires_at = (chrono::Utc::now() + chrono::TimeDelta::seconds(120)).to_rfc3339();
        let body_json = serde_json::to_string(&body).ok();
        if let Ok(approval) = state.store.create_approval(&id, &host_id, "POST", "/api/chat/sessions", body_json.as_deref(), &expires_at) {
            return Err((StatusCode::ACCEPTED, Json(serde_json::json!({
                "approvalRequired": true, "approvalId": approval.id, "expiresAt": approval.expires_at
            }))));
        }
    }

    // Determine agent command
    let agents = crate::hardware::agents::detect_agents();
    let agent = agents.iter().find(|a| a.id == body.agent_id).ok_or_else(|| {
        (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": format!("agent '{}' not found", body.agent_id) })))
    })?;

    let workdir = body.workdir.clone().unwrap_or_else(|| {
        std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string())
    });

    // Create terminal session with type "chat"
    let session = state.manager
        .create_session("chat", Some(&agent.name), &workdir)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e }))))?;

    // Create system message
    state.store.create_chat_message(
        &uuid::Uuid::new_v4().to_string(),
        &session.id,
        "system",
        &format!("Chat session started with {} on {}", agent.name, workdir),
    ).ok();

    // Auto-capture outcome
    let source_host_id = state.store.resolve_host_id_by_ip(&client_ip.0).ok().flatten();
    state.store.create_outcome(
        &uuid::Uuid::new_v4().to_string(), "daemon", source_host_id.as_deref(),
        "chat", "chat_session_created", Some(&agent.name), None, "success", None, None,
        Some(&serde_json::json!({"agentId": body.agent_id, "workdir": workdir}).to_string()),
    ).ok();

    Ok((StatusCode::CREATED, Json(serde_json::json!({
        "session": session,
        "agent": agent,
    }))))
}

// GET /api/chat/sessions/{id}/messages
pub async fn list_chat_messages(
    _tier: RequireReadOnly,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<ChatMessagesQuery>,
) -> Result<Json<Vec<crate::store::chat::ChatMessage>>, (StatusCode, Json<serde_json::Value>)> {
    state.store.list_chat_messages(&id, params.after.as_deref(), params.limit.unwrap_or(100))
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") }))))
}

#[derive(Deserialize)]
pub struct ChatMessagesQuery {
    pub after: Option<String>,
    pub limit: Option<usize>,
}

// POST /api/chat/sessions/{id}/message
#[derive(Deserialize)]
pub struct SendChatMessageBody {
    pub content: String,
}

pub async fn send_chat_message(
    _tier: RequireFullAccess,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SendChatMessageBody>,
) -> Result<Json<crate::store::chat::ChatMessage>, (StatusCode, Json<serde_json::Value>)> {
    // Store user message
    let msg = state.store.create_chat_message(
        &uuid::Uuid::new_v4().to_string(), &id, "user", &body.content,
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") }))))?;

    // Send to agent's stdin (append newline)
    state.manager.send_input(&id, body.content.as_bytes(), true).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e }))))?;

    Ok(Json(msg))
}
```

- [ ] **Step 2: Register chat routes in server.rs**

```rust
.route("/api/chat/sessions", post(http::create_chat_session))
.route("/api/chat/sessions/{id}/messages", get(http::list_chat_messages))
.route("/api/chat/sessions/{id}/message", post(http::send_chat_message))
```

- [ ] **Step 3: Add chat WebSocket ops**

In `ws.rs`, add to `WsMessage`:
```rust
#[serde(default)]
content: Option<String>,
```

Add new ops in `handle_op`:
```rust
"subscribe_chat" => {
    let Some(session_id) = msg.session_id else {
        return send_error(socket, "subscribe_chat requires sessionId").await;
    };
    // Replay chat messages from DB
    match state.store.list_chat_messages(&session_id, None, 1000) {
        Ok(messages) => {
            for m in &messages {
                let reply = serde_json::json!({
                    "op": "chat_message",
                    "message": m,
                });
                send_json(socket, &reply).await?;
            }
        }
        Err(e) => return send_error(socket, &format!("db error: {e}")).await,
    }
    // Subscribe to terminal broadcast for live updates
    // (chat adapter will parse and send chat_message ops)
    // For now, fall through to subscribe_terminal behavior
    let reply = serde_json::json!({ "op": "subscribed_chat", "sessionId": session_id });
    send_json(socket, &reply).await
}

"send_chat_message" => {
    if tier < crate::middleware::permissions::PeerTier::FullAccess {
        return send_error(socket, "write operations require full-access tier").await;
    }
    let Some(session_id) = msg.session_id else {
        return send_error(socket, "send_chat_message requires sessionId").await;
    };
    let Some(content) = msg.content else {
        return send_error(socket, "send_chat_message requires content").await;
    };
    // Store user message
    let msg_id = uuid::Uuid::new_v4().to_string();
    state.store.create_chat_message(&msg_id, &session_id, "user", &content).ok();
    // Send to agent stdin
    if let Err(e) = state.manager.send_input(&session_id, content.as_bytes(), true).await {
        return send_error(socket, &format!("input error: {e}")).await;
    }
    Ok(())
}
```

- [ ] **Step 4: Verify compilation**

Run: `cd daemon && cargo build`

- [ ] **Step 5: Commit**

```bash
git add daemon/src/transport/http.rs daemon/src/transport/ws.rs daemon/src/server.rs
git commit -m "feat(daemon): add chat session endpoints and WebSocket ops"
```

---

## Task 9: MCP Agents Resource & Tool

**Files:**
- Modify: `daemon/src/mcp/resources.rs`
- Modify: `daemon/src/mcp/transport.rs`

**Depends on:** Task 4

- [ ] **Step 1: Add agents resource and tool**

In `resources.rs`, add to `impl ResourceBuilder`:

```rust
pub async fn available_agents(&self) -> Result<Value, Box<dyn std::error::Error>> {
    let local_info = self.machine_info().await?;
    let local_agents = local_info["tools"]["agents"].clone();
    let hosts_data = self.network_hosts().await?;
    let perms_data: Value = match self.client()
        .get(format!("{}/api/permissions", self.base()))
        .send().await
    {
        Ok(resp) => resp.json().await.unwrap_or(json!([])),
        Err(_) => json!([]),
    };

    let mut peers = serde_json::Map::new();
    if let Some(hosts) = hosts_data["hosts"].as_array() {
        for h in hosts {
            let name = h["name"].as_str().unwrap_or("?");
            let agents = h.get("capabilities")
                .and_then(|c| c.get("agents"))
                .cloned()
                .unwrap_or(json!([]));
            peers.insert(name.to_string(), agents);
        }
    }

    Ok(json!({
        "local": local_agents,
        "peers": peers,
    }))
}
```

Add to `resource_list()`:
```rust
json!({
    "uri": "ghost://agents/available",
    "name": "Available Agents",
    "description": "Agent runtimes available across the mesh: which agents can run where",
    "mimeType": "application/json"
}),
```

- [ ] **Step 2: Register in transport.rs**

Add to resource read match:
```rust
"ghost://agents/available" => builder.available_agents().await,
```

Add `ghost_list_agents` to `tool_definitions()`:
```rust
{
    "name": "ghost_list_agents",
    "description": "List available agent runtimes across the mesh. Shows which agents (Claude Code, Ollama models, Hermes, etc.) are available on which machines.",
    "inputSchema": { "type": "object", "properties": {}, "required": [] }
}
```

Add to `call_tool()`:
```rust
"ghost_list_agents" => {
    let data = builder.available_agents().await?;
    Ok(serde_json::to_string_pretty(&data)?)
}
```

- [ ] **Step 3: Add agents to context briefing**

In `context_briefing()`, after the permission notes section, add:
```rust
// Available agents
let local_info = self.machine_info().await.unwrap_or(json!({}));
if let Some(agents) = local_info["tools"]["agents"].as_array() {
    if !agents.is_empty() {
        let agent_names: Vec<&str> = agents.iter()
            .filter_map(|a| a["name"].as_str())
            .collect();
        lines.push(format!("\nAvailable agents on {hostname}: {}", agent_names.join(", ")));
    }
}

if let Some(hosts) = hosts_data["hosts"].as_array() {
    for h in hosts {
        if let Some(agents) = h.get("capabilities").and_then(|c| c["agents"].as_array()) {
            if !agents.is_empty() {
                let name = h["name"].as_str().unwrap_or("?");
                let agent_names: Vec<&str> = agents.iter()
                    .filter_map(|a| a["name"].as_str())
                    .collect();
                lines.push(format!("Available agents on {name}: {}", agent_names.join(", ")));
            }
        }
    }
}
```

- [ ] **Step 4: Verify compilation**

Run: `cd daemon && cargo build`

- [ ] **Step 5: Commit**

```bash
git add daemon/src/mcp/resources.rs daemon/src/mcp/transport.rs
git commit -m "feat(daemon): add agents MCP resource, tool, and briefing section"
```

---

## Task 10: Desktop Types, API & Sidebar Agent Display

**Files:**
- Modify: `desktop/src/types.ts`
- Modify: `desktop/src/api.ts`
- Modify: `desktop/src/components/Sidebar.tsx`

**Depends on:** Task 5

- [ ] **Step 1: Add TypeScript types**

At the end of `desktop/src/types.ts`:
```typescript
// --- Agent & project types (Phase 3a) ---

export type AgentInfo = {
  id: string;
  name: string;
  agentType: "cli" | "api";
  command: string;
  version: string | null;
};

export type ProjectRecord = {
  id: string;
  name: string;
  workdir: string;
  configJson: string;
  registeredAt: string;
  updatedAt: string;
};

export type ChatMessage = {
  id: string;
  sessionId: string;
  role: "user" | "assistant" | "system";
  content: string;
  createdAt: string;
};
```

- [ ] **Step 2: Add API functions**

At the end of `desktop/src/api.ts`:
```typescript
import type { AgentInfo, ProjectRecord, ChatMessage } from "./types";

export async function listAgents(daemonUrl: string): Promise<AgentInfo[]> {
  return api<AgentInfo[]>(daemonUrl, "/api/agents");
}

export async function listProjects(daemonUrl: string): Promise<ProjectRecord[]> {
  return api<ProjectRecord[]>(daemonUrl, "/api/projects");
}

export async function createChatSession(
  daemonUrl: string,
  agentId: string,
  projectId?: string,
  workdir?: string,
): Promise<{ session: any; agent: AgentInfo }> {
  return api(daemonUrl, "/api/chat/sessions", {
    method: "POST",
    body: JSON.stringify({ agentId, projectId, workdir }),
  });
}

export async function listChatMessages(
  daemonUrl: string,
  sessionId: string,
): Promise<ChatMessage[]> {
  return api<ChatMessage[]>(daemonUrl, `/api/chat/sessions/${sessionId}/messages`);
}

export async function sendChatMessage(
  daemonUrl: string,
  sessionId: string,
  content: string,
): Promise<ChatMessage> {
  return api(daemonUrl, `/api/chat/sessions/${sessionId}/message`, {
    method: "POST",
    body: JSON.stringify({ content }),
  });
}
```

- [ ] **Step 3: Update Sidebar to show agents per connection**

In `desktop/src/components/Sidebar.tsx`, update the connection row to show agent info when available. In the connection list mapping, after the host name/IP:

```tsx
{conn.host.capabilities?.agents && conn.host.capabilities.agents.length > 0 && (
  <span className="sidebar-host-agents muted">
    {conn.host.capabilities.agents.map(a => a.name).join(", ")}
  </span>
)}
```

This requires the `HostConnection` type to carry capabilities. Check if it already does via the host data — if not, the sidebar can fetch agents from the API separately.

- [ ] **Step 4: Verify TypeScript compilation**

Run: `cd desktop && npx tsc --noEmit`

- [ ] **Step 5: Commit**

```bash
git add desktop/src/types.ts desktop/src/api.ts desktop/src/components/Sidebar.tsx
git commit -m "feat(desktop): add agent/project/chat types, API functions, sidebar agent display"
```

---

## Task 11: Revive ChatView & Wire Into App

**Files:**
- Modify: `desktop/src/components/ChatView.tsx`
- Modify: `desktop/src/App.tsx`

**Depends on:** Task 8, Task 10

- [ ] **Step 1: Rewrite ChatView with agent/machine picker**

The existing ChatView was built for Hermes. Rewrite it with:
- Agent + machine selector (dropdowns)
- Message list with role-based styling
- Input composer
- Streaming indicator
- Start new chat button

Props:
```typescript
type Props = {
  daemonUrl: string;
  hosts: SavedHost[];
  agents: AgentInfo[];
};
```

The component manages its own state: selected agent, selected machine, active chat session, messages, input. On "Start Chat", it calls `createChatSession` and then subscribes via WebSocket `subscribe_chat` op.

- [ ] **Step 2: Wire ChatView into App.tsx**

Uncomment/add the ChatView import:
```typescript
import { ChatView } from "./components/ChatView";
```

Add agents state:
```typescript
const [agents, setAgents] = useState<AgentInfo[]>([]);

const refreshAgents = useCallback(async () => {
  try {
    const a = await listAgents(LOCAL_DAEMON);
    setAgents(a);
  } catch { /* ignore */ }
}, []);

useEffect(() => {
  refreshAgents();
  const interval = setInterval(refreshAgents, 300000); // 5 min
  return () => clearInterval(interval);
}, [refreshAgents]);
```

Render ChatView in the chat section:
```tsx
<div style={{ display: mainView === "chat" ? "flex" : "none", flexDirection: "column", flex: 1, minHeight: 0 }}>
  <ChatView daemonUrl={LOCAL_DAEMON} hosts={hosts} agents={agents} />
</div>
```

- [ ] **Step 3: Verify build**

Run: `cd desktop && npm run build`

- [ ] **Step 4: Commit**

```bash
git add desktop/src/components/ChatView.tsx desktop/src/App.tsx
git commit -m "feat(desktop): revive ChatView with agent/machine picker"
```

---

## Task 12: Terminal Help Text

**Files:**
- Modify: `daemon/src/terminal/manager.rs`

**Depends on:** Task 4

- [ ] **Step 1: Inject help text on session creation**

In `daemon/src/terminal/manager.rs`, in the `create_session` method, after the session is running and the broadcaster is set up, inject a system chunk:

```rust
// Inject welcome message for local terminal sessions
if mode != "chat" {
    let hostname = crate::host::detect::get_system_info().hostname;
    let ip = crate::host::detect::get_tailscale_ip().unwrap_or_else(|| "local".to_string());
    let version = env!("CARGO_PKG_VERSION");
    let welcome = format!(
        "\x1b[2m\
Ghost Protocol v{version} — {hostname} ({ip})\n\
\n\
Commands:\n\
  ghost init          Set up a project in this directory\n\
  ghost status        Mesh overview (machines, sessions)\n\
  ghost agents        Available agents across the mesh\n\
  ghost chat <agent>  Start a chat with an agent\n\
  ghost projects      Registered projects\n\
  ghost help          Full command reference\n\
\x1b[0m\n"
    );
    self.store.append_terminal_chunk(&id, "system", &welcome).ok();
}
```

The `\x1b[2m...\x1b[0m` wraps the help text in dim ANSI so it's visible but doesn't dominate.

- [ ] **Step 2: Verify compilation**

Run: `cd daemon && cargo build`

- [ ] **Step 3: Commit**

```bash
git add daemon/src/terminal/manager.rs
git commit -m "feat(daemon): inject ghost CLI help text on terminal session start"
```

---

## Task 13: Package Script Update

**Files:**
- Modify: `scripts/package-linux.sh`

**Depends on:** Task 6

- [ ] **Step 1: Add ghost CLI to build and package**

In `scripts/package-linux.sh`, after the daemon build, add:

```bash
echo "==> Building CLI..."
cd "$ROOT_DIR/cli"
cargo build --release 2>&1 | tail -10
```

In the packaging section, after copying the daemon binary, add:

```bash
# CLI binary
cp "$ROOT_DIR/cli/target/release/ghost" "$DIST_DIR/ghost"
```

In the install script, add:
```bash
sudo install -Dm755 "$SCRIPT_DIR/ghost" /usr/local/bin/ghost
```

In the uninstall script, add:
```bash
sudo rm -f /usr/local/bin/ghost
```

- [ ] **Step 2: Commit**

```bash
git add scripts/package-linux.sh
git commit -m "chore: add ghost CLI binary to release package"
```

---

## Verification

After all tasks:

1. **Daemon tests:** `cd daemon && cargo test` — all pass
2. **Daemon build:** `cd daemon && cargo build` — clean
3. **CLI build:** `cd cli && cargo build` — clean
4. **Desktop build:** `cd desktop && npm run build` — clean
5. **Manual tests:**
   ```bash
   # Start daemon
   cd daemon && cargo run -- serve &

   # Test ghost CLI
   cd cli && cargo run -- agents
   cd /tmp/test-project && cargo run --manifest-path ~/projects/personal/ghost-protocol/cli/Cargo.toml -- init
   cargo run --manifest-path ~/projects/personal/ghost-protocol/cli/Cargo.toml -- projects
   cargo run --manifest-path ~/projects/personal/ghost-protocol/cli/Cargo.toml -- status

   # Test project API
   curl localhost:8787/api/projects
   curl localhost:8787/api/agents

   # Desktop: Chat view should be accessible from sidebar nav
   cd desktop && npm run tauri dev
   ```

---

## Parallelism Map

```
Task 1 (migration) ─┬─→ Task 2 (projects store)  ──→ Task 5 (HTTP endpoints) ──→ Task 6 (CLI) ──→ Task 13 (package)
                     ├─→ Task 3 (chat store)       ──→ Task 7 (adapters) ────────→ Task 8 (chat endpoints)
                     └─→ Task 12 (help text)
Task 4 (agent detection) ─┬─→ Task 5 (endpoints)
                           ├─→ Task 9 (MCP)
                           └─→ Task 12 (help text)
Task 10 (desktop types) ──→ Task 11 (ChatView + App)
```

**Batch 1 (parallel):** Task 1, Task 4, Task 10
**Batch 2 (parallel):** Task 2, Task 3, Task 12
**Batch 3 (parallel):** Task 5, Task 7, Task 9
**Batch 4 (parallel):** Task 6, Task 8
**Batch 5 (parallel):** Task 11, Task 13
