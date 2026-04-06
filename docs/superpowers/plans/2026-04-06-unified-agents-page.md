# Unified Agents Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the separate Chat and Terminal views with a single Agents page that lets users create, switch between, and monitor agent sessions — with real-time streaming chat, terminal TUI, and session handoff between modes.

**Architecture:** New `ChatProcessManager` in the daemon spawns agents as direct child processes with piped stdin/stdout for chat mode (parallel to the existing tmux-based `TerminalManager` for terminal mode). Frontend gets a unified `AgentsView` with resizable session sidebar, mode toggle, and streaming chat renderer. WebSocket protocol extended with `chat_delta`, `chat_message`, `chat_status`, and `session_meta` operations.

**Tech Stack:** Rust (tokio, axum, serde, rusqlite), React 19, TypeScript, xterm.js, WebSocket

**Spec:** `docs/superpowers/specs/2026-04-06-unified-agents-page-design.md`

---

## File Structure

### Daemon (Rust)

| File | Action | Responsibility |
|---|---|---|
| `daemon/migrations/007_session_delegation.sql` | Create | Add parent_session_id, host_id, host_name columns |
| `daemon/src/store/mod.rs` | Modify | Register migration 007 |
| `daemon/src/store/sessions.rs` | Modify | Add new columns to TerminalSessionRecord, update queries |
| `daemon/src/chat/manager.rs` | Create | ChatProcessManager — subprocess spawning, stdin/stdout piping, streaming |
| `daemon/src/chat/broadcaster.rs` | Create | ChatBroadcaster — broadcast channel for chat events (deltas, messages, status, meta) |
| `daemon/src/chat/mod.rs` | Modify | Register new modules |
| `daemon/src/chat/adapters/mod.rs` | Modify | Extend ParsedMessage with delta/metadata fields, add ChatEvent enum |
| `daemon/src/chat/adapters/claude.rs` | Modify | Real NDJSON parser for Claude Code stream-json output |
| `daemon/src/chat/adapters/ollama.rs` | Modify | Real streaming parser for Ollama stdout |
| `daemon/src/chat/adapters/generic.rs` | Modify | Line-buffered delta streaming |
| `daemon/src/transport/http.rs` | Modify | Add switch-mode endpoint, update AppState with ChatProcessManager, update create_chat_session |
| `daemon/src/transport/ws.rs` | Modify | Add subscribe_chat handler, chat_delta/chat_message/chat_status/session_meta forwarding |
| `daemon/src/server.rs` | Modify | Create ChatProcessManager, add to AppState, register switch-mode route |
| `daemon/src/hardware/agents.rs` | Modify | Add persistence flag to AgentInfo |
| `daemon/src/mcp/transport.rs` | Modify | Add ghost_spawn_remote_session tool |

### Desktop (TypeScript/React)

| File | Action | Responsibility |
|---|---|---|
| `desktop/src/types.ts` | Modify | Add chat event types, update TerminalSession with new fields, add SessionMode |
| `desktop/src/api.ts` | Modify | Add switchSessionMode(), update createChatSession() |
| `desktop/src/hooks/useChatSocket.ts` | Create | WebSocket hook for chat mode — deltas, messages, status, meta |
| `desktop/src/components/AgentsView.tsx` | Create | Top-level page: agent picker, session list, main area |
| `desktop/src/components/SessionSidebar.tsx` | Create | Resizable left panel with active + previous session cards |
| `desktop/src/components/SessionHeader.tsx` | Create | Mode toggle, metadata row, end session |
| `desktop/src/components/ChatRenderer.tsx` | Create | Message bubbles, streaming deltas, composer |
| `desktop/src/components/TerminalRenderer.tsx` | Create | xterm.js embed extracted from TerminalWorkspace |
| `desktop/src/components/Sidebar.tsx` | Modify | Replace "Chat" + "Terminal" nav items with single "Agents" |
| `desktop/src/App.tsx` | Modify | Replace ChatView + TerminalWorkspace with AgentsView, update MainView type |
| `desktop/src/App.css` | Modify | Add agents page styles (session cards, chat bubbles, resizable sidebar) |

---

## Task 1: Database Migration — Session Delegation Columns

**Files:**
- Create: `daemon/migrations/007_session_delegation.sql`
- Modify: `daemon/src/store/mod.rs:28-45`
- Modify: `daemon/src/store/sessions.rs`

- [ ] **Step 1: Create migration file**

```sql
-- daemon/migrations/007_session_delegation.sql
-- Adds parent_session_id, host_id, host_name to terminal_sessions
-- for delegated sessions and multi-host tracking.
-- Idempotent: uses ALTER TABLE with IF NOT EXISTS pattern via try/catch in Rust.
```

Since SQLite doesn't support `ALTER TABLE ... ADD COLUMN IF NOT EXISTS`, we use the same idempotent pattern as existing migrations (the Rust code wraps ALTER in `conn.execute_batch` and ignores "duplicate column" errors).

Write this file:

```sql
-- 007_session_delegation.sql
-- Parent-child session tracking and host identity
ALTER TABLE terminal_sessions ADD COLUMN parent_session_id TEXT REFERENCES terminal_sessions(id) ON DELETE SET NULL;
ALTER TABLE terminal_sessions ADD COLUMN host_id TEXT;
ALTER TABLE terminal_sessions ADD COLUMN host_name TEXT;
```

- [ ] **Step 2: Register migration in store/mod.rs**

In `daemon/src/store/mod.rs`, add the migration to the list. Find the block that runs migrations (after `006_projects_and_chat.sql`) and add:

```rust
conn.execute_batch(include_str!("../migrations/007_session_delegation.sql"))
    .ok(); // idempotent — ALTER TABLE may fail if columns already exist
```

This goes after the existing `006` migration execution, using `.ok()` to silently handle the "duplicate column" error on re-runs.

- [ ] **Step 3: Add new fields to TerminalSessionRecord**

In `daemon/src/store/sessions.rs`, update the `TerminalSessionRecord` struct:

```rust
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
    pub session_type: String,
    pub project_id: Option<String>,
    pub parent_session_id: Option<String>,
    pub host_id: Option<String>,
    pub host_name: Option<String>,
}
```

Update the `row_to_record` helper (or wherever rows are mapped) to read the three new columns. They'll be NULL for all existing rows. Update `list_terminal_sessions` and `get_terminal_session` SELECT queries to include the new columns.

- [ ] **Step 4: Build and verify**

```bash
cd daemon && cargo build 2>&1 | head -30
```

Expected: Compiles successfully. If there are errors from queries not selecting new columns, fix the SELECT statements.

- [ ] **Step 5: Commit**

```bash
git add daemon/migrations/007_session_delegation.sql daemon/src/store/mod.rs daemon/src/store/sessions.rs
git commit -m "feat(db): add parent_session_id, host_id, host_name to sessions"
```

---

## Task 2: Agent Persistence Flag

**Files:**
- Modify: `daemon/src/hardware/agents.rs`

- [ ] **Step 1: Add persistent field to AgentInfo**

In `daemon/src/hardware/agents.rs`, add a `persistent` field to the struct:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub agent_type: String,
    pub command: String,
    pub version: Option<String>,
    pub persistent: bool,
}
```

- [ ] **Step 2: Set persistent flag per agent in detect_agents()**

Update each agent detection block in the `detect_agents()` function:

For Claude Code:
```rust
agents.push(AgentInfo {
    id: "claude-code".into(),
    name: "Claude Code".into(),
    agent_type: "cli".into(),
    command: "claude".into(),
    version,
    persistent: true, // Claude Code supports --session-id / --resume
});
```

For all others (Hermes, Aider, OpenClaw, Ollama models), set `persistent: false`.

For custom agents loaded from config, default to `persistent: false` (can be overridden in agents.json).

- [ ] **Step 3: Build and verify**

```bash
cd daemon && cargo build 2>&1 | head -20
```

Expected: Clean compile.

- [ ] **Step 4: Commit**

```bash
git add daemon/src/hardware/agents.rs
git commit -m "feat(agents): add persistent flag for session handoff support"
```

---

## Task 3: ChatBroadcaster — Event Channel for Chat Sessions

**Files:**
- Create: `daemon/src/chat/broadcaster.rs`
- Modify: `daemon/src/chat/mod.rs`

- [ ] **Step 1: Create ChatEvent enum and ChatBroadcaster**

Create `daemon/src/chat/broadcaster.rs`:

```rust
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::broadcast;
use serde::Serialize;

use crate::store::chat::ChatMessage;

const BROADCAST_CAPACITY: usize = 256;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatEvent {
    Delta {
        session_id: String,
        message_id: String,
        delta: String,
    },
    Message {
        message: ChatMessage,
    },
    Status {
        session_id: String,
        status: String, // "thinking", "tool_use", "idle", "error"
    },
    Meta {
        session_id: String,
        tokens: Option<u64>,
        context_pct: Option<f64>,
    },
}

pub struct ChatBroadcaster {
    sender: broadcast::Sender<ChatEvent>,
    subscriber_count: AtomicUsize,
}

impl ChatBroadcaster {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            sender,
            subscriber_count: AtomicUsize::new(0),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ChatEvent> {
        self.subscriber_count.fetch_add(1, Ordering::SeqCst);
        self.sender.subscribe()
    }

    pub fn unsubscribe(&self) {
        self.subscriber_count.fetch_sub(1, Ordering::SeqCst);
    }

    pub fn send(&self, event: ChatEvent) {
        let _ = self.sender.send(event);
    }

    pub fn subscriber_count(&self) -> usize {
        self.subscriber_count.load(Ordering::SeqCst)
    }
}
```

- [ ] **Step 2: Update chat/mod.rs**

```rust
pub mod adapters;
pub mod broadcaster;
pub mod manager;
```

- [ ] **Step 3: Build and verify**

```bash
cd daemon && cargo build 2>&1 | head -20
```

Expected: Compiles (manager module doesn't exist yet, so temporarily comment out `pub mod manager;` or create an empty file).

- [ ] **Step 4: Commit**

```bash
git add daemon/src/chat/broadcaster.rs daemon/src/chat/mod.rs
git commit -m "feat(chat): add ChatBroadcaster with ChatEvent enum"
```

---

## Task 4: Chat Adapters — Real Streaming Parsers

**Files:**
- Modify: `daemon/src/chat/adapters/mod.rs`
- Modify: `daemon/src/chat/adapters/claude.rs`
- Modify: `daemon/src/chat/adapters/ollama.rs`
- Modify: `daemon/src/chat/adapters/generic.rs`

- [ ] **Step 1: Extend ParsedMessage and ChatAdapter trait**

Replace `daemon/src/chat/adapters/mod.rs`:

```rust
pub mod generic;
pub mod claude;
pub mod ollama;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ParsedMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub enum AdapterEvent {
    /// Streaming text delta — partial token for real-time display
    Delta(String),
    /// Complete message parsed from agent output
    Message(ParsedMessage),
    /// Agent status change (thinking, tool_use, idle, error)
    Status(String),
    /// Metadata update (tokens, context percentage)
    Meta { tokens: Option<u64>, context_pct: Option<f64> },
}

pub trait ChatAdapter: Send + Sync {
    /// Feed raw stdout text from the agent. Returns events to broadcast immediately.
    fn feed(&mut self, text: &str) -> Vec<AdapterEvent>;
    /// Flush remaining buffer (called when process exits). Returns final events.
    fn flush(&mut self) -> Vec<AdapterEvent>;
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

- [ ] **Step 2: Implement Claude adapter with NDJSON parsing**

Replace `daemon/src/chat/adapters/claude.rs`:

```rust
use super::{AdapterEvent, ChatAdapter, ParsedMessage};

/// Parses NDJSON output from `claude -p --output-format stream-json`.
///
/// Key event types from Claude Code SDK:
/// - {"type":"assistant","message":{...}} — start of response
/// - {"type":"content_block_delta","delta":{"type":"text_delta","text":"..."}} — streaming text
/// - {"type":"content_block_start","content_block":{"type":"tool_use",...}} — tool call
/// - {"type":"result","result":{...},"usage":{...}} — final result with token usage
pub struct ClaudeAdapter {
    line_buffer: String,
    current_message: String,
}

impl ClaudeAdapter {
    pub fn new() -> Self {
        Self {
            line_buffer: String::new(),
            current_message: String::new(),
        }
    }

    fn parse_line(&mut self, line: &str) -> Vec<AdapterEvent> {
        let mut events = Vec::new();

        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            return events;
        };

        match v.get("type").and_then(|t| t.as_str()) {
            Some("assistant") => {
                events.push(AdapterEvent::Status("thinking".into()));
            }
            Some("content_block_delta") => {
                if let Some(text) = v
                    .pointer("/delta/text")
                    .and_then(|t| t.as_str())
                {
                    self.current_message.push_str(text);
                    events.push(AdapterEvent::Delta(text.to_string()));
                }
            }
            Some("content_block_start") => {
                if let Some("tool_use") = v
                    .pointer("/content_block/type")
                    .and_then(|t| t.as_str())
                {
                    events.push(AdapterEvent::Status("tool_use".into()));
                }
            }
            Some("content_block_stop") => {
                // Content block ended — emit idle status
            }
            Some("result") => {
                // Final message — emit as complete message
                if !self.current_message.is_empty() {
                    events.push(AdapterEvent::Message(ParsedMessage {
                        role: "assistant".into(),
                        content: std::mem::take(&mut self.current_message),
                    }));
                }

                // Extract token usage if present
                let tokens = v.pointer("/usage/output_tokens").and_then(|t| t.as_u64());
                let input_tokens = v.pointer("/usage/input_tokens").and_then(|t| t.as_u64());
                let total = match (tokens, input_tokens) {
                    (Some(o), Some(i)) => Some(o + i),
                    (Some(o), None) => Some(o),
                    _ => None,
                };
                if total.is_some() {
                    events.push(AdapterEvent::Meta {
                        tokens: total,
                        context_pct: None, // Claude doesn't expose this directly
                    });
                }

                events.push(AdapterEvent::Status("idle".into()));
            }
            _ => {}
        }

        events
    }
}

impl ChatAdapter for ClaudeAdapter {
    fn feed(&mut self, text: &str) -> Vec<AdapterEvent> {
        self.line_buffer.push_str(text);
        let mut events = Vec::new();

        while let Some(newline_pos) = self.line_buffer.find('\n') {
            let line: String = self.line_buffer.drain(..=newline_pos).collect();
            let line = line.trim();
            if !line.is_empty() {
                events.extend(self.parse_line(line));
            }
        }

        events
    }

    fn flush(&mut self) -> Vec<AdapterEvent> {
        let mut events = Vec::new();

        // Parse any remaining line
        if !self.line_buffer.is_empty() {
            let remaining = std::mem::take(&mut self.line_buffer);
            let line = remaining.trim();
            if !line.is_empty() {
                events.extend(self.parse_line(line));
            }
        }

        // Emit any accumulated message not yet flushed
        if !self.current_message.is_empty() {
            events.push(AdapterEvent::Message(ParsedMessage {
                role: "assistant".into(),
                content: std::mem::take(&mut self.current_message),
            }));
        }

        events
    }
}
```

- [ ] **Step 3: Implement Ollama adapter with streaming**

Replace `daemon/src/chat/adapters/ollama.rs`:

```rust
use super::{AdapterEvent, ChatAdapter, ParsedMessage};

/// Parses output from `ollama run {model}`.
/// Ollama streams tokens directly to stdout character-by-character.
/// Detects response boundaries via ">>> " prompt marker.
pub struct OllamaAdapter {
    buffer: String,
    in_response: bool,
}

impl OllamaAdapter {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            in_response: false,
        }
    }
}

impl ChatAdapter for OllamaAdapter {
    fn feed(&mut self, text: &str) -> Vec<AdapterEvent> {
        let mut events = Vec::new();

        for ch in text.chars() {
            self.buffer.push(ch);

            // Check for prompt boundary ">>> "
            if self.buffer.ends_with(">>> ") {
                // Everything before ">>> " is the response
                let response = self.buffer[..self.buffer.len() - 4].to_string();
                if !response.is_empty() && self.in_response {
                    events.push(AdapterEvent::Message(ParsedMessage {
                        role: "assistant".into(),
                        content: response.trim().to_string(),
                    }));
                    events.push(AdapterEvent::Status("idle".into()));
                }
                self.buffer.clear();
                self.in_response = true;
                continue;
            }

            // Stream deltas char-by-char while in response mode
            if self.in_response && !self.buffer.ends_with(">>>") && !self.buffer.ends_with(">> ") {
                // Only emit delta for the latest character (not buffered prompt chars)
                events.push(AdapterEvent::Delta(ch.to_string()));
            }
        }

        events
    }

    fn flush(&mut self) -> Vec<AdapterEvent> {
        let mut events = Vec::new();
        let remaining = std::mem::take(&mut self.buffer).trim().to_string();
        if !remaining.is_empty() {
            events.push(AdapterEvent::Message(ParsedMessage {
                role: "assistant".into(),
                content: remaining,
            }));
        }
        events
    }
}
```

- [ ] **Step 4: Implement Generic adapter with line-buffered deltas**

Replace `daemon/src/chat/adapters/generic.rs`:

```rust
use super::{AdapterEvent, ChatAdapter, ParsedMessage};

/// Fallback adapter for unknown agents (Hermes, OpenClaw, Aider, etc.).
/// Treats all stdout as assistant text. Streams line-by-line as deltas.
pub struct GenericAdapter {
    line_buffer: String,
    message_buffer: String,
}

impl GenericAdapter {
    pub fn new() -> Self {
        Self {
            line_buffer: String::new(),
            message_buffer: String::new(),
        }
    }
}

impl ChatAdapter for GenericAdapter {
    fn feed(&mut self, text: &str) -> Vec<AdapterEvent> {
        self.line_buffer.push_str(text);
        let mut events = Vec::new();

        while let Some(newline_pos) = self.line_buffer.find('\n') {
            let line: String = self.line_buffer.drain(..=newline_pos).collect();
            self.message_buffer.push_str(&line);
            events.push(AdapterEvent::Delta(line));
        }

        events
    }

    fn flush(&mut self) -> Vec<AdapterEvent> {
        let mut events = Vec::new();

        // Emit remaining line buffer as delta
        if !self.line_buffer.is_empty() {
            self.message_buffer.push_str(&self.line_buffer);
            events.push(AdapterEvent::Delta(std::mem::take(&mut self.line_buffer)));
        }

        // Emit accumulated content as final message
        if !self.message_buffer.is_empty() {
            events.push(AdapterEvent::Message(ParsedMessage {
                role: "assistant".into(),
                content: std::mem::take(&mut self.message_buffer).trim().to_string(),
            }));
        }

        events
    }
}
```

- [ ] **Step 5: Build and verify**

```bash
cd daemon && cargo build 2>&1 | head -20
```

Expected: Clean compile.

- [ ] **Step 6: Commit**

```bash
git add daemon/src/chat/adapters/
git commit -m "feat(chat): implement real streaming adapters for Claude, Ollama, and generic agents"
```

---

## Task 5: ChatProcessManager — Subprocess Spawning and Streaming

**Files:**
- Create: `daemon/src/chat/manager.rs`

This is the core new daemon component. It spawns agent processes as direct children with piped stdin/stdout (no tmux), reads output, parses via adapters, and broadcasts events.

- [ ] **Step 1: Create ChatProcessManager**

Create `daemon/src/chat/manager.rs`:

```rust
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

use chrono::Utc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::chat::adapters::{adapter_for_agent, AdapterEvent};
use crate::chat::broadcaster::{ChatBroadcaster, ChatEvent};
use crate::hardware::agents::AgentInfo;
use crate::store::Store;

struct ManagedChatProcess {
    child: Child,
    stdin_tx: tokio::sync::mpsc::Sender<String>,
    agent_id: String,
}

#[derive(Clone)]
pub struct ChatProcessManager {
    processes: Arc<Mutex<HashMap<String, Arc<Mutex<ManagedChatProcess>>>>>,
    broadcasters: Arc<Mutex<HashMap<String, Arc<ChatBroadcaster>>>>,
    store: Store,
}

impl ChatProcessManager {
    pub fn new(store: Store) -> Self {
        Self {
            processes: Arc::new(Mutex::new(HashMap::new())),
            broadcasters: Arc::new(Mutex::new(HashMap::new())),
            store,
        }
    }

    /// Builds the command for chat mode based on agent type.
    fn build_chat_command(agent: &AgentInfo, session_id: &str) -> (String, Vec<String>) {
        match agent.id.as_str() {
            "claude-code" => {
                let program = "claude".to_string();
                let args = vec![
                    "-p".to_string(),
                    "--session-id".to_string(),
                    session_id.to_string(),
                    "--input-format".to_string(),
                    "stream-json".to_string(),
                    "--output-format".to_string(),
                    "stream-json".to_string(),
                ];
                (program, args)
            }
            _ if agent.command.contains(' ') => {
                // Multi-word commands like "ollama run llama3"
                let program = "bash".to_string();
                let args = vec!["-c".to_string(), agent.command.clone()];
                (program, args)
            }
            _ => {
                let program = agent.command.clone();
                let args = vec![];
                (program, args)
            }
        }
    }

    /// Spawns an agent subprocess for chat mode and starts the output reader.
    pub async fn spawn_session(
        &self,
        session_id: &str,
        agent: &AgentInfo,
        workdir: &str,
    ) -> Result<(), String> {
        let (program, args) = Self::build_chat_command(agent, session_id);

        let mut child = Command::new(&program)
            .args(&args)
            .current_dir(workdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("failed to spawn {}: {e}", agent.name))?;

        // Take ownership of stdin for writing
        let stdin = child.stdin.take().ok_or("failed to capture stdin")?;
        let stdout = child.stdout.take().ok_or("failed to capture stdout")?;

        // Create input channel
        let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::channel::<String>(64);

        // Create broadcaster
        let broadcaster = Arc::new(ChatBroadcaster::new());
        self.broadcasters
            .lock()
            .await
            .insert(session_id.to_string(), Arc::clone(&broadcaster));

        // Store managed process
        let managed = Arc::new(Mutex::new(ManagedChatProcess {
            child,
            stdin_tx: stdin_tx.clone(),
            agent_id: agent.id.clone(),
        }));
        self.processes
            .lock()
            .await
            .insert(session_id.to_string(), managed);

        // Spawn stdin writer task
        let session_id_stdin = session_id.to_string();
        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(data) = stdin_rx.recv().await {
                if let Err(e) = stdin.write_all(data.as_bytes()).await {
                    warn!(session_id = %session_id_stdin, error = %e, "stdin write failed");
                    break;
                }
                if let Err(e) = stdin.flush().await {
                    warn!(session_id = %session_id_stdin, error = %e, "stdin flush failed");
                    break;
                }
            }
        });

        // Spawn stdout reader task
        let session_id_read = session_id.to_string();
        let agent_id = agent.id.clone();
        let store = self.store.clone();
        let bc = Arc::clone(&broadcaster);
        let processes = Arc::clone(&self.processes);

        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut adapter = adapter_for_agent(&agent_id);
            let mut line = String::new();
            let msg_id = Uuid::new_v4().to_string();

            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break, // EOF — process exited
                    Ok(_) => {
                        let events = adapter.feed(&line);
                        for event in events {
                            match event {
                                AdapterEvent::Delta(text) => {
                                    bc.send(ChatEvent::Delta {
                                        session_id: session_id_read.clone(),
                                        message_id: msg_id.clone(),
                                        delta: text,
                                    });
                                }
                                AdapterEvent::Message(parsed) => {
                                    // Store in DB
                                    let id = Uuid::new_v4().to_string();
                                    if let Ok(chat_msg) = store.create_chat_message(
                                        &id,
                                        &session_id_read,
                                        &parsed.role,
                                        &parsed.content,
                                    ) {
                                        bc.send(ChatEvent::Message { message: chat_msg });
                                    }
                                }
                                AdapterEvent::Status(status) => {
                                    bc.send(ChatEvent::Status {
                                        session_id: session_id_read.clone(),
                                        status,
                                    });
                                }
                                AdapterEvent::Meta { tokens, context_pct } => {
                                    bc.send(ChatEvent::Meta {
                                        session_id: session_id_read.clone(),
                                        tokens,
                                        context_pct,
                                    });
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!(session_id = %session_id_read, error = %e, "stdout read error");
                        break;
                    }
                }
            }

            // Flush adapter
            let events = adapter.flush();
            for event in events {
                if let AdapterEvent::Message(parsed) = event {
                    let id = Uuid::new_v4().to_string();
                    if let Ok(chat_msg) = store.create_chat_message(
                        &id, &session_id_read, &parsed.role, &parsed.content,
                    ) {
                        bc.send(ChatEvent::Message { message: chat_msg });
                    }
                }
            }

            // Process exited — update DB status
            bc.send(ChatEvent::Status {
                session_id: session_id_read.clone(),
                status: "idle".into(),
            });

            // Clean up
            processes.lock().await.remove(&session_id_read);
            info!(session_id = %session_id_read, "chat process exited");
        });

        info!(session_id = %session_id, agent = %agent.name, "chat process spawned");
        Ok(())
    }

    /// Sends a message to the agent subprocess via stdin.
    /// Formats the input based on agent type (NDJSON for Claude, plain text for others).
    /// Uses the internally-stored agent_id to determine formatting.
    pub async fn send_input(
        &self,
        session_id: &str,
        content: &str,
    ) -> Result<(), String> {
        let processes = self.processes.lock().await;
        let process = processes
            .get(session_id)
            .ok_or_else(|| format!("no chat process for session {session_id}"))?;

        let managed = process.lock().await;

        let formatted = if managed.agent_id == "claude-code" || managed.agent_id.starts_with("claude") {
            // Claude Code expects NDJSON on stdin
            let msg = serde_json::json!({
                "type": "user",
                "message": {
                    "role": "user",
                    "content": content
                }
            });
            format!("{}\n", msg)
        } else {
            // Plain text + newline for Ollama, Hermes, etc.
            format!("{}\n", content)
        };

        managed
            .stdin_tx
            .send(formatted)
            .await
            .map_err(|e| format!("stdin send failed: {e}"))
    }

    /// Returns the broadcaster for a chat session.
    pub async fn get_broadcaster(
        &self,
        session_id: &str,
    ) -> Option<Arc<ChatBroadcaster>> {
        self.broadcasters.lock().await.get(session_id).cloned()
    }

    /// Kills the chat process for a session.
    pub async fn kill_session(&self, session_id: &str) -> Result<(), String> {
        if let Some(process) = self.processes.lock().await.remove(session_id) {
            let mut managed = process.lock().await;
            managed
                .child
                .kill()
                .await
                .map_err(|e| format!("kill failed: {e}"))?;
        }
        self.broadcasters.lock().await.remove(session_id);
        Ok(())
    }

    /// Checks if a chat process is running for a session.
    pub async fn has_session(&self, session_id: &str) -> bool {
        self.processes.lock().await.contains_key(session_id)
    }
}
```

- [ ] **Step 2: Build and verify**

```bash
cd daemon && cargo build 2>&1 | head -30
```

Expected: Compiles. May need to add `tokio` features for process — check `Cargo.toml` has `tokio` with `process` feature. If not:

```bash
cd daemon && grep -n "tokio" Cargo.toml
```

If `process` is missing from tokio features, add it.

- [ ] **Step 3: Commit**

```bash
git add daemon/src/chat/manager.rs daemon/src/chat/mod.rs
git commit -m "feat(chat): add ChatProcessManager for subprocess-based agent chat"
```

---

## Task 6: Wire ChatProcessManager into Daemon

**Files:**
- Modify: `daemon/src/transport/http.rs` (AppState, create_chat_session, send_chat_message, new switch-mode endpoint)
- Modify: `daemon/src/server.rs` (create ChatProcessManager, add to state, register route)

- [ ] **Step 1: Add ChatProcessManager to AppState**

In `daemon/src/transport/http.rs`, update the `AppState` struct:

```rust
use crate::chat::manager::ChatProcessManager;

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub manager: TerminalManager,
    pub chat_manager: ChatProcessManager,
    pub log_buffer: LogBuffer,
    pub bind_address: String,
    pub allowed_cidrs: Vec<String>,
}
```

- [ ] **Step 2: Update create_chat_session to use ChatProcessManager**

In the `create_chat_session` handler (~line 1193), replace the `state.manager.create_session("chat", ...)` call:

```rust
// Create session record in DB (no tmux — subprocess-based)
let id = uuid::Uuid::new_v4().to_string();
let cmd = vec![agent.command.clone()];
let mut session = state.store.create_terminal_session(
    &id, "chat", Some(&agent.name), &workdir, &cmd, "chat", None,
).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") }))))?;

// Update status to running
let now = chrono::Utc::now().to_rfc3339();
state.store.update_terminal_session(&id, Some("running"), Some(&now), None, None, None, None)
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") }))))?;
session.status = "running".to_string();
session.started_at = Some(now);

// Spawn chat subprocess
state.chat_manager.spawn_session(&id, &agent, &workdir).await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e }))))?;
```

- [ ] **Step 3: Update send_chat_message to use ChatProcessManager**

In the `send_chat_message` handler (~line 1246), replace `state.manager.send_input(...)`:

```rust
// Try chat process first (it knows the agent_id internally), fall back to terminal manager
if state.chat_manager.has_session(&id).await {
    state.chat_manager.send_input(&id, &body.content).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e }))))?;
} else {
    state.manager.send_input(&id, body.content.as_bytes(), true).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e }))))?;
}
```

- [ ] **Step 4: Add switch-mode endpoint**

Add to `daemon/src/transport/http.rs`:

```rust
// ---------------------------------------------------------------------------
// POST /api/sessions/{id}/switch-mode
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct SwitchModeBody {
    pub mode: String, // "chat" or "terminal"
    #[serde(default)]
    pub confirmed: bool,
}

pub async fn switch_session_mode(
    _tier: RequireFullAccess,
    client_ip: ClientIp,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SwitchModeBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // Get current session
    let session = state.store.get_terminal_session(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") }))))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "session not found" }))))?;

    if session.mode == body.mode {
        return Ok(Json(serde_json::json!({ "session": session })));
    }

    // Find agent info
    let agents = crate::hardware::agents::detect_agents();
    let agent = agents.iter().find(|a| Some(a.name.as_str()) == session.name.as_deref());

    let persistent = agent.map(|a| a.persistent).unwrap_or(false);

    // If not persistent and not confirmed, return warning
    if !persistent && !body.confirmed {
        return Ok(Json(serde_json::json!({
            "warning": "Switching modes will end the current conversation",
            "needsConfirmation": true
        })));
    }

    // Kill current process
    if session.mode == "chat" {
        state.chat_manager.kill_session(&id).await.ok();
    } else {
        // Terminal mode — kill tmux
        state.manager.terminate_session(&id).await.ok();
    }

    // Update session mode in DB
    let now = chrono::Utc::now().to_rfc3339();
    state.store.update_terminal_session(&id, Some("running"), None, None, None, None, None).ok();
    // Update mode — need a new store method or direct SQL
    {
        let conn = state.store.conn();
        conn.execute(
            "UPDATE terminal_sessions SET mode = ?1 WHERE id = ?2",
            rusqlite::params![body.mode, id],
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") }))))?;
    }

    // Spawn in new mode
    if let Some(agent) = agent {
        let workdir = &session.workdir;
        if body.mode == "chat" {
            state.chat_manager.spawn_session(&id, agent, workdir).await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e }))))?;
        } else {
            // Terminal mode — create tmux session and attach
            crate::terminal::tmux::new_session(&id, workdir, &agent.command)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e }))))?;
        }
    }

    let updated = state.store.get_terminal_session(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") }))))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "session not found after switch" }))))?;

    Ok(Json(serde_json::json!({ "session": updated })))
}
```

- [ ] **Step 5: Update server.rs — create ChatProcessManager and register route**

In `daemon/src/server.rs`, add the ChatProcessManager creation and route:

After the TerminalManager creation (~line 25):
```rust
let chat_manager = crate::chat::manager::ChatProcessManager::new(store.clone());
```

Update AppState construction to include `chat_manager`:
```rust
let state = AppState {
    store,
    manager,
    chat_manager,
    log_buffer,
    bind_address: settings.bind_hosts.join(","),
    allowed_cidrs: settings.allowed_cidrs.iter().map(|c| c.to_string()).collect(),
};
```

Add the switch-mode route to the router:
```rust
.route("/api/sessions/{id}/switch-mode", post(http::switch_session_mode))
```

- [ ] **Step 6: Build and verify**

```bash
cd daemon && cargo build 2>&1 | tail -20
```

Expected: Compiles. Fix any import issues.

- [ ] **Step 7: Commit**

```bash
git add daemon/src/transport/http.rs daemon/src/server.rs
git commit -m "feat: wire ChatProcessManager into daemon with switch-mode endpoint"
```

---

## Task 7: WebSocket Chat Subscriptions

**Files:**
- Modify: `daemon/src/transport/ws.rs`

- [ ] **Step 1: Add chat broadcast handling to WebSocket**

In `daemon/src/transport/ws.rs`, update the `handle_ws` function to support chat sessions. The `subscribe_chat` operation needs to:
1. Load existing messages from DB and replay them
2. Subscribe to the `ChatBroadcaster` for live events
3. Forward `ChatEvent`s as WebSocket messages

Add a `chat_rx` variable alongside the existing terminal `broadcast_rx`. In the main `tokio::select!` loop, add a branch for chat events:

In the `handle_op` function, add the `subscribe_chat` handler:

```rust
"subscribe_chat" => {
    let sid = msg.session_id.as_deref().unwrap_or("");
    if sid.is_empty() {
        return Some(serde_json::json!({"op": "error", "message": "missing sessionId"}));
    }

    // Replay existing messages from DB
    if let Ok(messages) = state.store.list_chat_messages(sid, None, 200) {
        for m in &messages {
            let _ = ws_sender.send(Message::Text(
                serde_json::to_string(&serde_json::json!({
                    "op": "chat_message",
                    "message": m
                })).unwrap().into()
            )).await;
        }
    }

    // Subscribe to live chat events
    if let Some(bc) = state.chat_manager.get_broadcaster(sid).await {
        *chat_broadcaster = Some(bc.clone());
        *chat_rx = Some(bc.subscribe());
    }

    Some(serde_json::json!({
        "op": "subscribed_chat",
        "sessionId": sid,
    }))
}
```

In the main `tokio::select!` loop, add a branch for chat events (similar to the existing terminal chunk branch):

```rust
// Chat event branch
result = async {
    match chat_rx.as_mut() {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
} => {
    if let Ok(event) = result {
        let msg = match &event {
            ChatEvent::Delta { session_id, message_id, delta } => {
                serde_json::json!({"op": "chat_delta", "sessionId": session_id, "messageId": message_id, "delta": delta})
            }
            ChatEvent::Message { message } => {
                serde_json::json!({"op": "chat_message", "message": message})
            }
            ChatEvent::Status { session_id, status } => {
                serde_json::json!({"op": "chat_status", "sessionId": session_id, "status": status})
            }
            ChatEvent::Meta { session_id, tokens, context_pct } => {
                serde_json::json!({"op": "session_meta", "sessionId": session_id, "tokens": tokens, "contextPct": context_pct})
            }
        };
        let _ = ws_sender.send(Message::Text(serde_json::to_string(&msg).unwrap().into())).await;
    }
}
```

You'll need to add the necessary imports at the top:
```rust
use crate::chat::broadcaster::{ChatBroadcaster, ChatEvent};
use crate::chat::manager::ChatProcessManager;
```

And add `chat_rx` and `chat_broadcaster` local variables in `handle_ws`:
```rust
let mut chat_rx: Option<broadcast::Receiver<ChatEvent>> = None;
let mut chat_broadcaster: Option<Arc<ChatBroadcaster>> = None;
```

- [ ] **Step 2: Build and verify**

```bash
cd daemon && cargo build 2>&1 | tail -20
```

Expected: Compiles.

- [ ] **Step 3: Commit**

```bash
git add daemon/src/transport/ws.rs
git commit -m "feat(ws): add subscribe_chat with delta/message/status/meta streaming"
```

---

## Task 8: Frontend Types and API

**Files:**
- Modify: `desktop/src/types.ts`
- Modify: `desktop/src/api.ts`

- [ ] **Step 1: Update types.ts**

Add/update these types in `desktop/src/types.ts`:

```typescript
// Update MainView — replace "chat" | "terminal" with "agents"
export type MainView = "agents" | "logs" | "settings";

// Update TerminalSession to include new fields
export type TerminalSession = {
  id: string;
  mode: "agent" | "project" | "rescue_shell" | "chat" | "terminal";
  status: "created" | "running" | "exited" | "terminated" | "error";
  name?: string | null;
  workdir: string;
  command: string[];
  createdAt: string;
  startedAt?: string | null;
  finishedAt?: string | null;
  lastChunkAt?: string | null;
  pid?: number | null;
  exitCode?: number | null;
  sessionType?: string;
  projectId?: string | null;
  parentSessionId?: string | null;
  hostId?: string | null;
  hostName?: string | null;
};

// Chat WebSocket event types
export type ChatDeltaEvent = {
  op: "chat_delta";
  sessionId: string;
  messageId: string;
  delta: string;
};

export type ChatMessageEvent = {
  op: "chat_message";
  message: ChatMessage;
};

export type ChatStatusEvent = {
  op: "chat_status";
  sessionId: string;
  status: "thinking" | "tool_use" | "idle" | "error";
};

export type SessionMetaEvent = {
  op: "session_meta";
  sessionId: string;
  tokens?: number | null;
  contextPct?: number | null;
};

export type ChatWsEvent =
  | ChatDeltaEvent
  | ChatMessageEvent
  | ChatStatusEvent
  | SessionMetaEvent;

// Update AgentInfo to include persistent flag
export type AgentInfo = {
  id: string;
  name: string;
  agentType: "cli" | "api";
  command: string;
  version: string | null;
  persistent: boolean;
};

// Session mode for the toggle
export type SessionMode = "chat" | "terminal";
```

- [ ] **Step 2: Add switchSessionMode to api.ts**

Add to `desktop/src/api.ts`:

```typescript
export async function switchSessionMode(
  daemonUrl: string,
  sessionId: string,
  mode: "chat" | "terminal",
  confirmed = false,
): Promise<{ session?: TerminalSession; warning?: string; needsConfirmation?: boolean }> {
  return api(daemonUrl, `/api/sessions/${sessionId}/switch-mode`, {
    method: "POST",
    body: JSON.stringify({ mode, confirmed }),
  });
}
```

- [ ] **Step 3: Commit**

```bash
git add desktop/src/types.ts desktop/src/api.ts
git commit -m "feat(desktop): add chat event types, session mode switching API"
```

---

## Task 9: useChatSocket Hook

**Files:**
- Create: `desktop/src/hooks/useChatSocket.ts`

- [ ] **Step 1: Create the hook**

Create `desktop/src/hooks/useChatSocket.ts`:

```typescript
import { useCallback, useEffect, useRef, useState } from "react";
import { wsUrlFromHttp } from "../api";
import type { ChatMessage, ChatWsEvent, SessionMode } from "../types";

export type UseChatSocketOptions = {
  baseUrl: string;
  sessionId: string | null;
  isActive: boolean;
  onError?: (message: string) => void;
};

export type ChatSessionMeta = {
  tokens: number | null;
  contextPct: number | null;
  status: string;
};

export type UseChatSocketReturn = {
  messages: ChatMessage[];
  streamingDelta: string;
  streamingMessageId: string | null;
  meta: ChatSessionMeta;
  isConnected: boolean;
  sendMessage: (content: string) => void;
};

export function useChatSocket({
  baseUrl,
  sessionId,
  isActive,
  onError,
}: UseChatSocketOptions): UseChatSocketReturn {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [streamingDelta, setStreamingDelta] = useState("");
  const [streamingMessageId, setStreamingMessageId] = useState<string | null>(null);
  const [meta, setMeta] = useState<ChatSessionMeta>({
    tokens: null,
    contextPct: null,
    status: "idle",
  });
  const [isConnected, setIsConnected] = useState(false);

  const wsRef = useRef<WebSocket | null>(null);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const backoffRef = useRef(500);

  // Reset state when session changes
  useEffect(() => {
    setMessages([]);
    setStreamingDelta("");
    setStreamingMessageId(null);
    setMeta({ tokens: null, contextPct: null, status: "idle" });
  }, [sessionId]);

  useEffect(() => {
    if (!isActive || !sessionId) return;

    let disposed = false;

    function connect() {
      if (disposed) return;

      const wsUrl = wsUrlFromHttp(baseUrl);
      const ws = new WebSocket(wsUrl);
      wsRef.current = ws;

      ws.onopen = () => {
        setIsConnected(true);
        backoffRef.current = 500;
        // Subscribe to chat session
        ws.send(JSON.stringify({
          op: "subscribe_chat",
          sessionId,
        }));
      };

      ws.onmessage = (event) => {
        let data: ChatWsEvent & { op: string };
        try {
          data = JSON.parse(event.data);
        } catch {
          return;
        }

        switch (data.op) {
          case "chat_message":
            if ("message" in data) {
              setMessages((prev) => {
                // Deduplicate by ID
                if (prev.some((m) => m.id === data.message.id)) return prev;
                return [...prev, data.message];
              });
              // Clear streaming state when complete message arrives
              setStreamingDelta("");
              setStreamingMessageId(null);
            }
            break;

          case "chat_delta":
            if ("delta" in data && "messageId" in data) {
              setStreamingMessageId(data.messageId);
              setStreamingDelta((prev) => prev + data.delta);
            }
            break;

          case "chat_status":
            if ("status" in data) {
              setMeta((prev) => ({ ...prev, status: data.status }));
            }
            break;

          case "session_meta":
            if ("tokens" in data || "contextPct" in data) {
              setMeta((prev) => ({
                ...prev,
                tokens: (data as any).tokens ?? prev.tokens,
                contextPct: (data as any).contextPct ?? prev.contextPct,
              }));
            }
            break;

          case "subscribed_chat":
            // Subscription confirmed
            break;

          case "error":
            onError?.((data as any).message ?? "WebSocket error");
            break;
        }
      };

      ws.onclose = () => {
        setIsConnected(false);
        wsRef.current = null;
        if (!disposed) {
          reconnectTimerRef.current = setTimeout(() => {
            backoffRef.current = Math.min(backoffRef.current * 2, 5000);
            connect();
          }, backoffRef.current);
        }
      };

      ws.onerror = () => {
        ws.close();
      };
    }

    connect();

    return () => {
      disposed = true;
      if (reconnectTimerRef.current) clearTimeout(reconnectTimerRef.current);
      wsRef.current?.close();
      wsRef.current = null;
      setIsConnected(false);
    };
  }, [baseUrl, sessionId, isActive, onError]);

  const sendMessage = useCallback(
    (content: string) => {
      // Send via HTTP POST (not WebSocket) — the daemon stores the message and writes to stdin
      if (!sessionId) return;
      fetch(`${baseUrl}/api/chat/sessions/${sessionId}/message`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ content }),
      })
        .then((res) => res.json())
        .then((msg: ChatMessage) => {
          // Add user message to local state immediately
          setMessages((prev) => {
            if (prev.some((m) => m.id === msg.id)) return prev;
            return [...prev, msg];
          });
        })
        .catch((e) => onError?.(e.message));
    },
    [baseUrl, sessionId, onError],
  );

  return {
    messages,
    streamingDelta,
    streamingMessageId,
    meta,
    isConnected,
    sendMessage,
  };
}
```

- [ ] **Step 2: Commit**

```bash
git add desktop/src/hooks/useChatSocket.ts
git commit -m "feat(desktop): add useChatSocket hook for streaming chat"
```

---

## Task 10: TerminalRenderer Component

**Files:**
- Create: `desktop/src/components/TerminalRenderer.tsx`

Extract the xterm.js terminal rendering from the existing TerminalWorkspace into a standalone component.

- [ ] **Step 1: Create TerminalRenderer**

Create `desktop/src/components/TerminalRenderer.tsx`:

```typescript
import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { useTerminalSocket } from "../hooks/useTerminalSocket";
import { useLocalTerminal } from "../hooks/useLocalTerminal";
import type { TerminalSession, LocalTerminalSession } from "../types";

type Props = {
  baseUrl: string;
  sessionId: string | null;
  isLocal: boolean;
  isActive: boolean;
  onSessionStatusChange?: (session: TerminalSession | LocalTerminalSession) => void;
  onError?: (message: string) => void;
};

const TERMINAL_THEME = {
  background: "#1a1f36",
  foreground: "#e2e8f0",
  cursor: "#93c5fd",
  green: "#10b981",
  blue: "#60a5fa",
  yellow: "#fbbf24",
  red: "#f87171",
  cyan: "#22d3ee",
  magenta: "#c084fc",
};

export function TerminalRenderer({
  baseUrl,
  sessionId,
  isLocal,
  isActive,
  onSessionStatusChange,
  onError,
}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);

  // Initialize xterm.js
  useEffect(() => {
    if (!containerRef.current || !isActive) return;

    const terminal = new Terminal({
      cursorBlink: true,
      fontSize: 13,
      fontFamily: "'JetBrains Mono', 'Fira Code', monospace",
      theme: TERMINAL_THEME,
      allowProposedApi: true,
    });

    const fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    terminal.open(containerRef.current);
    fitAddon.fit();

    terminalRef.current = terminal;
    fitAddonRef.current = fitAddon;

    const observer = new ResizeObserver(() => {
      try { fitAddon.fit(); } catch { /* ignore */ }
    });
    observer.observe(containerRef.current);

    return () => {
      observer.disconnect();
      terminal.dispose();
      terminalRef.current = null;
      fitAddonRef.current = null;
    };
  }, [isActive, sessionId]);

  // Remote terminal hook
  const remote = useTerminalSocket({
    baseUrl,
    sessionId: !isLocal ? sessionId : null,
    terminalRef,
    isActive: isActive && !isLocal,
    onSessionStatusChange: onSessionStatusChange as (s: TerminalSession) => void,
    onError,
  });

  // Local terminal hook
  const local = useLocalTerminal({
    sessionId: isLocal ? sessionId : null,
    terminalRef,
    isActive: isActive && isLocal,
    onSessionStatusChange: onSessionStatusChange as (s: LocalTerminalSession) => void,
    onError,
  });

  // Forward terminal input
  useEffect(() => {
    const terminal = terminalRef.current;
    if (!terminal || !isActive || !sessionId) return;

    const sendInput = isLocal ? local.sendInput : remote.sendInput;
    const disposable = terminal.onData((data) => {
      if (isLocal) {
        sendInput(data);
      } else {
        sendInput(data, false);
      }
    });

    return () => disposable.dispose();
  }, [isActive, sessionId, isLocal, local.sendInput, remote.sendInput]);

  // Forward resize
  useEffect(() => {
    const terminal = terminalRef.current;
    if (!terminal || !isActive || !sessionId) return;

    const resize = isLocal ? local.resize : remote.resize;
    const disposable = terminal.onResize(({ cols, rows }) => {
      resize(cols, rows);
    });

    return () => disposable.dispose();
  }, [isActive, sessionId, isLocal, local.resize, remote.resize]);

  return (
    <div
      ref={containerRef}
      className="terminal-host"
      style={{ flex: 1, minHeight: 0 }}
    />
  );
}
```

- [ ] **Step 2: Commit**

```bash
git add desktop/src/components/TerminalRenderer.tsx
git commit -m "feat(desktop): extract TerminalRenderer component from TerminalWorkspace"
```

---

## Task 11: ChatRenderer Component

**Files:**
- Create: `desktop/src/components/ChatRenderer.tsx`
- Modify: `desktop/src/App.css` (add chat styles)

- [ ] **Step 1: Create ChatRenderer**

Create `desktop/src/components/ChatRenderer.tsx`:

```typescript
import { useCallback, useEffect, useRef, useState } from "react";
import type { ChatMessage } from "../types";

type Props = {
  messages: ChatMessage[];
  streamingDelta: string;
  streamingMessageId: string | null;
  status: string;
  onSendMessage: (content: string) => void;
};

export function ChatRenderer({
  messages,
  streamingDelta,
  streamingMessageId,
  status,
  onSendMessage,
}: Props) {
  const [draft, setDraft] = useState("");
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // Auto-scroll to bottom on new messages/deltas
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, streamingDelta]);

  const handleSend = useCallback(() => {
    const content = draft.trim();
    if (!content) return;
    onSendMessage(content);
    setDraft("");
  }, [draft, onSendMessage]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        handleSend();
      }
    },
    [handleSend],
  );

  return (
    <div className="chat-renderer">
      <div className="chat-messages">
        {messages.map((msg) => (
          <div key={msg.id} className={`chat-bubble chat-bubble-${msg.role}`}>
            {msg.role === "system" ? (
              <div className="chat-system-msg">{msg.content}</div>
            ) : (
              <>
                <div className="chat-bubble-header">
                  <span className="chat-bubble-role">
                    {msg.role === "user" ? "You" : "Assistant"}
                  </span>
                </div>
                <div className="chat-bubble-content">{msg.content}</div>
              </>
            )}
          </div>
        ))}

        {/* Streaming bubble */}
        {streamingDelta && (
          <div className="chat-bubble chat-bubble-assistant chat-bubble-streaming">
            <div className="chat-bubble-header">
              <span className="chat-bubble-role">Assistant</span>
              <span className="chat-streaming-indicator">●</span>
            </div>
            <div className="chat-bubble-content">{streamingDelta}</div>
          </div>
        )}

        {/* Status indicator */}
        {status === "thinking" && !streamingDelta && (
          <div className="chat-status-indicator">Thinking…</div>
        )}
        {status === "tool_use" && (
          <div className="chat-status-indicator">Using tool…</div>
        )}

        <div ref={messagesEndRef} />
      </div>

      {/* Composer */}
      <div className="chat-composer">
        <textarea
          ref={textareaRef}
          className="chat-input"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Send a message..."
          rows={1}
        />
        <button
          className="btn-primary chat-send-btn"
          onClick={handleSend}
          disabled={!draft.trim()}
        >
          Send
        </button>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Add chat CSS to App.css**

Append to `desktop/src/App.css`:

```css
/* ------------------------------------------------------------------ */
/* Chat Renderer                                                       */
/* ------------------------------------------------------------------ */

.chat-renderer {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
}

.chat-messages {
  flex: 1;
  overflow-y: auto;
  padding: 20px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.chat-bubble {
  max-width: 70%;
  padding: 10px 14px;
  border-radius: var(--radius-md);
  font-size: 0.85rem;
  line-height: 1.5;
}

.chat-bubble-user {
  align-self: flex-end;
  background: #f0f4ff;
  border: 1px solid #93b4f8;
  border-radius: var(--radius-md) var(--radius-md) 4px var(--radius-md);
}

.chat-bubble-assistant {
  align-self: flex-start;
  background: #f0fdf4;
  border: 1px solid #6ee7b7;
  border-radius: var(--radius-md) var(--radius-md) var(--radius-md) 4px;
}

.chat-bubble-system {
  align-self: center;
  max-width: 100%;
  background: none;
  border: none;
  padding: 0;
}

.chat-system-msg {
  font-size: 0.75rem;
  color: var(--text-muted);
  background: var(--bg-elevated);
  padding: 4px 14px;
  border-radius: 12px;
  border: 1px solid var(--border);
  display: inline-block;
}

.chat-bubble-header {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-bottom: 4px;
}

.chat-bubble-role {
  font-size: 0.75rem;
  font-weight: 500;
  color: var(--text-secondary);
}

.chat-bubble-content {
  white-space: pre-wrap;
  word-break: break-word;
}

.chat-bubble-streaming {
  opacity: 0.9;
}

.chat-streaming-indicator {
  font-size: 0.6rem;
  color: var(--accent-green);
  animation: pulse 1.5s infinite;
}

.chat-status-indicator {
  align-self: flex-start;
  font-size: 0.78rem;
  color: var(--text-muted);
  padding: 4px 12px;
  font-style: italic;
}

.chat-composer {
  display: flex;
  gap: 10px;
  align-items: flex-end;
  padding: 12px 20px;
  border-top: 1px solid var(--border);
}

.chat-input {
  flex: 1;
  resize: none;
  font-family: inherit;
  min-height: 20px;
  max-height: 120px;
}

.chat-send-btn {
  flex-shrink: 0;
}
```

- [ ] **Step 3: Commit**

```bash
git add desktop/src/components/ChatRenderer.tsx desktop/src/App.css
git commit -m "feat(desktop): add ChatRenderer with message bubbles and streaming"
```

---

## Task 12: SessionSidebar Component

**Files:**
- Create: `desktop/src/components/SessionSidebar.tsx`
- Modify: `desktop/src/App.css` (add session sidebar styles)

- [ ] **Step 1: Create SessionSidebar**

Create `desktop/src/components/SessionSidebar.tsx`:

```typescript
import { useCallback, useRef, useState } from "react";
import type { TerminalSession } from "../types";

type Props = {
  activeSessions: TerminalSession[];
  previousSessions: TerminalSession[];
  activeSessionId: string | null;
  onSelectSession: (sessionId: string) => void;
};

const STATUS_BORDER_COLORS: Record<string, string> = {
  running: "var(--accent-green)",
  created: "var(--accent-green)",
  error: "var(--accent-red)",
  exited: "var(--border)",
  terminated: "var(--border)",
};

const STATUS_DOT_COLORS: Record<string, string> = {
  running: "var(--accent-green)",
  created: "var(--accent-blue)",
  error: "var(--accent-red)",
  exited: "var(--text-muted)",
  terminated: "var(--text-muted)",
};

function formatRelativeTime(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

function needsApproval(_session: TerminalSession): boolean {
  // TODO: check approval status from session metadata when available
  return false;
}

export function SessionSidebar({
  activeSessions,
  previousSessions,
  activeSessionId,
  onSelectSession,
}: Props) {
  const [sidebarWidth, setSidebarWidth] = useState(260);
  const dragging = useRef(false);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    dragging.current = true;

    const handleMouseMove = (e: MouseEvent) => {
      if (!dragging.current) return;
      setSidebarWidth(Math.max(200, Math.min(500, e.clientX)));
    };

    const handleMouseUp = () => {
      dragging.current = false;
      document.removeEventListener("mousemove", handleMouseMove);
      document.removeEventListener("mouseup", handleMouseUp);
    };

    document.addEventListener("mousemove", handleMouseMove);
    document.addEventListener("mouseup", handleMouseUp);
  }, []);

  return (
    <div className="session-sidebar" style={{ width: sidebarWidth }}>
      <div className="session-sidebar-content">
        {/* Active Sessions */}
        <div className="session-sidebar-section">
          <div className="session-sidebar-label">Active Sessions</div>
          {activeSessions.length === 0 && (
            <div className="muted" style={{ fontSize: "0.78rem", padding: "8px 0" }}>
              No active sessions
            </div>
          )}
          {activeSessions.map((session) => {
            const isSelected = session.id === activeSessionId;
            const approval = needsApproval(session);
            const borderColor = approval
              ? "var(--accent-yellow)"
              : STATUS_BORDER_COLORS[session.status] ?? "var(--border)";
            const indent = session.parentSessionId ? 20 : 0;

            return (
              <div
                key={session.id}
                className={`session-card ${isSelected ? "session-card-selected" : ""}`}
                style={{
                  borderColor,
                  marginLeft: indent,
                }}
                onClick={() => onSelectSession(session.id)}
              >
                <div className="session-card-header">
                  <span
                    className="status-dot"
                    style={{
                      background: approval
                        ? "var(--accent-yellow)"
                        : STATUS_DOT_COLORS[session.status],
                    }}
                  />
                  <span className="session-card-name">
                    {session.name ?? "Shell"}
                  </span>
                  <span className="session-card-type">
                    {session.hostName ?? "local"}
                  </span>
                </div>
                <div className="session-card-workdir">{session.workdir}</div>
                {approval && (
                  <div className="session-card-approval">⚠ Approval needed</div>
                )}
              </div>
            );
          })}
        </div>

        {/* Separator */}
        {previousSessions.length > 0 && <div className="session-sidebar-separator" />}

        {/* Previous Sessions */}
        {previousSessions.length > 0 && (
          <div className="session-sidebar-section session-sidebar-previous">
            <div className="session-sidebar-label">Previous Sessions</div>
            {previousSessions.map((session) => (
              <div
                key={session.id}
                className="session-card session-card-previous"
                onClick={() => onSelectSession(session.id)}
              >
                <div className="session-card-header">
                  <span
                    className="status-dot"
                    style={{ background: "var(--text-muted)" }}
                  />
                  <span className="session-card-name">
                    {session.name ?? "Shell"}
                  </span>
                  <span className="session-card-type">
                    {session.finishedAt
                      ? formatRelativeTime(session.finishedAt)
                      : ""}
                  </span>
                </div>
                <div className="session-card-workdir">{session.workdir}</div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Drag handle */}
      <div className="session-sidebar-handle" onMouseDown={handleMouseDown} />
    </div>
  );
}
```

- [ ] **Step 2: Add session sidebar CSS to App.css**

Append to `desktop/src/App.css`:

```css
/* ------------------------------------------------------------------ */
/* Session Sidebar                                                     */
/* ------------------------------------------------------------------ */

.session-sidebar {
  position: relative;
  display: flex;
  background: var(--bg-surface);
  border-right: 1px solid var(--border);
  min-width: 200px;
  max-width: 500px;
}

.session-sidebar-content {
  flex: 1;
  overflow-y: auto;
  padding: 12px;
  display: flex;
  flex-direction: column;
}

.session-sidebar-handle {
  position: absolute;
  right: -3px;
  top: 0;
  bottom: 0;
  width: 6px;
  cursor: col-resize;
  z-index: 10;
}

.session-sidebar-handle:hover {
  background: var(--accent-blue);
  opacity: 0.3;
}

.session-sidebar-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.session-sidebar-label {
  font-size: 0.72rem;
  text-transform: uppercase;
  letter-spacing: 0.5px;
  color: var(--text-muted);
  font-weight: 500;
}

.session-sidebar-separator {
  border-top: 1px solid var(--border);
  margin: 8px 0;
}

.session-sidebar-previous {
  opacity: 0.55;
}

.session-card {
  background: var(--bg-elevated);
  border: 1.5px solid var(--border);
  border-radius: var(--radius-sm);
  padding: 10px 12px;
  cursor: pointer;
  transition: border-color 0.15s;
}

.session-card:hover {
  border-color: var(--border-hover);
}

.session-card-selected {
  box-shadow: var(--shadow-sm);
}

.session-card-header {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-bottom: 4px;
}

.session-card-name {
  font-size: 0.85rem;
  font-weight: 500;
  color: var(--text-primary);
}

.session-card-type {
  font-size: 0.7rem;
  color: var(--text-muted);
  margin-left: auto;
}

.session-card-workdir {
  font-size: 0.75rem;
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.session-card-approval {
  font-size: 0.72rem;
  color: #b45309;
  background: #fef3c7;
  border-radius: 4px;
  padding: 3px 8px;
  margin-top: 4px;
  display: flex;
  align-items: center;
  gap: 4px;
}

.session-card-previous {
  border-color: var(--border);
}
```

- [ ] **Step 3: Commit**

```bash
git add desktop/src/components/SessionSidebar.tsx desktop/src/App.css
git commit -m "feat(desktop): add resizable SessionSidebar with active/previous cards"
```

---

## Task 13: SessionHeader Component

**Files:**
- Create: `desktop/src/components/SessionHeader.tsx`

- [ ] **Step 1: Create SessionHeader**

Create `desktop/src/components/SessionHeader.tsx`:

```typescript
import { useEffect, useState } from "react";
import type { TerminalSession, SessionMode } from "../types";
import type { ChatSessionMeta } from "../hooks/useChatSocket";

type Props = {
  session: TerminalSession;
  mode: SessionMode;
  meta: ChatSessionMeta | null;
  onSwitchMode: (mode: SessionMode) => void;
  onEndSession: () => void;
};

function formatDuration(startedAt: string | null | undefined): string {
  if (!startedAt) return "";
  const ms = Date.now() - new Date(startedAt).getTime();
  const secs = Math.floor(ms / 1000);
  const mins = Math.floor(secs / 60);
  const hrs = Math.floor(mins / 60);
  if (hrs > 0) return `${hrs}h ${mins % 60}m`;
  if (mins > 0) return `${mins}m ${secs % 60}s`;
  return `${secs}s`;
}

function formatTokens(tokens: number | null): string {
  if (tokens == null) return "";
  if (tokens >= 1000) return `${(tokens / 1000).toFixed(1)}k tokens`;
  return `${tokens} tokens`;
}

export function SessionHeader({
  session,
  mode,
  meta,
  onSwitchMode,
  onEndSession,
}: Props) {
  // Live duration timer
  const [duration, setDuration] = useState(formatDuration(session.startedAt));
  useEffect(() => {
    if (session.status !== "running") return;
    const interval = setInterval(() => {
      setDuration(formatDuration(session.startedAt));
    }, 1000);
    return () => clearInterval(interval);
  }, [session.startedAt, session.status]);

  const statusColor =
    session.status === "running"
      ? "var(--accent-green)"
      : session.status === "error"
        ? "var(--accent-red)"
        : "var(--text-muted)";

  const contextPct = meta?.contextPct;
  const contextWarning = contextPct != null && contextPct > 80;

  return (
    <div className="session-header">
      <div className="session-header-info">
        <span className="status-dot" style={{ background: statusColor }} />
        <span className="session-header-name">{session.name ?? "Shell"}</span>
        <span className="muted" style={{ fontSize: "0.82rem" }}>
          {session.workdir}
        </span>
        {session.hostName && (
          <span className="muted" style={{ fontSize: "0.78rem" }}>
            · {session.hostName}
          </span>
        )}
        {session.parentSessionId && (
          <span className="session-delegated-badge">Delegated</span>
        )}
      </div>

      <div className="session-header-meta">
        {duration && (
          <span className="session-meta-item">{duration}</span>
        )}
        {meta?.tokens != null && (
          <span className="session-meta-item">
            {formatTokens(meta.tokens)}
          </span>
        )}
        {contextPct != null && (
          <span
            className={`session-meta-item ${contextWarning ? "session-context-warn" : ""}`}
          >
            <span className="session-context-bar">
              <span
                className="session-context-fill"
                style={{
                  width: `${Math.min(contextPct, 100)}%`,
                  background: contextWarning ? "var(--accent-yellow)" : "var(--accent-blue)",
                }}
              />
            </span>
            {Math.round(contextPct)}%
          </span>
        )}
      </div>

      <div className="session-header-actions">
        {/* Mode toggle */}
        <div className="session-mode-toggle">
          <button
            className={`session-mode-btn ${mode === "chat" ? "session-mode-active" : ""}`}
            onClick={() => onSwitchMode("chat")}
          >
            Chat
          </button>
          <button
            className={`session-mode-btn ${mode === "terminal" ? "session-mode-active" : ""}`}
            onClick={() => onSwitchMode("terminal")}
          >
            Terminal
          </button>
        </div>

        {/* Open IDE — no-op for now */}
        <button
          className="btn-secondary"
          disabled
          title="code-server coming soon"
          style={{ opacity: 0.4, fontSize: "0.78rem", padding: "4px 10px" }}
        >
          Open IDE
        </button>

        <button
          className="btn-secondary"
          onClick={onEndSession}
          style={{ fontSize: "0.78rem", padding: "4px 10px" }}
        >
          End Session
        </button>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Add session header CSS to App.css**

Append to `desktop/src/App.css`:

```css
/* ------------------------------------------------------------------ */
/* Session Header                                                      */
/* ------------------------------------------------------------------ */

.session-header {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 16px;
  border-bottom: 1px solid var(--border);
  background: var(--bg-surface);
  flex-wrap: wrap;
}

.session-header-info {
  display: flex;
  align-items: center;
  gap: 8px;
}

.session-header-name {
  font-size: 0.9rem;
  font-weight: 600;
  color: var(--text-primary);
}

.session-header-meta {
  display: flex;
  align-items: center;
  gap: 12px;
  flex: 1;
}

.session-meta-item {
  font-size: 0.78rem;
  color: var(--text-muted);
  display: flex;
  align-items: center;
  gap: 4px;
}

.session-context-bar {
  display: inline-block;
  width: 40px;
  height: 4px;
  background: var(--bg-elevated);
  border-radius: 2px;
  overflow: hidden;
}

.session-context-fill {
  display: block;
  height: 100%;
  border-radius: 2px;
  transition: width 0.3s;
}

.session-context-warn {
  color: var(--accent-yellow);
  font-weight: 500;
}

.session-header-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.session-mode-toggle {
  display: flex;
  background: var(--bg-input);
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  overflow: hidden;
}

.session-mode-btn {
  padding: 4px 12px;
  font-size: 0.78rem;
  background: transparent;
  color: var(--text-secondary);
  border: none;
  cursor: pointer;
  font-family: inherit;
}

.session-mode-btn:not(:first-child) {
  border-left: 1px solid var(--border);
}

.session-mode-active {
  background: var(--accent-blue);
  color: #fff;
  font-weight: 500;
}

.session-delegated-badge {
  font-size: 0.7rem;
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  border-radius: 4px;
  padding: 1px 6px;
  color: var(--text-muted);
}
```

- [ ] **Step 3: Commit**

```bash
git add desktop/src/components/SessionHeader.tsx desktop/src/App.css
git commit -m "feat(desktop): add SessionHeader with mode toggle and metadata"
```

---

## Task 14: AgentsView — Unified Page Component

**Files:**
- Create: `desktop/src/components/AgentsView.tsx`

- [ ] **Step 1: Create AgentsView**

Create `desktop/src/components/AgentsView.tsx`:

```typescript
import { useCallback, useEffect, useState } from "react";
import {
  listAgents,
  createChatSession,
  switchSessionMode,
} from "../api";
import { useChatSocket } from "../hooks/useChatSocket";
import { SessionSidebar } from "./SessionSidebar";
import { SessionHeader } from "./SessionHeader";
import { ChatRenderer } from "./ChatRenderer";
import { TerminalRenderer } from "./TerminalRenderer";
import type {
  AgentInfo,
  TerminalSession,
  SessionMode,
  LocalTerminalSession,
} from "../types";

type Props = {
  daemonUrl: string;
  sessions: TerminalSession[];
  localSessions: LocalTerminalSession[];
  visible: boolean;
  onCreateLocalSession: () => void;
  onRefreshSessions: () => void;
};

const LOCAL_DAEMON = "http://127.0.0.1:8787";

export function AgentsView({
  daemonUrl,
  sessions,
  localSessions,
  visible,
  onCreateLocalSession,
  onRefreshSessions,
}: Props) {
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [selectedAgentId, setSelectedAgentId] = useState<string | null>(null);
  const [selectedMode, setSelectedMode] = useState<SessionMode>("chat");
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [activeMode, setActiveMode] = useState<SessionMode>("chat");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Fetch agents
  useEffect(() => {
    listAgents(daemonUrl)
      .then((a) => {
        setAgents(a);
        if (a.length > 0 && !selectedAgentId) setSelectedAgentId(a[0].id);
      })
      .catch(() => {});
  }, [daemonUrl]);

  // Derive active and previous sessions
  const activeSessions = sessions.filter(
    (s) => s.status === "running" || s.status === "created",
  );
  const previousSessions = sessions.filter(
    (s) => s.status !== "running" && s.status !== "created",
  );

  // Get current session object
  const activeSession = sessions.find((s) => s.id === activeSessionId) ?? null;

  // Determine if active session is a local shell
  const isLocalSession = localSessions.some((s) => s.id === activeSessionId);

  // Chat socket for chat mode
  const chatSocket = useChatSocket({
    baseUrl: LOCAL_DAEMON,
    sessionId: activeMode === "chat" && activeSessionId && !isLocalSession ? activeSessionId : null,
    isActive: visible && activeMode === "chat" && !!activeSessionId && !isLocalSession,
    onError: setError,
  });

  // Create new session
  const handleNewSession = useCallback(async () => {
    if (!selectedAgentId) return;
    setError(null);
    setLoading(true);
    try {
      if (selectedAgentId === "shell") {
        onCreateLocalSession();
        setActiveMode("terminal");
      } else if (selectedMode === "chat") {
        const result = await createChatSession(daemonUrl, selectedAgentId);
        const sessionId: string = result.session?.id ?? result.session;
        setActiveSessionId(sessionId);
        setActiveMode("chat");
        onRefreshSessions();
      } else {
        // Terminal mode — create via existing terminal session API
        const resp = await fetch(`${daemonUrl}/api/terminal/sessions`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ mode: "agent", agentId: selectedAgentId }),
        });
        const data = await resp.json();
        setActiveSessionId(data.id);
        setActiveMode("terminal");
        onRefreshSessions();
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create session");
    } finally {
      setLoading(false);
    }
  }, [daemonUrl, selectedAgentId, selectedMode, onCreateLocalSession, onRefreshSessions]);

  // Switch mode on active session
  const handleSwitchMode = useCallback(
    async (newMode: SessionMode) => {
      if (!activeSessionId || newMode === activeMode) return;
      setError(null);

      try {
        const result = await switchSessionMode(daemonUrl, activeSessionId, newMode);
        if (result.needsConfirmation) {
          const ok = window.confirm(result.warning ?? "Switching modes will end the current conversation. Continue?");
          if (!ok) return;
          await switchSessionMode(daemonUrl, activeSessionId, newMode, true);
        }
        setActiveMode(newMode);
        onRefreshSessions();
      } catch (err) {
        setError(err instanceof Error ? err.message : "Failed to switch mode");
      }
    },
    [daemonUrl, activeSessionId, activeMode, onRefreshSessions],
  );

  // End session
  const handleEndSession = useCallback(async () => {
    if (!activeSessionId) return;
    try {
      await fetch(`${daemonUrl}/api/terminal/sessions/${activeSessionId}/terminate`, {
        method: "POST",
      });
      setActiveSessionId(null);
      onRefreshSessions();
    } catch {
      // Ignore
    }
  }, [daemonUrl, activeSessionId, onRefreshSessions]);

  if (!visible) return null;

  return (
    <div className="agents-view">
      {/* Top bar: agent picker + mode + new session */}
      <div className="agents-topbar">
        <select
          value={selectedAgentId ?? ""}
          onChange={(e) => setSelectedAgentId(e.target.value || null)}
          disabled={agents.length === 0}
        >
          <option value="shell">Shell (local)</option>
          {agents.map((a) => (
            <option key={a.id} value={a.id}>
              {a.name} {a.version ? `v${a.version}` : ""} ({a.agentType})
            </option>
          ))}
        </select>

        {selectedAgentId !== "shell" && (
          <div className="session-mode-toggle">
            <button
              className={`session-mode-btn ${selectedMode === "chat" ? "session-mode-active" : ""}`}
              onClick={() => setSelectedMode("chat")}
            >
              Chat
            </button>
            <button
              className={`session-mode-btn ${selectedMode === "terminal" ? "session-mode-active" : ""}`}
              onClick={() => setSelectedMode("terminal")}
            >
              Terminal
            </button>
          </div>
        )}

        <button
          className="btn-primary"
          onClick={() => void handleNewSession()}
          disabled={loading || !selectedAgentId}
          style={{ fontSize: "0.85rem", padding: "7px 16px" }}
        >
          {loading ? "Starting…" : "+ New Session"}
        </button>

        {error && (
          <span style={{ color: "var(--accent-red)", fontSize: "0.78rem" }}>
            {error}
          </span>
        )}
      </div>

      {/* Main area: sidebar + content */}
      <div className="agents-main">
        <SessionSidebar
          activeSessions={activeSessions}
          previousSessions={previousSessions}
          activeSessionId={activeSessionId}
          onSelectSession={(id) => {
            setActiveSessionId(id);
            const session = sessions.find((s) => s.id === id);
            if (session) {
              setActiveMode(session.mode === "chat" ? "chat" : "terminal");
            }
          }}
        />

        <div className="agents-content">
          {activeSession ? (
            <>
              <SessionHeader
                session={activeSession}
                mode={activeMode}
                meta={activeMode === "chat" ? chatSocket.meta : null}
                onSwitchMode={handleSwitchMode}
                onEndSession={handleEndSession}
              />

              {activeMode === "chat" && !isLocalSession ? (
                <ChatRenderer
                  messages={chatSocket.messages}
                  streamingDelta={chatSocket.streamingDelta}
                  streamingMessageId={chatSocket.streamingMessageId}
                  status={chatSocket.meta.status}
                  onSendMessage={chatSocket.sendMessage}
                />
              ) : (
                <TerminalRenderer
                  baseUrl={LOCAL_DAEMON}
                  sessionId={activeSessionId}
                  isLocal={isLocalSession}
                  isActive={visible}
                  onError={setError}
                />
              )}
            </>
          ) : (
            <div className="agents-empty">
              <p>Select a session or create a new one to get started.</p>
              {agents.length === 0 && (
                <p className="muted">
                  No agents detected.{" "}
                  <a
                    href="#"
                    onClick={(e) => {
                      e.preventDefault();
                      setSelectedAgentId("shell");
                      setSelectedMode("terminal");
                      void handleNewSession();
                    }}
                  >
                    + Set up an agent
                  </a>
                </p>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Add agents view CSS to App.css**

Append to `desktop/src/App.css`:

```css
/* ------------------------------------------------------------------ */
/* Agents View                                                         */
/* ------------------------------------------------------------------ */

.agents-view {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
}

.agents-topbar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 16px;
  border-bottom: 1px solid var(--border);
  background: var(--bg-surface);
}

.agents-topbar select {
  min-width: 200px;
}

.agents-main {
  display: flex;
  flex: 1;
  min-height: 0;
}

.agents-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.agents-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  color: var(--text-secondary);
  font-size: 0.9rem;
}

.agents-empty a {
  color: var(--accent-blue);
  text-decoration: none;
  font-weight: 500;
}
```

- [ ] **Step 3: Commit**

```bash
git add desktop/src/components/AgentsView.tsx desktop/src/App.css
git commit -m "feat(desktop): add AgentsView — unified agents page"
```

---

## Task 15: Wire AgentsView into App — Replace Chat + Terminal

**Files:**
- Modify: `desktop/src/App.tsx`
- Modify: `desktop/src/components/Sidebar.tsx`

- [ ] **Step 1: Update Sidebar nav items**

In `desktop/src/components/Sidebar.tsx`, replace the `NAV_ITEMS` array. Remove the separate "Terminal" and "Chat" entries, add a single "Agents" entry:

```typescript
const NAV_ITEMS = [
  {
    view: "agents" as MainView,
    label: "Agents",
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <path d="M12 8V4H8" /><rect width="16" height="12" x="4" y="8" rx="2" /><path d="M2 14h2" /><path d="M20 14h2" /><path d="M15 13v2" /><path d="M9 13v2" />
      </svg>
    ),
  },
  {
    view: "logs" as MainView,
    label: "Logs",
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <path d="M14.5 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7.5L14.5 2z" /><polyline points="14 2 14 8 20 8" /><line x1="16" x2="8" y1="13" y2="13" /><line x1="16" x2="8" y1="17" y2="17" /><line x1="10" x2="8" y1="9" y2="9" />
      </svg>
    ),
  },
  {
    view: "settings" as MainView,
    label: "Settings",
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" /><circle cx="12" cy="12" r="3" />
      </svg>
    ),
  },
];
```

- [ ] **Step 2: Update App.tsx**

In `desktop/src/App.tsx`:

1. Change the default `mainView` state from `"terminal"` to `"agents"`:
```typescript
const [mainView, setMainView] = useState<MainView>("agents");
```

2. Replace the `ChatView` and `TerminalWorkspace` rendering with `AgentsView`:

Remove the ChatView and TerminalWorkspace imports and their JSX. Add:

```typescript
import { AgentsView } from "./components/AgentsView";
```

In the render section, replace the ChatView and TerminalWorkspace blocks with:

```tsx
<AgentsView
  daemonUrl={LOCAL_DAEMON}
  sessions={/* pass daemon sessions from all connections */}
  localSessions={localSessions}
  visible={mainView === "agents"}
  onCreateLocalSession={handleCreateLocalSession}
  onRefreshSessions={() => {
    // Refresh sessions from daemon
    // Re-fetch from all connected hosts
  }}
/>
```

3. Update the `MainView` type import to use the new type (which no longer has "chat" or "terminal").

- [ ] **Step 3: Build frontend and verify**

```bash
cd desktop && npx tsc --noEmit 2>&1 | head -30
```

Expected: TypeScript type checks pass. Fix any import or type errors.

- [ ] **Step 4: Commit**

```bash
git add desktop/src/App.tsx desktop/src/components/Sidebar.tsx
git commit -m "feat(desktop): wire AgentsView into App, replace Chat + Terminal nav"
```

---

## Task 16: MCP Tool — ghost_spawn_remote_session

**Files:**
- Modify: `daemon/src/mcp/transport.rs`

- [ ] **Step 1: Add tool definition to tools/list**

In the `tools/list` handler in `daemon/src/mcp/transport.rs`, add to the tools array:

```rust
serde_json::json!({
    "name": "ghost_spawn_remote_session",
    "description": "Spawn an agent session on a remote machine in the mesh. Creates a fire-and-forget chat session. Returns the session ID and status.",
    "inputSchema": {
        "type": "object",
        "properties": {
            "targetMachine": {
                "type": "string",
                "description": "Name or IP of the target machine"
            },
            "agentId": {
                "type": "string",
                "description": "Agent ID to spawn (e.g., 'claude-code', 'ollama:llama3')"
            },
            "task": {
                "type": "string",
                "description": "Task description / initial message for the agent"
            },
            "workdir": {
                "type": "string",
                "description": "Working directory on the remote machine"
            }
        },
        "required": ["targetMachine", "agentId", "task"]
    }
})
```

- [ ] **Step 2: Add tool handler to tools/call**

In the `tools/call` match block, add:

```rust
"ghost_spawn_remote_session" => {
    let target = args.get("targetMachine").and_then(|v| v.as_str()).unwrap_or("");
    let agent_id = args.get("agentId").and_then(|v| v.as_str()).unwrap_or("");
    let task = args.get("task").and_then(|v| v.as_str()).unwrap_or("");
    let workdir = args.get("workdir").and_then(|v| v.as_str());

    // Find the target host URL
    let hosts = store.list_known_hosts().unwrap_or_default();
    let host = hosts.iter().find(|h| h.name == target || h.url.contains(target));

    match host {
        Some(host) => {
            let url = format!("{}/api/chat/sessions", host.url);
            let mut body = serde_json::json!({
                "agentId": agent_id,
            });
            if let Some(wd) = workdir {
                body["workdir"] = serde_json::Value::String(wd.to_string());
            }

            match client.post(&url)
                .json(&body)
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    let result: serde_json::Value = resp.json().await.unwrap_or_default();
                    let session_id = result["session"]["id"].as_str().unwrap_or("unknown");

                    // Send initial task message
                    let msg_url = format!("{}/api/chat/sessions/{}/message", host.url, session_id);
                    let _ = client.post(&msg_url)
                        .json(&serde_json::json!({"content": task}))
                        .send()
                        .await;

                    serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": format!("Spawned {} on {} (session: {}). Task: {}", agent_id, target, session_id, task)
                        }]
                    })
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    serde_json::json!({
                        "content": [{"type": "text", "text": format!("Failed to spawn session: {} {}", status, body)}],
                        "isError": true
                    })
                }
                Err(e) => {
                    serde_json::json!({
                        "content": [{"type": "text", "text": format!("Request failed: {}", e)}],
                        "isError": true
                    })
                }
            }
        }
        None => {
            serde_json::json!({
                "content": [{"type": "text", "text": format!("Machine '{}' not found in mesh. Known hosts: {}", target, hosts.iter().map(|h| h.name.as_str()).collect::<Vec<_>>().join(", "))}],
                "isError": true
            })
        }
    }
}
```

- [ ] **Step 3: Build and verify**

```bash
cd daemon && cargo build 2>&1 | tail -10
```

- [ ] **Step 4: Commit**

```bash
git add daemon/src/mcp/transport.rs
git commit -m "feat(mcp): add ghost_spawn_remote_session tool for agent delegation"
```

---

## Task 17: Integration Build and Smoke Test

**Files:** None new — verification only.

- [ ] **Step 1: Full daemon build**

```bash
cd daemon && cargo build 2>&1
```

Expected: Clean compile.

- [ ] **Step 2: Full desktop TypeScript check**

```bash
cd desktop && npx tsc --noEmit 2>&1
```

Expected: No type errors.

- [ ] **Step 3: Desktop dev build**

```bash
cd desktop && npm run build 2>&1 | tail -20
```

Expected: Vite build succeeds.

- [ ] **Step 4: Start daemon and verify endpoints**

```bash
cd daemon && cargo run -- serve &
sleep 2
curl -s http://127.0.0.1:8787/health | jq .
curl -s http://127.0.0.1:8787/api/agents | jq .
```

Expected: Health returns `{"ok": true}`. Agents returns a list of detected agents with `persistent` field.

- [ ] **Step 5: Verify switch-mode endpoint exists**

```bash
curl -s -X POST http://127.0.0.1:8787/api/sessions/nonexistent/switch-mode \
  -H 'Content-Type: application/json' \
  -d '{"mode":"chat"}' | jq .
```

Expected: Returns a 404 or error (not 405 method not allowed), confirming the route is registered.

- [ ] **Step 6: Kill daemon and commit if needed**

```bash
kill %1 2>/dev/null
```

If any fixes were needed during verification, commit them:

```bash
git add -A && git commit -m "fix: integration build fixes for unified agents page"
```

---

## Task 18: Clean Up Old Components

**Files:**
- Delete or mark deprecated: `desktop/src/components/ChatView.tsx`
- Delete or mark deprecated: `desktop/src/components/TerminalWorkspace.tsx`

- [ ] **Step 1: Remove old ChatView import and usage from App.tsx**

Verify that `ChatView` is no longer imported or used in `App.tsx`. If any references remain, remove them.

- [ ] **Step 2: Remove old TerminalWorkspace import and usage from App.tsx**

Verify that `TerminalWorkspace` is no longer imported or used in `App.tsx`. If any references remain, remove them.

- [ ] **Step 3: Delete the old files**

```bash
rm desktop/src/components/ChatView.tsx
rm desktop/src/components/TerminalWorkspace.tsx
```

- [ ] **Step 4: Verify build still passes**

```bash
cd desktop && npx tsc --noEmit 2>&1 | head -20
```

Expected: No errors. If other files import ChatView or TerminalWorkspace, update them.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "chore: remove old ChatView and TerminalWorkspace (replaced by AgentsView)"
```
