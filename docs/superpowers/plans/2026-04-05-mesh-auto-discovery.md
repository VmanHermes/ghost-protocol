# Mesh Auto-Discovery & Connection UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Auto-discover Ghost Protocol peers on the Tailscale mesh, replace the manual host management with a sorted Connections sidebar, and move permission management to Settings.

**Architecture:** The daemon's health poller is extended with `tailscale status --json` peer discovery. New peers are stored in a `discovered_peers` table and surfaced via localhost-only API. The desktop sidebar becomes "Connections" with discovery notifications, and the Settings page absorbs permission management from the right panel.

**Tech Stack:** Rust (axum, rusqlite, tokio, serde), TypeScript/React (Tauri 2 desktop)

**Spec:** `docs/superpowers/specs/2026-04-05-mesh-auto-discovery-design.md`

---

## File Structure

### New Files

| File | Responsibility |
|---|---|
| `daemon/migrations/004_discovered_peers.sql` | Schema for `discovered_peers` table |
| `daemon/src/store/discoveries.rs` | CRUD for discovered_peers |

### Modified Files

| File | Change |
|---|---|
| `daemon/src/store/mod.rs` | Register discoveries module, run migration 004 |
| `daemon/src/host/detect.rs` | Add `list_tailscale_peers()` |
| `daemon/src/server.rs` | Extend health poller with discovery loop, register routes |
| `daemon/src/transport/http.rs` | Add discovery endpoints (list, accept, dismiss) |
| `desktop/src/types.ts` | Add `DiscoveredPeer` type |
| `desktop/src/api.ts` | Add discovery API functions |
| `desktop/src/components/Sidebar.tsx` | Rewrite: Connections + discovery cards, remove hosting |
| `desktop/src/components/RightPanel.tsx` | Simplify to approvals-only |
| `desktop/src/App.tsx` | Remove hosting state, add discovery polling, move permissions to Settings |
| `desktop/src-tauri/src/lib.rs` | Remove `install_daemon`, `start_daemon`, `stop_daemon` |
| `desktop/src-tauri/src/detect.rs` | Remove those three functions |

### Deleted Files

| File | Reason |
|---|---|
| `desktop/src/hosts.ts` | localStorage helpers no longer needed |

---

## Task 1: Database Migration & Store for Discoveries

**Files:**
- Create: `daemon/migrations/004_discovered_peers.sql`
- Create: `daemon/src/store/discoveries.rs`
- Modify: `daemon/src/store/mod.rs`

- [ ] **Step 1: Write the migration SQL**

Create `daemon/migrations/004_discovered_peers.sql`:

```sql
CREATE TABLE IF NOT EXISTS discovered_peers (
    tailscale_ip TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    discovered_at TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
);
```

- [ ] **Step 2: Register the migration in store/mod.rs**

In `daemon/src/store/mod.rs`, add at line 4 (after `pub mod permissions;`):
```rust
pub mod discoveries;
```

After the migration_003 block (line 26), add:
```rust
let migration_004 = include_str!("../../migrations/004_discovered_peers.sql");
conn.execute_batch(migration_004)?;
```

- [ ] **Step 3: Write failing tests for discoveries store**

Create `daemon/src/store/discoveries.rs`:

```rust
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::Store;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredPeer {
    pub tailscale_ip: String,
    pub name: String,
    pub discovered_at: String,
    pub status: String,
}

#[cfg(test)]
mod tests {
    use super::super::test_store;

    #[test]
    fn test_upsert_and_list_pending() {
        let store = test_store();
        store.upsert_discovered_peer("100.64.1.5", "work-laptop").unwrap();
        store.upsert_discovered_peer("100.64.1.6", "server").unwrap();

        let pending = store.list_pending_discoveries().unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].name, "work-laptop");
        assert_eq!(pending[0].status, "pending");
    }

    #[test]
    fn test_upsert_does_not_overwrite_dismissed() {
        let store = test_store();
        store.upsert_discovered_peer("100.64.1.5", "work-laptop").unwrap();
        store.dismiss_discovery("100.64.1.5").unwrap();

        // Re-discovering should NOT reset dismissed status
        store.upsert_discovered_peer("100.64.1.5", "work-laptop").unwrap();
        let pending = store.list_pending_discoveries().unwrap();
        assert_eq!(pending.len(), 0);
    }

    #[test]
    fn test_accept_discovery() {
        let store = test_store();
        store.upsert_discovered_peer("100.64.1.5", "work-laptop").unwrap();
        store.accept_discovery("100.64.1.5").unwrap();

        let pending = store.list_pending_discoveries().unwrap();
        assert_eq!(pending.len(), 0);

        let peer = store.get_discovery("100.64.1.5").unwrap().unwrap();
        assert_eq!(peer.status, "added");
    }

    #[test]
    fn test_dismiss_discovery() {
        let store = test_store();
        store.upsert_discovered_peer("100.64.1.5", "work-laptop").unwrap();
        store.dismiss_discovery("100.64.1.5").unwrap();

        let pending = store.list_pending_discoveries().unwrap();
        assert_eq!(pending.len(), 0);

        let peer = store.get_discovery("100.64.1.5").unwrap().unwrap();
        assert_eq!(peer.status, "dismissed");
    }

    #[test]
    fn test_is_known_or_dismissed() {
        let store = test_store();

        // Unknown IP
        assert!(!store.is_known_or_dismissed("100.64.1.5").unwrap());

        // Add to discovered_peers as dismissed
        store.upsert_discovered_peer("100.64.1.5", "work-laptop").unwrap();
        store.dismiss_discovery("100.64.1.5").unwrap();
        assert!(store.is_known_or_dismissed("100.64.1.5").unwrap());

        // Add to known_hosts
        store.add_known_host("h1", "server", "100.64.1.6", "http://100.64.1.6:8787").unwrap();
        assert!(store.is_known_or_dismissed("100.64.1.6").unwrap());
    }
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cd daemon && cargo test store::discoveries`
Expected: FAIL — methods not implemented yet

- [ ] **Step 5: Implement the store methods**

Add to `daemon/src/store/discoveries.rs`, above the `#[cfg(test)]` block:

```rust
impl Store {
    pub fn upsert_discovered_peer(&self, ip: &str, name: &str) -> Result<(), rusqlite::Error> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn();
        // Only insert if not already present (preserves dismissed/added status)
        conn.execute(
            "INSERT INTO discovered_peers (tailscale_ip, name, discovered_at, status)
             VALUES (?1, ?2, ?3, 'pending')
             ON CONFLICT(tailscale_ip) DO UPDATE SET name = ?2
             WHERE discovered_peers.status = 'pending'",
            params![ip, name, now],
        )?;
        Ok(())
    }

    pub fn list_pending_discoveries(&self) -> Result<Vec<DiscoveredPeer>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT tailscale_ip, name, discovered_at, status
             FROM discovered_peers WHERE status = 'pending'
             ORDER BY discovered_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(DiscoveredPeer {
                tailscale_ip: row.get(0)?,
                name: row.get(1)?,
                discovered_at: row.get(2)?,
                status: row.get(3)?,
            })
        })?;
        rows.collect()
    }

    pub fn get_discovery(&self, ip: &str) -> Result<Option<DiscoveredPeer>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT tailscale_ip, name, discovered_at, status
             FROM discovered_peers WHERE tailscale_ip = ?1",
        )?;
        let mut rows = stmt.query_map(params![ip], |row| {
            Ok(DiscoveredPeer {
                tailscale_ip: row.get(0)?,
                name: row.get(1)?,
                discovered_at: row.get(2)?,
                status: row.get(3)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn accept_discovery(&self, ip: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "UPDATE discovered_peers SET status = 'added' WHERE tailscale_ip = ?1",
            params![ip],
        )?;
        Ok(())
    }

    pub fn dismiss_discovery(&self, ip: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "UPDATE discovered_peers SET status = 'dismissed' WHERE tailscale_ip = ?1",
            params![ip],
        )?;
        Ok(())
    }

    pub fn is_known_or_dismissed(&self, ip: &str) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        // Check known_hosts
        let known: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM known_hosts WHERE tailscale_ip = ?1",
            params![ip],
            |row| row.get(0),
        )?;
        if known {
            return Ok(true);
        }
        // Check dismissed discoveries
        let dismissed: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM discovered_peers WHERE tailscale_ip = ?1 AND status IN ('dismissed', 'added')",
            params![ip],
            |row| row.get(0),
        )?;
        Ok(dismissed)
    }
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cd daemon && cargo test store::discoveries`
Expected: All 5 tests PASS

- [ ] **Step 7: Commit**

```bash
git add daemon/migrations/004_discovered_peers.sql daemon/src/store/discoveries.rs daemon/src/store/mod.rs
git commit -m "feat(daemon): add discovered_peers store for mesh auto-discovery"
```

---

## Task 2: Tailscale Peer Discovery Function

**Files:**
- Modify: `daemon/src/host/detect.rs`

**Depends on:** None

- [ ] **Step 1: Add TailscalePeer struct and list_tailscale_peers function**

In `daemon/src/host/detect.rs`, add after the existing `is_ssh_available` function (line 38):

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TailscalePeer {
    pub name: String,
    pub ip: String,
    pub online: bool,
}

pub fn list_tailscale_peers() -> Vec<TailscalePeer> {
    let output = match Command::new("tailscale")
        .args(["status", "--json"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return vec![],
    };

    let json: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    let peers = match json.get("Peer").and_then(|p| p.as_object()) {
        Some(p) => p,
        None => return vec![],
    };

    let mut result = Vec::new();
    for (_key, peer) in peers {
        let name = peer["HostName"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();
        let online = peer["Online"].as_bool().unwrap_or(false);

        // Get first IPv4 address from TailscaleIPs
        let ip = peer["TailscaleIPs"]
            .as_array()
            .and_then(|ips| {
                ips.iter()
                    .filter_map(|v| v.as_str())
                    .find(|s| !s.contains(':'))  // skip IPv6
                    .map(|s| s.to_string())
            });

        if let Some(ip) = ip {
            result.push(TailscalePeer { name, ip, online });
        }
    }
    result
}
```

Add `use serde_json;` to the imports at top if not present. The `serde_json` crate is already a dependency of the daemon.

- [ ] **Step 2: Verify compilation**

Run: `cd daemon && cargo build`
Expected: Compiles

- [ ] **Step 3: Commit**

```bash
git add daemon/src/host/detect.rs
git commit -m "feat(daemon): add Tailscale peer discovery via tailscale status --json"
```

---

## Task 3: Extend Health Poller with Discovery Loop

**Files:**
- Modify: `daemon/src/server.rs`

**Depends on:** Task 1, Task 2

- [ ] **Step 1: Add discovery phase to the health poller**

In `daemon/src/server.rs`, modify the background host health poller (lines 31-67). Add a discovery phase at the beginning of the loop body, before the known_hosts polling:

Replace the entire health poller block (lines 31-67) with:

```rust
// 5. Start background host health poller + discovery
{
    let store = store.clone();
    tokio::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .build()
            .unwrap_or_default();
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;

            // Phase 1: Discover new peers via Tailscale
            let peers = crate::host::detect::list_tailscale_peers();
            for peer in &peers {
                if !peer.online {
                    continue;
                }
                // Skip if already known or dismissed
                if store.is_known_or_dismissed(&peer.ip).unwrap_or(true) {
                    continue;
                }
                // Probe for Ghost Protocol daemon
                let health_url = format!("http://{}:8787/health", peer.ip);
                match client.get(&health_url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        store.upsert_discovered_peer(&peer.ip, &peer.name).ok();
                        tracing::info!(peer = %peer.name, ip = %peer.ip, "discovered new Ghost Protocol peer");
                    }
                    _ => {}
                }
            }

            // Phase 2: Poll known hosts (existing logic)
            if let Ok(hosts) = store.list_known_hosts() {
                for host in hosts {
                    let url = format!("{}/api/system/hardware", host.url);
                    let status = match client.get(&url).send().await {
                        Ok(resp) if resp.status().is_success() => {
                            let caps = resp.json::<serde_json::Value>().await.ok().map(|v| {
                                crate::store::hosts::HostCapabilities {
                                    gpu: v["gpu"]["model"].as_str().map(|s| s.to_string()),
                                    ram_gb: v["ramGb"].as_f64(),
                                    hermes: v["tools"]["hermes"].is_string(),
                                    ollama: v["tools"]["ollama"].is_string(),
                                }
                            });
                            store.update_host_status(&host.id, "online", caps.as_ref()).ok();
                            "online"
                        }
                        _ => {
                            store.update_host_status(&host.id, "offline", None).ok();
                            "offline"
                        }
                    };
                    tracing::debug!(host = %host.name, status, "health poll");
                }
            }
        }
    });
}
```

Note: The client timeout is changed from 5s to 3s to match the spec's "3s timeout" for discovery probes.

- [ ] **Step 2: Verify compilation**

Run: `cd daemon && cargo build`
Expected: Compiles

- [ ] **Step 3: Commit**

```bash
git add daemon/src/server.rs
git commit -m "feat(daemon): extend health poller with Tailscale peer discovery"
```

---

## Task 4: Discovery HTTP Endpoints

**Files:**
- Modify: `daemon/src/transport/http.rs`
- Modify: `daemon/src/server.rs`

**Depends on:** Task 1

- [ ] **Step 1: Add discovery handlers to http.rs**

Add these handlers at the end of `daemon/src/transport/http.rs`:

```rust
// ---------------------------------------------------------------------------
// GET /api/discoveries (localhost-only)
// ---------------------------------------------------------------------------

pub async fn list_discoveries(
    _guard: RequireLocalhostOnly,
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::store::discoveries::DiscoveredPeer>>, (StatusCode, Json<serde_json::Value>)> {
    state
        .store
        .list_pending_discoveries()
        .map(Json)
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") })))
        })
}

// ---------------------------------------------------------------------------
// PUT /api/discoveries/{ip}/accept (localhost-only)
// ---------------------------------------------------------------------------

pub async fn accept_discovery(
    _guard: RequireLocalhostOnly,
    State(state): State<AppState>,
    Path(ip): Path<String>,
) -> Result<(StatusCode, Json<crate::store::hosts::KnownHost>), (StatusCode, Json<serde_json::Value>)> {
    let peer = state.store.get_discovery(&ip).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") })))
    })?.ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "discovery not found" })))
    })?;

    // Create known_host entry
    let id = uuid::Uuid::new_v4().to_string();
    let url = format!("http://{}:8787", ip);
    let host = state.store.add_known_host(&id, &peer.name, &ip, &url).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") })))
    })?;

    // Create default peer_permission (no-access)
    state.store.set_peer_permission(&id, "no-access").map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") })))
    })?;

    // Mark discovery as added
    state.store.accept_discovery(&ip).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") })))
    })?;

    Ok((StatusCode::CREATED, Json(host)))
}

// ---------------------------------------------------------------------------
// PUT /api/discoveries/{ip}/dismiss (localhost-only)
// ---------------------------------------------------------------------------

pub async fn dismiss_discovery(
    _guard: RequireLocalhostOnly,
    State(state): State<AppState>,
    Path(ip): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    state.store.dismiss_discovery(&ip).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") })))
    })?;
    Ok(StatusCode::NO_CONTENT)
}
```

- [ ] **Step 2: Register routes in server.rs**

In `daemon/src/server.rs`, after the approval routes (line 130), add:

```rust
.route("/api/discoveries", get(http::list_discoveries))
.route("/api/discoveries/{ip}/accept", axum::routing::put(http::accept_discovery))
.route("/api/discoveries/{ip}/dismiss", axum::routing::put(http::dismiss_discovery))
```

- [ ] **Step 3: Verify compilation**

Run: `cd daemon && cargo build`
Expected: Compiles

- [ ] **Step 4: Commit**

```bash
git add daemon/src/transport/http.rs daemon/src/server.rs
git commit -m "feat(daemon): add discovery list/accept/dismiss HTTP endpoints"
```

---

## Task 5: Desktop Types & API for Discoveries

**Files:**
- Modify: `desktop/src/types.ts`
- Modify: `desktop/src/api.ts`

**Depends on:** None (can parallel with backend tasks)

- [ ] **Step 1: Add DiscoveredPeer type**

At the end of `desktop/src/types.ts`, add:

```typescript
// --- Discovery types (Phase 2e) ---

export type DiscoveredPeer = {
  tailscaleIp: string;
  name: string;
  discoveredAt: string;
};
```

- [ ] **Step 2: Add discovery API functions**

At the end of `desktop/src/api.ts`, add:

```typescript
import type { DiscoveredPeer } from "./types";

export async function listDiscoveries(daemonUrl: string): Promise<DiscoveredPeer[]> {
  return api<DiscoveredPeer[]>(daemonUrl, "/api/discoveries");
}

export async function acceptDiscovery(
  daemonUrl: string,
  ip: string,
): Promise<ApiHost> {
  return api<ApiHost>(daemonUrl, `/api/discoveries/${ip}/accept`, {
    method: "PUT",
  });
}

export async function dismissDiscovery(
  daemonUrl: string,
  ip: string,
): Promise<void> {
  await fetch(`${daemonUrl}/api/discoveries/${ip}/dismiss`, { method: "PUT" });
}
```

Note: Add `DiscoveredPeer` to the existing import from `"./types"` if one exists, otherwise add a new import.

- [ ] **Step 3: Verify TypeScript compilation**

Run: `cd desktop && npx tsc --noEmit`
Expected: No type errors

- [ ] **Step 4: Commit**

```bash
git add desktop/src/types.ts desktop/src/api.ts
git commit -m "feat(desktop): add discovery types and API functions"
```

---

## Task 6: Rewrite Sidebar as "Connections"

**Files:**
- Modify: `desktop/src/components/Sidebar.tsx`

**Depends on:** Task 5

- [ ] **Step 1: Rewrite Sidebar component**

Replace the entire contents of `desktop/src/components/Sidebar.tsx` with a new implementation:

**New Props type:**
```typescript
type Props = {
  hosts: HostConnection[];
  discoveries: DiscoveredPeer[];
  mainView: MainView;
  onChangeView: (view: MainView) => void;
  onAddHost: (name: string, url: string) => void;
  onRemoveHost: (hostId: string) => void;
  onAcceptDiscovery: (ip: string) => void;
  onDismissDiscovery: (ip: string) => void;
};
```

**Key changes:**
- Import `DiscoveredPeer` from `../types`
- Remove all hosting-related props and UI (`hostingStatus`, `hostingError`, `hostingAddress`, `onStartHosting`, `onStopHosting`, `showSetupChecklist`, `onShowSetupChecklist`)
- Change section header from "Hosts" to "Connections"
- Add discovery notification cards before the connection list
- Sort connections: connected first, then connecting, then error/offline
- Keep the "+" button for manual add (existing form logic), but make it smaller/subtler
- Remove the "Host a Connection" button and all hosting UI (lines 178-234)
- Remove the "Set up this computer" link (lines 235-242)

**Discovery card UI:**
Each pending discovery shows as a compact card:
```tsx
{discoveries.map((peer) => (
  <div key={peer.tailscaleIp} className="sidebar-discovery-card">
    <div className="sidebar-discovery-info">
      <span className="sidebar-discovery-name">{peer.name}</span>
      <span className="sidebar-discovery-ip muted">{peer.tailscaleIp}</span>
    </div>
    <div className="sidebar-discovery-actions">
      <button className="btn-discovery-add" onClick={() => onAcceptDiscovery(peer.tailscaleIp)}>Add</button>
      <button className="btn-discovery-dismiss" onClick={() => onDismissDiscovery(peer.tailscaleIp)}>
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" /></svg>
      </button>
    </div>
  </div>
))}
```

**Connection sorting:**
```typescript
const sortedHosts = [...hosts].sort((a, b) => {
  const order = { connected: 0, connecting: 1, error: 2, idle: 3 };
  return (order[a.state] ?? 3) - (order[b.state] ?? 3);
});
```

- [ ] **Step 2: Verify TypeScript compilation**

Run: `cd desktop && npx tsc --noEmit`
Expected: Will fail because App.tsx still passes old props — that's expected, fixed in Task 8

- [ ] **Step 3: Commit**

```bash
git add desktop/src/components/Sidebar.tsx
git commit -m "feat(desktop): rewrite Sidebar as Connections with discovery cards"
```

---

## Task 7: Simplify RightPanel to Approvals-Only

**Files:**
- Modify: `desktop/src/components/RightPanel.tsx`

**Depends on:** None

- [ ] **Step 1: Remove tabs, render ApprovalsTab directly**

Replace the contents of `desktop/src/components/RightPanel.tsx`:

```tsx
import { useState } from "react";
import { ApprovalsTab } from "./ApprovalsTab";

type Props = {
  daemonUrl: string;
};

export function RightPanel({ daemonUrl }: Props) {
  const [pendingCount, setPendingCount] = useState(0);

  return (
    <aside className="right-panel">
      <div className="right-panel-header">
        <h3>Approvals</h3>
        {pendingCount > 0 && (
          <span className="tab-badge">{pendingCount}</span>
        )}
      </div>
      <div className="right-panel-content">
        <ApprovalsTab
          daemonUrl={daemonUrl}
          onPendingCountChange={setPendingCount}
        />
      </div>
    </aside>
  );
}
```

Remove the `PermissionsTab` import entirely.

- [ ] **Step 2: Verify TypeScript compilation**

Run: `cd desktop && npx tsc --noEmit`
Expected: Compiles (PermissionsTab.tsx still exists, just not imported here)

- [ ] **Step 3: Commit**

```bash
git add desktop/src/components/RightPanel.tsx
git commit -m "feat(desktop): simplify RightPanel to approvals-only"
```

---

## Task 8: Update App.tsx — Discovery, Settings, Cleanup

**Files:**
- Modify: `desktop/src/App.tsx`
- Delete: `desktop/src/hosts.ts`

**Depends on:** Task 5, Task 6, Task 7

This is the largest frontend task. It removes hosting state, adds discovery polling, moves permissions to Settings, and updates Sidebar props.

- [ ] **Step 1: Remove hosting imports and state**

In `desktop/src/App.tsx`:

Remove the import of `hosts.ts`:
```typescript
// DELETE this line:
import { loadHosts, addHost as persistAddHost, removeHost as persistRemoveHost } from "./hosts";
```

Add discovery API imports:
```typescript
import { api, listHosts, addHostApi, removeHostApi, listDiscoveries, acceptDiscovery, dismissDiscovery, type ApiHost } from "./api";
```

Add PermissionsTab import:
```typescript
import { PermissionsTab } from "./components/PermissionsTab";
```

Add DiscoveredPeer to type imports:
```typescript
import type {
  DiscoveredPeer,
  HostConnection,
  LocalTerminalSession,
  SavedHost,
  TerminalSession,
} from "./types";
```

Remove all hosting state declarations:
```typescript
// DELETE these lines:
const [showSetupChecklist, setShowSetupChecklist] = useState(() => loadHosts().length === 0);
const [hostingStatus, setHostingStatus] = useState<"idle" | "starting" | "active" | "error">("idle");
const [hostingError, setHostingError] = useState<string | null>(null);
const [hostingAddress, setHostingAddress] = useState<string | null>(null);
```

Add discovery state:
```typescript
const [discoveries, setDiscoveries] = useState<DiscoveredPeer[]>([]);
```

- [ ] **Step 2: Add discovery polling**

Add after the `refreshHosts` callback:

```typescript
const refreshDiscoveries = useCallback(async () => {
  try {
    const disc = await listDiscoveries(LOCAL_DAEMON);
    setDiscoveries(disc);
  } catch {
    // ignore — daemon may not be running
  }
}, []);

useEffect(() => {
  refreshDiscoveries();
  const interval = setInterval(refreshDiscoveries, 30000);
  return () => clearInterval(interval);
}, [refreshDiscoveries]);
```

Add accept/dismiss handlers:

```typescript
const handleAcceptDiscovery = useCallback(async (ip: string) => {
  try {
    await acceptDiscovery(LOCAL_DAEMON, ip);
    await refreshHosts();
    await refreshDiscoveries();
  } catch (error) {
    appLog.error("discovery", `Failed to accept discovery: ${error instanceof Error ? error.message : String(error)}`);
  }
}, [refreshHosts, refreshDiscoveries]);

const handleDismissDiscovery = useCallback(async (ip: string) => {
  try {
    await dismissDiscovery(LOCAL_DAEMON, ip);
    await refreshDiscoveries();
  } catch (error) {
    appLog.error("discovery", `Failed to dismiss discovery: ${error instanceof Error ? error.message : String(error)}`);
  }
}, [refreshDiscoveries]);
```

- [ ] **Step 3: Remove hosting handlers**

Delete `handleStartHosting` and `handleStopHosting` callbacks entirely. Also delete `handleHostDetected`.

Remove the hosting state restore effect (the one that calls `detect_daemon` and `detect_tailscale_ip` to set hosting status).

Simplify `handleAddHost` to remove localStorage fallback:
```typescript
const handleAddHost = useCallback(async (name: string, url: string) => {
  try {
    const ip = new URL(url).hostname;
    await addHostApi(LOCAL_DAEMON, name, ip);
    await refreshHosts();
  } catch (error) {
    appLog.error("app", `Failed to add host: ${error instanceof Error ? error.message : String(error)}`);
  }
}, [refreshHosts]);
```

Simplify `handleRemoveHost` to remove localStorage fallback:
```typescript
const handleRemoveHost = useCallback(async (hostId: string) => {
  const host = hosts.find((h) => h.id === hostId);
  const conn = connections.get(hostId);
  if (host && conn?.sessions) {
    const active = conn.sessions.filter((s) => s.status === "created" || s.status === "running");
    for (const session of active) {
      void api(host.url, `/api/terminal/sessions/${session.id}/terminate`, { method: "POST" }).catch(() => {});
    }
  }
  try {
    await removeHostApi(LOCAL_DAEMON, hostId);
    await refreshHosts();
  } catch (error) {
    appLog.error("app", `Failed to remove host: ${error instanceof Error ? error.message : String(error)}`);
  }
  setConnections((prev) => {
    const next = new Map(prev);
    next.delete(hostId);
    return next;
  });
  if (conn?.sessions?.some((s) => s.id === activeTerminalSessionId)) {
    setActiveTerminalSessionId(null);
  }
};
```

- [ ] **Step 4: Update Sidebar props**

Update the `<Sidebar>` JSX to use the new props:

```tsx
<Sidebar
  hosts={hostConnections}
  discoveries={discoveries}
  mainView={mainView}
  onChangeView={setMainView}
  onAddHost={handleAddHost}
  onRemoveHost={handleRemoveHost}
  onAcceptDiscovery={handleAcceptDiscovery}
  onDismissDiscovery={handleDismissDiscovery}
/>
```

Remove old props: `showSetupChecklist`, `onShowSetupChecklist`, `hostingStatus`, `hostingError`, `hostingAddress`, `onStartHosting`, `onStopHosting`.

- [ ] **Step 5: Move permissions to Settings view**

Replace the Settings view section (currently around line 486-516) with:

```tsx
<div style={{ display: mainView === "settings" ? "flex" : "none", flexDirection: "column", flex: 1, minHeight: 0 }}>
  <div className="settings-view">
    <div className="settings-header">
      <h2>Settings</h2>
      <p className="muted">Configuration & permissions</p>
    </div>
    <div className="settings-content">
      <div className="settings-section">
        <h3>Permissions</h3>
        <PermissionsTab daemonUrl={LOCAL_DAEMON} />
      </div>
      <div className="settings-section">
        <h3>Network</h3>
        <div className="settings-info-grid">
          <div className="settings-info-item">
            <span className="settings-info-label">Daemon</span>
            <span className="settings-info-value">{LOCAL_DAEMON}</span>
          </div>
          <div className="settings-info-item">
            <span className="settings-info-label">Known hosts</span>
            <span className="settings-info-value">{hosts.length}</span>
          </div>
        </div>
      </div>
    </div>
  </div>
</div>
```

- [ ] **Step 6: Delete hosts.ts**

```bash
rm desktop/src/hosts.ts
```

- [ ] **Step 7: Verify TypeScript compilation**

Run: `cd desktop && npx tsc --noEmit`
Expected: No type errors

- [ ] **Step 8: Commit**

```bash
git add -A desktop/src/
git commit -m "feat(desktop): add discovery polling, move permissions to settings, remove hosting flow"
```

---

## Task 9: Remove Tauri Hosting Commands

**Files:**
- Modify: `desktop/src-tauri/src/lib.rs`
- Modify: `desktop/src-tauri/src/detect.rs`

**Depends on:** Task 8

- [ ] **Step 1: Remove hosting functions from detect.rs**

In `desktop/src-tauri/src/detect.rs`, delete:
- `daemon_bin_path()` function (lines 6-14)
- `version_gte()` function (lines 17-27) — wait, check if anything else uses it. `detect_tmux` and `detect_tailscale` use it. Keep it.
- `install_daemon()` function (lines 119-181)
- `start_daemon()` function (lines 183-212)
- `stop_daemon()` function (lines 214-225)

Also delete `daemon_bin_path()` since only `install_daemon` and `start_daemon` use it.

- [ ] **Step 2: Remove from invoke_handler in lib.rs**

In `desktop/src-tauri/src/lib.rs`, update the `invoke_handler` to remove the three commands:

```rust
.invoke_handler(tauri::generate_handler![
    pty_spawn, pty_write, pty_resize, pty_kill,
    detect::detect_tmux,
    detect::detect_tailscale,
    detect::detect_daemon,
    detect::detect_platform,
    detect::detect_package_manager,
    detect::detect_tailscale_ip,
])
```

Remove: `detect::install_daemon`, `detect::start_daemon`, `detect::stop_daemon`.

- [ ] **Step 3: Remove unused imports**

In `detect.rs`, remove `use std::path::PathBuf;` if no longer used.

- [ ] **Step 4: Verify compilation**

Run: `cd desktop && npx tauri build --debug 2>&1 | tail -5` or `cd desktop/src-tauri && cargo check`
Expected: Compiles

- [ ] **Step 5: Commit**

```bash
git add desktop/src-tauri/src/lib.rs desktop/src-tauri/src/detect.rs
git commit -m "chore(desktop): remove install_daemon, start_daemon, stop_daemon commands"
```

---

## Task 10: CSS for Discovery Cards

**Files:**
- Modify: `desktop/src/App.css`

**Depends on:** Task 6

- [ ] **Step 1: Add discovery card styles**

Add to `desktop/src/App.css`:

```css
/* Discovery cards */
.sidebar-discovery-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
  margin: 4px 8px;
  border-radius: 6px;
  background: rgba(96, 165, 250, 0.08);
  border: 1px solid rgba(96, 165, 250, 0.2);
}

.sidebar-discovery-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.sidebar-discovery-name {
  font-size: 0.85rem;
  font-weight: 500;
  color: var(--text-primary, #e2e8f0);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.sidebar-discovery-ip {
  font-size: 0.75rem;
}

.sidebar-discovery-actions {
  display: flex;
  gap: 4px;
  flex-shrink: 0;
}

.btn-discovery-add {
  padding: 3px 10px;
  border-radius: 4px;
  border: none;
  background: rgba(16, 185, 129, 0.15);
  color: #10b981;
  font-size: 0.75rem;
  font-weight: 500;
  cursor: pointer;
}

.btn-discovery-add:hover {
  background: rgba(16, 185, 129, 0.25);
}

.btn-discovery-dismiss {
  padding: 3px 6px;
  border-radius: 4px;
  border: none;
  background: transparent;
  color: var(--text-muted, #94a3b8);
  cursor: pointer;
  display: flex;
  align-items: center;
}

.btn-discovery-dismiss:hover {
  background: rgba(239, 68, 68, 0.15);
  color: #ef4444;
}
```

Also update the section header to say "Connections" — check if the CSS references the old "Hosts" text in any hardcoded way (it shouldn't — the text is in JSX).

- [ ] **Step 2: Add right-panel-header style**

Add for the simplified RightPanel:

```css
.right-panel-header {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 16px;
  border-bottom: 1px solid var(--border-color, rgba(255, 255, 255, 0.06));
}

.right-panel-header h3 {
  margin: 0;
  font-size: 0.95rem;
  font-weight: 600;
  color: var(--text-primary, #e2e8f0);
}
```

- [ ] **Step 3: Verify build**

Run: `cd desktop && npm run build`
Expected: Build succeeds

- [ ] **Step 4: Commit**

```bash
git add desktop/src/App.css
git commit -m "feat(desktop): add CSS for discovery cards and simplified right panel header"
```

---

## Verification

After all tasks are complete:

1. **Backend tests:** `cd daemon && cargo test` — all tests pass (including 5 new discovery store tests)
2. **Backend compilation:** `cd daemon && cargo build` — no errors
3. **Frontend compilation:** `cd desktop && npx tsc --noEmit` — no errors
4. **Frontend build:** `cd desktop && npm run build` — succeeds
5. **Manual test flow:**
   - Start daemon: `cd daemon && cargo run -- serve`
   - Simulate a discovery: `curl -X PUT localhost:8787/api/discoveries/100.64.1.5/accept` (after manually inserting a discovered peer)
   - List discoveries: `curl localhost:8787/api/discoveries`
   - Accept: `curl -X PUT localhost:8787/api/discoveries/100.64.1.5/accept`
   - Dismiss: `curl -X PUT localhost:8787/api/discoveries/100.64.1.6/dismiss`
6. **Desktop manual test:**
   - `cd desktop && npm run tauri dev`
   - Sidebar shows "Connections" instead of "Hosts"
   - No "Host a Connection" button
   - Settings page shows Permissions section
   - Right panel shows only Approvals (no tab bar)
