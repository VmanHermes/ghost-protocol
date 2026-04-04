# Ghost Protocol — Project Plan

## Vision

A unified control plane for the Hermes AI agent across all your devices. Host terminal and chat sessions on any Linux machine, join from any other device on your Tailscale mesh, and eventually from iPhone.

## Current status: v0.1.1

The desktop app and backend daemon are functional for multi-machine terminal sharing over Tailscale.

### What works

- **Tauri 2 desktop app** — React + TypeScript + xterm.js
- **Local terminal sessions** — PTY via portable-pty in the Tauri backend
- **Setup checklist** — detects Python, tmux, Tailscale, mesh connectivity, and daemon; shows install commands per distro
- **Host a connection** — auto-installs daemon, starts it bound to Tailscale IP, detached process survives app close
- **Join a connection** — add remote hosts by Tailscale IP, health polling, reconnect on failure
- **Remote terminal sessions** — create/view terminal sessions on connected hosts via WebSocket
- **tmux-backed session persistence** — sessions survive daemon restarts, no input lag
- **Log viewer** — unified client + server log stream with filtering and export
- **Packaged release** — `scripts/package-linux.sh` builds a redistributable tarball with install script
- **Wayland compatibility** — `.desktop` launcher includes WebKit/GDK workarounds

### Known issues

- Remote terminal creation from the joining machine needs debugging (logging added, needs testing after reinstall)
- Chat/conversation UI exists but is not wired to working backend endpoints yet

---

## Phase 2: Chat + Terminal convergence

**Goal:** Restore the AI chat interface and make it work alongside shared terminals.

### 2a: Chat sessions (next)

- Wire ChatView component to backend conversation + message APIs
- Real-time message streaming via WebSocket
- Conversation list in sidebar (create, switch, delete)
- Messages persist in backend event store

### 2b: Terminal ↔ Chat sync

- Link a terminal session to a conversation — agent can see terminal output, user can see agent actions
- Agent commands execute in the linked terminal
- Shared context: both local and remote clients see the same chat + terminal state

### 2c: Multi-client chat

- Multiple clients connected to the same host see the same conversation in real time
- Presence indicators (who's connected)
- Input from any client is visible to all

---

## Phase 3: Observability + remote control

**Goal:** Visibility into what's actually running across machines.

### 3a: Task observability

- Dashboard showing active agent runs, tool executions, and their status
- Works for both local and remote hosts
- History of completed tasks with duration, outcome, and logs

### 3b: code-server integration

- Open a code-server instance on the remote host from a link below the terminal
- Allows full IDE access to the remote machine's workspace

### 3c: Remote desktop / screenshot

- Command to take a screenshot of the remote machine's display
- Stretch: lightweight remote desktop view (VNC/RDP over Tailscale)

---

## Phase 4: iPhone support

**Goal:** Join connections from iPhone.

- Native iOS app or responsive web client
- Read-only terminal view at minimum, interactive stretch goal
- Chat interface works fully
- Push notifications for agent events (task complete, approval needed)

---

## Architecture principles

- **Hermes runtime stays headless** — Ghost Protocol wraps it, doesn't replace it
- **Daemon is the source of truth** — all state flows through the Python backend
- **Tailscale for networking** — WireGuard-encrypted mesh, no HTTPS certificates needed for security
- **Desktop app is a thin client** — Tauri 2 + React, talks to daemon over HTTP + WebSocket
- **Sessions survive daemon restarts** — tmux keeps sessions alive, daemon reattaches on recovery

## Workspace layout

- `backend/` — Python daemon: event store, projections, HTTP + WebSocket transport
- `desktop/` — Tauri 2 app: React + TypeScript frontend, Rust backend for PTY and system detection
- `docs/` — architecture, specs, plans
- `scripts/` — packaging and deployment
