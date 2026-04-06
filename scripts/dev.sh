#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PIDS=()

cleanup() {
    echo ""
    echo "==> Stopping..."
    for pid in "${PIDS[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
    wait 2>/dev/null
    echo "==> Done."
}
trap cleanup EXIT INT TERM

# Reset database if --reset flag passed
if [[ "${1:-}" == "--reset" ]]; then
    echo "==> Resetting database..."
    rm -f "$ROOT_DIR/data/ghost_protocol.db"
    shift
fi

# 0. Build and install ghost CLI to ~/.local/bin
echo "==> Building ghost CLI..."
cd "$ROOT_DIR/cli"
cargo build 2>&1 | tail -5
mkdir -p "$HOME/.local/bin"
ln -sf "$ROOT_DIR/cli/target/debug/ghost" "$HOME/.local/bin/ghost"
ln -sf "$ROOT_DIR/daemon/target/debug/ghost-protocol-daemon" "$HOME/.local/bin/ghost-protocol-daemon"
echo "==> ghost CLI installed to ~/.local/bin/ghost"

# Symlink daemon binary for Tauri sidecar
TAURI_BIN_DIR="$ROOT_DIR/desktop/src-tauri/binaries"
mkdir -p "$TAURI_BIN_DIR"
TARGET_TRIPLE="$(rustc -vV | grep host | cut -d' ' -f2)"
ln -sf "$ROOT_DIR/daemon/target/debug/ghost-protocol-daemon" "$TAURI_BIN_DIR/ghost-protocol-daemon-$TARGET_TRIPLE"
ln -sf "$ROOT_DIR/cli/target/debug/ghost" "$TAURI_BIN_DIR/ghost-$TARGET_TRIPLE"

# Ensure ~/.local/bin is in PATH for this session
export PATH="$HOME/.local/bin:$PATH"

# 1. Start daemon
echo "==> Starting daemon..."
cd "$ROOT_DIR/daemon"
cargo run -- serve &
PIDS+=($!)

# Wait for daemon to be ready
echo "==> Waiting for daemon..."
for i in $(seq 1 15); do
    if curl -s http://127.0.0.1:8787/health > /dev/null 2>&1; then
        echo "==> Daemon ready."
        break
    fi
    if [ "$i" -eq 15 ]; then
        echo "==> Warning: daemon not responding after 15s, starting desktop anyway."
    fi
    sleep 1
done

# 2. Start desktop app
echo "==> Starting desktop app..."
cd "$ROOT_DIR/desktop"
npm run tauri dev &
PIDS+=($!)

echo ""
echo "==> Ghost Protocol dev environment running."
echo "    Daemon:  http://127.0.0.1:8787"
echo "    Desktop: Tauri dev window"
echo "    CLI:     cd cli && cargo run -- <command>"
echo ""
echo "    Press Ctrl+C to stop all."
echo ""

# Wait for any process to exit
wait -n "${PIDS[@]}" 2>/dev/null || true
