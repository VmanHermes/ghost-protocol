# Host a Connection — Design Spec

**Goal:** Allow users to start hosting a Ghost Protocol daemon from the app, bound to their Tailscale IP so other mesh peers can connect. The Tailscale IP:port is displayed for manual sharing.

**Scope:** New Rust commands in `detect.rs`, Sidebar UI additions, App.tsx state management. No changes to the Phase 3 setup checklist.

---

## 1. Rust Commands (`detect.rs`)

Three new Tauri commands added to the existing `detect.rs` module.

### `detect_tailscale_ip`

| | |
|---|---|
| Signature | `Result<String, String>` |
| Command | `tailscale ip -4` |
| Ok | Tailscale IPv4 address, e.g. `"100.64.1.23"` |
| Err | `"not_connected"` if Tailscale is not running or not connected to a mesh |

Trims whitespace from stdout. Returns Err if the command fails, exits non-zero, or produces empty output.

### `start_daemon`

| | |
|---|---|
| Signature | `(bind_host: String, port: u16) -> Result<String, String>` |
| Pre-check | Runs `python3 -c "import ghost_protocol_daemon"` to verify the package is installed. Returns `Err("not_installed")` if this fails. |
| Spawn | Starts `python3 -m ghost_protocol_daemon` as a detached background process with environment variables: `GHOST_PROTOCOL_BIND_HOST=<bind_host>,127.0.0.1`, `GHOST_PROTOCOL_BIND_PORT=<port>`, `GHOST_PROTOCOL_ALLOWED_CIDRS=100.64.0.0/10,fd7a:115c:a1e0::/48,127.0.0.1/32` |
| Ok | `"spawned"` — the process was started. The frontend polls `detect_daemon` to confirm it's healthy. |
| Err | `"not_installed"` or `"spawn_failed:<detail>"` |

The bind host is set to `<tailscale_ip>,127.0.0.1` so the daemon listens on both addresses. The CIDR filter restricts accepted connections to Tailscale ranges + localhost.

The process is spawned detached (not a child of the Tauri app) so it survives app close. On Linux, this means using `Command::new("setsid")` or equivalent to detach from the parent process group.

### `stop_daemon`

| | |
|---|---|
| Signature | `Result<String, String>` |
| Method | Runs `pkill -f "python.*ghost_protocol_daemon"` to find and kill the daemon process. |
| Ok | `"stopped"` |
| Err | `"not_running"` if no matching process found |

### Registration

All three commands are added to `generate_handler!` in `lib.rs` alongside the existing detect commands.

---

## 2. Sidebar UI

### Button Placement

Below the host list, above the existing "Set up this computer" link. Both are always visible (when applicable).

### States

**Idle (default):**
```
[ > Host a connection ]
```
A button styled like the existing "Add host" toggle. Play/share icon + text.

**Starting:**
```
[ ... Starting... ]
```
Disabled, with a subtle animation. Shown while pre-checks run and daemon starts (typically 2-5 seconds).

**Active:**
```
[ ■ Hosting ]
100.64.1.23:8787  [Copy]
```
Green "Hosting" label with a stop icon. Below it, the Tailscale IP:port in a small mono-font line with a Copy button. Clicking the stop icon calls `stop_daemon`.

**Error:**
```
[ > Host a connection ]
Daemon not installed. Set up this computer
```
or
```
[ > Host a connection ]
Tailscale not connected to a mesh.
```
Error text appears below the button in red/muted text. For "not installed", the text "Set up this computer" is a clickable link that reopens the setup checklist. The error clears on the next click.

### Start Flow (triggered by clicking the button)

1. Set `hostingStatus = "starting"`
2. Call `detect_tailscale_ip`
   - If Err → set error "Tailscale not connected to a mesh", return to idle
3. Call `start_daemon(tailscaleIp, 8787)`
   - If Err `"not_installed"` → set error "Daemon not installed", return to idle
   - If Err other → set error with detail, return to idle
4. Poll `detect_daemon` every 1 second, up to 10 seconds
   - If Ok → set `hostingStatus = "active"`, `hostingAddress = "<tailscaleIp>:8787"`
   - If timeout → set error "Daemon failed to start", call `stop_daemon`, return to idle
5. Add `http://127.0.0.1:8787` as "This Computer" host if not already present (via existing `handleAddHost`)

### Stop Flow

1. Call `stop_daemon`
2. Set `hostingStatus = "idle"`, clear `hostingAddress`
3. Do NOT remove "This Computer" from host list — it naturally goes to "unreachable" on the next health poll

---

## 3. App.tsx State and Integration

### New State

```typescript
const [hostingStatus, setHostingStatus] = useState<"idle" | "starting" | "active" | "error">("idle");
const [hostingError, setHostingError] = useState<string | null>(null);
const [hostingAddress, setHostingAddress] = useState<string | null>(null);
```

### Handlers

**`handleStartHosting()`:**
Implements the start flow from Section 2. Uses `invoke()` to call the Rust commands. On success, calls existing `handleAddHost("This Computer", "http://127.0.0.1:8787")` if the URL isn't already in the hosts list.

**`handleStopHosting()`:**
Calls `invoke("stop_daemon")`, resets `hostingStatus` to `"idle"`, clears `hostingAddress` and `hostingError`.

### Props to Sidebar

```typescript
<Sidebar
  // ... existing props ...
  hostingStatus={hostingStatus}
  hostingError={hostingError}
  hostingAddress={hostingAddress}
  onStartHosting={() => void handleStartHosting()}
  onStopHosting={() => void handleStopHosting()}
/>
```

### On App Launch — Restore Hosting State

In the initial mount effect, after `initializeHosts`:

1. Call `detect_daemon` — if daemon is running on localhost
2. Call `detect_tailscale_ip` — if Tailscale is connected
3. If both succeed → set `hostingStatus = "active"`, `hostingAddress = "<tailscaleIp>:8787"`
4. If daemon is running but no Tailscale → stay idle (daemon was started manually, not via hosting)

This restores the hosting indicator without the user needing to click anything after reopening the app.

### Interaction with Phase 3 Checklist

No changes. The checklist continues to detect all 4 items (Python, tmux, Tailscale, Daemon) independently. If the user starts hosting and the daemon starts, the checklist's daemon check will also go green. The existing `handleHostDetected` deduplicates, so no double "This Computer" entries.

---

## 4. Daemon Lifecycle

### Detached Process

The daemon runs as a detached process, not a child of the Tauri app. Closing Ghost Protocol does not stop the daemon. This is intentional — the daemon should serve other peers even when the hosting user isn't actively using the app.

### Explicit Stop Only

The daemon stops only when the user clicks the stop button in the sidebar. No auto-stop on app close, no idle timeout.

### Port Conflict

If port 8787 is already in use (daemon already running, or another process), `start_daemon` will fail. The start flow's health polling will detect the existing daemon and transition to "active" if it responds. If something else is on port 8787, the polling times out and shows an error.

---

## Non-Goals

- **Auto-discovery** — peers must manually add the Tailscale IP. No mDNS, no Tailscale API peer scanning.
- **Connection codes** — the Tailscale IP:port is shown as plain text. No encoding.
- **Multiple ports** — always port 8787. No configuration UI.
- **Windows support** — `setsid`, `pkill` are Unix-only. Windows deferred.
- **Daemon version management** — no upgrade prompts or version checking.
