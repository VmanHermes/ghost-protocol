#!/usr/bin/env bash
set -euo pipefail

# Build and package Ghost Protocol for Arch Linux
# Output: dist/ghost-protocol-0.1.0-linux-x86_64.tar.gz
#
# On the target machine, extract and run: ./install.sh

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
VERSION="0.2.1"
DIST_DIR="$ROOT_DIR/dist/ghost-protocol-$VERSION"

echo "==> Building daemon..."
cd "$ROOT_DIR/daemon"
cargo build --release 2>&1 | tail -10

echo "==> Building CLI..."
cd "$ROOT_DIR/cli"
cargo build --release 2>&1 | tail -10

echo "==> Building app (frontend + Rust)..."
cd "$ROOT_DIR/desktop"
npx tauri build --bundles deb 2>&1 | tail -20

echo "==> Packaging..."
rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR"

# Tauri app binary
cp "$ROOT_DIR/desktop/src-tauri/target/release/ghost_protocol" "$DIST_DIR/ghost-protocol"

# Daemon binary
cp "$ROOT_DIR/daemon/target/release/ghost-protocol-daemon" "$DIST_DIR/ghost-protocol-daemon"

# CLI binary
cp "$ROOT_DIR/cli/target/release/ghost" "$DIST_DIR/ghost"

# Icon
cp "$ROOT_DIR/desktop/src-tauri/icons/icon.png" "$DIST_DIR/ghost-protocol.png"

# Desktop entry
cat > "$DIST_DIR/ghost-protocol.desktop" << 'DESKTOP'
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
cat > "$DIST_DIR/install.sh" << 'INSTALL'
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

echo "==> Done! Launch from app menu or run: ghost-protocol"
INSTALL
chmod +x "$DIST_DIR/install.sh"

# Uninstall script
cat > "$DIST_DIR/uninstall.sh" << 'UNINSTALL'
#!/usr/bin/env bash
set -euo pipefail
echo "==> Removing Ghost Protocol..."
sudo rm -f /usr/local/bin/ghost-protocol
sudo rm -f /usr/local/bin/ghost-protocol-daemon
sudo rm -f /usr/local/bin/ghost
sudo rm -f /usr/local/share/icons/ghost-protocol.png
sudo rm -f /usr/share/applications/ghost-protocol.desktop
echo "==> Done."
UNINSTALL
chmod +x "$DIST_DIR/uninstall.sh"

# Tarball
cd "$ROOT_DIR/dist"
tar czf "ghost-protocol-$VERSION-linux-x86_64.tar.gz" "ghost-protocol-$VERSION"

SIZE=$(du -h "ghost-protocol-$VERSION-linux-x86_64.tar.gz" | cut -f1)
echo ""
echo "==> Package ready: dist/ghost-protocol-$VERSION-linux-x86_64.tar.gz ($SIZE)"
echo ""
echo "    To install on another Arch machine:"
echo "      scp dist/ghost-protocol-$VERSION-linux-x86_64.tar.gz user@host:~/"
echo "      ssh user@host 'tar xzf ghost-protocol-$VERSION-linux-x86_64.tar.gz && cd ghost-protocol-$VERSION && ./install.sh'"
