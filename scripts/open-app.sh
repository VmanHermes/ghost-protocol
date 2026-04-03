#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
mkdir -p logs data

if [ -f .env ]; then
  set -a
  source .env
  set +a
fi

BIND_HOST="${GHOST_PROTOCOL_BIND_HOST:-${HERMES_DESKTOP_BIND_HOST:-127.0.0.1}}"
BIND_PORT="${GHOST_PROTOCOL_BIND_PORT:-${HERMES_DESKTOP_BIND_PORT:-8787}}"
BACKEND_URL="http://${BIND_HOST}:${BIND_PORT}"
APP_LOG="$ROOT/logs/ghost-protocol-app.log"
SERVICE_NAME="ghost-protocol-backend.service"

backend_healthy() {
  curl -fsS "$BACKEND_URL/health" >/dev/null 2>&1
}

desktop_dev_running() {
  curl -fsS http://localhost:1420 >/dev/null 2>&1 && pgrep -af 'target/debug/ghost_protocol' >/dev/null 2>&1
}

systemctl --user start "$SERVICE_NAME"
for _ in $(seq 1 40); do
  if backend_healthy; then
    break
  fi
  sleep 0.5
done

if ! backend_healthy; then
  echo "Backend did not become healthy at $BACKEND_URL" >&2
  exit 1
fi

cd "$ROOT/desktop"
if [ ! -d node_modules ] || [ ! -f node_modules/@tauri-apps/cli/cli.linux-x64-gnu.node ]; then
  npm install >>"$APP_LOG" 2>&1
fi

if desktop_dev_running; then
  exit 0
fi

if curl -fsS http://localhost:1420 >/dev/null 2>&1; then
  echo "Port 1420 is already in use by another process." >&2
  exit 1
fi

nohup npm run tauri dev >>"$APP_LOG" 2>&1 < /dev/null &
