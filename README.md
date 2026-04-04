# Ghost Protocol

![Platform](https://img.shields.io/badge/platform-Linux-blue)
![Desktop](https://img.shields.io/badge/desktop-Tauri%202-7c3aed)
![Frontend](https://img.shields.io/badge/frontend-React%20%2B%20Vite-61dafb)
![Backend](https://img.shields.io/badge/backend-Rust-dea584)

Ghost Protocol is a multi-machine terminal and AI agent interface built on Tailscale. It lets you host a connection on one Linux machine and join from another, sharing terminal sessions over a private mesh network. The long-term goal is a unified control plane for the Hermes AI agent runtime across all your devices.

## What it does

- **Host a connection** — start the daemon on any machine, bound to its Tailscale IP. Other devices on your mesh can connect.
- **Join a connection** — add a remote host by Tailscale IP. See its terminal sessions and create new ones.
- **Shared terminals** — terminal sessions persist via the daemon. Multiple clients can view and interact with the same session.
- **Setup checklist** — guided onboarding that detects tmux, Tailscale, mesh connectivity, and the daemon, with one-click install commands.
- **Log viewer** — unified client and server logs for debugging connection lifecycle.

## Architecture

```
┌─────────────────────┐        Tailscale mesh        ┌─────────────────────┐
│  Machine A (host)   │◄────────────────────────────►│  Machine B (join)   │
│                     │     HTTP + WebSocket          │                     │
│  ghost-protocol     │                               │  ghost-protocol     │
│  (Tauri 2 app)      │                               │  (Tauri 2 app)      │
│       │              │                               │                     │
│       ▼              │                               │                     │
│  ghost_protocol_     │                               │                     │
│  daemon (Rust)       │                               │                     │
│       │              │                               │                     │
│       ▼              │                               │                     │
│  terminal sessions   │                               │                     │
│  (PTY / tmux)        │                               │                     │
└─────────────────────┘                               └─────────────────────┘
```

- `daemon/` — Rust daemon with HTTP + WebSocket APIs for terminal sessions
- `desktop/` — Tauri 2 desktop client (React + TypeScript + xterm.js)
- `docs/` — architecture, design specs, and project plan

## Requirements

- Linux (Arch, Ubuntu, Fedora, etc.)
- tmux (required on hosts for session persistence)
- Tailscale installed and connected to a mesh
- For development: Node.js + npm, Rust + Cargo

## Install (packaged release)

Download the latest release from GitHub:

```bash
# Download and extract
tar xzf ghost-protocol-0.1.1-linux-x86_64.tar.gz
cd ghost-protocol-0.1.1

# Install system-wide
sudo ./install.sh
```

The app auto-installs the daemon when you click "Host a Connection".

## Install (from source)

```bash
git clone git@github.com:VmanHermes/ghost-protocol.git
cd ghost-protocol
npm --prefix desktop install
cd daemon && cargo build --release
```

## Run

### Packaged app

Launch from your application menu, or:
```bash
ghost-protocol
```

### Development

```bash
cd desktop
npm run tauri dev
```

The backend daemon can be started separately for development:
```bash
cd daemon
cargo build --release
./target/release/ghost-protocol-daemon --bind-host 127.0.0.1
```

## Usage

1. **First launch** — the setup checklist guides you through installing tmux and Tailscale, and connecting to a Tailscale mesh.
2. **Host a connection** — click the play button in the sidebar. This installs/starts the daemon bound to your Tailscale IP.
3. **Join from another machine** — on a second machine, click "Add Host" and enter the first machine's Tailscale IP (e.g. `http://100.x.x.x:8787`).
4. **Create terminals** — use the `+` button to create local terminals, or the dropdown to create sessions on a connected remote host.
5. **View logs** — click "Logs" in the sidebar to see client and server logs for debugging.

## Useful commands

```bash
# Check daemon status
curl http://127.0.0.1:8787/health

# List Tailscale IP
tailscale ip -4

# Build a release package
bash scripts/package-linux.sh

# Daemon type-check
cd daemon && cargo check

# Frontend type-check
cd desktop && npx tsc --noEmit

# Cargo check
cd desktop/src-tauri && cargo check
```

## Docs

- [Project plan](docs/project-plan.md) — roadmap and current status
- [Architecture](docs/architecture.md) — system design
- [Runbook](docs/runbook.md) — operational reference
