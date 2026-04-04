# Runbook

## Backend daemon
Recommended local run using the existing Hermes environment:

```bash
cd /path/to/ghost-protocol
set -a
source .env
set +a
PYTHONPATH=backend/src ~/.hermes/hermes-agent/venv/bin/python -m ghost_protocol_daemon.server
```

## Backend service
```bash
systemctl --user daemon-reload
systemctl --user enable --now ghost-protocol-backend.service
systemctl --user status ghost-protocol-backend.service --no-pager
```

## Desktop launcher
```bash
cd /path/to/ghost-protocol
./scripts/open-app.sh
```

A desktop entry named `Ghost Protocol` is also installed in `~/.local/share/applications/`.

## Desktop app
```bash
cd /path/to/ghost-protocol/desktop
npm install
npm run tauri dev
```

## Validation commands
Backend compile:
```bash
python3 -m py_compile backend/src/ghost_protocol_daemon/*.py
```

Frontend build:
```bash
cd desktop && npm run build
```

Tauri Rust check:
```bash
cd desktop/src-tauri && cargo check
```

## Useful APIs
- `GET /health`
- `GET /api/system/status`
- `GET /api/conversations`
- `POST /api/conversations`
- `POST /api/conversations/{id}/messages`
- `POST /api/runs`
- `GET /api/runs/{id}`
- `POST /api/runs/{id}/retry`
- `POST /api/runs/{id}/cancel`
- `GET /ws`

## Telegram bridge
Phase 2 adds an outbound Telegram bridge driven from the same daemon event stream.
Set these in `.env` if you want progress + final updates:
- `GHOST_PROTOCOL_TELEGRAM_ENABLED=1`
- `GHOST_PROTOCOL_TELEGRAM_BOT_TOKEN=***`
- `GHOST_PROTOCOL_TELEGRAM_CHAT_ID=...`
