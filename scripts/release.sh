#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"

usage() {
  cat <<'EOF'
Usage: ./scripts/release.sh <version> [--skip-checks] [--skip-package]

Prepares a Ghost Protocol release by:
  1. Syncing the version across manifests/docs
  2. Running focused release checks
  3. Building the Arch tarball release artifact
  4. Printing the remaining git/GitHub release steps

Examples:
  ./scripts/release.sh 0.2.2
  ./scripts/release.sh 0.2.2 --skip-package
EOF
}

if [[ $# -lt 1 ]]; then
  usage
  exit 1
fi

VERSION=""
SKIP_CHECKS=false
SKIP_PACKAGE=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-checks)
      SKIP_CHECKS=true
      shift
      ;;
    --skip-package)
      SKIP_PACKAGE=true
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    -*)
      echo "ERROR: unknown option: $1" >&2
      usage
      exit 1
      ;;
    *)
      if [[ -n "$VERSION" ]]; then
        echo "ERROR: version already provided: $VERSION" >&2
        usage
        exit 1
      fi
      VERSION="$1"
      shift
      ;;
  esac
done

if [[ -z "$VERSION" ]]; then
  echo "ERROR: missing version" >&2
  usage
  exit 1
fi

echo "==> Preparing Ghost Protocol release ${VERSION}"
echo ""

echo "==> Syncing version files..."
bash "$ROOT_DIR/scripts/sync-version.sh" "$VERSION"

if [[ "$SKIP_CHECKS" != true ]]; then
  echo ""
  echo "==> Running daemon tests..."
  (
    cd "$ROOT_DIR/daemon"
    cargo test
  )

  echo ""
  echo "==> Running desktop typecheck..."
  (
    cd "$ROOT_DIR/desktop"
    npm exec tsc --noEmit
  )
fi

if [[ "$SKIP_PACKAGE" != true ]]; then
  echo ""
  echo "==> Building release package..."
  bash "$ROOT_DIR/scripts/package.sh" --arch
fi

TARBALL_PATH="$ROOT_DIR/dist/ghost-protocol-${VERSION}-linux-x86_64.tar.gz"

echo ""
echo "==> Release ${VERSION} is prepared."
echo ""
echo "Next steps:"
echo "  1. Review the diff:"
echo "     git diff --stat"
echo "  2. Commit the release changes:"
echo "     git add VERSION README.md docs/project-plan.md scripts/release.sh scripts/sync-version.sh scripts/version.sh daemon/Cargo.toml cli/Cargo.toml desktop/package.json desktop/package-lock.json desktop/src-tauri/Cargo.toml desktop/src-tauri/tauri.conf.json"
echo "     git commit -m \"chore: release v${VERSION}\""
echo "  3. Push the branch:"
echo "     git push origin main"
echo "  4. Create and push the tag:"
echo "     git tag v${VERSION}"
echo "     git push origin v${VERSION}"
echo "  5. Create the GitHub release and upload:"
if [[ "$SKIP_PACKAGE" != true ]]; then
  echo "     ${TARBALL_PATH}"
else
  echo "     Build artifact not created because --skip-package was used."
fi
