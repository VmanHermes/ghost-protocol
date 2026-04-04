# Rust Daemon Rewrite — Design Spec

## Context

Ghost Protocol v0.1.1 has a working Python daemon (`ghost_protocol_daemon`) that manages terminal sessions over WebSocket, backed by tmux for persistence. It also contains agent runtime integration (Hermes), conversations, approvals, and a Telegram bridge — features that were designed for an agent-centric control plane.

The project vision has shifted toward a **general remote host platform**: terminal access, session management, code editing, screenshots, and remote control across any devices on a Tailscale mesh. Agents (Claude Code, Hermes, OpenClaw) are users of this platform — they run inside terminal sessions rather than being integrated into the daemon runtime.

This design replaces the Python daemon with a Rust binary that focuses on **terminal multiplexing and host capabilities**, dropping agent-specific features. The result is a simpler, faster, single-binary daemon with no Python dependency on remote hosts.

### Design decisions made

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Terminal transport | WebSocket PTY (keep) | Already works, provides chunk replay, multi-client sync, and event bus integration that SSH doesn't offer natively |
| SSH role | Cross-machine agent access | Agents use SSH (via Tailscale) for raw host access when needed. Structured MCP access deferred to later phase. |
| Daemon language | Rust | Single binary distribution, no Python/venv dependency, same language as Tauri backend, portable-pty reuse |
| Daemon deployment | Separate binary | Not merged into Tauri app. Standalone `ghost-protocol-daemon` process. |
| Agent integration | None in daemon | Agents run in terminal sessions. Daemon doesn't manage agent lifecycle, runs, approvals, or artifacts. |
| Session persistence | tmux | Sessions survive daemon restarts. Daemon manages tmux lifecycle (create, attach, detach, kill). |
| Frontend impact | Minimal | Same REST + WebSocket API contract. Frontend needs minor cleanup only. |

---

## Architecture

### System overview

```
┌─────────────────────────────────────────────────────────┐
│  Machine A (any host on Tailscale mesh)                 │
│                                                         │
│  ghost-protocol-daemon (Rust binary)                    │
│  ├── HTTP API (axum)          :8787                     │
│  ├── WebSocket (terminal streaming, events)             │
│  ├── Terminal sessions (tmux + PTY)                     │
│  ├── SQLite (sessions, chunks)                          │
│  └── Host capabilities (health, system info, logs)      │
│                                                         │
│  ghost-protocol (Tauri app) ─── optional, UI client     │
│  ├── Local terminal (portable-pty, no daemon needed)    │
│  ├── Remote terminals (WebSocket to any daemon)         │
│  └── SSH (for agent cross-machine access via terminal)  │
└─────────────────────────────────────────────────────────┘
          │                          ▲
          │    Tailscale mesh        │
          │    (WireGuard encrypted) │
          ▼                          │
┌─────────────────────────────────────────────────────────┐
│  Machine B (another host on Tailscale mesh)             │
│  Same setup: daemon + optional app                      │
└─────────────────────────────────────────────────────────┘
```

### P2P model

Every machine running the daemon is both a client and a host. The "host" vs "join" distinction is a UI concept only — any daemon serves terminals to any mesh peer that connects. Peer discovery can use Tailscale peer probing (the setup checklist already does this).

### Security model

- **Network layer:** Tailscale mesh provides WireGuard encryption and device identity
- **Application layer:** CIDR-based IP allowlist (same as current Python daemon)
- **Agent access:** SSH over Tailscale for cross-machine agent access (Tailscale SSH for keyless auth)
- **No bearer tokens in v1** — Tailscale provides the trust boundary

---

## Rust daemon internal structure

```
ghost-protocol-daemon/
├── Cargo.toml
└── src/
    ├── main.rs              — CLI args, config loading, server startup
    ├── config.rs            — Settings from env vars (bind host, port, CIDRs, log dir)
    ├── server.rs            — axum router, middleware stack, graceful shutdown
    │
    ├── terminal/
    │   ├── mod.rs           — TerminalManager (owns all sessions, recovery on startup)
    │   ├── session.rs       — ManagedSession (tmux lifecycle, PTY attach/detach)
    │   ├── broadcaster.rs   — Multi-client broadcast (tokio::broadcast channels)
    │   └── tmux.rs          — tmux CLI wrapper (create, attach, kill, list)
    │
    ├── transport/
    │   ├── http.rs          — REST handlers (session CRUD, health, system info)
    │   └── ws.rs            — WebSocket handler (subscribe, stream, input, resize)
    │
    ├── store/
    │   ├── mod.rs           — Database pool, embedded migrations
    │   ├── sessions.rs      — Terminal session CRUD (create, update, list, get)
    │   └── chunks.rs        — Terminal chunk storage & replay queries
    │
    ├── middleware/
    │   ├── tailscale.rs     — CIDR-based IP allowlist
    │   └── cors.rs          — CORS headers (reflect Origin, standard methods)
    │
    └── host/
        ├── detect.rs        — Tailscale IP detection, SSH status, system info
        └── logs.rs          — In-memory ring buffer, log streaming endpoint
```

### Key Rust crates

| Crate | Purpose |
|-------|---------|
| `axum` | HTTP + WebSocket server |
| `tokio` | Async runtime, process spawning, broadcast channels |
| `tokio-tungstenite` | WebSocket protocol (via axum) |
| `rusqlite` | SQLite with bundled build — synchronous DB access is fine since terminal I/O is the hot path, not queries |
| `nix` | PTY pair creation (`openpty`), signals, process groups — lighter than portable-pty for Unix-only daemon |
| `serde` / `serde_json` | JSON serialization for API |
| `tracing` | Structured logging |
| `clap` | CLI argument parsing |

---

## Terminal session lifecycle

Same model as the current Python daemon, implemented in Rust:

### Creation
1. Generate UUID session ID
2. Insert DB record: `status = 'created'`, mode, name, workdir
3. Spawn tmux: `tmux new-session -d -s ghost-{id} -c {workdir}`
4. Configure tmux: status off, mouse off, pane-border off
5. Update DB: `status = 'running'`, PID

### Subscription (client connects via WebSocket)
1. Client sends `{op: "subscribe_terminal", sessionId, afterChunkId?}`
2. If session has no active PTY attachment, spawn one:
   - Create PTY pair via `nix::pty::openpty`
   - Run `tmux attach-session -t ghost-{id}` on the slave FD
   - Start async reader task on master FD (read → broadcast + persist chunks)
3. Replay chunks from DB where `id > afterChunkId`
4. Add client's WebSocket sender to broadcast subscriber list
5. Forward live chunks from broadcast channel to client

### Input
- Client sends `{op: "terminal_input", sessionId, input}`
- Write input bytes to PTY master FD

### Resize
- Client sends `{op: "resize_terminal", sessionId, cols, rows}`
- `ioctl` TIOCSWINSZ on PTY master FD

### Unsubscription (client disconnects)
- Remove from broadcast subscriber list
- If no subscribers remain, start idle timer (120 seconds)
- On idle timeout: detach PTY (close FD, kill attach process), keep tmux alive
- On next subscription: reattach (create new PTY, attach to existing tmux session)

### Termination
- `tmux kill-session -t ghost-{id}`
- Close master FD, terminate attach process
- Update DB: `status = 'terminated'`, finishedAt

### Recovery (daemon startup)
1. List active tmux sessions with `ghost-` prefix
2. Reconcile with DB: claim matching sessions, mark orphaned DB records as terminated
3. Kill orphaned tmux sessions (ghost- prefix with no DB record)

---

## API contract

Same endpoints as the current Python daemon (minus agent-specific features). The React frontend works unchanged.

### HTTP endpoints

```
GET  /health
  → {ok: true}

GET  /api/system/status
  → {activeTerminalSessions: number, bindAddress: string, allowedCidrs: string[]}

GET  /api/system/logs?limit=200&level=INFO
  → [{level, message, timestamp, source}]

GET  /api/terminal/sessions
  → [TerminalSession]

POST /api/terminal/sessions
  body: {mode?: "agent"|"project"|"rescue_shell", name?: string, workdir?: string}
  → TerminalSession

GET  /api/terminal/sessions/{id}
  → TerminalSession (auto-reattaches if detached)

POST /api/terminal/sessions/{id}/input
  body: {input: string, appendNewline?: bool}
  → 204

POST /api/terminal/sessions/{id}/resize
  body: {cols: number, rows: number}
  → 204

POST /api/terminal/sessions/{id}/terminate
  → 204
```

### WebSocket protocol (`/ws`)

Client → Server:
```json
{"op": "subscribe_terminal", "sessionId": "...", "afterChunkId": 0}
{"op": "terminal_input", "sessionId": "...", "input": "ls\n"}
{"op": "resize_terminal", "sessionId": "...", "cols": 120, "rows": 30}
{"op": "interrupt_terminal", "sessionId": "..."}
{"op": "terminate_terminal", "sessionId": "..."}
{"op": "ping", "ts": "..."}
```

Server → Client:
```json
{"op": "subscribed_terminal", "session": {...}}
{"op": "terminal_chunk", "chunk": {"id": 1, "sessionId": "...", "stream": "stdout", "chunk": "...", "createdAt": "..."}}
{"op": "terminal_status", "session": {...}}
{"op": "heartbeat", "ts": "..."}
{"op": "error", "message": "..."}
```

---

## Broadcaster pattern

Each terminal session maintains a broadcast channel:

```rust
struct ManagedSession {
    session_id: String,
    tmux_session_name: String,
    master_fd: OwnedFd,
    attach_process: Child,
    broadcaster: broadcast::Sender<TerminalChunk>,
}
```

When the reader task reads from the PTY master:
1. Decode bytes as UTF-8
2. Create `TerminalChunk` with auto-incremented ID
3. Persist chunk to SQLite
4. Send on broadcast channel → all subscribers receive it
5. Subscribers forward chunk to their WebSocket

On reconnect, the client provides `afterChunkId`. The server:
1. Queries SQLite for chunks where `id > afterChunkId`
2. Sends those as replay
3. Then subscribes to live broadcast

This gives you multi-client sync (everyone sees every character) and reconnect resilience (no lost output).

---

## What drops from the frontend

The React frontend needs minor cleanup, not a rewrite:

| Component | Change |
|-----------|--------|
| `useTerminalSocket.ts` | None — same WebSocket protocol |
| `TerminalWorkspace.tsx` | None — same session model |
| `Sidebar.tsx` | None — host list, hosting control unchanged |
| `SetupChecklist.tsx` | Update: detect Rust binary instead of Python venv |
| `App.tsx` | Remove/hide agent-specific UI (runs panel, approval buttons) |
| `ChatView.tsx` | Hide for v1 (no chat endpoint). Keep component for Phase 2. |
| `InspectorPanel.tsx` | Simplify — show only terminal sessions and host status |
| `types.ts` | Remove agent-related types (RunRecord, ApprovalRecord, etc.) or keep for later |
| `detect.rs` (Tauri) | Update `install_daemon()` to download/copy Rust binary instead of pip install |

### Daemon installation flow change

Current (Python):
```
install_daemon() → create venv → pip install → python -m ghost_protocol_daemon
```

New (Rust):
```
install_daemon() → cargo install from bundled source or download pre-built binary from GitHub release → chmod +x → ./ghost-protocol-daemon
```

For development: `cargo build --release` in `daemon/` produces the binary. For packaged releases: the binary is included in the tarball alongside the Tauri app. For remote hosts: the Tauri app copies the binary via `scp` or downloads it from a GitHub release URL.

Setup checklist: Python is no longer required for hosting. Only tmux, Tailscale, and the daemon binary.

---

## Repo layout after migration

```
ghost-protocol/
├── daemon/                  ← NEW: Rust daemon (replaces backend/)
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── config.rs
│       ├── server.rs
│       ├── terminal/
│       ├── transport/
│       ├── store/
│       ├── middleware/
│       └── host/
├── backend/                 ← REMOVED after migration verified
├── desktop/                 ← Existing Tauri app (minimal changes)
│   ├── src/                 ← React frontend
│   └── src-tauri/           ← Tauri Rust backend (PTY, detect)
├── docs/
└── scripts/
```

---

## Future extension points

The daemon is designed so these plug in as new modules without restructuring core terminal infrastructure:

### Peer discovery
- Daemon advertises itself on Tailscale mesh (mDNS or Tailscale tags)
- App auto-discovers Ghost Protocol daemons on the mesh
- Extends existing setup checklist peer probing

### code-server integration (next priority)
- New module: `host/code_server.rs`
- Endpoints: `POST /api/code-server/start`, `POST /api/code-server/stop`, `GET /api/code-server/status`
- Daemon launches code-server as subprocess, bound to Tailscale IP
- App opens URL in iframe or new browser tab

### Screenshots / remote display
- New module: `host/screenshot.rs`
- Endpoint: `GET /api/screenshot` → PNG
- Uses `grim` (Wayland) or `scrot` (X11) via subprocess
- Binary frames over WebSocket for live view

### Chat / messaging
- New module tree: `chat/` with store, WebSocket pub/sub
- Same API contract as current Python daemon — frontend ChatView works unchanged
- Endpoints: `GET /api/conversations`, `POST /api/conversations`, `POST /api/conversations/{id}/messages`

### Telegram bridge
- New module: `integrations/telegram.rs`
- Optional feature flag: `--features telegram`
- Poll Telegram API, bridge to daemon events

### Maybe: MCP server (structured agent cross-mesh access)
- New module tree: `mcp/` with tools, permissions, protocol handler
- Either built into daemon (`--mcp` flag starts MCP stdio server) or separate binary
- Tools: `list_hosts`, `list_sessions`, `send_input`, `read_output`, `take_screenshot`
- Per-host permission policies, audit logging, approval gates


### The extension pattern

Each new capability follows the same pattern:
1. Add a module under `host/`, `chat/`, `mcp/`, or `integrations/`
2. Register routes in `server.rs`
3. Add SQLite tables via embedded migration
4. Expose via existing WebSocket for real-time features
5. Core terminal infrastructure remains unchanged

---

## Migration strategy

1. **Build Rust daemon alongside Python daemon** — both exist in the repo during development
2. **Implement core terminal features** — session CRUD, WebSocket streaming, chunk replay, multi-client broadcast
3. **Test with existing frontend** — point the Tauri app at Rust daemon on same port, verify all terminal operations
4. **Update detect.rs** — switch installation from Python venv to Rust binary
5. **Frontend cleanup** — remove/hide agent-specific UI
6. **Remove Python daemon** — once Rust daemon is fully verified
7. **Update packaging scripts** — `scripts/package-linux.sh` includes Rust binary instead of Python package

---

## Verification plan

### Terminal operations
- [ ] Create session → tmux session appears, PTY attached
- [ ] Subscribe via WebSocket → receive terminal output in real-time
- [ ] Send input → appears in tmux session
- [ ] Resize → terminal reflowed correctly
- [ ] Interrupt (Ctrl+C) → signal delivered
- [ ] Terminate → tmux session killed, DB updated
- [ ] Disconnect and reconnect → chunks replayed from afterChunkId
- [ ] Multiple clients subscribe → all see same output simultaneously
- [ ] Daemon restart → existing tmux sessions recovered, reattachable

### API compatibility
- [ ] All HTTP endpoints return same JSON shape as Python daemon
- [ ] WebSocket message format identical to Python daemon
- [ ] Existing React frontend works without changes (except agent UI removal)

### Installation
- [ ] Single binary copied to remote host runs without dependencies (except tmux)
- [ ] detect.rs installs Rust binary, launches successfully
- [ ] Setup checklist no longer requires Python for hosting

### Cross-machine
- [ ] Machine A hosts daemon, Machine B connects via Tauri app
- [ ] Create remote terminal session from Machine B → session appears on Machine A
- [ ] Multiple machines subscribe to same session → real-time sync
- [ ] Agent (Claude Code) in local terminal SSHs to remote host via Tailscale