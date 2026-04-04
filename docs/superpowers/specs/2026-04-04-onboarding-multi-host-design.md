# Ghost Protocol: Onboarding & Multi-Host Terminal Design

## Problem

Ghost Protocol currently has no onboarding flow. A new user who downloads the app on their laptop sees a connection error and has no guidance on how to set up the daemon on a host machine, install Tailscale, or connect. The app also assumes a single daemon connection (`baseUrl`), which prevents users from managing terminals across multiple machines.

## Goals

1. A new user can download the app and be productive within minutes, with no prior knowledge of the system.
2. The app guides host setup (daemon, tmux, Tailscale) using its own terminal — no external docs needed.
3. Users can connect to multiple host machines simultaneously.
4. Tailscale is the recommended networking layer but not required.
5. No visual/UX changes to the existing layout (sidebar, terminal workspace, inspector panel).

## Non-Goals

- Bundling the Python daemon inside the Tauri app binary.
- Deep Tailscale API integration (ACLs, identity-based auth). CIDR allowlisting remains.
- Mobile (iPhone) support in this phase — desktop only.

---

## Architecture

### Terminal Source Model

The app supports three terminal sources:

| Source | Backing | Availability | Label example |
|---|---|---|---|
| Local | Tauri-spawned PTY (Rust) | Always | `Local · shell` |
| Remote | Daemon-managed tmux session over WebSocket | When connected to a host | `Desktop · rescue shell` |
| Remote | Same, different host | When connected | `Server · agent` |

Local terminals require no daemon. They are spawned directly by Tauri's Rust backend using the system's default shell. This makes the app immediately usable on first launch.

Remote terminals are the existing daemon-managed tmux sessions, unchanged. Each host connection is independent — its own WebSocket, session list, and inspector data.

### Saved Hosts

Hosts are persisted in localStorage as a list:

```json
[
  { "id": "h1", "name": "Desktop", "url": "http://desktop.tail1234.ts.net:8787" },
  { "id": "h2", "name": "Server", "url": "http://server.tail1234.ts.net:8787" }
]
```

Each host has:
- `id` — stable identifier (UUID, generated on creation)
- `name` — user-chosen label
- `url` — daemon base URL (HTTP)

On app launch, the app attempts to connect to all saved hosts in parallel. Each host independently transitions through connection states: `idle → connecting → connected | error`.

### Connection Management

Each host connection is a self-contained unit that manages:
- Health check polling (`/health`)
- WebSocket connection for terminal streaming
- Session list (`/api/terminal/sessions`)
- System status (`/api/system/status`)
- Run/conversation data (if applicable)

Multiple hosts can be connected simultaneously. The app multiplexes terminal sessions from all connected hosts plus local sessions into a single tab bar.

---

## First-Run Flow

On launch, the app checks for saved hosts. Two paths:

### Path A: No saved hosts (first launch)

The app opens to the normal layout with a **local terminal already running** in the terminal workspace. The terminal header area shows a connection bar:

```
Not connected to any host    [Connect to a host]  [Set up this computer as a host]
```

The user can immediately use the local terminal. Remote features (remote shells, agents) are unavailable until a host is connected.

### Path B: Saved hosts exist

The app opens normally, connects to saved hosts in the background. If a host is unreachable, a non-blocking banner appears in the terminal header:

```
Couldn't reach Desktop (desktop.tailnet:8787)    [Retry]  [Remove]  [Change]
```

The app is always usable — local terminal works, other connected hosts work. No blocking modals.

---

## "Connect to a Host" Flow

Triggered by the connection bar button or from the "+" menu ("Add new host...").

An inline form expands in the terminal header area (not a modal):

```
Host name: [Desktop          ]
Address:   [desktop.tail1234.ts.net:8787]    [Find on Tailscale]  [Test Connection]  [Save & Connect]
```

### Tailscale Auto-Discovery

The "Find on Tailscale" button:
1. Runs `tailscale status --json` via Tauri command (local execution)
2. Parses the peer list
3. Probes each peer on port 8787 for `/health` (with a short timeout)
4. Shows a dropdown of discovered hosts with Ghost Protocol daemons
5. User picks one, address field is populated

If Tailscale is not installed, the button shows "Tailscale not detected" with a subtle "Install Tailscale" link. Manual entry always works.

### Connection Test

"Test Connection" hits `/health` on the entered address. Shows:
- Green check: "Daemon reachable"
- Red x: "Connection failed — check address and ensure daemon is running"

"Save & Connect" persists the host to localStorage and establishes the connection.

---

## "Set Up This Computer as a Host" Flow

Triggered by the connection bar button. This flow uses the already-open local terminal.

### Checklist Strip

A checklist strip appears above the terminal (inside the terminal header, part of the existing layout):

```
Host Setup    Python ✓  tmux ✓  Daemon ✗  Tailscale —    [curl -fsSL https://ghost-protocol.dev/install.sh | bash]  [Copy]
```

Each item is a status indicator:
- `✓` Green — detected and ready
- `✗` Red — not found, required
- `—` Gray — not detected, optional (Tailscale)
- Spinner — checking

### Detection Logic

The checklist polls every 3 seconds by running detection commands via Tauri commands (silent background execution, not visible in the terminal):

| Dependency | Detection | Required |
|---|---|---|
| Python 3.10+ | `python3 --version` | Yes |
| tmux | `tmux -V` | Yes |
| Ghost Protocol Daemon | `curl -s http://localhost:8787/health` | Yes |
| Tailscale | `tailscale status` exit code | No |

### User Interaction

The user installs dependencies in the local terminal that's already open below the checklist. They can:
- Run the one-liner to install everything at once
- Or install each dependency manually, watching the checklist update in real time

### Completion

Once Python, tmux, and daemon are all green:
- The checklist shows: **"Host ready!"** with the connection address (Tailscale hostname if available, or LAN IP)
- A "Done" button dismisses the checklist strip
- The app auto-connects to `localhost:8787` and saves it as a host
- If Tailscale is active, the checklist also shows: "Other devices can connect to: `desktop.tail1234.ts.net:8787`"

### Install Script

The one-liner (`curl ... | bash`) generates a script that:
1. Detects OS (Linux/macOS)
2. Installs Python 3.10+ if missing (via system package manager)
3. Installs tmux if missing
4. Installs the Ghost Protocol daemon (pip install or clone + venv)
5. Creates and starts a systemd user service (Linux) or launchd plist (macOS)
6. Optionally installs Tailscale (prompts yes/no)
7. Prints the connection address for other devices

---

## Terminal Tab Bar Changes

### Tab Labels

Every tab shows its source:

- Local sessions: `Local · shell`
- Remote sessions: `Desktop · rescue shell`, `Server · agent`

The source label uses a subtle secondary color. The session name/mode remains the primary text.

### "+" Button (New Session)

The "+" button opens a small menu grouped by source:

```
Local
  Shell

Desktop (connected)
  Shell
  Run Agent

Server (unreachable)
  Shell        [grayed out]
  Run Agent    [grayed out]

---
Add new host...
```

Unreachable hosts are shown but disabled, with their status visible.

### Tab Grouping

Tabs are ordered by source: local tabs first, then remote tabs grouped by host. Within each group, tabs are ordered by creation time.

---

## Sidebar Changes

The sidebar's bottom connection area currently shows a single connection status. With multi-host:

```
Hosts
  ● Desktop        connected
  ● Server         connected
  ○ Staging        unreachable
  + Add host
```

Each host shows a colored dot (green/red/gray) and its name. Clicking a host could filter the session list or just scroll to its tabs. The "+ Add host" link opens the "Connect to a host" inline form.

---

## Inspector Panel Changes

The inspector panel's metric cards adapt to show aggregated or per-host data:

- **Active Sessions**: Total across all hosts + local
- **Token Usage**: Aggregated across all hosts
- **Alerts**: Aggregated, with host name prefixed

When a specific remote terminal tab is active, the inspector could show host-specific details in a small section:
- Host name and address
- Daemon uptime
- Tailscale hostname (if applicable)

---

## Local Terminal Implementation (Tauri Side)

The local PTY is a new Tauri capability:

### Rust Backend

A Tauri plugin or command set that:
- Spawns a shell process (`$SHELL` or `/bin/bash`) in a PTY
- Streams stdout to the frontend via Tauri events
- Accepts stdin from the frontend via Tauri commands
- Handles resize (ioctl TIOCSWINSZ)
- Tracks multiple local sessions by ID

### Frontend Integration

A new `useLocalTerminal` hook (parallel to `useTerminalSocket`) that:
- Creates a local PTY via Tauri command
- Connects xterm.js to the Tauri event stream (instead of WebSocket)
- Sends input via Tauri command (instead of WS `terminal_input`)
- Same interface shape as `useTerminalSocket` for the terminal workspace to consume uniformly

### Session Abstraction

The terminal workspace needs a unified session interface:

```typescript
type TerminalSource =
  | { type: "local"; sessionId: string }
  | { type: "remote"; hostId: string; sessionId: string };
```

The tab bar, "+" menu, and status bar all use this abstraction. The workspace picks the right hook (`useLocalTerminal` vs `useTerminalSocket`) based on the source type.

---

## Data Flow Summary

```
App Launch
  ├─ Spawn local PTY (always)
  ├─ Load saved hosts from localStorage
  ├─ For each host: attempt connection (parallel)
  │     ├─ GET /health → connected / error
  │     ├─ GET /api/terminal/sessions → populate session list
  │     └─ Open WebSocket for active sessions
  └─ Render terminal workspace
       ├─ Local tabs (from Tauri PTY)
       └─ Remote tabs (from each connected host)
```

---

## Error Handling

| Scenario | Behavior |
|---|---|
| No hosts saved, first launch | Show connection bar, local terminal active |
| Saved host unreachable on launch | Non-blocking banner, local terminal works, other hosts work |
| Host disconnects mid-session | Terminal tab shows "disconnected" overlay, auto-reconnect with backoff |
| Tailscale not installed | "Find on Tailscale" disabled, manual entry works |
| Setup script fails | User sees the error in the local terminal, checklist stays red |
| Daemon crashes on host | Terminal tabs for that host show disconnected, reconnect when daemon returns |

---

## Migration from Current State

The current single-`baseUrl` model migrates cleanly:
- If `ghost-protocol.baseUrl` exists in localStorage, convert it to a single saved host entry: `{ id: uuid(), name: "Default", url: <baseUrl> }`
- Remove the old key
- All existing functionality continues to work

---

## Implementation Phases

This design decomposes into three independent sub-projects:

### Phase 1: Local Terminal (Tauri PTY)
- Rust-side PTY spawning and I/O streaming
- `useLocalTerminal` hook
- Terminal workspace renders local sessions
- App is usable without any daemon

### Phase 2: Multi-Host Connections
- Saved hosts model (localStorage)
- Per-host connection management
- Tab labels with source prefixes
- "+" menu grouped by source
- Sidebar host list
- Migration from single `baseUrl`

### Phase 3: Onboarding & Setup
- First-run connection bar
- "Connect to a host" inline form with Tailscale discovery
- "Set up this computer" checklist strip + detection polling
- Install script generation
- Host setup completion flow

Each phase is independently shippable and useful.
