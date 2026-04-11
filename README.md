# Ghost Protocol

![Platform](https://img.shields.io/badge/platform-Linux-blue)
![Desktop](https://img.shields.io/badge/desktop-Tauri%202-7c3aed)
![Frontend](https://img.shields.io/badge/frontend-React%20%2B%20Vite-61dafb)
![Backend](https://img.shields.io/badge/backend-Rust-dea584)

Ghost Protocol is a multi-machine AI agent control plane built on Tailscale. Your machines become a mesh where AI agents are first-class citizens — they can run on any device, remember what happened before, and report back what they did. Instead of managing terminals and agents per-machine, you get a single control plane that spans all your hardware.

**[Why this exists](docs/vision.md)** · **[Project plan & roadmap](docs/project-plan.md)** · **[Runbook](docs/runbook.md)**

## What it does

- **Auto-discovery** — finds Ghost Protocol peers on your Tailscale mesh automatically
- **Connections** — sidebar shows all machines sorted by state (connected/offline)
- **Terminals** — create and share terminal sessions across machines, persisted via tmux
- **Agent chat** — discover available AI agents (Claude Code, Hermes, Ollama, Aider, OpenClaw) and chat with them
- **Permissions** — 4 tiers per machine: full-access, approval-required, read-only, no-access
- **Approval flow** — write operations from guarded peers queue for your approval with 120s timeout
- **Outcome log** — agents report work results, daemon auto-captures terminal lifecycle
- **Intelligence layer** — embedded LLM-powered memory that learns from agent sessions (pre-session enrichment, post-session extraction, on-demand recall)
- **Code-server** — start/stop/adopt VS Code in browser instances, detected automatically, managed alongside terminal sessions
- **MCP tools** — `ghost_recall`, `ghost_report_outcome`, `ghost_check_mesh`, `ghost_list_machines`, `ghost_list_agents`
- **Ghost CLI** — `ghost init`, `ghost status`, `ghost agents`, `ghost chat`, `ghost projects`
- **Settings** — permission management per host with tier dropdowns

## Architecture

```
┌─────────────────���────┐       Tailscale mesh       ┌──────────────────────┐
│  Machine A           │◄──────────────────────────►│  Machine B           │
│                      │    HTTP + WebSocket         │                      │
│  ghost-protocol      │                             │  ghost-protocol      │
│  (Tauri 2 desktop)   │                             │  (Tauri 2 desktop)   │
│       │              │                             │       │              │
│       ▼              │                             │       ▼              │
│  ghost-protocol-     │                             │  ghost-protocol-     │
│  daemon (Rust)       │                             │  daemon (Rust)       │
│  ├─ terminal sessions│                             │  ├─ terminal sessions│
│  ├─ chat sessions    │                             │  ├─ chat sessions    │
│  ├─ agent detection  │                             │  ├─ agent detection  │
│  ├─ intelligence     │                             │  ├─ intelligence     │
│  ├─ MCP server       │                             │  ├─ MCP server       │
│  └─ SQLite store     │                             │  └─ SQLite store     │
└──────────────────────┘                             └─────────────────���────┘
```

- `daemon/` — Rust daemon: HTTP + WebSocket + MCP server
- `desktop/` — Tauri 2 desktop app: React + TypeScript + xterm.js
- `cli/` — Ghost CLI: project init, status, agents, chat
- `docs/` — architecture, design specs, plans

## Requirements

- Linux (Arch, Ubuntu, Fedora, etc.)
- tmux 3.0+ (for session persistence)
- Tailscale installed and connected to a mesh
- For development: Node.js + npm, Rust + Cargo

## Install (packaged release)

```bash
# Download latest release
curl -LO https://github.com/VmanHermes/ghost-protocol/releases/latest/download/ghost-protocol-0.2.5-linux-x86_64.tar.gz
tar xzf ghost-protocol-0.2.5-linux-x86_64.tar.gz
cd ghost-protocol-0.2.5

# Install system-wide (installs ghost-protocol, ghost-protocol-daemon, ghost CLI)
sudo ./install.sh
```

## Install (from source)

```bash
git clone git@github.com:VmanHermes/ghost-protocol.git
cd ghost-protocol

# Install desktop dependencies once
cd desktop && npm install && cd ..

# Build daemon
cd daemon && cargo build --release && cd ..

# Build CLI
cd cli && cargo build --release && cd ..

# Build desktop app
cd desktop && npm run tauri build

# Or build the full packaged app flow
bash scripts/package.sh
```

## Run

### Development

```bash
# Start everything (daemon + desktop app)
bash scripts/dev.sh

# Or with a fresh database
bash scripts/dev.sh --reset
```
Build locally with: 
./scripts/package.sh --arch

Press `Ctrl+C` to stop all processes.

To use the ghost CLI while dev is running:
```bash
cd cli && cargo run -- status
cd cli && cargo run -- agents
```

To start components individually:
```bash
# Shared dev DB path
export GHOST_PROTOCOL_DB="$(pwd)/data/dev/ghost_protocol-dev.db"

# Terminal 1: Daemon
cd daemon && cargo run -- serve

# Terminal 2: Desktop app
cd desktop && GHOST_NO_SIDECAR=1 npm run tauri dev
```

If you do not set `GHOST_NO_SIDECAR=1`, the desktop app will try to launch its own daemon sidecar.

### Production

```bash
# Start daemon (binds to Tailscale IP + localhost)
ghost-protocol-daemon --bind-host 100.64.x.x,127.0.0.1

# Launch desktop app
ghost-protocol

# Use CLI
ghost status
ghost agents
ghost init        # in a project directory
ghost chat claude-code
```

## Ghost CLI

```bash
ghost init          # Initialize a project — creates .ghost/config.json, registers with daemon
ghost status        # Mesh overview: machines, online count
ghost agents        # List detected agents on this machine
ghost projects      # List registered projects
ghost chat <agent>  # Start a chat with an agent (e.g., ghost chat claude-code)
ghost help          # Show available commands
```

## Daemon CLI

```bash
ghost-protocol-daemon serve                # Start HTTP server (default)
ghost-protocol-daemon mcp                  # Start MCP resource server over stdio
ghost-protocol-daemon status               # One-line machine summary
ghost-protocol-daemon status --json        # Machine status as JSON
ghost-protocol-daemon sessions             # List terminal sessions
ghost-protocol-daemon hosts                # List known mesh peers
ghost-protocol-daemon info                 # Full machine profile as JSON
```

## Database

Ghost Protocol uses SQLite.

- Raw daemon CLI default: `./data/ghost_protocol.db`
- Desktop dev default: `./data/dev/ghost_protocol-dev.db`
- Packaged desktop default: Tauri app data directory (`ghost_protocol.db`)
- Any mode can be overridden with `--db-path` or `GHOST_PROTOCOL_DB`

**Migrations run automatically** on daemon startup — no manual migration step needed.

**Reset the dev database:**
```bash
# Show the current dev DB path
cd desktop && npm run db:path:dev

# Stop the app/daemon first, then reset it
cd desktop && npm run db:reset:dev

# Restart dev
bash scripts/dev.sh
```

**Reset a manually-run daemon database:**
```bash
# Stop the daemon first, then:
rm -f data/ghost_protocol.db data/ghost_protocol.db-wal data/ghost_protocol.db-shm

# Restart — daemon recreates the database with all tables
ghost-protocol-daemon serve
```

**Custom DB location:**
```bash
ghost-protocol-daemon --db-path /path/to/custom.db serve
```

## MCP Integration

The daemon exposes an MCP server for AI agent integration:

```json
// .mcp.json (for Claude Code)
{
  "mcpServers": {
    "ghost-daemon": {
      "type": "stdio",
      "command": "ghost-protocol-daemon",
      "args": ["mcp"]
    }
  }
}
```

**Resources:** machine/info, machine/status, network/hosts, terminal/sessions, agent/hints, context/briefing, outcomes/recent, agents/available

**Tools:** ghost_recall, ghost_report_outcome, ghost_check_mesh, ghost_list_machines, ghost_list_agents, ghost_spawn_remote_session

## Configuration

| Environment Variable | CLI Flag | Default | Description |
|---|---|---|---|
| `GHOST_PROTOCOL_BIND_HOST` | `--bind-host` | `127.0.0.1` | Bind address (comma-separated for multiple) |
| `GHOST_PROTOCOL_BIND_PORT` | `--bind-port` | `8787` | HTTP port |
| `GHOST_PROTOCOL_ALLOWED_CIDRS` | `--allowed-cidrs` | Tailscale ranges + localhost | IP allowlist |
| `GHOST_PROTOCOL_DB` | `--db-path` | `./data/ghost_protocol.db` (daemon CLI) | SQLite database path override |
| `GHOST_PROTOCOL_LOG_DIR` | `--log-dir` | `./data/logs` | Log directory |

## Useful commands

```bash
# Health check
curl http://127.0.0.1:8787/health

# List agents
curl http://127.0.0.1:8787/api/agents

# List projects
curl http://127.0.0.1:8787/api/projects

# List hosts with permissions
curl http://127.0.0.1:8787/api/permissions

# Report an outcome
curl -X POST http://127.0.0.1:8787/api/outcomes \
  -H 'Content-Type: application/json' \
  -d '{"category":"build","action":"cargo build","status":"success","durationSecs":12.5}'

# Type-check daemon
cd daemon && cargo check

# Type-check frontend
cd desktop && npx tsc --noEmit

# Run daemon tests
cd daemon && cargo test

# Build packaged artifacts locally
bash scripts/package.sh

# Prepare a release (sync version, run checks, build artifacts)
bash scripts/release.sh 0.2.5
```

## Docs

- [Vision](docs/vision.md) — why this exists and where it's going
- [Project plan](docs/project-plan.md) — roadmap and current status
- [Runbook](docs/runbook.md) — operational reference
