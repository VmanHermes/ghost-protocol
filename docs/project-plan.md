# Ghost Protocol — Project Plan

## Vision

A unified control plane for the Hermes AI agent across all your devices. Host terminal and chat sessions on any Linux machine, join from any other device on your Tailscale mesh, and eventually from iPhone.

## Current status: v0.2.0

The desktop app and backend daemon are functional for multi-machine terminal sharing over Tailscale.

### What works

- **Tauri 2 desktop app** — React + TypeScript + xterm.js
- **Local terminal sessions** — PTY via portable-pty in the Tauri backend
- **Rust daemon** — single binary, no Python dependency for hosting
- **Setup checklist** — detects tmux, Tailscale, mesh connectivity, and daemon; shows install commands per distro
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

## Phase 1 (complete): The Interface

Terminal sharing, local PTY, multi-host connections over Tailscale.

(See "What works" section above for details.)

---

## Phase 2 (current): The Context Layer

**Goal:** Make the mesh legible to AI agents so they can understand the network and act directly.

### 2a: MCP resource server

- Each daemon exposes an MCP server (JSON-RPC over stdio)
- Resources: machine/info, machine/status, network/hosts, terminal/sessions, agent/hints, context/briefing
- Dynamic agent briefing generated from live data

### 2b: CLI tools

- `ghost-protocol-daemon status` — one-line machine summary
- `ghost-protocol-daemon sessions` — terminal session list
- `ghost-protocol-daemon hosts` — known network peers
- `ghost-protocol-daemon info` — full machine profile as JSON
- All commands support `--json` flag for agent consumption

### 2c: Host registry

- Host list migrated from frontend localStorage to daemon SQLite
- Background health polling between daemons (30s interval)
- Capabilities auto-discovered from peer hardware endpoints

---

## Phase 3: Mobile + Polish

**Goal:** iPhone support and chat interface.

- Native iOS app or responsive web client
- Approve/deny UI for agent-gated tasks
- Push notifications for agent events
- Chat interface wired to Hermes
- Remote screenshots

---

## Phase 4: Deep Performance

**Goal:** Evaluate and add optimizations based on real usage patterns.

- Task queues for inference offloading (if needed)
- Advanced sandboxing for work/casual environment isolation
- Storage optimization (if large dataset access becomes a bottleneck)

---

## Architecture principles

- **Hermes runtime stays headless** — Ghost Protocol wraps it, doesn't replace it
- **Daemon is the source of truth** — all state flows through the Rust daemon
- **Tailscale for networking** — WireGuard-encrypted mesh, no HTTPS certificates needed for security
- **Desktop app is a thin client** — Tauri 2 + React, talks to daemon over HTTP + WebSocket
- **Sessions survive daemon restarts** — tmux keeps sessions alive, daemon reattaches on recovery

## Workspace layout

- `daemon/` — Rust daemon: HTTP + WebSocket transport for terminal sessions
- `desktop/` — Tauri 2 app: React + TypeScript frontend, Rust backend for PTY and system detection
- `docs/` — architecture, specs, plans
- `scripts/` — packaging and deployment
