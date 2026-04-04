# Phase 3: Onboarding & Setup Checklist — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a setup checklist that detects Python, tmux, Tailscale, and the daemon, shows per-platform install commands, and auto-adds localhost when the daemon is found.

**Architecture:** New Rust `detect.rs` module exposes 6 Tauri commands for dependency detection. A new `SetupChecklist` React component renders above the terminal, polling every 3 seconds. Integration into App.tsx and Sidebar.tsx controls visibility.

**Tech Stack:** Rust (Tauri 2 commands, `std::process::Command`, `reqwest` for HTTP), TypeScript/React, CSS

---

## File Structure

| File | Action | Responsibility |
|---|---|---|
| `desktop/src-tauri/src/detect.rs` | **Create** | 6 Tauri commands: `detect_python`, `detect_tmux`, `detect_tailscale`, `detect_daemon`, `detect_platform`, `detect_package_manager` |
| `desktop/src-tauri/src/lib.rs` | **Modify** | Register `mod detect` and add commands to `generate_handler!` |
| `desktop/src-tauri/Cargo.toml` | **Modify** | Add `reqwest` dependency for HTTP health check |
| `desktop/src/components/SetupChecklist.tsx` | **Create** | Setup checklist UI component with polling, install commands, copy button |
| `desktop/src/components/TerminalWorkspace.tsx` | **Modify** | Accept `setupChecklist` prop, render `<SetupChecklist>` between tabs and terminal |
| `desktop/src/components/Sidebar.tsx` | **Modify** | Add "Set up this computer" link |
| `desktop/src/App.tsx` | **Modify** | Add `showSetupChecklist` state, `handleHostDetected`, wire props |
| `desktop/src/App.css` | **Modify** | Add `.setup-checklist-*` styles |

---

### Task 1: Rust Detection Commands — Version Helpers and `detect_platform` / `detect_package_manager`

**Files:**
- Create: `desktop/src-tauri/src/detect.rs`
- Modify: `desktop/src-tauri/src/lib.rs:1` (add `mod detect`)
- Modify: `desktop/src-tauri/src/lib.rs:42-44` (add to `generate_handler!`)

- [ ] **Step 1: Create `detect.rs` with version parsing helper and the two non-version commands**

```rust
// desktop/src-tauri/src/detect.rs

use std::process::Command;

/// Compare "major.minor" strings. Returns true if actual >= minimum.
fn version_gte(actual: &str, minimum: &str) -> bool {
    let parse = |s: &str| -> (u32, u32) {
        let parts: Vec<&str> = s.split('.').collect();
        let major = parts.first().and_then(|p| p.parse().ok()).unwrap_or(0);
        let minor = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(0);
        (major, minor)
    };
    let (a_maj, a_min) = parse(actual);
    let (m_maj, m_min) = parse(minimum);
    (a_maj, a_min) >= (m_maj, m_min)
}

#[tauri::command]
pub fn detect_platform() -> String {
    std::env::consts::OS.to_string()
}

#[tauri::command]
pub fn detect_package_manager() -> Result<String, String> {
    for name in ["apt", "dnf", "pacman", "brew"] {
        let result = Command::new("which").arg(name).output();
        if let Ok(output) = result {
            if output.status.success() {
                return Ok(name.to_string());
            }
        }
    }
    Err("unknown".to_string())
}
```

- [ ] **Step 2: Register `mod detect` and add to `generate_handler!` in `lib.rs`**

In `desktop/src-tauri/src/lib.rs`, add `mod detect;` at line 1 (after `mod pty;`), and update the `generate_handler!` call:

```rust
mod pty;
mod detect;

// ... existing code ...

        .invoke_handler(tauri::generate_handler![
            pty_spawn, pty_write, pty_resize, pty_kill,
            detect::detect_python,
            detect::detect_tmux,
            detect::detect_tailscale,
            detect::detect_daemon,
            detect::detect_platform,
            detect::detect_package_manager,
        ])
```

Note: `detect_python`, `detect_tmux`, `detect_tailscale`, `detect_daemon` don't exist yet — they'll be added in the next tasks. For now, add only `detect_platform` and `detect_package_manager` to the handler, and add the other four after Task 2 and Task 3.

Actually, to avoid partial builds failing, register all six names but implement the remaining four as stubs that return `Err("not_implemented")`:

Add these stubs to `detect.rs`:

```rust
#[tauri::command]
pub fn detect_python() -> Result<String, String> {
    Err("not_implemented".to_string())
}

#[tauri::command]
pub fn detect_tmux() -> Result<String, String> {
    Err("not_implemented".to_string())
}

#[tauri::command]
pub fn detect_tailscale() -> Result<String, String> {
    Err("not_implemented".to_string())
}

#[tauri::command]
pub fn detect_daemon() -> Result<String, String> {
    Err("not_implemented".to_string())
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cd desktop && cargo build 2>&1 | tail -5`
Expected: Build succeeds (warnings about unused `version_gte` are fine).

- [ ] **Step 4: Commit**

```bash
git add desktop/src-tauri/src/detect.rs desktop/src-tauri/src/lib.rs
git commit -m "feat: add detect.rs with platform/package_manager commands and stubs"
```

---

### Task 2: Rust Detection Commands — `detect_python`, `detect_tmux`, `detect_tailscale`

**Files:**
- Modify: `desktop/src-tauri/src/detect.rs`

- [ ] **Step 1: Replace the three stubs with real implementations**

Replace the `detect_python`, `detect_tmux`, and `detect_tailscale` stubs in `detect.rs` with:

```rust
#[tauri::command]
pub fn detect_python() -> Result<String, String> {
    let output = Command::new("python3")
        .arg("--version")
        .output()
        .map_err(|_| "not_found".to_string())?;
    if !output.status.success() {
        return Err("not_found".to_string());
    }
    // Output: "Python 3.12.1\n"
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout
        .trim()
        .strip_prefix("Python ")
        .unwrap_or(stdout.trim());
    if version_gte(version, "3.10") {
        Ok(version.to_string())
    } else {
        Err(format!("version_too_old:{}", version))
    }
}

#[tauri::command]
pub fn detect_tmux() -> Result<String, String> {
    let output = Command::new("tmux")
        .arg("-V")
        .output()
        .map_err(|_| "not_found".to_string())?;
    if !output.status.success() {
        return Err("not_found".to_string());
    }
    // Output: "tmux 3.4\n"
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout
        .trim()
        .strip_prefix("tmux ")
        .unwrap_or(stdout.trim());
    if version_gte(version, "3.0") {
        Ok(version.to_string())
    } else {
        Err(format!("version_too_old:{}", version))
    }
}

#[tauri::command]
pub fn detect_tailscale() -> Result<String, String> {
    let output = Command::new("tailscale")
        .arg("version")
        .output()
        .map_err(|_| "not_found".to_string())?;
    if !output.status.success() {
        return Err("not_found".to_string());
    }
    // Output: first line is version like "1.62.0\n..."
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout.lines().next().unwrap_or("").trim().to_string();
    if version.is_empty() {
        return Err("not_found".to_string());
    }
    if version_gte(&version, "1.0") {
        Ok(version)
    } else {
        Err(format!("version_too_old:{}", version))
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cd desktop && cargo build 2>&1 | tail -5`
Expected: Build succeeds with no errors.

- [ ] **Step 3: Commit**

```bash
git add desktop/src-tauri/src/detect.rs
git commit -m "feat: implement detect_python, detect_tmux, detect_tailscale commands"
```

---

### Task 3: Rust Detection Command — `detect_daemon` (HTTP health check)

**Files:**
- Modify: `desktop/src-tauri/Cargo.toml` (add reqwest)
- Modify: `desktop/src-tauri/src/detect.rs`

- [ ] **Step 1: Add `reqwest` dependency to Cargo.toml**

Add to `[dependencies]` in `desktop/src-tauri/Cargo.toml`:

```toml
reqwest = { version = "0.12", features = ["blocking"], default-features = false }
```

We use `blocking` because Tauri commands run on a thread pool and `reqwest::blocking` is simpler here than async.

- [ ] **Step 2: Replace the `detect_daemon` stub with a real implementation**

Replace the `detect_daemon` stub in `detect.rs` with:

```rust
#[tauri::command]
pub fn detect_daemon() -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .map_err(|e| format!("not_running:{}", e))?;
    let resp = client
        .get("http://127.0.0.1:8787/health")
        .send()
        .map_err(|_| "not_running".to_string())?;
    if resp.status().is_success() {
        Ok("running".to_string())
    } else {
        Err("not_running".to_string())
    }
}
```

Add `use reqwest;` is not needed since we use the full path. But add nothing — `reqwest::blocking` is fully qualified.

- [ ] **Step 3: Verify it compiles**

Run: `cd desktop && cargo build 2>&1 | tail -5`
Expected: Build succeeds. First build with reqwest will take longer due to dependency download.

- [ ] **Step 4: Commit**

```bash
git add desktop/src-tauri/Cargo.toml desktop/src-tauri/src/detect.rs
git commit -m "feat: implement detect_daemon command with reqwest health check"
```

---

### Task 4: SetupChecklist Component — Core Structure and Detection Polling

**Files:**
- Create: `desktop/src/components/SetupChecklist.tsx`

- [ ] **Step 1: Create the SetupChecklist component**

```tsx
// desktop/src/components/SetupChecklist.tsx

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type CheckStatus = "pending" | "ok" | "missing" | "too_old";

type CheckItem = {
  name: string;
  key: string;
  status: CheckStatus;
  version: string | null;
  minVersion: string;
};

type Props = {
  visible: boolean;
  onDismiss: () => void;
  onHostDetected: (name: string, url: string) => void;
};

const INITIAL_CHECKS: CheckItem[] = [
  { name: "Python", key: "python", status: "pending", version: null, minVersion: "3.10" },
  { name: "tmux", key: "tmux", status: "pending", version: null, minVersion: "3.0" },
  { name: "Tailscale", key: "tailscale", status: "pending", version: null, minVersion: "1.0" },
  { name: "Daemon", key: "daemon", status: "pending", version: null, minVersion: "" },
];

type InstallCommands = Record<string, Record<string, string>>;

function getInstallCommands(packageManager: string): InstallCommands {
  const pm = packageManager;
  return {
    python: {
      apt: "sudo apt install python3",
      dnf: "sudo dnf install python3",
      pacman: "sudo pacman -S python",
      brew: "brew install python3",
      unknown: "Install Python 3.10+ using your system package manager",
    },
    tmux: {
      apt: "sudo apt install tmux",
      dnf: "sudo dnf install tmux",
      pacman: "sudo pacman -S tmux",
      brew: "brew install tmux",
      unknown: "Install tmux 3.0+ using your system package manager",
    },
    tailscale: {
      apt: "curl -fsSL https://tailscale.com/install.sh | sh",
      dnf: "curl -fsSL https://tailscale.com/install.sh | sh",
      pacman: "sudo pacman -S tailscale",
      brew: "brew install tailscale",
      unknown: "curl -fsSL https://tailscale.com/install.sh | sh",
    },
    daemon: {
      [pm]: "pip install ghost-protocol-daemon && ghost-protocol-daemon",
      unknown: "pip install ghost-protocol-daemon && ghost-protocol-daemon",
    },
  };
}

function getCommand(commands: InstallCommands, key: string, pm: string): string {
  const group = commands[key];
  if (!group) return "";
  return group[pm] ?? group["unknown"] ?? "";
}

const DOT_COLORS: Record<CheckStatus, string> = {
  pending: "#8c95a4",
  ok: "#10b981",
  missing: "#ef4444",
  too_old: "#ef4444",
};

export function SetupChecklist({ visible, onDismiss, onHostDetected }: Props) {
  const [checks, setChecks] = useState<CheckItem[]>(INITIAL_CHECKS);
  const [packageManager, setPackageManager] = useState("unknown");
  const [allDone, setAllDone] = useState(false);
  const hostDetectedRef = useRef(false);

  const runDetection = useCallback(async () => {
    // Detect platform info once (cheap, stable)
    try {
      const pm = await invoke<string>("detect_package_manager").catch(() => "unknown");
      setPackageManager(typeof pm === "string" ? pm : "unknown");
    } catch {
      // ignore
    }

    const results: CheckItem[] = [...INITIAL_CHECKS];

    // Run all four detections in parallel
    const [pythonResult, tmuxResult, tailscaleResult, daemonResult] = await Promise.allSettled([
      invoke<string>("detect_python"),
      invoke<string>("detect_tmux"),
      invoke<string>("detect_tailscale"),
      invoke<string>("detect_daemon"),
    ]);

    const processResult = (
      index: number,
      result: PromiseSettledResult<string>,
    ) => {
      if (result.status === "fulfilled") {
        results[index] = { ...results[index], status: "ok", version: result.value };
      } else {
        const reason = String(result.reason ?? "");
        if (reason.includes("version_too_old")) {
          const ver = reason.split("version_too_old:")[1] ?? null;
          results[index] = { ...results[index], status: "too_old", version: ver };
        } else {
          results[index] = { ...results[index], status: "missing", version: null };
        }
      }
    };

    processResult(0, pythonResult);
    processResult(1, tmuxResult);
    processResult(2, tailscaleResult);
    processResult(3, daemonResult);

    setChecks(results);

    // Auto-add localhost when daemon is detected
    if (results[3].status === "ok" && !hostDetectedRef.current) {
      hostDetectedRef.current = true;
      onHostDetected("This Computer", "http://127.0.0.1:8787");
    }

    // Check if all are resolved
    if (results.every((c) => c.status === "ok")) {
      setAllDone(true);
    }
  }, [onHostDetected]);

  // Poll every 3 seconds
  useEffect(() => {
    if (!visible) return;
    void runDetection();
    const interval = setInterval(() => void runDetection(), 3000);
    return () => clearInterval(interval);
  }, [visible, runDetection]);

  // Auto-dismiss 2 seconds after all green
  useEffect(() => {
    if (!allDone) return;
    const timer = setTimeout(onDismiss, 2000);
    return () => clearTimeout(timer);
  }, [allDone, onDismiss]);

  if (!visible) return null;

  const installCommands = getInstallCommands(packageManager);
  const activeItem = allDone ? null : checks.find((c) => c.status === "missing" || c.status === "too_old") ?? null;

  const handleCopy = (text: string) => {
    void navigator.clipboard.writeText(text);
  };

  return (
    <div className="setup-checklist">
      <div className="setup-checklist-items">
        {checks.map((item) => (
          <div key={item.key} className="setup-checklist-item">
            <span
              className="setup-checklist-dot"
              style={{ background: DOT_COLORS[item.status] }}
            />
            <span className="setup-checklist-label">
              {item.name}
              {item.status === "ok" && item.version ? ` ${item.version}` : ""}
            </span>
          </div>
        ))}
      </div>

      {allDone && (
        <div className="setup-checklist-message">All set!</div>
      )}

      {activeItem && !allDone && (
        <div className="setup-checklist-detail">
          <div className="setup-checklist-message">
            {activeItem.status === "too_old"
              ? `${activeItem.name} ${activeItem.version} is installed but version ${activeItem.minVersion}+ is required.`
              : `${activeItem.name} is not installed.`}
          </div>
          {(() => {
            const cmd = getCommand(installCommands, activeItem.key, packageManager);
            return cmd ? (
              <div className="setup-checklist-command">
                <code>{cmd}</code>
                <button
                  className="setup-checklist-copy"
                  onClick={() => handleCopy(cmd)}
                  title="Copy command"
                >
                  Copy
                </button>
              </div>
            ) : null;
          })()}
        </div>
      )}

      <button className="setup-checklist-dismiss" onClick={onDismiss}>
        Dismiss
      </button>
    </div>
  );
}
```

- [ ] **Step 2: Verify frontend compiles (component isn't mounted yet, but should have no type errors)**

Run: `cd desktop && npx tsc --noEmit 2>&1 | tail -10`
Expected: No errors from SetupChecklist.tsx (may have pre-existing warnings).

- [ ] **Step 3: Commit**

```bash
git add desktop/src/components/SetupChecklist.tsx
git commit -m "feat: add SetupChecklist component with detection polling and install commands"
```

---

### Task 5: CSS Styles for SetupChecklist

**Files:**
- Modify: `desktop/src/App.css` (append after `.terminal-main` / before `.terminal-statusbar` section)

- [ ] **Step 1: Add setup-checklist styles**

Append these styles to `desktop/src/App.css`:

```css
/* ─── Setup checklist ─── */
.setup-checklist {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px 16px;
  background: #1e2340;
  border-bottom: 1px solid rgba(255, 255, 255, 0.08);
  flex-shrink: 0;
}
.setup-checklist-items {
  display: flex;
  gap: 20px;
  align-items: center;
}
.setup-checklist-item {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.82rem;
  color: #e2e8f0;
}
.setup-checklist-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
}
.setup-checklist-label {
  white-space: nowrap;
}
.setup-checklist-detail {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.setup-checklist-message {
  font-size: 0.82rem;
  color: #94a3b8;
}
.setup-checklist-command {
  display: flex;
  align-items: center;
  gap: 8px;
  background: rgba(0, 0, 0, 0.25);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 6px;
  padding: 8px 12px;
  font-family: SFMono-Regular, Consolas, "Liberation Mono", Menlo, monospace;
  font-size: 0.82rem;
  color: #e2e8f0;
}
.setup-checklist-command code {
  flex: 1;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.setup-checklist-copy {
  flex-shrink: 0;
  padding: 4px 10px;
  border: 1px solid rgba(255, 255, 255, 0.15);
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.06);
  color: #94a3b8;
  font-size: 0.75rem;
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.setup-checklist-copy:hover {
  background: rgba(255, 255, 255, 0.12);
  color: #e2e8f0;
}
.setup-checklist-dismiss {
  align-self: flex-end;
  padding: 4px 12px;
  border: none;
  border-radius: 4px;
  background: transparent;
  color: #64748b;
  font-size: 0.75rem;
  cursor: pointer;
  transition: color 0.15s;
}
.setup-checklist-dismiss:hover {
  color: #94a3b8;
}
```

- [ ] **Step 2: Commit**

```bash
git add desktop/src/App.css
git commit -m "feat: add CSS styles for setup checklist component"
```

---

### Task 6: Integrate SetupChecklist into TerminalWorkspace

**Files:**
- Modify: `desktop/src/components/TerminalWorkspace.tsx:16-29` (Props), `desktop/src/components/TerminalWorkspace.tsx:322-324` (render)

- [ ] **Step 1: Add `setupChecklist` prop to TerminalWorkspace**

In `desktop/src/components/TerminalWorkspace.tsx`, add to the `Props` type:

```typescript
type Props = {
  // ... all existing props ...
  setupChecklist?: {
    visible: boolean;
    onDismiss: () => void;
    onHostDetected: (name: string, url: string) => void;
  };
};
```

Add it to the destructured props:

```typescript
export function TerminalWorkspace({
  // ... all existing props ...
  setupChecklist,
}: Props) {
```

- [ ] **Step 2: Import and render SetupChecklist between tabs and terminal**

Add import at the top of the file:

```typescript
import { SetupChecklist } from "./SetupChecklist";
```

In the JSX, between the `</div>` closing the `.terminal-tabs` div and `{error ? ...}`, insert:

```tsx
      {setupChecklist && (
        <SetupChecklist
          visible={setupChecklist.visible}
          onDismiss={setupChecklist.onDismiss}
          onHostDetected={setupChecklist.onHostDetected}
        />
      )}
```

The render section should look like:

```tsx
      {/* Session tabs */}
      <div className="terminal-tabs">
        {/* ... existing tab content ... */}
      </div>

      {setupChecklist && (
        <SetupChecklist
          visible={setupChecklist.visible}
          onDismiss={setupChecklist.onDismiss}
          onHostDetected={setupChecklist.onHostDetected}
        />
      )}

      {error ? <div className="error-banner">{error}</div> : null}

      {/* Terminal area */}
      <div className="terminal-main">
```

- [ ] **Step 3: Verify frontend compiles**

Run: `cd desktop && npx tsc --noEmit 2>&1 | tail -10`
Expected: No errors. The new prop is optional so existing callers don't break.

- [ ] **Step 4: Commit**

```bash
git add desktop/src/components/TerminalWorkspace.tsx
git commit -m "feat: integrate SetupChecklist into TerminalWorkspace"
```

---

### Task 7: Wire Up App.tsx — State, Handler, and Props

**Files:**
- Modify: `desktop/src/App.tsx`

- [ ] **Step 1: Add `showSetupChecklist` state**

After `const [actionError, setActionError] = useState("");` (line 37), add:

```typescript
  const [showSetupChecklist, setShowSetupChecklist] = useState(() => loadHosts().length === 0);
```

- [ ] **Step 2: Add `handleHostDetected` handler**

After the `handleRemoveHost` callback (after line 470), add:

```typescript
  const handleHostDetected = useCallback((name: string, url: string) => {
    // Check if this host URL already exists
    const alreadyExists = hosts.some((h) => h.url === url);
    if (!alreadyExists) {
      handleAddHost(name, url);
    }
    setShowSetupChecklist(false);
  }, [hosts, handleAddHost]);
```

- [ ] **Step 3: Pass `setupChecklist` prop to TerminalWorkspace**

In the JSX where `<TerminalWorkspace>` is rendered (around line 530), add the new prop:

```tsx
          <TerminalWorkspace
            hosts={hosts}
            hostConnections={connections}
            localSessions={localSessions}
            allRemoteSessions={allRemoteSessions}
            activeSessionId={activeTerminalSessionId}
            visible={mainView === "terminal"}
            onSelect={setActiveTerminalSessionId}
            onCreateRemoteSession={(hostId, mode) => void handleCreateRemoteSession(hostId, mode)}
            onCreateLocalSession={() => void handleCreateLocalSession()}
            onRemoteSessionStatusChange={handleRemoteSessionStatusChange}
            onLocalSessionStatusChange={handleLocalSessionStatusChange}
            onKillRemoteSession={(id) => void handleKillRemoteSession(id)}
            onKillLocalSession={(id) => void handleKillLocalSession(id)}
            setupChecklist={{
              visible: showSetupChecklist,
              onDismiss: () => setShowSetupChecklist(false),
              onHostDetected: handleHostDetected,
            }}
          />
```

- [ ] **Step 4: Verify it compiles**

Run: `cd desktop && npx tsc --noEmit 2>&1 | tail -10`
Expected: No errors.

- [ ] **Step 5: Commit**

```bash
git add desktop/src/App.tsx
git commit -m "feat: wire showSetupChecklist state and handleHostDetected into TerminalWorkspace"
```

---

### Task 8: Add "Set up this computer" Link to Sidebar

**Files:**
- Modify: `desktop/src/components/Sidebar.tsx`

- [ ] **Step 1: Add `onShowSetupChecklist` prop**

Update the `Props` type:

```typescript
type Props = {
  hosts: HostConnection[];
  mainView: MainView;
  onChangeView: (view: MainView) => void;
  onAddHost: (name: string, url: string) => void;
  onRemoveHost: (hostId: string) => void;
  showSetupChecklist: boolean;
  onShowSetupChecklist: () => void;
};
```

Destructure the new props:

```typescript
export function Sidebar({
  hosts,
  mainView,
  onChangeView,
  onAddHost,
  onRemoveHost,
  showSetupChecklist,
  onShowSetupChecklist,
}: Props) {
```

- [ ] **Step 2: Add the link below the host list**

After the closing `</div>` of `.sidebar-hosts` (line 164), add a "Set up this computer" link. Actually, place it inside `.sidebar-hosts`, after the add-host toggle button, before the closing `</div>`:

```tsx
        {!showSetupChecklist && (
          <button
            className="sidebar-setup-link"
            onClick={onShowSetupChecklist}
          >
            Set up this computer
          </button>
        )}
      </div>
```

So the end of the hosts section becomes:

```tsx
        {showAddForm ? (
          /* ... existing form ... */
        ) : (
          <button className="sidebar-add-host-toggle" onClick={() => setShowAddForm(true)}>
            {/* ... existing + icon ... */}
            Add host
          </button>
        )}

        {!showSetupChecklist && (
          <button
            className="sidebar-setup-link"
            onClick={onShowSetupChecklist}
          >
            Set up this computer
          </button>
        )}
      </div>
```

- [ ] **Step 3: Add CSS for the link**

Append to `desktop/src/App.css`:

```css
.sidebar-setup-link {
  display: block;
  width: 100%;
  padding: 6px 12px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  font-size: 0.78rem;
  text-align: left;
  cursor: pointer;
  transition: color 0.15s;
}
.sidebar-setup-link:hover {
  color: var(--accent-blue);
}
```

- [ ] **Step 4: Update App.tsx to pass the new Sidebar props**

In `desktop/src/App.tsx`, update the `<Sidebar>` render:

```tsx
      <Sidebar
        hosts={hostConnections}
        mainView={mainView}
        onChangeView={setMainView}
        onAddHost={handleAddHost}
        onRemoveHost={handleRemoveHost}
        showSetupChecklist={showSetupChecklist}
        onShowSetupChecklist={() => setShowSetupChecklist(true)}
      />
```

- [ ] **Step 5: Verify it compiles**

Run: `cd desktop && npx tsc --noEmit 2>&1 | tail -10`
Expected: No errors.

- [ ] **Step 6: Commit**

```bash
git add desktop/src/components/Sidebar.tsx desktop/src/App.tsx desktop/src/App.css
git commit -m "feat: add 'Set up this computer' link to sidebar"
```

---

### Task 9: Full Build Verification

**Files:** None (verification only)

- [ ] **Step 1: Run full Rust build**

Run: `cd desktop/src-tauri && cargo build 2>&1 | tail -10`
Expected: Build succeeds.

- [ ] **Step 2: Run TypeScript type check**

Run: `cd desktop && npx tsc --noEmit 2>&1`
Expected: No errors.

- [ ] **Step 3: Run Vite dev build (if applicable)**

Run: `cd desktop && npx vite build 2>&1 | tail -10`
Expected: Build succeeds.
