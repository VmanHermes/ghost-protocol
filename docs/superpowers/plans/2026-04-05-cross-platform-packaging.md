# Cross-Platform Packaging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Package Ghost Protocol as a single desktop install (daemon + app + CLI) for Linux/Mac/Windows, with a PWA served from the daemon for mobile access.

**Architecture:** Tauri 2's bundler produces platform installers with daemon and CLI as sidecars via `externalBin`. The React frontend gets a parallel PWA build (via vite-plugin-pwa) that the daemon serves as static files. Tauri-specific API calls are gated behind `isTauri()` so the same components work in both contexts.

**Tech Stack:** Tauri 2, Vite, vite-plugin-pwa, Axum (tower-http serve_dir), React 19

---

### Task 1: Add `tauri-plugin-shell` for Sidecar Support

The desktop app needs `tauri-plugin-shell` to spawn and manage the daemon sidecar binary.

**Files:**
- Modify: `desktop/src-tauri/Cargo.toml`
- Modify: `desktop/src-tauri/tauri.conf.json`
- Modify: `desktop/src-tauri/src/lib.rs`
- Modify: `desktop/package.json`
- Modify: `desktop/src-tauri/capabilities/default.json` (if it exists, otherwise create)

- [ ] **Step 1: Add tauri-plugin-shell to Cargo.toml**

In `desktop/src-tauri/Cargo.toml`, add to `[dependencies]`:

```toml
tauri-plugin-shell = "2"
```

- [ ] **Step 2: Add the npm companion package**

```bash
cd desktop && npm install @tauri-apps/plugin-shell
```

- [ ] **Step 3: Register the plugin in lib.rs**

Read `desktop/src-tauri/src/lib.rs` and add `.plugin(tauri_plugin_shell::init())` to the Tauri builder chain.

- [ ] **Step 4: Update tauri.conf.json with externalBin and bundle config**

Replace the `"bundle"` section in `desktop/src-tauri/tauri.conf.json`:

```json
{
  "bundle": {
    "active": true,
    "targets": "all",
    "externalBin": [
      "binaries/ghost-protocol-daemon",
      "binaries/ghost"
    ],
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ],
    "resources": [
      "resources/web/**/*"
    ],
    "linux": {
      "deb": {},
      "appimage": {}
    },
    "macOS": {
      "dmg": {}
    },
    "windows": {
      "nsis": {}
    }
  }
}
```

- [ ] **Step 5: Add shell permission for sidecar execution**

Check if `desktop/src-tauri/capabilities/default.json` exists. If it does, add `"shell:allow-execute"` and `"shell:allow-kill"` to its permissions array. If not, create it:

```json
{
  "identifier": "default",
  "description": "Default capability for the main window",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "opener:default",
    "shell:allow-execute",
    "shell:allow-kill",
    "shell:default"
  ]
}
```

- [ ] **Step 6: Create the binaries directory and gitignore it**

```bash
mkdir -p desktop/src-tauri/binaries
echo '*' > desktop/src-tauri/binaries/.gitignore
echo '!.gitignore' >> desktop/src-tauri/binaries/.gitignore
```

- [ ] **Step 7: Create the resources/web directory placeholder**

```bash
mkdir -p desktop/src-tauri/resources/web
echo '{}' > desktop/src-tauri/resources/web/.gitkeep
```

- [ ] **Step 8: Verify it compiles**

```bash
cd desktop && npx tauri build --debug 2>&1 | tail -5
```

This will likely fail because the sidecar binaries don't exist yet — that's expected. The Cargo compile of the Tauri Rust code should succeed.

- [ ] **Step 9: Commit**

```bash
git add desktop/src-tauri/Cargo.toml desktop/src-tauri/tauri.conf.json desktop/src-tauri/src/lib.rs desktop/package.json desktop/package-lock.json desktop/src-tauri/capabilities/ desktop/src-tauri/binaries/.gitignore desktop/src-tauri/resources/
git commit -m "feat(desktop): add tauri-plugin-shell and sidecar config for daemon+CLI bundling"
```

---

### Task 2: Daemon Sidecar Lifecycle in Tauri App

The desktop app should auto-start the daemon when it launches and kill it on exit.

**Files:**
- Modify: `desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: Read the current lib.rs**

Read `desktop/src-tauri/src/lib.rs` to understand the current Tauri setup and command registrations.

- [ ] **Step 2: Add daemon sidecar spawn on setup**

Add a `setup` hook to the Tauri builder that spawns the daemon sidecar. The sidecar name must match the `externalBin` entry without the target-triple suffix — so `"binaries/ghost-protocol-daemon"` means we call `app.shell().sidecar("binaries/ghost-protocol-daemon")`.

```rust
use tauri::Manager;
use tauri_plugin_shell::ShellExt;
use std::sync::Mutex;

// Add to the builder chain, before .run():
.setup(|app| {
    // Spawn the daemon sidecar
    let sidecar = app.shell().sidecar("binaries/ghost-protocol-daemon")
        .map_err(|e| format!("failed to create daemon sidecar: {e}"))?;
    let (mut rx, child) = sidecar.spawn()
        .map_err(|e| format!("failed to spawn daemon sidecar: {e}"))?;

    // Store the child so we can kill it on exit
    app.manage(Mutex::new(Some(child)));

    // Log daemon output in background
    tauri::async_runtime::spawn(async move {
        use tauri_plugin_shell::process::CommandEvent;
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stdout(line) => {
                    let line = String::from_utf8_lossy(&line);
                    tracing::info!(target: "daemon", "{}", line);
                }
                CommandEvent::Stderr(line) => {
                    let line = String::from_utf8_lossy(&line);
                    tracing::warn!(target: "daemon", "{}", line);
                }
                CommandEvent::Terminated(status) => {
                    tracing::info!(target: "daemon", "daemon exited: {:?}", status);
                    break;
                }
                _ => {}
            }
        }
    });

    Ok(())
})
```

- [ ] **Step 3: Add on_exit hook to kill the daemon**

Add an `on_exit` handler after `.setup()` that kills the daemon child process:

```rust
use tauri_plugin_shell::process::CommandChild;

// Add to the builder chain:
.on_exit(|app, _exit_code| {
    if let Some(state) = app.try_state::<Mutex<Option<CommandChild>>>() {
        if let Ok(mut guard) = state.lock() {
            if let Some(child) = guard.take() {
                let _ = child.kill();
            }
        }
    }
})
```

- [ ] **Step 4: Verify it compiles**

```bash
cd desktop/src-tauri && cargo check
```

Expected: compiles without errors (sidecar binary doesn't need to exist for `cargo check`).

- [ ] **Step 5: Commit**

```bash
git add desktop/src-tauri/src/lib.rs
git commit -m "feat(desktop): auto-start daemon sidecar on launch, kill on exit"
```

---

### Task 3: Platform Detection Utility (`isTauri`)

Gate Tauri-specific APIs so the same React code works in both Tauri desktop and browser (PWA) contexts.

**Files:**
- Create: `desktop/src/lib/platform.ts`

- [ ] **Step 1: Create the platform detection module**

```typescript
// desktop/src/lib/platform.ts

/**
 * Returns true when running inside the Tauri desktop app.
 * In a browser/PWA context this returns false.
 */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
```

- [ ] **Step 2: Verify the file exists and is importable**

```bash
cd desktop && npx tsc --noEmit src/lib/platform.ts
```

Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add desktop/src/lib/platform.ts
git commit -m "feat(desktop): add isTauri() platform detection utility"
```

---

### Task 4: Gate Tauri-Specific Imports in App.tsx

`App.tsx` imports `invoke` from `@tauri-apps/api/core` and uses it for PTY operations. In PWA mode, local PTY is unavailable — these features should be hidden.

**Files:**
- Modify: `desktop/src/App.tsx`

- [ ] **Step 1: Replace static Tauri import with dynamic gating**

In `desktop/src/App.tsx`, replace:

```typescript
import { invoke } from "@tauri-apps/api/core";
```

with:

```typescript
import { isTauri } from "./lib/platform";
```

- [ ] **Step 2: Update handleCreateLocalSession to check isTauri**

Replace the `handleCreateLocalSession` callback:

```typescript
const handleCreateLocalSession = useCallback(async () => {
  if (!isTauri()) return;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const cols = 120;
    const rows = 30;
    const sessionId = await invoke<string>("pty_spawn", { cols, rows, workdir: null });
    const session: LocalTerminalSession = {
      id: sessionId,
      status: "running",
      createdAt: new Date().toISOString(),
    };
    setLocalSessions((prev) => [...prev, session]);
    setActiveTerminalSessionId(sessionId);
    setMainView("terminal");
  } catch (error) {
    const msg = error instanceof Error ? error.message : String(error);
    appLog.error("app", `Failed to spawn local terminal: ${msg}`);
    setActionError(`Failed to spawn local terminal: ${msg}`);
  }
}, []);
```

- [ ] **Step 3: Update handleKillLocalSession to dynamically import invoke**

Replace the `handleKillLocalSession` callback:

```typescript
const handleKillLocalSession = useCallback(async (sessionId: string) => {
  if (!isTauri()) return;
  const existing = localSessions.find((s) => s.id === sessionId);
  if (!existing || existing.status !== "running") return;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("pty_kill", { sessionId });
    setLocalSessions((prev) =>
      prev.map((s) => (s.id === sessionId ? { ...s, status: "terminated" as const } : s)),
    );
    if (activeTerminalSessionId === sessionId) {
      const remaining = localSessions.filter((s) => s.id !== sessionId && s.status === "running");
      setActiveTerminalSessionId(remaining[0]?.id ?? null);
    }
  } catch (error) {
    setActionError(error instanceof Error ? error.message : "Kill local session failed");
  }
}, [activeTerminalSessionId, localSessions]);
```

- [ ] **Step 4: Guard auto-spawn of local terminal**

Find the `useEffect` that calls `handleCreateLocalSession` on first mount and wrap it:

```typescript
useEffect(() => {
  if (localSpawnedRef.current || !isTauri()) return;
  localSpawnedRef.current = true;
  void handleCreateLocalSession();
}, []); // eslint-disable-line react-hooks/exhaustive-deps
```

- [ ] **Step 5: Verify it compiles**

```bash
cd desktop && npx tsc --noEmit
```

Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add desktop/src/App.tsx
git commit -m "refactor(desktop): gate Tauri invoke calls behind isTauri() for PWA compat"
```

---

### Task 5: Gate Tauri Imports in useLocalTerminal Hook

This hook uses `invoke` and `listen` from Tauri. In PWA mode it should be a no-op.

**Files:**
- Modify: `desktop/src/hooks/useLocalTerminal.ts`

- [ ] **Step 1: Add early return when not in Tauri**

At the top of the `useLocalTerminal` function, after the refs and state declarations, the hook already returns early if `sessionId` is null. For PWA mode, add an `isTauri()` guard. Replace the static imports:

```typescript
import { isTauri } from "../lib/platform";
```

Remove the static imports:
```typescript
// REMOVE these lines:
// import { invoke } from "@tauri-apps/api/core";
// import { listen } from "@tauri-apps/api/event";
```

- [ ] **Step 2: Update the main useEffect to dynamically import Tauri APIs**

In the `useEffect` that sets up listeners (around line 70), wrap the listener setup:

```typescript
useEffect(() => {
  if (!sessionId || !isTauri()) {
    setSessionMeta(null);
    setIsConnected(false);
    return;
  }

  let cancelled = false;
  const currentSessionId = sessionId;

  appLog.info(SRC, `Attaching to PTY session ${currentSessionId.slice(0, 8)}`);

  if (isActiveRef.current) {
    const terminal = terminalRef.current;
    if (terminal) terminal.reset();
  }
  chunkBufferRef.current = [];

  const sessionCreatedAt = new Date().toISOString();
  setSessionMeta({
    id: currentSessionId,
    status: "running",
    createdAt: sessionCreatedAt,
  });
  setIsConnected(true);

  let chunkUnlistenPromise: Promise<() => void> | undefined;
  let statusUnlistenPromise: Promise<() => void> | undefined;

  (async () => {
    const { listen } = await import("@tauri-apps/api/event");

    chunkUnlistenPromise = listen<PtyChunkPayload>("pty:chunk", (event) => {
      if (cancelled || event.payload.session_id !== currentSessionId) return;
      if (isActiveRef.current) {
        const term = terminalRef.current;
        if (term) {
          term.write(event.payload.data);
        } else {
          chunkBufferRef.current.push(event.payload.data);
        }
      } else {
        chunkBufferRef.current.push(event.payload.data);
      }
    });

    statusUnlistenPromise = listen<PtyStatusPayload>("pty:status", (event) => {
      if (cancelled || event.payload.session_id !== currentSessionId) return;
      const status = event.payload.status as LocalTerminalSession["status"];
      appLog.info(SRC, `Session ${currentSessionId.slice(0, 8)} status: ${status} (exit_code=${event.payload.exit_code})`);
      const updated: LocalTerminalSession = {
        id: currentSessionId,
        status,
        createdAt: sessionCreatedAt,
        exitCode: event.payload.exit_code,
      };
      setSessionMeta(updated);
      setIsConnected(status === "running");
      onStatusChangeRef.current?.(updated);
    });
  })();

  return () => {
    cancelled = true;
    setIsConnected(false);
    chunkUnlistenPromise?.then((unlisten) => unlisten());
    statusUnlistenPromise?.then((unlisten) => unlisten());
    appLog.info(SRC, `Detached from PTY session ${currentSessionId.slice(0, 8)}`);
  };
}, [sessionId, terminalRef]);
```

- [ ] **Step 3: Update sendInput, resize, kill to dynamically import invoke**

```typescript
const sendInput = useCallback((data: string) => {
  const sid = sessionIdRef.current;
  if (!sid || !isTauri()) return;
  import("@tauri-apps/api/core").then(({ invoke }) => {
    invoke("pty_write", { sessionId: sid, data }).catch((err: unknown) => {
      appLog.error(SRC, `pty_write failed: ${err}`);
      onErrorRef.current?.(`Failed to send input: ${err}`);
    });
  });
}, []);

const resize = useCallback((cols: number, rows: number) => {
  const sid = sessionIdRef.current;
  if (!sid || !isTauri()) return;
  import("@tauri-apps/api/core").then(({ invoke }) => {
    invoke("pty_resize", { sessionId: sid, cols, rows }).catch((err: unknown) => {
      appLog.error(SRC, `pty_resize failed: ${err}`);
    });
  });
}, []);

const kill = useCallback(() => {
  const sid = sessionIdRef.current;
  if (!sid) {
    onErrorRef.current?.("No active PTY session");
    return;
  }
  if (!isTauri()) return;
  appLog.info(SRC, `Killing PTY session ${sid.slice(0, 8)}`);
  import("@tauri-apps/api/core").then(({ invoke }) => {
    invoke("pty_kill", { sessionId: sid }).catch((err: unknown) => {
      appLog.error(SRC, `pty_kill failed: ${err}`);
      onErrorRef.current?.(`Failed to kill session: ${err}`);
    });
  });
}, []);
```

- [ ] **Step 4: Verify it compiles**

```bash
cd desktop && npx tsc --noEmit
```

- [ ] **Step 5: Commit**

```bash
git add desktop/src/hooks/useLocalTerminal.ts
git commit -m "refactor(desktop): dynamically import Tauri APIs in useLocalTerminal for PWA compat"
```

---

### Task 6: Gate Tauri Imports in SetupChecklist

`SetupChecklist.tsx` uses `invoke` for system detection commands. These are desktop-only.

**Files:**
- Modify: `desktop/src/components/SetupChecklist.tsx`

- [ ] **Step 1: Replace static import and guard detection**

Replace:
```typescript
import { invoke } from "@tauri-apps/api/core";
```

with:
```typescript
import { isTauri } from "../lib/platform";
```

- [ ] **Step 2: Update runDetection to dynamically import invoke**

At the start of `runDetection`, add:

```typescript
const runDetection = useCallback(async () => {
  if (!isTauri()) {
    // In PWA mode, skip local detection — all checks show as N/A
    setChecks(INITIAL_CHECKS.map((c) => ({ ...c, status: "ok" as CheckStatus, version: "N/A (PWA)" })));
    setAllDone(true);
    return;
  }

  const { invoke } = await import("@tauri-apps/api/core");

  // ... rest of the function unchanged, but now `invoke` is in scope from the dynamic import
```

Make sure the rest of the function body (the `invoke` calls for `detect_package_manager`, `detect_tmux`, `detect_tailscale`, `detect_tailscale_ip`, `detect_daemon`) uses the dynamically imported `invoke`.

- [ ] **Step 3: Verify it compiles**

```bash
cd desktop && npx tsc --noEmit
```

- [ ] **Step 4: Commit**

```bash
git add desktop/src/components/SetupChecklist.tsx
git commit -m "refactor(desktop): gate SetupChecklist Tauri calls for PWA compat"
```

---

### Task 7: PWA Build Configuration

Set up a parallel Vite config for building the PWA version of the frontend.

**Files:**
- Create: `desktop/vite.config.pwa.ts`
- Create: `desktop/pwa/manifest.json`
- Create: `desktop/pwa/sw.ts`
- Modify: `desktop/package.json` (add build:pwa script + vite-plugin-pwa dep)

- [ ] **Step 1: Install vite-plugin-pwa**

```bash
cd desktop && npm install -D vite-plugin-pwa
```

- [ ] **Step 2: Create PWA manifest**

Create `desktop/pwa/manifest.json`:

```json
{
  "name": "Ghost Protocol",
  "short_name": "Ghost",
  "description": "AI agent control plane",
  "start_url": "/",
  "display": "standalone",
  "background_color": "#1a1a2e",
  "theme_color": "#1a1a2e",
  "icons": [
    {
      "src": "/icons/icon-192.png",
      "sizes": "192x192",
      "type": "image/png"
    },
    {
      "src": "/icons/icon-512.png",
      "sizes": "512x512",
      "type": "image/png"
    }
  ]
}
```

- [ ] **Step 3: Create the PWA Vite config**

Create `desktop/vite.config.pwa.ts`:

```typescript
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { VitePWA } from "vite-plugin-pwa";

export default defineConfig({
  plugins: [
    react(),
    VitePWA({
      registerType: "autoUpdate",
      manifest: false, // We provide our own manifest
      workbox: {
        globPatterns: ["**/*.{js,css,html,ico,png,svg}"],
      },
    }),
  ],
  build: {
    outDir: "dist-pwa",
  },
  // Don't set a fixed port or Tauri-specific options
});
```

- [ ] **Step 4: Copy the manifest into the public directory for the PWA build**

Create `desktop/public/manifest.json` by copying `desktop/pwa/manifest.json`. Also copy existing icons:

```bash
cp desktop/pwa/manifest.json desktop/public/manifest.json
```

Note: The icons referenced in manifest.json (icon-192.png, icon-512.png) need to be generated from the existing icon. For now, copy the existing 128x128 as a placeholder:

```bash
mkdir -p desktop/public/icons
cp desktop/src-tauri/icons/128x128.png desktop/public/icons/icon-192.png
cp desktop/src-tauri/icons/128x128@2x.png desktop/public/icons/icon-512.png
```

- [ ] **Step 5: Add build:pwa script to package.json**

In `desktop/package.json`, add to the `"scripts"` section:

```json
"build:pwa": "tsc && vite build --config vite.config.pwa.ts"
```

- [ ] **Step 6: Test the PWA build**

```bash
cd desktop && npm run build:pwa
```

Expected: builds successfully into `desktop/dist-pwa/`.

- [ ] **Step 7: Commit**

```bash
git add desktop/vite.config.pwa.ts desktop/pwa/ desktop/public/manifest.json desktop/public/icons/ desktop/package.json desktop/package-lock.json
git commit -m "feat(desktop): add PWA build config with vite-plugin-pwa"
```

---

### Task 8: Daemon Static File Serving for PWA

Add a route to the daemon that serves the PWA frontend as static files.

**Files:**
- Modify: `daemon/Cargo.toml`
- Modify: `daemon/src/server.rs`

- [ ] **Step 1: Add tower-http serve-dir feature**

In `daemon/Cargo.toml`, the `tower-http` dependency currently has `features = ["cors"]`. Update it:

```toml
tower-http = { version = "0.6", features = ["cors", "fs"] }
```

- [ ] **Step 2: Add static file serving to the router**

In `daemon/src/server.rs`, add the import at the top:

```rust
use tower_http::services::ServeDir;
```

After the existing router is built (after `.layer(Extension(Arc::new(settings.clone())))` on line 169), add a fallback for serving the web directory:

```rust
    // 8b. Optionally serve PWA frontend
    let app = {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));
        let web_dir = exe_dir
            .as_ref()
            .map(|d| d.join("resources/web"))
            .filter(|p| p.is_dir())
            .or_else(|| {
                // Fallback: check relative to cwd (for development)
                let cwd = std::env::current_dir().ok()?;
                let p = cwd.join("web");
                p.is_dir().then_some(p)
            });

        if let Some(web_path) = web_dir {
            info!(path = %web_path.display(), "serving PWA frontend");
            app.fallback_service(ServeDir::new(web_path))
        } else {
            app
        }
    };
```

This serves the PWA at the root URL when a `resources/web/` directory exists next to the daemon binary, or a `web/` directory in the cwd (for dev). API routes take priority since they're defined first.

- [ ] **Step 3: Verify it compiles**

```bash
cd daemon && cargo check
```

Expected: compiles without errors.

- [ ] **Step 4: Test manually**

Build the PWA, copy it to a test location, and verify the daemon serves it:

```bash
cd desktop && npm run build:pwa
mkdir -p /tmp/ghost-web-test/web
cp -r dist-pwa/* /tmp/ghost-web-test/web/
cd ../daemon && cargo run -- --bind-host 127.0.0.1 &
sleep 2
curl -s http://127.0.0.1:8787/ | head -5
kill %1
```

Expected: HTML content from the PWA build.

- [ ] **Step 5: Commit**

```bash
git add daemon/Cargo.toml daemon/src/server.rs
git commit -m "feat(daemon): serve PWA frontend as static files when web/ directory exists"
```

---

### Task 9: Unified Build Script

Replace `scripts/package-linux.sh` with a cross-platform `scripts/package.sh`.

**Files:**
- Create: `scripts/package.sh`

- [ ] **Step 1: Create the build script**

Create `scripts/package.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
VERSION="0.2.1"

# Parse arguments
PWA_ONLY=false
ARCH_MODE=false
for arg in "$@"; do
  case "$arg" in
    --pwa-only) PWA_ONLY=true ;;
    --arch) ARCH_MODE=true ;;
    --help|-h)
      echo "Usage: $0 [--pwa-only] [--arch]"
      echo ""
      echo "  --pwa-only   Build only the PWA frontend"
      echo "  --arch       Build Arch Linux tarball (like the old package-linux.sh)"
      echo ""
      echo "Without flags, builds the full Tauri desktop package for the current platform."
      exit 0
      ;;
  esac
done

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)   TARGET_OS="linux" ;;
  Darwin)  TARGET_OS="macos" ;;
  MINGW*|MSYS*|CYGWIN*) TARGET_OS="windows" ;;
  *) echo "Unsupported OS: $OS"; exit 1 ;;
esac

# Detect target triple
case "$ARCH" in
  x86_64|amd64)
    case "$TARGET_OS" in
      linux)   TARGET_TRIPLE="x86_64-unknown-linux-gnu" ;;
      macos)   TARGET_TRIPLE="x86_64-apple-darwin" ;;
      windows) TARGET_TRIPLE="x86_64-pc-windows-msvc" ;;
    esac
    ;;
  aarch64|arm64)
    case "$TARGET_OS" in
      linux)   TARGET_TRIPLE="aarch64-unknown-linux-gnu" ;;
      macos)   TARGET_TRIPLE="aarch64-apple-darwin" ;;
      windows) TARGET_TRIPLE="aarch64-pc-windows-msvc" ;;
    esac
    ;;
  *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

echo "==> Platform: $TARGET_OS ($TARGET_TRIPLE)"

# Step 1: Build the PWA
build_pwa() {
  echo "==> Building PWA frontend..."
  cd "$ROOT_DIR/desktop"
  npm run build:pwa
  echo "    PWA built to desktop/dist-pwa/"
}

# Step 2: Build daemon + CLI
build_rust_binaries() {
  echo "==> Building daemon..."
  cd "$ROOT_DIR/daemon"
  cargo build --release 2>&1 | tail -5

  echo "==> Building CLI..."
  cd "$ROOT_DIR/cli"
  cargo build --release 2>&1 | tail -5
}

# Step 3: Copy sidecars with target-triple naming
prepare_sidecars() {
  local bin_dir="$ROOT_DIR/desktop/src-tauri/binaries"
  mkdir -p "$bin_dir"

  local ext=""
  if [ "$TARGET_OS" = "windows" ]; then
    ext=".exe"
  fi

  echo "==> Copying sidecar binaries..."
  cp "$ROOT_DIR/daemon/target/release/ghost-protocol-daemon${ext}" \
     "$bin_dir/ghost-protocol-daemon-${TARGET_TRIPLE}${ext}"
  cp "$ROOT_DIR/cli/target/release/ghost${ext}" \
     "$bin_dir/ghost-${TARGET_TRIPLE}${ext}"

  echo "    Sidecars: $bin_dir/"
}

# Step 4: Copy PWA into Tauri resources
prepare_pwa_resource() {
  local web_dir="$ROOT_DIR/desktop/src-tauri/resources/web"
  rm -rf "$web_dir"
  mkdir -p "$web_dir"
  cp -r "$ROOT_DIR/desktop/dist-pwa/"* "$web_dir/"
  echo "    PWA copied to src-tauri/resources/web/"
}

# Step 5: Build Tauri desktop package
build_tauri() {
  echo "==> Building Tauri desktop package..."
  cd "$ROOT_DIR/desktop"
  npx tauri build 2>&1 | tail -20

  echo ""
  echo "==> Packages:"
  local bundle_dir="$ROOT_DIR/desktop/src-tauri/target/release/bundle"
  if [ -d "$bundle_dir" ]; then
    find "$bundle_dir" -maxdepth 2 -type f \( -name "*.deb" -o -name "*.AppImage" -o -name "*.dmg" -o -name "*.msi" -o -name "*.exe" \) -exec echo "    {}" \;
  fi
}

# Arch Linux tarball mode (replaces old package-linux.sh)
build_arch_tarball() {
  echo "==> Building Arch Linux tarball..."
  local dist_dir="$ROOT_DIR/dist/ghost-protocol-$VERSION"
  rm -rf "$dist_dir"
  mkdir -p "$dist_dir"

  # Copy binaries
  cp "$ROOT_DIR/desktop/src-tauri/target/release/ghost_protocol" "$dist_dir/ghost-protocol" 2>/dev/null \
    || cp "$ROOT_DIR/desktop/src-tauri/target/release/ghost-protocol" "$dist_dir/ghost-protocol" 2>/dev/null \
    || echo "    Warning: desktop binary not found"
  cp "$ROOT_DIR/daemon/target/release/ghost-protocol-daemon" "$dist_dir/ghost-protocol-daemon"
  cp "$ROOT_DIR/cli/target/release/ghost" "$dist_dir/ghost"

  # Copy PWA for daemon to serve
  if [ -d "$ROOT_DIR/desktop/dist-pwa" ]; then
    cp -r "$ROOT_DIR/desktop/dist-pwa" "$dist_dir/web"
  fi

  # Icon
  cp "$ROOT_DIR/desktop/src-tauri/icons/icon.png" "$dist_dir/ghost-protocol.png" 2>/dev/null || true

  # Desktop entry
  cat > "$dist_dir/ghost-protocol.desktop" << 'DESKTOP'
[Desktop Entry]
Name=Ghost Protocol
Comment=AI Agent Control Plane
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

# Install PWA web files for daemon
if [ -d "$SCRIPT_DIR/web" ]; then
  sudo mkdir -p /usr/local/share/ghost-protocol/web
  sudo cp -r "$SCRIPT_DIR/web/"* /usr/local/share/ghost-protocol/web/
  echo "    PWA frontend installed to /usr/local/share/ghost-protocol/web/"
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

  # Tarball
  cd "$ROOT_DIR/dist"
  tar czf "ghost-protocol-$VERSION-linux-x86_64.tar.gz" "ghost-protocol-$VERSION"
  local size
  size=$(du -h "ghost-protocol-$VERSION-linux-x86_64.tar.gz" | cut -f1)
  echo ""
  echo "==> Arch tarball ready: dist/ghost-protocol-$VERSION-linux-x86_64.tar.gz ($size)"
}

# --- Main ---

if [ "$PWA_ONLY" = true ]; then
  build_pwa
  echo "==> Done (PWA only)."
  exit 0
fi

build_pwa
build_rust_binaries
prepare_sidecars
prepare_pwa_resource

if [ "$ARCH_MODE" = true ]; then
  build_tauri
  build_arch_tarball
else
  build_tauri
fi

echo "==> Done!"
```

- [ ] **Step 2: Make it executable**

```bash
chmod +x scripts/package.sh
```

- [ ] **Step 3: Test with --help**

```bash
./scripts/package.sh --help
```

Expected: usage text printed.

- [ ] **Step 4: Test --pwa-only**

```bash
./scripts/package.sh --pwa-only
```

Expected: PWA builds into `desktop/dist-pwa/`.

- [ ] **Step 5: Commit**

```bash
git add scripts/package.sh
git commit -m "feat: add unified cross-platform package.sh build script"
```

---

### Task 10: Add Responsive CSS for Mobile

Add basic responsive styles so the PWA is usable on phone screens.

**Files:**
- Modify: `desktop/src/App.css`

- [ ] **Step 1: Read the current App.css to understand the layout structure**

Read `desktop/src/App.css` and identify the key layout classes: `.shell`, `.sidebar`, `.main-panel`, `.right-panel`.

- [ ] **Step 2: Append responsive media queries**

Add to the end of `desktop/src/App.css`:

```css
/* --- Mobile / PWA responsive --- */
@media (max-width: 768px) {
  .shell {
    flex-direction: column;
  }

  .sidebar {
    width: 100%;
    height: auto;
    max-height: 60px;
    flex-direction: row;
    overflow-x: auto;
    overflow-y: hidden;
    border-right: none;
    border-bottom: 1px solid var(--border, #2a2a3e);
  }

  .sidebar .sidebar-nav {
    flex-direction: row;
    gap: 0.25rem;
  }

  .sidebar .sidebar-hosts,
  .sidebar .sidebar-footer {
    display: none;
  }

  .main-panel {
    flex: 1;
    min-height: 0;
  }

  .right-panel {
    display: none;
  }

  .terminal-workspace .terminal-tabs {
    flex-wrap: nowrap;
    overflow-x: auto;
  }

  .settings-view {
    padding: 1rem;
  }
}
```

- [ ] **Step 3: Verify the build**

```bash
cd desktop && npm run build:pwa
```

- [ ] **Step 4: Commit**

```bash
git add desktop/src/App.css
git commit -m "feat(desktop): add responsive CSS for mobile PWA layout"
```

---

### Task 11: Full Build Integration Test

Run the full package script and verify the output.

**Files:** None (test only)

- [ ] **Step 1: Run the full build**

```bash
./scripts/package.sh
```

Expected: daemon, CLI, PWA, and Tauri desktop package all build successfully. Output shows paths to generated packages.

- [ ] **Step 2: Verify sidecar binaries exist with correct names**

```bash
ls -la desktop/src-tauri/binaries/
```

Expected: `ghost-protocol-daemon-x86_64-unknown-linux-gnu` and `ghost-x86_64-unknown-linux-gnu` present.

- [ ] **Step 3: Verify PWA resources are copied**

```bash
ls desktop/src-tauri/resources/web/index.html
```

Expected: file exists.

- [ ] **Step 4: Verify Tauri packages were generated**

```bash
ls desktop/src-tauri/target/release/bundle/deb/*.deb 2>/dev/null
ls desktop/src-tauri/target/release/bundle/appimage/*.AppImage 2>/dev/null
```

Expected: at least one package file exists.

- [ ] **Step 5: Test the Arch tarball variant**

```bash
./scripts/package.sh --arch
ls dist/ghost-protocol-*-linux-x86_64.tar.gz
```

Expected: tarball exists.

- [ ] **Step 6: Commit any fixups**

If any fixes were needed during the integration test, commit them:

```bash
git add -A
git commit -m "fix: integration test fixups for cross-platform packaging"
```

---

### Task 12: Test PWA in Browser

Verify the PWA works when served by the daemon.

**Files:** None (test only)

- [ ] **Step 1: Build the PWA and start the daemon**

```bash
cd desktop && npm run build:pwa
mkdir -p ../daemon/web
cp -r dist-pwa/* ../daemon/web/
cd ../daemon && cargo run -- --bind-host 127.0.0.1 &
DAEMON_PID=$!
sleep 2
```

- [ ] **Step 2: Verify the PWA is served**

```bash
curl -s http://127.0.0.1:8787/ | grep -o '<title>.*</title>'
```

Expected: `<title>Ghost Protocol</title>` (or similar).

- [ ] **Step 3: Verify API endpoints still work alongside static files**

```bash
curl -s http://127.0.0.1:8787/health | head -1
```

Expected: JSON health response (API routes take priority over static files).

- [ ] **Step 4: Clean up**

```bash
kill $DAEMON_PID 2>/dev/null || true
rm -rf ../daemon/web
```

- [ ] **Step 5: Commit if any fixes were needed**

```bash
git add -A && git diff --cached --quiet || git commit -m "fix: PWA serving adjustments from browser testing"
```
