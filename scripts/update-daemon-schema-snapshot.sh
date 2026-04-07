#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DAEMON_DIR="$ROOT_DIR/daemon"
OUTPUT_PATH="${1:-$DAEMON_DIR/schema/latest.sql}"

mkdir -p "$(dirname "$OUTPUT_PATH")"

tmp_db="$(mktemp /tmp/ghost-protocol-schema-XXXXXX.db)"
cleanup() {
  rm -f "$tmp_db"
}
trap cleanup EXIT

for migration in "$DAEMON_DIR"/migrations/*.sql; do
  sqlite3 "$tmp_db" < "$migration"
done

sqlite3 "$tmp_db" ".schema" \
  | awk '!/^CREATE TABLE sqlite_sequence/' \
  > "$OUTPUT_PATH"
