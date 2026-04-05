# Ghost CLI, Project System & Agent Chat

**Date:** 2026-04-05
**Status:** Draft
**Phase:** 3a (Agent Chat — full feature)

## Context

Ghost Protocol manages terminals across a Tailscale mesh but lacks a project-level abstraction and agent interaction. Users need a way to initialize projects, configure which agents work on them, and start agent conversations from any machine. Instead of auto-detecting agents by scanning for binaries, we take an intentional approach: users run `ghost init` to set up a project with a full manifest.

## Goals

1. A `ghost` CLI tool with `init`, `status`, `agents`, `chat`, and other subcommands
2. Project manifest (`.ghost/config.json`) defining agents, machines, build/test/deploy config
3. Daemon project registry — daemon tracks registered projects
4. Agent chat sessions that wrap terminal sessions with a message layer
5. Terminal help text on local terminal open — shows available ghost commands
6. Unified session system with `session_type` field (terminal, chat, code-server)

## Non-Goals

- Code-server session management (Phase 3b, will follow the session type pattern)
- Distribution advisor / routing intelligence (Experimental/TBD)

---

## Ghost CLI

A new binary (`ghost`) installed alongside the daemon. Communicates with the local daemon via HTTP (localhost:8787).

### Commands

| Command | Description |
|---|---|
| `ghost init` | Initialize a project in the current directory |
| `ghost status` | Show mesh status (machines, sessions, agents) |
| `ghost agents` | List available agents on all machines |
| `ghost chat <agent>` | Start a chat session with an agent |
| `ghost projects` | List registered projects |
| `ghost help` | Show available commands |

### `ghost init`

Interactive setup flow:

```
$ cd ~/projects/my-app
$ ghost init

Initializing Ghost Protocol project...

Project name (my-app): 
Description: A web application built with React

Detecting available agents...
  ✓ Claude Code v1.0.25
  ✓ Ollama (llama3, codellama)
  ✓ Hermes

Select agents for this project (comma-separated, or 'all'):
> claude-code, ollama:llama3

Preferred machine for builds (leave empty for any):
> shared-host

Created .ghost/config.json
Registered project with daemon.

Run 'ghost chat claude-code' to start working.
```

Creates `.ghost/config.json` and registers the project with the daemon via `POST /api/projects`.

### `ghost chat <agent>`

Starts an interactive chat session with the specified agent:

```
$ ghost chat claude-code
Starting Claude Code on laptop...

Claude Code > How can I help you with my-app?

You > Can you review the authentication module?
```

This creates a chat session on the daemon, wrapping a terminal session that runs the agent CLI process. The ghost CLI acts as a thin client — input goes to daemon, streamed output comes back.

---

## Project Manifest

### `.ghost/config.json`

```json
{
  "name": "my-app",
  "workdir": "/home/vman/projects/my-app",
  "agents": [
    {
      "id": "claude-code",
      "enabled": true,
      "preferredMachine": null
    },
    {
      "id": "ollama:llama3",
      "enabled": true,
      "preferredMachine": "shared-host"
    }
  ],
  "machines": {
    "shared-host": {
      "roles": ["build", "inference"]
    }
  },
  "commands": {
    "build": "cargo build --release",
    "test": "cargo test",
    "lint": "cargo clippy",
    "deploy": null
  },
  "environment": {
    "RUST_LOG": "debug"
  }
}
```

### Fields

| Field | Required | Description |
|---|---|---|
| `name` | yes | Project name (defaults to directory name) |
| `workdir` | yes | Absolute path to project root (set automatically by `ghost init`) |
| `agents` | yes | Which agents are configured for this project |
| `agents[].id` | yes | Agent identifier (e.g., `claude-code`, `ollama:llama3`, `hermes`) |
| `agents[].enabled` | yes | Whether this agent is active |
| `agents[].preferredMachine` | no | Hostname to prefer for running this agent (null = any) |
| `machines` | no | Per-machine role assignments for this project |
| `commands` | no | Project commands: build, test, lint, deploy |
| `environment` | no | Environment variables to set when running agents in this project |

---

## Agent Detection

During `ghost init`, the CLI probes for available agents to suggest them to the user. This is a **one-time interactive detection** (not a continuous background scan).

### Detectors

| Agent | Detection | Notes |
|---|---|---|
| Claude Code | `which claude` + `claude --version` | CLI agent |
| Hermes | `which hermes` | CLI agent |
| Ollama | `curl -s localhost:11434/api/tags` | Lists installed models, each is a separate agent |
| Aider | `which aider` + `aider --version` | CLI agent |
| OpenClaw | `which openclaw` (verify binary name) | CLI agent |

Detection results are shown to the user during init. The user picks which agents to enable for the project.

### AgentInfo Type (daemon-side)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub agent_type: String,   // "cli" | "api"
    pub command: String,
    pub version: Option<String>,
}
```

The daemon also runs detection on startup (and every 5 minutes) to know which agents are available on the local machine — this feeds into the API and MCP resources.

---

## Daemon Project Registry

### Migration: `006_projects.sql`

```sql
CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    workdir TEXT NOT NULL UNIQUE,
    config_json TEXT NOT NULL,
    registered_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

ALTER TABLE terminal_sessions ADD COLUMN session_type TEXT NOT NULL DEFAULT 'terminal';
ALTER TABLE terminal_sessions ADD COLUMN project_id TEXT REFERENCES projects(id);
```

### API Endpoints

**`POST /api/projects`** — register a project (called by `ghost init`)
```json
{
  "name": "my-app",
  "workdir": "/home/vman/projects/my-app",
  "config": { ... full .ghost/config.json contents ... }
}
```

**`GET /api/projects`** — list registered projects

**`GET /api/projects/{id}`** — get project details

**`PUT /api/projects/{id}`** — update project config (called when .ghost/config.json changes)

**`DELETE /api/projects/{id}`** — unregister a project

**`GET /api/agents`** — list agents available on this machine (from detection, not project config)

---

## Chat Sessions

### Architecture

A chat session wraps a terminal session:

1. User runs `ghost chat claude-code` (or picks agent in desktop UI)
2. Daemon creates a terminal session with `session_type = 'chat'`
3. The agent CLI process runs in tmux (e.g., `claude --print` or `ollama run llama3`)
4. Raw PTY output captured as `terminal_chunks` (existing)
5. Additionally, an **agent adapter** parses the output into structured `chat_messages`
6. User input goes to the agent's stdin via the terminal session

### Agent Adapters

Each agent type gets a parsing adapter:

| Agent | Adapter strategy |
|---|---|
| Claude Code | Parse markdown output, detect tool calls, separate thinking from response |
| Ollama | Stream text — each response between prompts is one assistant message |
| Hermes | Parse structured output format |
| Generic | Delimiter-based: user input = user message, everything between inputs = assistant message |

Adapters are daemon-side Rust modules. Start with Claude Code + Hermes + Ollama + Generic. Others added as needed.

### Chat Messages Table

**Migration (part of `006_projects.sql`):**

```sql
CREATE TABLE IF NOT EXISTS chat_messages (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES terminal_sessions(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_chat_messages_session ON chat_messages(session_id);
```

**Roles:** `user`, `assistant`, `system`

### WebSocket Ops (new)

| Op | Direction | Description |
|---|---|---|
| `subscribe_chat` | client→server | Subscribe to chat messages for a session (like subscribe_terminal) |
| `chat_message` | server→client | New parsed message (role, content, timestamp) |
| `send_chat_message` | client→server | User sends a message to the agent |

The `subscribe_chat` op replays existing messages from DB, then streams new ones as the adapter parses them. `send_chat_message` writes the user's text to the agent's stdin and creates a user message record.

---

## Terminal Help Text

When a local terminal opens in Ghost Protocol, inject a welcome message:

```
Ghost Protocol v0.2.1 — laptop (100.64.1.1)

Commands:
  ghost init          Set up a project in this directory
  ghost status        Mesh overview (machines, sessions)
  ghost agents        Available agents across the mesh
  ghost chat <agent>  Start a chat with an agent
  ghost projects      Registered projects
  ghost help          Full command reference

Mesh: 2 machines connected. Run 'ghost status' for details.
```

This is injected as the first chunk when a local terminal session starts (a system chunk with stream type `system`). The existing `terminal_chunks` table already has a `stream` field that supports `stdout`, `stderr`, and `system`.

---

## Desktop UI Changes

### Chat View (revive and adapt)

The existing `ChatView.tsx` component gets revived with new props:

- Machine + agent picker (dropdown or sidebar selector)
- Message list with role-based styling (user/assistant/system)
- Input composer with send button
- Streaming indicator for in-progress responses
- Project context display (which project this chat is for)

### Sidebar

Connections show available agents per machine:
```
● shared-host    100.64.1.3
  Claude Code, Ollama (3 models)
```

### Session Tabs

Terminal workspace tab bar shows both terminal and chat sessions:
- Terminal tabs: existing behavior
- Chat tabs: agent icon + name, machine name
- Click to switch between terminal and chat views

---

## MCP Integration

### Updated Resources

**`ghost://agents/available`** — agents per machine (from daemon detection + peer capabilities)

### Updated Tools

**`ghost_list_agents`** — structured agent data for routing decisions

### Context Briefing

```
Available agents:
  laptop (this machine): Claude Code v1.0.25
  shared-host: Claude Code, Ollama (llama3, codellama), Hermes

Registered projects:
  my-app (/home/vman/projects/my-app) — agents: claude-code, ollama:llama3
```

---

## Files to Create/Modify

### New: Ghost CLI Binary

| File | Description |
|---|---|
| `cli/` | New crate for the ghost CLI |
| `cli/Cargo.toml` | Dependencies (clap, reqwest, serde, etc.) |
| `cli/src/main.rs` | Entry point, subcommand dispatch |
| `cli/src/init.rs` | `ghost init` — interactive project setup |
| `cli/src/chat.rs` | `ghost chat` — thin client for chat sessions |
| `cli/src/commands.rs` | `ghost status`, `agents`, `projects`, `help` |
| `cli/src/detect.rs` | Agent detection probes (reuse from daemon) |

### Daemon (Rust)

| File | Change |
|---|---|
| `daemon/migrations/006_projects.sql` | Projects table, session_type column, chat_messages table |
| `daemon/src/store/projects.rs` | New — CRUD for projects |
| `daemon/src/store/chat.rs` | New — CRUD for chat_messages |
| `daemon/src/store/mod.rs` | Register new modules, run migration |
| `daemon/src/store/sessions.rs` | Add session_type and project_id to TerminalSessionRecord |
| `daemon/src/hardware/agents.rs` | New — agent detection logic |
| `daemon/src/hardware/mod.rs` | Integrate agent detection |
| `daemon/src/chat/` | New module — agent adapters (claude, ollama, generic) |
| `daemon/src/server.rs` | Register project/agent routes, spawn detection task |
| `daemon/src/transport/http.rs` | Add project CRUD + agent list endpoints |
| `daemon/src/transport/ws.rs` | Add chat WebSocket ops |
| `daemon/src/mcp/resources.rs` | Add agents resource, update briefing |
| `daemon/src/mcp/transport.rs` | Register new resource/tool |

### Desktop (TypeScript/React)

| File | Change |
|---|---|
| `desktop/src/types.ts` | Add AgentInfo, Project, ChatMessage types |
| `desktop/src/api.ts` | Add project/agent/chat API functions |
| `desktop/src/components/ChatView.tsx` | Revive with new props and agent/machine picker |
| `desktop/src/components/Sidebar.tsx` | Show agents per connection |
| `desktop/src/App.tsx` | Wire up chat view, add chat session state |
