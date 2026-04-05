# Cross-Platform Packaging Design

**Date**: 2026-04-05
**Status**: Draft

## Summary

Package Ghost Protocol for desktop (Linux, Mac, Windows) and mobile (iOS, Android) using Tauri 2's built-in bundler with sidecar binaries for desktop, and a PWA served from the daemon for mobile.

## Goals

- Single desktop install includes daemon, desktop app, and CLI
- Desktop packages for: `.deb` + `.AppImage` (Linux), AUR/PKGBUILD (Arch), `.dmg` (Mac), `.msi`/NSIS (Windows)
- Mobile access via PWA — same React frontend adapted for small screens, served by the daemon
- One build script to produce packages for the current platform
- Local-first — no CI/CD automation in this phase

## Non-Goals

- Native mobile apps (Tauri mobile, React Native, etc.) — future upgrade path
- CI/CD pipelines or automated releases
- Cross-compilation from a single machine (build on each target platform)
- App store distribution

## Architecture

### Desktop Packaging

Tauri 2's bundler produces platform-specific installers. The daemon and CLI are bundled as **sidecars** via `externalBin` in `tauri.conf.json`.

#### Sidecar Binary Naming

Tauri expects target-triple suffixed binaries in `desktop/src-tauri/binaries/`:

```
binaries/ghost-protocol-daemon-x86_64-unknown-linux-gnu
binaries/ghost-protocol-daemon-x86_64-apple-darwin
binaries/ghost-protocol-daemon-x86_64-pc-windows-msvc.exe
binaries/ghost-x86_64-unknown-linux-gnu
binaries/ghost-x86_64-apple-darwin
binaries/ghost-x86_64-pc-windows-msvc.exe
```

Only the current platform's binaries need to be present at build time.

#### Platform Outputs

| Platform | Format | Notes |
|----------|--------|-------|
| Linux (Debian/Ubuntu) | `.deb` | Tauri bundler native |
| Linux (universal) | `.AppImage` | Tauri bundler native, works on Arch too |
| Arch Linux | PKGBUILD | Builds from source, extracts deb output into package |
| macOS | `.dmg` with `.app` | Tauri bundler native |
| Windows | `.msi` or NSIS | Tauri bundler native |

#### Arch Linux PKGBUILD

A PKGBUILD that:
1. Clones the repo (or uses local source)
2. Installs npm dependencies
3. Runs `cargo tauri build -b deb`
4. Extracts the deb contents into the package directory

This can live in `packaging/arch/PKGBUILD` in the repo.

#### Desktop App Daemon Lifecycle

The desktop app auto-starts the daemon sidecar on launch and stops it on exit. This is handled in the Tauri Rust backend using the `tauri-plugin-shell` sidecar API.

### Mobile Strategy (PWA)

The daemon already exposes HTTP + WebSocket APIs. The PWA leverages this directly.

#### How It Works

1. The daemon serves the PWA frontend as static files from a `/web` route
2. Users open `http://<daemon-host>:port` from their phone browser
3. The daemon is reachable via LAN or Tailscale (already in use)
4. PWA manifest enables "Add to Home Screen" for an app-like experience

#### PWA Capabilities

**Can do:**
- View machine status and terminal output (read-only or interactive via WebSocket)
- Chat with agents
- Approve/deny permission requests
- View projects and sessions

**Cannot do:**
- Spawn local PTY sessions (remote viewer only)
- Work without a daemon running somewhere on the network

#### Frontend Adaptation

The existing `api.ts` already uses HTTP/WebSocket directly, making it largely PWA-compatible. The key changes:

1. **Abstract Tauri-specific APIs** — gate `@tauri-apps/api` usage behind an `isTauri()` check so the same components work in both desktop and browser contexts
2. **Shared API client** — extract a `lib/api-client.ts` that works in both environments
3. **Responsive CSS** — adapt the existing layout for phone-sized screens
4. **PWA assets** — manifest.json, service worker for offline shell caching

#### Two Build Targets

Same React source, two Vite configs:
- `vite.config.ts` — existing Tauri desktop build
- `vite.config.pwa.ts` — PWA build that excludes Tauri deps, adds PWA plugin (vite-plugin-pwa)

### Daemon Changes

- Add a static file serving route for the PWA build output
- The route is only active when a `web/` directory exists alongside the daemon binary (Tauri bundles it as a resource; the daemon discovers it relative to its own path)
- No other daemon API changes needed — the existing HTTP + WebSocket API is the mobile backend

## Directory Structure Changes

```
desktop/
├── src-tauri/
│   ├── binaries/              # NEW — sidecar binaries (gitignored)
│   └── tauri.conf.json        # UPDATED — externalBin + bundle config
├── src/
│   ├── lib/
│   │   └── api-client.ts      # NEW — shared HTTP/WS client
│   └── ...
├── pwa/                       # NEW — PWA-specific assets
│   ├── manifest.json
│   └── sw.ts                  # Service worker
├── vite.config.ts             # UPDATED — conditional PWA plugin
└── vite.config.pwa.ts         # NEW — PWA-specific vite config

packaging/                     # NEW — platform-specific packaging files
└── arch/
    └── PKGBUILD

scripts/
└── package.sh                 # NEW — unified build script (replaces package-linux.sh)
```

## Build Script (`scripts/package.sh`)

### Usage

```bash
# Build desktop package for current platform
./scripts/package.sh

# Build just the PWA
./scripts/package.sh --pwa-only

# Build with Arch PKGBUILD
./scripts/package.sh --arch
```

### Steps

1. Detect current platform and architecture
2. Build daemon + CLI: `cargo build --release -p ghost-protocol-daemon -p ghost-cli`
3. Copy binaries to `desktop/src-tauri/binaries/` with target-triple suffixes
4. Build PWA: `cd desktop && npx vite build --config vite.config.pwa.ts --outDir dist-pwa`
5. Copy PWA output to `desktop/src-tauri/resources/web/` — Tauri bundles this as a resource, and the daemon serves it from its adjacent `web/` directory at runtime
6. Run `cargo tauri build` to produce the desktop package
7. (If `--arch`) Generate/run PKGBUILD

### Output

Built packages land in `desktop/src-tauri/target/release/bundle/`:
- `deb/ghost-protocol_<version>_amd64.deb`
- `appimage/ghost-protocol_<version>_amd64.AppImage`
- `dmg/ghost-protocol_<version>_x64.dmg` (on Mac)
- `msi/ghost-protocol_<version>_x64_en-US.msi` (on Windows)

## Configuration Changes

### `tauri.conf.json` Updates

```json
{
  "bundle": {
    "active": true,
    "targets": "all",
    "externalBin": [
      "binaries/ghost-protocol-daemon",
      "binaries/ghost"
    ],
    "icon": ["icons/icon.png"],
    "linux": {
      "deb": {},
      "appimage": {}
    },
    "macOS": {
      "dmg": {}
    },
    "windows": {
      "msi": {},
      "nsis": null
    }
  }
}
```

## Testing Strategy

- **Desktop**: Build on each platform, verify the installer works, daemon starts/stops with the app, CLI is accessible from terminal
- **PWA**: Build PWA, start daemon manually, open in phone browser, verify core flows work (machine list, terminal viewing, chat, approvals)
- **Arch**: Test PKGBUILD in a clean Arch container/VM

## Future Upgrade Path

When Tauri 2 mobile stabilizes, the PWA can be replaced with native Tauri mobile builds. The frontend abstraction (`isTauri()` gating) makes this transition straightforward — swap the PWA build target for Tauri mobile targets, and the same React components work natively.
