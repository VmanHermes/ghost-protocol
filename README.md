# Ghost Protocol

![Platform](https://img.shields.io/badge/platform-Linux-blue)
![Desktop](https://img.shields.io/badge/desktop-Tauri%202-7c3aed)
![Frontend](https://img.shields.io/badge/frontend-React%20%2B%20Vite-61dafb)
![Backend](https://img.shields.io/badge/backend-Python-3776ab)

Ghost Protocol is the new primary interface for Hermes, using a headless Linux daemon plus a Tauri 2 desktop client.

Phase 1 goals implemented here:
- inspect and preserve the existing Hermes runtime instead of rebuilding orchestration
- add an explicit persistent event log with a stable event envelope
- expose HTTP + WebSocket transport for conversations, runs, and live events
- scaffold a Tauri 2 + React + Vite app under the Ghost Protocol name against the same backend API

Workspace layout:
- `backend/` — Python daemon, event store, projections, HTTP and WebSocket transport
- `desktop/` — Tauri 2 Ghost Protocol client (React + TypeScript)
- `docs/` — architecture notes, implementation notes, and phase plan

Current architecture decision:
- keep Hermes runtime headless and outside the Ghost Protocol shell
- use the existing `AIAgent` runtime from `/home/vmandesk/.hermes/hermes-agent`
- add a thin adapter layer that emits persistent events and exposes explicit APIs
- use WebSocket for primary realtime delivery and HTTP for request-response APIs

## Requirements
- Linux desktop
- Python 3.11+
- Node.js + npm
- Rust + Cargo
- a working Hermes runtime at `/home/vmandesk/.hermes/hermes-agent`

## Install
```bash
git clone git@github.com:VmanHermes/ghost-protocol.git
cd ghost-protocol
npm --prefix desktop install
```

## Run
Preferred local workflow:
```bash
systemctl --user enable --now ghost-protocol-backend.service
./scripts/open-app.sh
```

Manual start:
```bash
cd backend
python -m venv .venv
source .venv/bin/activate
pip install -e .
python -m ghost_protocol_daemon.server

cd ../desktop
npm install
npm run tauri dev
```

## Useful commands
```bash
systemctl --user status ghost-protocol-backend.service --no-pager
journalctl --user -u ghost-protocol-backend.service -f
python3 -m py_compile backend/src/ghost_protocol_daemon/*.py
cd desktop && npm run build
cd desktop/src-tauri && cargo check
```

## Notes
Telegram is not replaced in Hermes itself. This project builds the new primary interface and daemon event pipeline while preserving the current runtime path for later Telegram adapter unification.

See also:
- `docs/runbook.md`
- `docs/architecture.md`
- `docs/implementation-notes.md`
