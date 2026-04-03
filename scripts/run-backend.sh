#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [ ! -d backend/.venv ]; then
  python3 -m venv backend/.venv
  source backend/.venv/bin/activate
  pip install -e backend
else
  source backend/.venv/bin/activate
fi
python -m ghost_protocol_daemon
