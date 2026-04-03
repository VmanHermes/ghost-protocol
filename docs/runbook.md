# Runbook

## Backend daemon
Recommended local run using the existing Hermes environment:

```bash
cd /home/vmandesk/Work/projects/hermes-desktop-v1
set -a
source .env
set +a
PYTHONPATH=backend/src /home/vmandesk/.hermes/hermes-agent/venv/bin/python -m hermes_desktop_daemon.server
```

## Desktop app
```bash
cd /home/vmandesk/Work/projects/hermes-desktop-v1/desktop
npm install
npm run tauri dev
```

## Validation commands
Backend compile:
```bash
python3 -m py_compile backend/src/hermes_desktop_daemon/*.py
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
- `HERMES_TELEGRAM_ENABLED=1`
- `HERMES_TELEGRAM_BOT_TOKEN=...`
- `HERMES_TELEGRAM_CHAT_ID=...`
