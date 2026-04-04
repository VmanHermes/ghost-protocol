# Phase 3: Onboarding & Setup Checklist — Design Spec

**Goal:** Provide a guided setup experience that detects whether Python, tmux, Tailscale, and the Ghost Protocol daemon are installed, shows per-platform install commands for missing dependencies, and auto-adds localhost as a host once the daemon is running.

**Scope:** New Rust detection commands in `detect.rs`, a new `SetupChecklist` React component, and minor integration into `App.tsx` and `Sidebar.tsx`.

---

## 1. Rust Detection Commands (`detect.rs`)

A new Tauri module `desktop/src-tauri/src/detect.rs` exposes six commands:

### Commands

| Command | Returns | Logic |
|---|---|---|
| `detect_python` | `Result<String, String>` | Runs `python3 --version`, parses version, checks >= 3.10. Ok = installed version string. Err = "not_found" or "version_too_old:3.8" |
| `detect_tmux` | `Result<String, String>` | Runs `tmux -V`, parses version, checks >= 3.0. Same Ok/Err pattern. |
| `detect_tailscale` | `Result<String, String>` | Runs `tailscale version`, parses first line, checks >= 1.0. Same Ok/Err pattern. |
| `detect_daemon` | `Result<String, String>` | HTTP GET `http://127.0.0.1:8787/health` with 2-second timeout. Ok = "running". Err = "not_running". |
| `detect_platform` | `String` | Returns `"linux"`, `"macos"`, or `"windows"` via `std::env::consts::OS`. |
| `detect_package_manager` | `Result<String, String>` | Checks for `apt`, `dnf`, `pacman`, `brew` in PATH (in that order). Ok = first found. Err = "unknown". |

### Version Parsing

Each version-checking command follows the same pattern:

1. Run the command, capture stdout
2. Extract version string via simple parsing (e.g., `Python 3.12.1` → `3.12.1`, `tmux 3.4` → `3.4`)
3. Compare major.minor against the minimum (e.g., `3.10` for Python)
4. If installed version >= minimum: `Ok("3.12.1")`
5. If installed but too old: `Err("version_too_old:3.8.10")` (includes actual version)
6. If command not found: `Err("not_found")`

### Minimum Versions

| Dependency | Minimum Version | Rationale |
|---|---|---|
| Python | 3.10 | Match expressions, structural pattern matching used by daemon |
| tmux | 3.0 | Stable popup/hooks API |
| Tailscale | 1.0 | Stable CLI interface |

### Registration

Commands are registered in `main.rs` via `tauri::generate_handler![...]` alongside the existing PTY commands.

---

## 2. SetupChecklist Component

A horizontal strip rendered **above the terminal area** (between the tab bar and the xterm viewport) when `showSetupChecklist` is true.

### Layout

```
┌─────────────────────────────────────────────────────────┐
│ ● Python 3.12  ● tmux 3.4  ○ Tailscale  ○ Daemon      │
│                                                         │
│  Tailscale is not installed.                            │
│  ┌──────────────────────────────────────────────┐       │
│  │ sudo apt install tailscale          [Copy]   │       │
│  └──────────────────────────────────────────────┘       │
│                                              [Dismiss]  │
└─────────────────────────────────────────────────────────┘
```

### Status Items

Four items displayed horizontally: **Python**, **tmux**, **Tailscale**, **Daemon**.

Each item shows:
- **Green dot + version** — detected and meets minimum version (e.g., `● Python 3.12`)
- **Red dot + label** — not found or version too old (e.g., `○ Tailscale`)
- **Gray dot + label** — detection in progress (initial state)

### Behavior

1. **On mount:** Run all four `detect_*` commands in parallel. Update dots as each resolves.
2. **Polling:** Re-run all detection commands every 3 seconds. This allows the checklist to react as the user installs dependencies in a terminal tab.
3. **Active item:** The first unresolved (red) item is the "active" item. Its install command is shown below the status row. Only one command is shown at a time to avoid overwhelming the user.
4. **Install commands:** Generated from `detect_platform()` + `detect_package_manager()`. See Section 3.
5. **Daemon special case:** When daemon detection succeeds, auto-add `http://127.0.0.1:8787` as a host named "This Computer" (if not already present) via the `onHostDetected(name, url)` callback. This is the completion signal.
6. **All green:** When all four items are resolved, show a brief "All set!" message, then auto-dismiss after 2 seconds.
7. **Dismiss button:** Always visible. Hides the checklist without affecting state. User can re-open via sidebar link.
8. **Version too old:** If a dependency is found but below the minimum version, show: "tmux 2.8 is installed but version 3.0+ is required." followed by the upgrade command for the platform.

### Props

```typescript
type SetupChecklistProps = {
  visible: boolean;
  onDismiss: () => void;
  onHostDetected: (name: string, url: string) => void;
};
```

### State (internal)

```typescript
type CheckStatus = "pending" | "ok" | "missing" | "too_old";

type CheckItem = {
  name: string;           // "Python", "tmux", "Tailscale", "Daemon"
  status: CheckStatus;
  version: string | null; // detected version or null
  minVersion: string;     // "3.10", "3.0", "1.0", ""
};
```

---

## 3. Platform-Specific Install Commands

Commands are determined by combining `detect_platform()` and `detect_package_manager()` results.

### Command Matrix

| Dependency | apt (Linux) | dnf (Linux) | pacman (Linux) | brew (macOS) |
|---|---|---|---|---|
| Python | `sudo apt install python3` | `sudo dnf install python3` | `sudo pacman -S python` | `brew install python3` |
| tmux | `sudo apt install tmux` | `sudo dnf install tmux` | `sudo pacman -S tmux` | `brew install tmux` |
| Tailscale | `curl -fsSL https://tailscale.com/install.sh \| sh` | `curl -fsSL https://tailscale.com/install.sh \| sh` | `sudo pacman -S tailscale` | `brew install tailscale` |
| Daemon | `pip install ghost-protocol-daemon && ghost-protocol-daemon` | (same) | (same) | (same) |

### Fallback

If `detect_package_manager()` returns `Err("unknown")`:
- Linux: Show the `curl` one-liner for Tailscale, generic `pip` for daemon, and "Install Python/tmux using your system package manager" as fallback text.
- macOS: Suggest installing Homebrew first.
- Windows: Not supported in Phase 3.

### Copy Button

Each command block has a **Copy** button that copies the command string to the clipboard via `navigator.clipboard.writeText()`.

---

## 4. Integration

### App.tsx

- New state: `showSetupChecklist: boolean`, initialized to `true` when `hosts.length === 0` on first load (no saved hosts = first run).
- `handleHostDetected(name: string, url: string)`: Adds the host via `addHost()`, triggers health check, sets `showSetupChecklist = false`.
- Passes `showSetupChecklist`, `onDismiss`, `onHostDetected` to `TerminalWorkspace`.

### TerminalWorkspace.tsx

- New optional prop: `setupChecklist?: { visible: boolean; onDismiss: () => void; onHostDetected: (name: string, url: string) => void }`.
- When `setupChecklist.visible` is true, renders `<SetupChecklist>` between the tab bar and the terminal host div.

### Sidebar.tsx

- Below the host list, add a text link: **"Set up this computer"**.
- Clicking it calls a new `onShowSetupChecklist()` callback prop, which sets `showSetupChecklist = true` in App.tsx.
- Only shown when the checklist is not already visible.

### App.css

- `.setup-checklist` — the horizontal strip container with subtle top/bottom border, dark background matching terminal theme.
- `.setup-checklist-items` — flexbox row for the four status items.
- `.setup-checklist-item` — individual item with dot + label.
- `.setup-checklist-command` — code block area with monospace font, copy button.
- `.setup-checklist-dismiss` — dismiss button, subtle styling.

---

## Non-Goals (Phase 3)

- **Windows support** — detection commands assume Unix-like environment.
- **Tailscale peer scanning** — only detects if Tailscale is installed, not peers.
- **Auto-install** — commands are copy-paste only; no automated installation.
- **Daemon version checking** — health endpoint confirms running; version check deferred.
