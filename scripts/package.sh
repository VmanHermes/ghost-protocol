#!/usr/bin/env bash
set -euo pipefail

# Unified cross-platform build script for Ghost Protocol
# Replaces package-linux.sh with support for Linux, macOS, and Windows
#
# Usage:
#   ./scripts/package.sh              # Build everything (PWA + Rust + sidecars + Tauri)
#   ./scripts/package.sh --pwa-only   # Build only the PWA
#   ./scripts/package.sh --arch       # Build everything + Arch Linux tarball

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
VERSION="$(bash "$ROOT_DIR/scripts/version.sh")"

bash "$ROOT_DIR/scripts/sync-version.sh" >/dev/null

# ---------------------------------------------------------------------------
# Platform / architecture detection
# ---------------------------------------------------------------------------

detect_platform() {
  case "$(uname -s)" in
    Linux*)  PLATFORM="linux" ;;
    Darwin*) PLATFORM="macos" ;;
    CYGWIN*|MINGW*|MSYS*) PLATFORM="windows" ;;
    *) echo "ERROR: Unsupported platform: $(uname -s)"; exit 1 ;;
  esac
}

detect_arch() {
  case "$(uname -m)" in
    x86_64|amd64)  ARCH="x86_64" ;;
    aarch64|arm64)  ARCH="aarch64" ;;
    *) echo "ERROR: Unsupported architecture: $(uname -m)"; exit 1 ;;
  esac
}

target_triple() {
  case "$PLATFORM" in
    linux)   echo "${ARCH}-unknown-linux-gnu" ;;
    macos)   echo "${ARCH}-apple-darwin" ;;
    windows) echo "${ARCH}-pc-windows-msvc" ;;
  esac
}

bin_ext() {
  if [[ "$PLATFORM" == "windows" ]]; then
    echo ".exe"
  else
    echo ""
  fi
}

# ---------------------------------------------------------------------------
# Build functions
# ---------------------------------------------------------------------------

build_pwa() {
  echo "==> Building PWA..."
  cd "$ROOT_DIR/desktop"
  npm run build:pwa
}

build_rust_binaries() {
  local ext
  ext="$(bin_ext)"

  echo "==> Building daemon..."
  cd "$ROOT_DIR/daemon"
  cargo build --release 2>&1 | tail -10

  echo "==> Building CLI..."
  cd "$ROOT_DIR/cli"
  cargo build --release 2>&1 | tail -10
}

prepare_sidecars() {
  local triple ext
  triple="$(target_triple)"
  ext="$(bin_ext)"

  echo "==> Preparing sidecars for ${triple}..."
  local bin_dir="$ROOT_DIR/desktop/src-tauri/binaries"
  mkdir -p "$bin_dir"

  # Install via install(1) which creates a new inode, avoiding "Text file busy"
  # when an old sidecar binary is still running.
  install -m 755 "$ROOT_DIR/daemon/target/release/ghost-protocol-daemon${ext}" \
     "$bin_dir/ghost-protocol-daemon-${triple}${ext}"

  install -m 755 "$ROOT_DIR/cli/target/release/ghost${ext}" \
     "$bin_dir/ghost-${triple}${ext}"

  echo "    Sidecars placed in desktop/src-tauri/binaries/"
}

prepare_pwa_resource() {
  echo "==> Copying PWA build to Tauri resources..."
  local dest="$ROOT_DIR/desktop/src-tauri/resources/web"
  rm -rf "$dest"
  mkdir -p "$dest"
  cp -r "$ROOT_DIR/desktop/dist-pwa/"* "$dest/"
  echo "    PWA resources placed in desktop/src-tauri/resources/web/"
}

build_tauri() {
  echo "==> Building Tauri app..."
  cd "$ROOT_DIR/desktop"
  if [[ -n "${GHOST_TAURI_BUNDLES:-}" ]]; then
    echo "    Bundles: ${GHOST_TAURI_BUNDLES}"
    npx tauri build --bundles "${GHOST_TAURI_BUNDLES}"
  else
    # The Arch tarball is assembled separately below, so avoid requiring
    # optional platform bundlers like linuxdeploy for the default local flow.
    npx tauri build --no-bundle
  fi
}

build_arch_tarball() {
  local triple ext
  triple="$(target_triple)"
  ext="$(bin_ext)"

  echo "==> Packaging Arch Linux tarball..."
  local dist_dir="$ROOT_DIR/dist/ghost-protocol-$VERSION"
  rm -rf "$dist_dir"
  mkdir -p "$dist_dir"

  # Tauri app binary
  cp "$ROOT_DIR/desktop/src-tauri/target/release/ghost_protocol${ext}" \
     "$dist_dir/ghost-protocol${ext}"

  # Daemon binary
  cp "$ROOT_DIR/daemon/target/release/ghost-protocol-daemon${ext}" \
     "$dist_dir/ghost-protocol-daemon${ext}"

  # CLI binary
  cp "$ROOT_DIR/cli/target/release/ghost${ext}" \
     "$dist_dir/ghost${ext}"

  # Icon
  cp "$ROOT_DIR/desktop/src-tauri/icons/icon.png" "$dist_dir/ghost-protocol.png"

  # PWA web files
  if [[ -d "$ROOT_DIR/desktop/dist-pwa" ]]; then
    mkdir -p "$dist_dir/web"
    cp -r "$ROOT_DIR/desktop/dist-pwa/"* "$dist_dir/web/"
  fi

  # Desktop entry
  cat > "$dist_dir/ghost-protocol.desktop" << 'DESKTOP'
[Desktop Entry]
Name=Ghost Protocol
Comment=Developer Console
Exec=env WEBKIT_DISABLE_COMPOSITING_MODE=1 GDK_BACKEND=x11 /usr/local/bin/ghost-protocol
Icon=/usr/local/share/icons/ghost-protocol.png
Terminal=false
Type=Application
Categories=Development;
DESKTOP

  # Install script
  cat > "$dist_dir/install.sh" << 'INSTALL'
#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

echo "==> Checking dependencies..."
MISSING=()
pacman -Q webkit2gtk-4.1 &>/dev/null || MISSING+=(webkit2gtk-4.1)
pacman -Q gtk3 &>/dev/null || MISSING+=(gtk3)

if [ ${#MISSING[@]} -gt 0 ]; then
  echo "    Installing: ${MISSING[*]}"
  sudo pacman -S --needed --noconfirm "${MISSING[@]}"
else
  echo "    All dependencies present."
fi

echo "==> Installing Ghost Protocol..."
sudo install -Dm755 "$SCRIPT_DIR/ghost-protocol" /usr/local/bin/ghost-protocol
sudo install -Dm755 "$SCRIPT_DIR/ghost-protocol-daemon" /usr/local/bin/ghost-protocol-daemon
sudo install -Dm755 "$SCRIPT_DIR/ghost" /usr/local/bin/ghost
sudo install -Dm644 "$SCRIPT_DIR/ghost-protocol.png" /usr/local/share/icons/ghost-protocol.png
sudo install -Dm644 "$SCRIPT_DIR/ghost-protocol.desktop" /usr/share/applications/ghost-protocol.desktop

# Install PWA web files
if [ -d "$SCRIPT_DIR/web" ]; then
  echo "==> Installing PWA web files..."
  sudo mkdir -p /usr/local/share/ghost-protocol/web
  sudo cp -r "$SCRIPT_DIR/web/"* /usr/local/share/ghost-protocol/web/
fi

echo "==> Done! Launch from app menu or run: ghost-protocol"
INSTALL
  chmod +x "$dist_dir/install.sh"

  # Uninstall script
  cat > "$dist_dir/uninstall.sh" << 'UNINSTALL'
#!/usr/bin/env bash
set -euo pipefail
echo "==> Removing Ghost Protocol..."
sudo rm -f /usr/local/bin/ghost-protocol
sudo rm -f /usr/local/bin/ghost-protocol-daemon
sudo rm -f /usr/local/bin/ghost
sudo rm -f /usr/local/share/icons/ghost-protocol.png
sudo rm -f /usr/share/applications/ghost-protocol.desktop
sudo rm -rf /usr/local/share/ghost-protocol
echo "==> Done."
UNINSTALL
  chmod +x "$dist_dir/uninstall.sh"

  # Create tarball
  cd "$ROOT_DIR/dist"
  local tarball="ghost-protocol-${VERSION}-linux-${ARCH}.tar.gz"
  tar czf "$tarball" "ghost-protocol-$VERSION"

  local size
  size=$(du -h "$tarball" | cut -f1)
  echo ""
  echo "==> Arch tarball ready: dist/${tarball} (${size})"
  echo ""
  echo "    To install on another Arch machine:"
  echo "      tar xzf ${tarball} && cd ghost-protocol-${VERSION} && ./install.sh"
}

# ---------------------------------------------------------------------------
# Help
# ---------------------------------------------------------------------------

show_help() {
  cat << EOF
Ghost Protocol Build Script v${VERSION}

Usage: $(basename "$0") [OPTIONS]

Options:
  --pwa-only    Build only the PWA (no Rust, no Tauri)
  --arch        Build everything and create an Arch Linux tarball
  --help        Show this help message

Env:
  GHOST_TAURI_BUNDLES  Override Tauri bundles, e.g. deb or deb,rpm

Modes:
  Default       Build PWA, Rust binaries, sidecars, and Tauri app
  --pwa-only    Only build the PWA frontend
  --arch        Full build + Arch Linux distribution tarball

Detected environment:
  Platform:     $(detect_platform && echo "$PLATFORM")
  Architecture: $(detect_arch && echo "$ARCH")
  Target:       $(detect_platform && detect_arch && target_triple)
EOF
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

main() {
  detect_platform
  detect_arch

  local mode="default"

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --pwa-only) mode="pwa-only"; shift ;;
      --arch)     mode="arch"; shift ;;
      --help|-h)  show_help; exit 0 ;;
      *) echo "Unknown option: $1"; show_help; exit 1 ;;
    esac
  done

  local triple
  triple="$(target_triple)"
  echo "==> Ghost Protocol build (${VERSION})"
  echo "    Platform: ${PLATFORM} | Arch: ${ARCH} | Target: ${triple}"
  echo ""

  case "$mode" in
    pwa-only)
      build_pwa
      ;;
    arch)
      build_pwa
      build_rust_binaries
      prepare_sidecars
      prepare_pwa_resource
      build_tauri
      build_arch_tarball
      ;;
    default)
      build_pwa
      build_rust_binaries
      prepare_sidecars
      prepare_pwa_resource
      build_tauri
      ;;
  esac

  echo ""
  echo "==> Build complete!"
}

main "$@"
