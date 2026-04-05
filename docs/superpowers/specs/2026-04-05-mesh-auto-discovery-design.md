# Mesh Auto-Discovery & Connection UX

**Date:** 2026-04-05
**Status:** Draft
**Phase:** 2e (extends Phase 2: The Context Layer)

## Context

Ghost Protocol currently requires manual host addition — users type a name and URL into a sidebar form. With Tailscale providing a private mesh, the daemon should automatically discover peers running Ghost Protocol and surface them to the user. The sidebar should reflect live connection state, and permission management should live in Settings where configuration belongs.

## Goals

1. Auto-discover Ghost Protocol peers on the Tailscale mesh
2. Notify user of new discoveries (add/dismiss), don't auto-add
3. Replace "Hosts" sidebar with sorted "Connections" list
4. Remove redundant hosting flow (daemon auto-starts with app)
5. Move permission management to Settings page
6. Simplify right panel to approvals-only

## Non-Goals

- mDNS or custom broadcast discovery (Tailscale CLI is sufficient)
- Auto-adding discovered peers without user confirmation
- Changing the daemon's network security model (Tailscale CIDR guard stays)

---

## Auto-Discovery — Daemon Side

### Discovery Loop

The existing 30-second health poller in `server.rs` is extended with a discovery phase:

1. Run `tailscale status --json` to get all mesh peers
2. Parse each peer's hostname and first IPv4 address
3. For each online peer IP not in `known_hosts` and not dismissed in `discovered_peers`:
   - Probe `http://{ip}:8787/health` with 3s timeout
   - If probe succeeds, upsert into `discovered_peers` with status `pending`
   - If probe fails, skip (not running Ghost Protocol)
4. Continue with existing known_hosts health polling as before

### Data Model

**Migration: `004_discovered_peers.sql`**

```sql
CREATE TABLE IF NOT EXISTS discovered_peers (
    tailscale_ip TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    discovered_at TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
);
```

- `status`: `pending` (awaiting user action), `added` (moved to known_hosts), `dismissed` (user declined)
- Dismissed peers stay dismissed permanently for that IP. If the user wants to connect to a dismissed peer later, they use the manual "+" add button. This avoids nagging.

### Tailscale Status Parsing

New function in `daemon/src/host/detect.rs`:

```rust
pub struct TailscalePeer {
    pub name: String,
    pub ip: String,
    pub online: bool,
}

pub fn list_tailscale_peers() -> Vec<TailscalePeer>
```

Parses `tailscale status --json` output. The JSON structure:
```json
{
  "Peer": {
    "nodekey:abc123": {
      "HostName": "work-laptop",
      "TailscaleIPs": ["100.64.1.5", "fd7a:..."],
      "Online": true
    }
  }
}
```

Extracts hostname + first IPv4 from each peer. Returns empty vec on failure (tailscale not installed, not connected, etc.).

### API Endpoints

**`GET /api/discoveries`** (localhost-only)
Returns pending discovered peers:
```json
[
  { "tailscaleIp": "100.64.1.5", "name": "work-laptop", "discoveredAt": "2026-04-05T10:00:00Z" }
]
```

**`PUT /api/discoveries/{ip}/accept`** (localhost-only)
- Creates a `known_hosts` entry from the discovery (name, IP, url=`http://{ip}:8787`)
- Creates a `peer_permissions` entry with default tier `no-access`
- Updates discovery status to `added`
- Returns the new KnownHost record

**`PUT /api/discoveries/{ip}/dismiss`** (localhost-only)
- Updates discovery status to `dismissed`
- Returns 204

---

## Sidebar — "Connections"

### Layout

The sidebar "Hosts" section becomes "Connections":

- **Section header:** "Connections" with a small "+" button for manual add (fallback)
- **Discovery cards** at top (when pending discoveries exist):
  - Peer name + IP
  - Add / Dismiss buttons
  - Compact card style, visually distinct from connections
- **Connection list** below:
  - Sorted: connected first, then connecting, then offline
  - Each row: status dot (green/amber/gray), name, Tailscale IP muted
  - Click to expand/select (existing behavior)
  - Right-click or "..." menu for remove

### Removed Elements

- **"Host a Connection" button** and all hosting flow UI (start/stop hosting, hosting status, IP display)
- **"Add host" form** — replaced by "+" button that opens a minimal dialog
- **SetupChecklist auto-show** — simplified to first-launch-only (shows when no known_hosts and no discoveries exist)

### Props Changes (Sidebar)

**Removed:**
- `showSetupChecklist`, `onShowSetupChecklist`
- `hostingStatus`, `hostingError`, `hostingAddress`
- `onStartHosting`, `onStopHosting`

**Added:**
- `discoveries: DiscoveredPeer[]`
- `onAcceptDiscovery: (ip: string) => void`
- `onDismissDiscovery: (ip: string) => void`

---

## Settings Page — Permission Management

### Layout

The Settings view (`mainView === "settings"`) gets two sections:

**1. Permissions section**
- Reuses the PermissionsTab component logic (host list with tier dropdowns)
- Rendered inline in the settings page, not as a separate tab
- Shows all known hosts with online/offline status + tier dropdown
- Changes save immediately

**2. Network section**
- Bind address + port
- Allowed CIDRs
- Active terminal session count
- Same info as current settings, just better organized

### Right Panel Changes

- Remove "Permissions" tab from RightPanel
- Remove tab bar (only Approvals remains)
- RightPanel renders ApprovalsTab directly with a simple header
- Notification badge moves to the sidebar "Approvals" or stays on the panel header

---

## Cleanup — Removed Code

### Desktop (TypeScript/React)

| Remove | File | Reason |
|---|---|---|
| Hosting state + handlers | `App.tsx` | `hostingStatus`, `hostingError`, `hostingAddress`, `handleStartHosting`, `handleStopHosting` |
| Hosting props from Sidebar | `Sidebar.tsx` | `hostingStatus`, `hostingError`, `hostingAddress`, `onStartHosting`, `onStopHosting` |
| Hosting UI section | `Sidebar.tsx` | "Host a Connection" button, hosting status display, IP copy |
| Manual add host form | `Sidebar.tsx` | Replaced by "+" button with minimal dialog |
| SetupChecklist auto-show logic | `App.tsx` | Simplified to first-launch-only |
| localStorage host helpers | `hosts.ts` | `loadHosts`, `addHost`, `removeHost` — all data from daemon API now |
| PermissionsTab from RightPanel | `RightPanel.tsx` | Moved to Settings page |
| Tab bar in RightPanel | `RightPanel.tsx` | Single content, no tabs needed |

### Desktop (Tauri/Rust)

| Remove | File | Reason |
|---|---|---|
| `install_daemon` command | `lib.rs` | Daemon managed externally, not by desktop app |
| `start_daemon` command | `lib.rs` | Same |
| `stop_daemon` command | `lib.rs` | Same |

### Keep

| Keep | Reason |
|---|---|
| `SetupChecklist.tsx` | First-launch guidance |
| `detect_tailscale_ip` | Used for self-identification |
| `detect_daemon` | Used for startup health check |
| `PermissionsTab.tsx` | Reused in Settings page (import moves) |
| All daemon host endpoints | Still needed for CRUD |

---

## Files to Modify/Create

### Daemon (Rust)

| File | Change |
|---|---|
| `daemon/migrations/004_discovered_peers.sql` | New migration |
| `daemon/src/store/discoveries.rs` | New — CRUD for discovered_peers |
| `daemon/src/store/mod.rs` | Register discoveries module, run migration |
| `daemon/src/host/detect.rs` | Add `list_tailscale_peers()` |
| `daemon/src/server.rs` | Extend health poller with discovery, register new routes |
| `daemon/src/transport/http.rs` | Add discovery endpoints |

### Desktop (TypeScript/React)

| File | Change |
|---|---|
| `desktop/src/types.ts` | Add `DiscoveredPeer` type |
| `desktop/src/api.ts` | Add discovery API functions |
| `desktop/src/components/Sidebar.tsx` | Rewrite: Connections + discovery cards, remove hosting |
| `desktop/src/components/RightPanel.tsx` | Simplify to approvals-only, remove tabs |
| `desktop/src/App.tsx` | Remove hosting state, add discovery polling, move permissions to settings |
| `desktop/src/hosts.ts` | Remove file entirely |

### Desktop (Tauri/Rust)

| File | Change |
|---|---|
| `desktop/src-tauri/src/lib.rs` | Remove `install_daemon`, `start_daemon`, `stop_daemon` commands |
