# Ghost Protocol — Project Plan

## Vision

A unified control plane for the Hermes AI agent across all your devices. Host terminal and chat sessions on any Linux machine, join from any other device on your Tailscale mesh, and eventually from iPhone.

## Current status: v0.2.1

The desktop app and backend daemon are functional for multi-machine terminal sharing over Tailscale, with per-machine permissions, auto-discovery, and agent observability.

### What works

- **Tauri 2 desktop app** — React + TypeScript + xterm.js
- **Local terminal sessions** — PTY via portable-pty in the Tauri backend
- **Rust daemon** — single binary, no Python dependency for hosting
- **Setup checklist** — detects tmux, Tailscale, mesh connectivity, and daemon; shows install commands per distro
- **Auto-discovery** — daemon discovers Ghost Protocol peers via `tailscale status --json`, notifies user to add/dismiss
- **Connections sidebar** — sorted by state (connected/connecting/offline), replaces manual host management
- **Remote terminal sessions** — create/view terminal sessions on connected hosts via WebSocket
- **tmux-backed session persistence** — sessions survive daemon restarts, no input lag
- **Per-machine permissions** — 4 tiers (full-access, approval-required, read-only, no-access) enforced by daemon
- **Approval flow** — write operations from approval-required peers queued for owner approval via desktop notification
- **MCP resource server** — 7 read-only resources exposing machine info, sessions, hosts, outcomes, and context briefing
- **MCP tools** — `ghost_report_outcome`, `ghost_check_mesh`, `ghost_list_machines` for active agent interaction
- **Outcome log** — agents report work outcomes, daemon auto-captures terminal lifecycle, exposed via MCP
- **CLI tools** — `status`, `sessions`, `hosts`, `info` subcommands with `--json` flag
- **Log viewer** — unified client + server log stream with filtering and export
- **Settings page** — permission management per host with tier dropdowns
- **Right panel** — approval queue with approve/deny and countdown timers
- **Packaged release** — `scripts/package-linux.sh` builds a redistributable tarball with install script
- **Wayland compatibility** — `.desktop` launcher includes WebKit/GDK workarounds

### Known issues

- Chat/conversation UI exists but is not wired to working backend endpoints yet
- Session exit detection (natural exit with exit code) not yet captured in outcome log — only create/terminate

---

## Phase 1 (complete): The Interface

Terminal sharing, local PTY, multi-host connections over Tailscale.

(See "What works" section above for details.)

---

## Phase 2 (complete): The Context Layer

**Goal:** Make the mesh legible to AI agents so they can understand the network and act directly.

### 2a: MCP resource server ✓

- Each daemon exposes an MCP server (JSON-RPC over stdio)
- Resources: machine/info, machine/status, network/hosts, terminal/sessions, agent/hints, context/briefing, outcomes/recent
- Dynamic agent briefing generated from live data

### 2b: CLI tools ✓

- `ghost-protocol-daemon status` — one-line machine summary
- `ghost-protocol-daemon sessions` — terminal session list
- `ghost-protocol-daemon hosts` — known network peers
- `ghost-protocol-daemon info` — full machine profile as JSON
- All commands support `--json` flag for agent consumption

### 2c: Host registry ✓

- Host list migrated from frontend localStorage to daemon SQLite
- Background health polling between daemons (30s interval)
- Capabilities auto-discovered from peer hardware endpoints

### 2d: Peer permissions ✓

- 4 permission tiers: full-access, approval-required, read-only, no-access
- Per-machine identity via Tailscale IP, configured in desktop Settings
- Approval queue with 120s timeout, desktop notification UI
- WebSocket tier enforcement (read-only peers can't send input)
- Permission-aware MCP context briefings

### 2e: Mesh auto-discovery ✓

- Daemon discovers peers via `tailscale status --json` on 30s interval
- Probes port 8787 to confirm Ghost Protocol is running
- Discovery notifications in sidebar (add/dismiss)
- Sidebar renamed "Connections", sorted by state
- Removed manual hosting flow (daemon started independently)

### 2f: Outcome log ✓

- Daemon auto-captures terminal create/terminate events
- Agents report richer outcomes via POST /api/outcomes
- Free-form category/action taxonomy (agents choose labels)
- MCP resource ghost://outcomes/recent for agent awareness
- Activity summary in context briefing

### 2g: MCP tools ✓

- tools/list and tools/call JSON-RPC handlers
- ghost_report_outcome — agents report work results
- ghost_check_mesh — on-demand mesh state briefing
- ghost_list_machines — structured machine data for routing
- Context briefing includes tool usage instructions

---

## Phase 3: Intelligence + Chat

**Goal:** Make agents smarter about the mesh and wire up the chat interface.

### 3a: Distribution Advisor

- LLM with RAG over the outcome log
- Suggests which machine to route work to based on historical performance, current load, and capabilities
- Exposed as MCP tool: `ghost_route_advice`

### 3b: Behavioral Oversight ("Police" LLM)

- Monitors inter-agent communication patterns
- Flags anomalies (repeated write attempts from low-trust peers, unusual request patterns)
- Could auto-downgrade tiers or enforce rate limits

### 3c: Chat interface

- Chat UI wired to Hermes runtime
- Conversation/message persistence in daemon SQLite
- Agent run lifecycle visible in desktop app

### 3d: Session exit detection

- Detect natural session exits (PTY EOF) with exit codes
- Auto-capture `session_exited` outcomes with duration and exit code
- Richer data for distribution advisor

---

## Phase 4: Mobile + Polish

**Goal:** iPhone support and cross-platform polish.

- Native iOS app or responsive web client
- Push notifications for agent events and approval requests
- Remote screenshots
- Tailscale ACL integration beyond CIDR allowlisting
- Subagent tree reconstruction from delegated runs

---

## Phase 5: Deep Performance

**Goal:** Evaluate and add optimizations based on real usage patterns.

- Task queues for inference offloading (if needed)
- Advanced sandboxing for work/casual environment isolation
- Storage optimization (if large dataset access becomes a bottleneck)
- Artifact persistence linked to originating events

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
