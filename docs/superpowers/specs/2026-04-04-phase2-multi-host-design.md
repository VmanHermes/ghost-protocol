# Phase 2: Multi-Host Connections — Design Spec

**Goal:** Allow Ghost Protocol to connect to multiple remote daemon hosts simultaneously, with per-host data isolation, lazy data loading, and a sidebar host management UI. The active host context is determined implicitly by which terminal tab is selected.

**Scope:** Frontend-only. No Rust/backend changes required.

---

## 1. Data Model & Persistence

### New Types

```typescript
type SavedHost = {
  id: string;        // crypto.randomUUID()
  name: string;      // user-facing label, e.g. "Desktop", "Server"
  url: string;       // e.g. "http://192.168.1.50:8787"
};

type HostConnectionState = "idle" | "connecting" | "connected" | "error";

type HostConnection = {
  host: SavedHost;
  state: HostConnectionState;
  message: string;   // "Connected" / "Unreachable" / error detail
  // Lazy-loaded data (null until first terminal tab activated for this host)
  sessions: TerminalSession[] | null;
  runs: RunRecord[] | null;
  conversations: Conversation[] | null;
  systemStatus: SystemStatus | null;
};
```

### localStorage

- **Key:** `ghost-protocol.hosts` — `SavedHost[]` JSON array
- **Old key:** `ghost-protocol.baseUrl` — removed after migration

### Migration Logic

On app launch:
1. If `ghost-protocol.hosts` exists → parse and use
2. Else if `ghost-protocol.baseUrl` exists → migrate to `[{ id: uuid(), name: "Default", url: baseUrl }]`, save as `ghost-protocol.hosts`, remove old key
3. Else → empty `[]` (fresh install, local-only mode)

```typescript
function loadHosts(): SavedHost[] {
  const stored = localStorage.getItem("ghost-protocol.hosts");
  if (stored) return JSON.parse(stored);

  const legacy = localStorage.getItem("ghost-protocol.baseUrl");
  if (legacy) {
    const hosts = [{ id: crypto.randomUUID(), name: "Default", url: legacy }];
    localStorage.setItem("ghost-protocol.hosts", JSON.stringify(hosts));
    localStorage.removeItem("ghost-protocol.baseUrl");
    return hosts;
  }

  return [];
}
```

---

## 2. App.tsx State Refactor

### Removed State

```
baseUrl, draftBaseUrl, connectionState, connectionMessage,
terminalSessions, runs, conversations, systemStatus
```

These are replaced by per-host equivalents.

### New State

```typescript
// Persisted host list
const [hosts, setHosts] = useState<SavedHost[]>(loadHosts());

// Per-host runtime state (keyed by host.id)
const [connections, setConnections] = useState<Map<string, HostConnection>>(new Map());

// Active host derived from selected terminal tab
const activeHostId: string | null = useMemo(() => {
  // Look up activeTerminalSessionId across all host connections
  // If found in a host's sessions → return that hostId
  // If it's a local session → return null
}, [activeTerminalSessionId, connections, localSessions]);

// Convenience accessor
const activeConnection = activeHostId ? connections.get(activeHostId) ?? null : null;
```

### Unchanged State

- `localSessions` — still managed by Tauri PTY
- `mainView` — unchanged
- `activeTerminalSessionId` — still a single string
- `activeRunId`, `selectedConversationId` — unchanged

### Key Behavior Changes

- `initialize()` → `initializeHosts()`: health-checks all saved hosts in parallel
- Per-host data loading triggered lazily on first terminal tab activation
- `handleCreateRemoteSession(hostId, mode)` replaces `handleCreateSession()` — requires knowing which host
- Inspector and chat panels read from `activeConnection` instead of top-level state
- Each host gets its own conversation WebSocket when chat is used in that host's context

---

## 3. Host Connection Lifecycle

### On App Launch

1. Load `SavedHost[]` from localStorage
2. For each host, set `state: "connecting"` and fire `GET /health` in parallel
3. On success → `state: "connected"`, `message: "Connected"`
4. On failure → `state: "error"`, `message: error detail` (e.g. "Connection refused")
5. No session/run/conversation data loaded yet — all data fields remain `null`

### Lazy Data Loading (First Activation)

When a host's terminal tab is selected for the first time (i.e., `activeHostId` changes to a host whose data is `null`):

1. Load `sessions`, `runs`, `conversations`, `systemStatus` via parallel `api()` calls to that host's URL
2. Store results in that host's `HostConnection` entry in the `connections` map
3. Subsequent tab switches back to this host reuse cached data
4. Cached data is kept fresh via WebSocket events and periodic polling

### Health Polling

Every 30 seconds, re-check `GET /health` for all saved hosts:
- Updates sidebar status dots
- Does not touch loaded data
- If a previously-connected host goes unreachable, its state flips to `"error"` but cached data remains visible (stale)
- If an unreachable host comes back, state flips to `"connected"`

### Adding a Host

1. User clicks "+ Add host" in sidebar, fills inline form (name + URL)
2. Basic URL validation (must start with `http://` or `https://`)
3. Save to `SavedHost[]` in localStorage
4. Immediately health-check the new host
5. On success → appears as connected in sidebar, ready for terminal sessions

### Removing a Host

1. Small "x" button next to host in sidebar (visible on hover)
2. Terminates any active terminal sessions on that host
3. Removes from `SavedHost[]` in localStorage
4. Removes from `connections` map

---

## 4. Sidebar Changes

### Current Layout (bottom of sidebar)

```
● Connected to http://127.0.0.1:8787
```

### New Layout (replaces `.sidebar-connection`)

```
Hosts
  ● Desktop           connected
  ● Server            connected
  ○ Staging           unreachable
  [+] Add host

  ┌─────────────────────────┐
  │ Name: [_______________] │
  │ URL:  [_______________] │
  │ [Connect]    [Cancel]   │
  └─────────────────────────┘  ← inline form, toggled by "+ Add host"
```

### Props Change

```typescript
// Old
connectionState: "idle" | "connecting" | "connected" | "error";
connectionMessage: string;

// New
hosts: HostConnection[];
onAddHost: (name: string, url: string) => void;
onRemoveHost: (hostId: string) => void;
```

### Behavior

- Status dots use existing `.status-dot` CSS classes (green=connected, red=error, grey=idle/connecting)
- Host name truncated with ellipsis if too long
- Hover on a host row reveals a small "x" remove button
- "+ Add host" toggles the inline form open/closed
- Form validates URL format before allowing submit
- No click-to-select behavior on hosts — context follows terminal tab selection
- Empty state (no hosts): just shows "+ Add host" with helper text "Add a remote host to connect"

---

## 5. Terminal Tab Bar & Session Creation

### Tab Labels

Unchanged from Phase 1 format. Local tabs: `"Local · shell"`. Remote tabs: `"HostName · mode"` where `HostName` is looked up from `SavedHost.name` via `TerminalSource.hostId`.

Example with multiple hosts:
```
[Local · shell] [Desktop · rescue shell] [Server · agent] [+]
```

### "+" Button — Now a Dropdown Menu

Clicking "+" opens a dropdown menu grouped by source:

```
┌──────────────────────┐
│ Local shell           │
│ ─────────────────     │
│ Desktop               │
│   rescue shell        │
│   agent               │
│   project             │
│ ─────────────────     │
│ Server                │
│   rescue shell        │
│   agent               │
│   project             │
│ ─────────────────     │
│ Staging (unreachable) │  ← greyed out, not clickable
└──────────────────────┘
```

- "Local shell" always first
- Connected hosts listed with their session mode sub-options
- Unreachable hosts shown greyed out with "(unreachable)" label
- Clicking an option calls `handleCreateLocalSession()` or `handleCreateRemoteSession(hostId, mode)`
- Menu closes on selection or click-outside

### Tab Ordering

Local tabs first, then remote grouped by host (ordered by host list position), within each host ordered by creation time. Same rule as Phase 1, now across multiple hosts.

---

## 6. Inspector Panel & Chat Context

### Inspector

Switches data based on `activeHostId`:

- **Local tab active:** minimal info — local session count only, no runs/agents/approvals (no daemon)
- **Remote tab active:** that host's data — runs, system status, approvals, token usage. Same layout as today, scoped to the active host.

Header shows context: `"Desktop — Inspector"` or `"Local — Inspector"`

Props change:
```typescript
// Added
activeHostName: string | null;  // "Desktop", "Server", or null (local)

// Unchanged shape — App.tsx passes active host's data
systemStatus: SystemStatus | null;
terminalSessionCount: number;
runs: RunRecord[];
events: EventRecord[];
```

Minimal change to InspectorPanel internals — App.tsx feeds it the right host's data.

### Chat Panel

Same approach:
- Remote tab active → chat shows conversations from that host, WebSocket connects to that host's URL
- Local tab active → chat is empty/disabled (no daemon to talk to)
- Switching hosts tears down the previous host's conversation WebSocket (if any) and establishes a new one for the new host. The existing `useEffect` cleanup pattern in App.tsx handles this naturally since the WebSocket depends on `baseUrl` which now comes from the active host.

---

## 7. File Changes Summary

| File | Change |
|---|---|
| `types.ts` | Add `SavedHost`, `HostConnection`, `HostConnectionState` types |
| `api.ts` | Remove `defaultBaseUrl` export. Keep `api()`, `wsUrlFromHttp()`, `fmt()` unchanged. |
| `App.tsx` | Major refactor: multi-host state, `loadHosts()` migration, `initializeHosts()`, lazy loading, derived `activeHostId`, `handleCreateRemoteSession(hostId, mode)`, pass active host data to children |
| `Sidebar.tsx` | Replace `.sidebar-connection` with host list + inline add/remove form |
| `TerminalWorkspace.tsx` | "+" button becomes dropdown menu with host grouping, host name lookup for tab labels via `SavedHost` list |
| `InspectorPanel.tsx` | Accept `activeHostName` prop, display in header, handle null (local) gracefully |
| `App.css` | New styles: `.sidebar-hosts`, `.sidebar-host-row`, `.sidebar-host-remove`, `.sidebar-add-host-form`, `.terminal-add-menu`, `.terminal-add-menu-group` |

### No Changes

- `pty.rs`, `lib.rs` — no backend changes
- `useLocalTerminal.ts` — unchanged
- `useTerminalSocket.ts` — unchanged (already takes `baseUrl` param)

---

## 8. Design Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Data scoping | Everything per-host | Cleanest mental model; each host is its own world |
| Add-host UI | Inline sidebar form | Lightweight temporary UI; Phase 3 replaces with terminal-guided flow |
| Sidebar click behavior | Status-only, no selection | Host context follows terminal tab; simplest approach |
| Connection strategy | Connect all + lazy-load data | Sidebar shows status immediately; data loaded only when needed |
