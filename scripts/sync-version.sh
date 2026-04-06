#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
VERSION_FILE="$ROOT_DIR/VERSION"

usage() {
  echo "Usage: $0 [version]" >&2
  exit 1
}

if [[ $# -gt 1 ]]; then
  usage
fi

if [[ $# -eq 1 ]]; then
  VERSION="$1"
  if [[ -z "$VERSION" ]]; then
    echo "ERROR: version must not be empty" >&2
    exit 1
  fi
  printf '%s\n' "$VERSION" > "$VERSION_FILE"
else
  VERSION="$(tr -d '[:space:]' < "$VERSION_FILE")"
fi

if [[ -z "$VERSION" ]]; then
  echo "ERROR: VERSION file is empty" >&2
  exit 1
fi

export VERSION ROOT_DIR

perl -0pi -e 's/^version = ".*?"$/version = "$ENV{VERSION}"/m' \
  "$ROOT_DIR/daemon/Cargo.toml" \
  "$ROOT_DIR/cli/Cargo.toml" \
  "$ROOT_DIR/desktop/src-tauri/Cargo.toml"

node <<'NODE'
const fs = require("fs");
const path = require("path");

const root = process.env.ROOT_DIR;
const version = process.env.VERSION;

const updateJson = (relativePath, mutate) => {
  const file = path.join(root, relativePath);
  const json = JSON.parse(fs.readFileSync(file, "utf8"));
  mutate(json);
  fs.writeFileSync(file, `${JSON.stringify(json, null, 2)}\n`);
};

updateJson("desktop/package.json", (json) => {
  json.version = version;
});

updateJson("desktop/src-tauri/tauri.conf.json", (json) => {
  json.version = version;
});

updateJson("desktop/package-lock.json", (json) => {
  json.version = version;
  if (json.packages && json.packages[""]) {
    json.packages[""].version = version;
  }
});
NODE

perl -0pi -e 's/ghost-protocol-[0-9.]+-linux-x86_64\.tar\.gz/ghost-protocol-$ENV{VERSION}-linux-x86_64.tar.gz/g; s/cd ghost-protocol-[0-9.]+/cd ghost-protocol-$ENV{VERSION}/g' \
  "$ROOT_DIR/README.md"

perl -0pi -e 's/## Current status: v[0-9.]+/## Current status: v$ENV{VERSION}/' \
  "$ROOT_DIR/docs/project-plan.md"

echo "Synced project version to $VERSION"
