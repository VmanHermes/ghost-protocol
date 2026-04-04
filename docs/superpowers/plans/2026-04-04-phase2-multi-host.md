# Phase 2: Multi-Host Connections Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow Ghost Protocol to connect to multiple remote daemon hosts simultaneously, with per-host data isolation, lazy data loading, and a sidebar host management UI.

**Architecture:** Replace the single `baseUrl` global with a `SavedHost[]` persisted in localStorage and a `Map<string, HostConnection>` runtime state. Each host gets independent health checks, lazy-loaded data, and session management. The active host is derived implicitly from which terminal tab is selected. Sidebar shows host list with connection status; "+" button becomes a dropdown menu grouped by source.

**Tech Stack:** React 19, TypeScript, localStorage, existing `api()` / `wsUrlFromHttp()` utilities

---

## File Structure

### Frontend (new files)

| File | Responsibility |
|---|---|
| `desktop/src/hosts.ts` | `SavedHost` persistence: `loadHosts()`, `saveHosts()`, `addHost()`, `removeHost()`, migration from legacy `baseUrl` |

### Frontend (modified files)

| File | Change |
|---|---|
| `desktop/src/types.ts` | Add `SavedHost`, `HostConnectionState`, `HostConnection` types |
| `desktop/src/api.ts` | Remove `defaultBaseUrl` export |
| `desktop/src/App.tsx` | Major refactor: multi-host state, `initializeHosts()`, lazy data loading, derived `activeHostId`, per-host handlers |
| `desktop/src/components/Sidebar.tsx` | Replace single connection status with host list + inline add-host form |
| `desktop/src/components/TerminalWorkspace.tsx` | "+" button becomes dropdown menu, host name lookup for remote tab labels |
| `desktop/src/components/InspectorPanel.tsx` | Accept `activeHostName` prop, show in header, handle local-only mode |
| `desktop/src/components/LogViewer.tsx` | Accept `baseUrl | null`, show empty state when null |
| `desktop/src/App.css` | New styles for host list, add-host form, "+" dropdown menu |

---

## Task 1: Add Multi-Host Types

**Files:**
- Modify: `desktop/src/types.ts`

- [ ] **Step 1: Add types at end of file**

Add after the existing `TerminalTab` type:

```typescript
// --- Multi-host types (Phase 2) ---

export type SavedHost = {
  id: string;
  name: string;
  url: string;
};

export type HostConnectionState = "idle" | "connecting" | "connected" | "error";

export type HostConnection = {
  host: SavedHost;
  state: HostConnectionState;
  message: string;
  sessions: TerminalSession[] | null;
  runs: RunRecord[] | null;
  conversations: Conversation[] | null;
  systemStatus: SystemStatus | null;
};
```

- [ ] **Step 2: Verify it compiles**

Run:
```bash
cd desktop && npx tsc --noEmit
```
Expected: clean output, no errors.

- [ ] **Step 3: Commit**

```bash
git add desktop/src/types.ts
git commit -m "feat: add multi-host types for Phase 2"
```

---

## Task 2: Create Host Persistence Module

**Files:**
- Create: `desktop/src/hosts.ts`
- Modify: `desktop/src/api.ts`

- [ ] **Step 1: Create `hosts.ts` with load/save/migration logic**

```typescript
import type { SavedHost } from "./types";

const STORAGE_KEY = "ghost-protocol.hosts";
const LEGACY_KEY = "ghost-protocol.baseUrl";

export function loadHosts(): SavedHost[] {
  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored) {
    try {
      return JSON.parse(stored) as SavedHost[];
    } catch {
      return [];
    }
  }

  const legacy = localStorage.getItem(LEGACY_KEY);
  if (legacy) {
    const hosts: SavedHost[] = [{ id: crypto.randomUUID(), name: "Default", url: legacy }];
    localStorage.setItem(STORAGE_KEY, JSON.stringify(hosts));
    localStorage.removeItem(LEGACY_KEY);
    return hosts;
  }

  return [];
}

export function saveHosts(hosts: SavedHost[]): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(hosts));
}

export function addHost(hosts: SavedHost[], name: string, url: string): SavedHost[] {
  const next = [...hosts, { id: crypto.randomUUID(), name, url }];
  saveHosts(next);
  return next;
}

export function removeHost(hosts: SavedHost[], hostId: string): SavedHost[] {
  const next = hosts.filter((h) => h.id !== hostId);
  saveHosts(next);
  return next;
}
```

- [ ] **Step 2: Remove `defaultBaseUrl` from `api.ts`**

Replace the first line of `desktop/src/api.ts`:

```typescript
// Remove: export const defaultBaseUrl = localStorage.getItem("ghost-protocol.baseUrl") ?? "http://127.0.0.1:8787";
// The file should now start with the wsUrlFromHttp function.
```

The full `api.ts` after this change:

```typescript
export function wsUrlFromHttp(baseUrl: string) {
  if (baseUrl.startsWith("https://")) return baseUrl.replace("https://", "wss://") + "/ws";
  if (baseUrl.startsWith("http://")) return baseUrl.replace("http://", "ws://") + "/ws";
  return `ws://${baseUrl}/ws`;
}

export function fmt(ts?: string | null) {
  if (!ts) return "—";
  try {
    return new Date(ts).toLocaleString();
  } catch {
    return ts;
  }
}

export async function api<T>(baseUrl: string, path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${baseUrl}${path}`, {
    headers: { "Content-Type": "application/json", ...(init?.headers ?? {}) },
    ...init,
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(text || `Request failed: ${res.status}`);
  }
  return res.json() as Promise<T>;
}
```

- [ ] **Step 3: Verify it compiles**

Run:
```bash
cd desktop && npx tsc --noEmit
```

This will fail because `App.tsx` still imports `defaultBaseUrl`. That's expected — we fix it in Task 5.

- [ ] **Step 4: Commit**

```bash
git add desktop/src/hosts.ts desktop/src/api.ts
git commit -m "feat: add host persistence module with migration from legacy baseUrl"
```

---

## Task 3: Update Sidebar for Multi-Host

**Files:**
- Modify: `desktop/src/components/Sidebar.tsx`

- [ ] **Step 1: Rewrite Sidebar.tsx**

Replace the full contents of `desktop/src/components/Sidebar.tsx`:

```typescript
import { ReactNode, useState } from "react";
import type { HostConnection, MainView } from "../types";

type Props = {
  hosts: HostConnection[];
  mainView: MainView;
  onChangeView: (view: MainView) => void;
  onAddHost: (name: string, url: string) => void;
  onRemoveHost: (hostId: string) => void;
};

const NAV_ITEMS: { view: MainView; label: string; icon: ReactNode }[] = [
  {
    view: "terminal",
    label: "Terminal",
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <polyline points="4 17 10 11 4 5" />
        <line x1="12" y1="19" x2="20" y2="19" />
      </svg>
    ),
  },
  {
    view: "chat",
    label: "Chat",
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
      </svg>
    ),
  },
  {
    view: "logs",
    label: "Logs",
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
        <polyline points="14 2 14 8 20 8" />
        <line x1="16" y1="13" x2="8" y2="13" />
        <line x1="16" y1="17" x2="8" y2="17" />
        <polyline points="10 9 9 9 8 9" />
      </svg>
    ),
  },
  {
    view: "settings",
    label: "Settings",
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <circle cx="12" cy="12" r="3" />
        <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
      </svg>
    ),
  },
];

export function Sidebar({
  hosts,
  mainView,
  onChangeView,
  onAddHost,
  onRemoveHost,
}: Props) {
  const [showAddForm, setShowAddForm] = useState(false);
  const [draftName, setDraftName] = useState("");
  const [draftUrl, setDraftUrl] = useState("http://");

  const handleSubmitHost = () => {
    const name = draftName.trim();
    const url = draftUrl.trim();
    if (!name || !url) return;
    if (!url.startsWith("http://") && !url.startsWith("https://")) return;
    onAddHost(name, url);
    setDraftName("");
    setDraftUrl("http://");
    setShowAddForm(false);
  };

  return (
    <aside className="sidebar">
      <div className="sidebar-brand">
        <div className="sidebar-brand-icon">
          <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M12 2L2 7l10 5 10-5-10-5z" />
            <path d="M2 17l10 5 10-5" />
            <path d="M2 12l10 5 10-5" />
          </svg>
        </div>
        <div>
          <div className="sidebar-brand-title">Ghost Protocol</div>
          <div className="sidebar-brand-subtitle">Developer Console</div>
        </div>
      </div>

      <nav className="sidebar-nav">
        {NAV_ITEMS.map((item) => (
          <button
            key={item.view}
            className={`sidebar-nav-item ${mainView === item.view ? "active" : ""}`}
            onClick={() => onChangeView(item.view)}
          >
            {item.icon}
            <span>{item.label}</span>
          </button>
        ))}
      </nav>

      <div className="sidebar-spacer" />

      <div className="sidebar-hosts">
        <div className="sidebar-hosts-header">Hosts</div>
        {hosts.length === 0 && !showAddForm && (
          <div className="sidebar-hosts-empty">Add a remote host to connect</div>
        )}
        {hosts.map((conn) => (
          <div key={conn.host.id} className="sidebar-host-row">
            <span className={`status-dot ${conn.state}`} />
            <span className="sidebar-host-name">{conn.host.name}</span>
            <span className="sidebar-host-status">
              {conn.state === "connected" ? "connected" : conn.state === "connecting" ? "connecting" : "unreachable"}
            </span>
            <button
              className="sidebar-host-remove"
              onClick={() => onRemoveHost(conn.host.id)}
              title="Remove host"
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </button>
          </div>
        ))}

        {showAddForm ? (
          <div className="sidebar-add-host-form">
            <input
              className="sidebar-add-host-input"
              placeholder="Host name"
              value={draftName}
              onChange={(e) => setDraftName(e.currentTarget.value)}
              autoFocus
            />
            <input
              className="sidebar-add-host-input"
              placeholder="http://host:port"
              value={draftUrl}
              onChange={(e) => setDraftUrl(e.currentTarget.value)}
              onKeyDown={(e) => { if (e.key === "Enter") handleSubmitHost(); }}
            />
            <div className="sidebar-add-host-actions">
              <button className="btn-primary sidebar-add-host-btn" onClick={handleSubmitHost}>Connect</button>
              <button className="btn-secondary sidebar-add-host-btn" onClick={() => setShowAddForm(false)}>Cancel</button>
            </div>
          </div>
        ) : (
          <button className="sidebar-add-host-toggle" onClick={() => setShowAddForm(true)}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <line x1="12" y1="5" x2="12" y2="19" />
              <line x1="5" y1="12" x2="19" y2="12" />
            </svg>
            Add host
          </button>
        )}
      </div>

      <div className="sidebar-user">
        <div className="sidebar-user-avatar">D</div>
        <div>
          <div className="sidebar-user-name">Developer</div>
          <div className="sidebar-user-email">dev@ghost-protocol</div>
        </div>
      </div>
    </aside>
  );
}
```

- [ ] **Step 2: Commit**

```bash
git add desktop/src/components/Sidebar.tsx
git commit -m "feat: update Sidebar for multi-host list with add/remove UI"
```

---

## Task 4: Update InspectorPanel for Host Context

**Files:**
- Modify: `desktop/src/components/InspectorPanel.tsx`

- [ ] **Step 1: Add `activeHostName` prop**

In `InspectorPanel.tsx`, add `activeHostName` to the `Props` type:

```typescript
type Props = {
  activeHostName: string | null;
  activeRun: RunRecord | null;
  runs: RunRecord[];
  runDetail: RunDetail | null;
  systemStatus: SystemStatus | null;
  terminalSessionCount: number;
  events: EventEnvelope[];
  activeRunId: string | null;
  onSelectRun: (runId: string) => void;
  onResolveApproval: (approvalId: string, status: "approved" | "rejected") => void;
};
```

- [ ] **Step 2: Destructure the new prop and update the header**

Add `activeHostName` to the destructure:

```typescript
export function InspectorPanel({
  activeHostName,
  activeRun: _activeRun,
  runs,
  // ... rest unchanged
```

Replace the header section:

```typescript
      <div className="observability-header">
        <h2>{activeHostName ? `${activeHostName} — Observability` : "Local — Observability"}</h2>
        <p className="muted">Real-time metrics & alerts</p>
      </div>
```

- [ ] **Step 3: Commit**

```bash
git add desktop/src/components/InspectorPanel.tsx
git commit -m "feat: add activeHostName context to InspectorPanel header"
```

---

## Task 5: Refactor App.tsx for Multi-Host State

**Files:**
- Modify: `desktop/src/App.tsx`

This is the largest task. It replaces the single-host state model with per-host connections.

- [ ] **Step 1: Update imports**

Replace the top of `App.tsx`:

```typescript
import { FormEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { api, wsUrlFromHttp } from "./api";
import { loadHosts, addHost as persistAddHost, removeHost as persistRemoveHost } from "./hosts";
import { appLog } from "./log";
import type {
  Conversation,
  ConversationDetail,
  EventEnvelope,
  HostConnection,
  LocalTerminalSession,
  Message,
  RunDetail,
  RunRecord,
  SavedHost,
  SystemStatus,
  TerminalSession,
} from "./types";
import { Sidebar } from "./components/Sidebar";
import { ChatView } from "./components/ChatView";
import { InspectorPanel } from "./components/InspectorPanel";
import { TerminalWorkspace } from "./components/TerminalWorkspace";
import { LogViewer } from "./components/LogViewer";
import "./App.css";
```

- [ ] **Step 2: Replace state declarations**

Remove the old single-host state block (lines 27-44 of current file). Replace with:

```typescript
type MainView = "chat" | "terminal" | "logs" | "settings";

function App() {
  // --- Multi-host state ---
  const [hosts, setHosts] = useState<SavedHost[]>(() => loadHosts());
  const [connections, setConnections] = useState<Map<string, HostConnection>>(new Map());

  // --- Shared state (unchanged) ---
  const [mainView, setMainView] = useState<MainView>("terminal");
  const [activeTerminalSessionId, setActiveTerminalSessionId] = useState<string | null>(null);
  const [localSessions, setLocalSessions] = useState<LocalTerminalSession[]>([]);
  const [actionError, setActionError] = useState("");

  // --- Per-active-host UI state ---
  const [selectedConversationId, setSelectedConversationId] = useState<string | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [messageInput, setMessageInput] = useState("");
  const [events, setEvents] = useState<EventEnvelope[]>([]);
  const [activeRunId, setActiveRunId] = useState<string | null>(null);
  const [runDetail, setRunDetail] = useState<RunDetail | null>(null);

  const activeRunIdRef = useRef(activeRunId);
  useEffect(() => { activeRunIdRef.current = activeRunId; }, [activeRunId]);
```

- [ ] **Step 3: Add derived state for active host**

```typescript
  // Derive active host from selected terminal tab
  const activeHostId: string | null = useMemo(() => {
    if (!activeTerminalSessionId) return null;
    if (localSessions.some((s) => s.id === activeTerminalSessionId)) return null;
    for (const [hostId, conn] of connections) {
      if (conn.sessions?.some((s) => s.id === activeTerminalSessionId)) return hostId;
    }
    return null;
  }, [activeTerminalSessionId, connections, localSessions]);

  const activeConnection = activeHostId ? connections.get(activeHostId) ?? null : null;
  const activeHost = activeHostId ? hosts.find((h) => h.id === activeHostId) ?? null : null;

  // Convenience accessors for active host data (used by child components)
  const activeHostUrl = activeHost?.url ?? null;
  const activeRuns = activeConnection?.runs ?? [];
  const activeSystemStatus = activeConnection?.systemStatus ?? null;
  const activeConversations = activeConnection?.conversations ?? [];
  const activeTerminalSessions = activeConnection?.sessions ?? [];

  const selectedConversation = useMemo(
    () => activeConversations.find((item) => item.id === selectedConversationId) ?? null,
    [activeConversations, selectedConversationId],
  );
  const activeRun = useMemo(
    () => activeRuns.find((item) => item.id === activeRunId) ?? null,
    [activeRuns, activeRunId],
  );
```

- [ ] **Step 4: Add connection helper to update a single host's connection state**

```typescript
  // Helper: update a single host's connection in the map
  const updateConnection = useCallback((hostId: string, update: Partial<HostConnection>) => {
    setConnections((prev) => {
      const next = new Map(prev);
      const existing = next.get(hostId);
      if (existing) {
        next.set(hostId, { ...existing, ...update });
      }
      return next;
    });
  }, []);
```

- [ ] **Step 5: Add host initialization and health polling**

```typescript
  // Health-check a single host
  const checkHostHealth = useCallback(async (host: SavedHost) => {
    try {
      const health = await api<{ ok: boolean; telegramEnabled?: boolean }>(host.url, "/health");
      const msg = health.telegramEnabled ? "Connected · Telegram on" : "Connected";
      updateConnection(host.id, { state: "connected", message: msg });
    } catch (error) {
      const msg = error instanceof Error ? error.message : "Connection failed";
      updateConnection(host.id, { state: "error", message: msg });
    }
  }, [updateConnection]);

  // Initialize all hosts on mount: create connection entries + health check
  const initializeHosts = useCallback((hostList: SavedHost[]) => {
    const initial = new Map<string, HostConnection>();
    for (const host of hostList) {
      initial.set(host.id, {
        host,
        state: "connecting",
        message: "Connecting...",
        sessions: null,
        runs: null,
        conversations: null,
        systemStatus: null,
      });
    }
    setConnections(initial);
    for (const host of hostList) {
      void checkHostHealth(host);
    }
  }, [checkHostHealth]);

  // Load full data for a host (lazy — called on first tab activation)
  const loadHostData = useCallback(async (hostId: string, url: string) => {
    try {
      const [sessions, runs, conversations, systemStatus] = await Promise.all([
        api<TerminalSession[]>(url, "/api/terminal/sessions"),
        api<RunRecord[]>(url, "/api/runs"),
        api<Conversation[]>(url, "/api/conversations"),
        api<SystemStatus>(url, "/api/system/status"),
      ]);
      updateConnection(hostId, { sessions, runs, conversations, systemStatus });
    } catch (error) {
      appLog.error("app", `Failed to load data for host ${hostId}: ${error instanceof Error ? error.message : String(error)}`);
    }
  }, [updateConnection]);
```

- [ ] **Step 6: Add effects for initialization, lazy loading, and health polling**

```typescript
  // Initialize hosts on mount
  useEffect(() => {
    initializeHosts(hosts);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Auto-spawn a local terminal on first mount
  const localSpawnedRef = useRef(false);
  useEffect(() => {
    if (localSpawnedRef.current) return;
    localSpawnedRef.current = true;
    void handleCreateLocalSession();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Lazy-load host data when a host becomes active for the first time
  useEffect(() => {
    if (!activeHostId || !activeHost) return;
    const conn = connections.get(activeHostId);
    if (!conn || conn.state !== "connected" || conn.sessions !== null) return;
    void loadHostData(activeHostId, activeHost.url);
  }, [activeHostId, activeHost, connections, loadHostData]);

  // Health polling every 30s
  useEffect(() => {
    if (hosts.length === 0) return;
    const interval = setInterval(() => {
      for (const host of hosts) {
        void checkHostHealth(host);
      }
    }, 30000);
    return () => clearInterval(interval);
  }, [hosts, checkHostHealth]);

  // Reset per-host UI state when active host changes
  useEffect(() => {
    setSelectedConversationId(null);
    setMessages([]);
    setActiveRunId(null);
    setRunDetail(null);
    setEvents([]);
    if (activeConnection?.conversations?.length) {
      setSelectedConversationId(activeConnection.conversations[0].id);
    }
    if (activeConnection?.runs?.length) {
      setActiveRunId(activeConnection.runs[0].id);
    }
  }, [activeHostId]); // eslint-disable-line react-hooks/exhaustive-deps
```

- [ ] **Step 7: Add conversation loading and WebSocket effect (scoped to active host)**

```typescript
  // Load conversation detail
  useEffect(() => {
    if (!selectedConversationId || !activeHostUrl) return;
    const url = activeHostUrl;
    api<ConversationDetail>(url, `/api/conversations/${selectedConversationId}`)
      .then((data) => {
        setMessages(data.messages);
      })
      .catch((error) => {
        appLog.error("app", `Failed to load conversation: ${error instanceof Error ? error.message : String(error)}`);
      });
  }, [selectedConversationId, activeHostUrl]);

  // Load run detail
  useEffect(() => {
    if (!activeRunId || !activeHostUrl) return;
    const url = activeHostUrl;
    api<RunDetail>(url, `/api/runs/${activeRunId}`)
      .then((data) => {
        setRunDetail(data);
      })
      .catch((error) => {
        appLog.error("app", `Failed to load run: ${error instanceof Error ? error.message : String(error)}`);
      });
  }, [activeRunId, activeHostUrl]);

  // Conversation WebSocket — scoped to active host
  useEffect(() => {
    if (!selectedConversationId || !activeHostUrl) return;

    let refreshTimer: ReturnType<typeof setTimeout> | null = null;
    const ws = new WebSocket(wsUrlFromHttp(activeHostUrl));

    ws.onopen = () => {
      appLog.info("conv-ws", "Connected");
      ws.send(JSON.stringify({ op: "subscribe", conversationId: selectedConversationId, afterSeq: 0 }));
    };

    ws.onmessage = (event) => {
      const data = JSON.parse(event.data);
      if (data.op === "event") {
        const envelope = data.event as EventEnvelope;
        setEvents((current) => [...current.slice(-299), envelope]);
        if (envelope.type === "message_created") {
          const payload = envelope.payload as { messageId?: string; role?: "user" | "assistant" | "system"; content?: string };
          if (
            typeof payload.messageId === "string"
            && typeof payload.role === "string"
            && typeof payload.content === "string"
            && envelope.conversationId === selectedConversationId
          ) {
            const nextMessage: Message = {
              id: payload.messageId,
              conversationId: selectedConversationId,
              role: payload.role,
              content: payload.content,
              createdAt: envelope.ts,
              runId: envelope.runId,
            };
            setMessages((current) => current.some((item) => item.id === nextMessage.id) ? current : [...current, nextMessage]);
          }
        }
        if (!refreshTimer && activeHostId) {
          const hostId = activeHostId;
          const hostUrl = activeHostUrl;
          refreshTimer = setTimeout(() => {
            refreshTimer = null;
            void Promise.all([
              api<RunRecord[]>(hostUrl, "/api/runs"),
              api<SystemStatus>(hostUrl, "/api/system/status"),
            ]).then(([runs, systemStatus]) => {
              updateConnection(hostId, { runs, systemStatus });
            });
          }, 500);
        }
        const currentRunId = activeRunIdRef.current;
        if (envelope.runId && currentRunId === envelope.runId && activeHostUrl) {
          api<RunDetail>(activeHostUrl, `/api/runs/${envelope.runId}`)
            .then((data) => setRunDetail(data))
            .catch(() => {});
        }
      } else if (data.op === "error") {
        appLog.error("conv-ws", `Server error: ${data.message ?? "unknown"}`);
      }
    };

    ws.onerror = () => {
      appLog.error("conv-ws", "WebSocket error event");
    };
    ws.onclose = (event) => {
      appLog.warn("conv-ws", `Disconnected: code=${event.code} reason=${event.reason || "none"}`);
    };

    return () => {
      if (refreshTimer) clearTimeout(refreshTimer);
      ws.close();
    };
  }, [activeHostUrl, selectedConversationId]); // eslint-disable-line react-hooks/exhaustive-deps
```

- [ ] **Step 8: Add action handlers**

```typescript
  // --- Action handlers ---

  const handleSendMessage = useCallback(async (event: FormEvent) => {
    event.preventDefault();
    if (!selectedConversationId || !messageInput.trim() || !activeHostUrl) return;
    const content = messageInput.trim();
    const url = activeHostUrl;
    setMessageInput("");
    setActionError("");
    await api(url, `/api/conversations/${selectedConversationId}/messages`, {
      method: "POST",
      body: JSON.stringify({ content }),
    });
    const run = await api<{ runId: string }>(url, "/api/runs", {
      method: "POST",
      body: JSON.stringify({ conversationId: selectedConversationId, content }),
    });
    setActiveRunId(run.runId);
    if (activeHostId) {
      const [runs, systemStatus] = await Promise.all([
        api<RunRecord[]>(url, "/api/runs"),
        api<SystemStatus>(url, "/api/system/status"),
      ]);
      updateConnection(activeHostId, { runs, systemStatus });
    }
  }, [activeHostUrl, activeHostId, selectedConversationId, messageInput, updateConnection]);

  const handleRetryRun = useCallback(async () => {
    if (!activeRunId || !activeHostUrl) return;
    try {
      const data = await api<{ runId: string }>(activeHostUrl, `/api/runs/${activeRunId}/retry`, { method: "POST" });
      setActiveRunId(data.runId);
    } catch (error) {
      setActionError(error instanceof Error ? error.message : "Retry failed");
    }
  }, [activeHostUrl, activeRunId]);

  const handleCancelRun = useCallback(async () => {
    if (!activeRunId || !activeHostUrl) return;
    try {
      await api(activeHostUrl, `/api/runs/${activeRunId}/cancel`, { method: "POST" });
    } catch (error) {
      setActionError(error instanceof Error ? error.message : "Cancel failed");
    }
  }, [activeHostUrl, activeRunId]);

  const handleCreateRemoteSession = useCallback(async (hostId: string, mode: "agent" | "rescue_shell" | "project") => {
    const host = hosts.find((h) => h.id === hostId);
    if (!host) return;
    try {
      const session = await api<TerminalSession>(host.url, "/api/terminal/sessions", {
        method: "POST",
        body: JSON.stringify({ mode }),
      });
      // Update that host's session list
      updateConnection(hostId, {
        sessions: [...(connections.get(hostId)?.sessions ?? []), session],
      });
      setActiveTerminalSessionId(session.id);
      setMainView("terminal");
    } catch (error) {
      setActionError(error instanceof Error ? error.message : "Create session failed");
    }
  }, [hosts, connections, updateConnection]);

  const handleRemoteSessionStatusChange = useCallback((session: TerminalSession) => {
    if (!activeHostId || !activeHostUrl) return;
    const hostId = activeHostId;
    const url = activeHostUrl;
    void api<TerminalSession[]>(url, "/api/terminal/sessions").then((sessions) => {
      updateConnection(hostId, { sessions });
    });
  }, [activeHostId, activeHostUrl, updateConnection]);

  const handleKillRemoteSession = useCallback(async (sessionId: string) => {
    if (!activeHostId || !activeHostUrl) return;
    const hostId = activeHostId;
    const url = activeHostUrl;
    try {
      await api(url, `/api/terminal/sessions/${sessionId}/terminate`, { method: "POST" });
      const sessions = await api<TerminalSession[]>(url, "/api/terminal/sessions");
      updateConnection(hostId, { sessions });
      if (activeTerminalSessionId === sessionId) {
        const nextActive = sessions.find((s) => s.id !== sessionId && (s.status === "created" || s.status === "running"));
        setActiveTerminalSessionId(nextActive?.id ?? null);
      }
    } catch (error) {
      setActionError(error instanceof Error ? error.message : "Terminate session failed");
    }
  }, [activeHostId, activeHostUrl, activeTerminalSessionId, updateConnection]);

  const handleCreateLocalSession = useCallback(async () => {
    try {
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

  const handleLocalSessionStatusChange = useCallback((session: LocalTerminalSession) => {
    setLocalSessions((prev) =>
      prev.map((s) => (s.id === session.id ? session : s)),
    );
  }, []);

  const handleKillLocalSession = useCallback(async (sessionId: string) => {
    const existing = localSessions.find((s) => s.id === sessionId);
    if (!existing || existing.status !== "running") return;
    try {
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

  const handleResolveApproval = useCallback(async (approvalId: string, status: "approved" | "rejected") => {
    if (!activeHostUrl || !activeHostId) return;
    try {
      await api(activeHostUrl, `/api/approvals/${approvalId}/resolve`, {
        method: "POST",
        body: JSON.stringify({ status, resolvedBy: "ghost-protocol-app" }),
      });
      const systemStatus = await api<SystemStatus>(activeHostUrl, "/api/system/status");
      updateConnection(activeHostId, { systemStatus });
    } catch (error) {
      setActionError(error instanceof Error ? error.message : "Approval resolution failed");
    }
  }, [activeHostUrl, activeHostId, updateConnection]);

  // Host management handlers
  const handleAddHost = useCallback((name: string, url: string) => {
    const updated = persistAddHost(hosts, name, url);
    setHosts(updated);
    const newHost = updated[updated.length - 1];
    setConnections((prev) => {
      const next = new Map(prev);
      next.set(newHost.id, {
        host: newHost,
        state: "connecting",
        message: "Connecting...",
        sessions: null,
        runs: null,
        conversations: null,
        systemStatus: null,
      });
      return next;
    });
    void checkHostHealth(newHost);
  }, [hosts, checkHostHealth]);

  const handleRemoveHost = useCallback((hostId: string) => {
    const updated = persistRemoveHost(hosts, hostId);
    setHosts(updated);
    setConnections((prev) => {
      const next = new Map(prev);
      next.delete(hostId);
      return next;
    });
    // If active session was on this host, clear it
    const conn = connections.get(hostId);
    if (conn?.sessions?.some((s) => s.id === activeTerminalSessionId)) {
      setActiveTerminalSessionId(null);
    }
  }, [hosts, connections, activeTerminalSessionId]);
```

- [ ] **Step 9: Update the render/return JSX**

```typescript
  // --- Render ---

  // Build connections array for Sidebar
  const hostConnections = useMemo(
    () => hosts.map((h) => connections.get(h.id)).filter((c): c is HostConnection => c != null),
    [hosts, connections],
  );

  // Gather all remote sessions across all hosts for TerminalWorkspace
  const allRemoteSessions = useMemo(() => {
    const result: Array<{ hostId: string; hostName: string; session: TerminalSession }> = [];
    for (const host of hosts) {
      const conn = connections.get(host.id);
      if (!conn?.sessions) continue;
      for (const session of conn.sessions) {
        result.push({ hostId: host.id, hostName: host.name, session });
      }
    }
    return result;
  }, [hosts, connections]);

  return (
    <main className="shell">
      <Sidebar
        hosts={hostConnections}
        mainView={mainView}
        onChangeView={setMainView}
        onAddHost={handleAddHost}
        onRemoveHost={handleRemoveHost}
      />

      <section className="main-panel">
        <div style={{ display: mainView === "chat" ? "flex" : "none", flexDirection: "column", flex: 1, minHeight: 0 }}>
          <div className="terminal-header">
            <div>
              <h2>{selectedConversation?.title || "Chat"}</h2>
              <p className="muted">{activeHost ? `${activeHost.name} — Conversation` : "Select a remote host to chat"}</p>
            </div>
          </div>
          {activeHostUrl ? (
            <ChatView
              messages={messages}
              messageInput={messageInput}
              selectedConversationId={selectedConversationId}
              activeRunId={activeRunId}
              actionError={actionError}
              onChangeMessageInput={setMessageInput}
              onSendMessage={(e) => void handleSendMessage(e)}
              onRetryRun={() => void handleRetryRun()}
              onCancelRun={() => void handleCancelRun()}
            />
          ) : (
            <div className="empty-state" style={{ padding: "2rem" }}>
              Select a remote terminal tab to access chat
            </div>
          )}
        </div>
        <div style={{ display: mainView === "terminal" ? "flex" : "none", flexDirection: "column", flex: 1, minHeight: 0 }}>
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
          />
        </div>
        <div style={{ display: mainView === "logs" ? "flex" : "none", flexDirection: "column", flex: 1, minHeight: 0 }}>
          <LogViewer baseUrl={activeHostUrl} />
        </div>
        <div style={{ display: mainView === "settings" ? "flex" : "none", flexDirection: "column", flex: 1, minHeight: 0 }}>
          <div className="settings-view">
            <div className="settings-header">
              <h2>Settings</h2>
              <p className="muted">Host connections and configuration</p>
            </div>
            <div className="settings-content">
              <div className="settings-section">
                <h3>Active Host: {activeHost?.name ?? "None (Local)"}</h3>
                {activeSystemStatus && (
                  <div className="settings-info-grid">
                    <div className="settings-info-item">
                      <span className="settings-info-label">Bind address</span>
                      <span className="settings-info-value">{activeSystemStatus.connection?.bindHost ?? "—"}:{activeSystemStatus.connection?.bindPort ?? "—"}</span>
                    </div>
                    <div className="settings-info-item">
                      <span className="settings-info-label">Telegram</span>
                      <span className="settings-info-value">{activeSystemStatus.connection?.telegramEnabled ? "Enabled" : "Disabled"}</span>
                    </div>
                    <div className="settings-info-item">
                      <span className="settings-info-label">Active agents</span>
                      <span className="settings-info-value">{activeSystemStatus.activeAgents?.length ?? 0}</span>
                    </div>
                    <div className="settings-info-item">
                      <span className="settings-info-label">Terminal sessions</span>
                      <span className="settings-info-value">{activeTerminalSessions.length} total</span>
                    </div>
                    <div className="settings-info-item">
                      <span className="settings-info-label">Allowed CIDRs</span>
                      <span className="settings-info-value">{activeSystemStatus.connection?.allowedCidrs?.join(", ") ?? "—"}</span>
                    </div>
                  </div>
                )}
                {!activeSystemStatus && (
                  <p className="muted">Select a remote terminal tab to view host settings</p>
                )}
              </div>
            </div>
          </div>
        </div>
      </section>

      <InspectorPanel
        activeHostName={activeHost?.name ?? null}
        activeRun={activeRun}
        runs={activeRuns}
        runDetail={runDetail}
        systemStatus={activeSystemStatus}
        terminalSessionCount={
          activeTerminalSessions.filter((s) => s.status === "created" || s.status === "running").length
          + localSessions.filter((s) => s.status === "running").length
        }
        events={events}
        activeRunId={activeRunId}
        onSelectRun={setActiveRunId}
        onResolveApproval={(id, status) => void handleResolveApproval(id, status)}
      />
    </main>
  );
}

export default App;
```

- [ ] **Step 10: Verify it compiles**

Run:
```bash
cd desktop && npx tsc --noEmit
```

This will fail because `TerminalWorkspace` and `LogViewer` have different prop signatures now. That's expected — we fix them in the next tasks.

- [ ] **Step 11: Commit**

```bash
git add desktop/src/App.tsx
git commit -m "feat: refactor App.tsx for multi-host state management"
```

---

## Task 6: Update TerminalWorkspace for Multi-Host

**Files:**
- Modify: `desktop/src/components/TerminalWorkspace.tsx`

- [ ] **Step 1: Update props and imports**

Replace the `Props` type and add `SavedHost` / `HostConnection` imports:

```typescript
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { useTerminalSocket } from "../hooks/useTerminalSocket";
import { useLocalTerminal } from "../hooks/useLocalTerminal";
import { fmt } from "../api";
import type { HostConnection, LocalTerminalSession, SavedHost, TerminalSession } from "../types";

type RemoteSessionEntry = {
  hostId: string;
  hostName: string;
  session: TerminalSession;
};

type Props = {
  hosts: SavedHost[];
  hostConnections: Map<string, HostConnection>;
  localSessions: LocalTerminalSession[];
  allRemoteSessions: RemoteSessionEntry[];
  activeSessionId: string | null;
  visible: boolean;
  onSelect: (sessionId: string) => void;
  onCreateRemoteSession: (hostId: string, mode: "agent" | "rescue_shell" | "project") => void;
  onCreateLocalSession: () => void;
  onRemoteSessionStatusChange: (session: TerminalSession) => void;
  onLocalSessionStatusChange: (session: LocalTerminalSession) => void;
  onKillRemoteSession: (sessionId: string) => void;
  onKillLocalSession: (sessionId: string) => void;
};
```

- [ ] **Step 2: Update component signature and derived state**

Replace the destructure and first derived state block:

```typescript
export function TerminalWorkspace({
  hosts,
  hostConnections,
  localSessions,
  allRemoteSessions,
  activeSessionId,
  visible,
  onSelect,
  onCreateRemoteSession,
  onCreateLocalSession,
  onRemoteSessionStatusChange,
  onLocalSessionStatusChange,
  onKillRemoteSession,
  onKillLocalSession,
}: Props) {
  const terminalHostRef = useRef<HTMLDivElement | null>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const [error, setError] = useState("");
  const [showAddMenu, setShowAddMenu] = useState(false);
  const addMenuRef = useRef<HTMLDivElement | null>(null);

  const ACTIVE_STATUSES = new Set(["created", "running"]);

  const activeRemoteSessions = useMemo(
    () => allRemoteSessions.filter((e) => ACTIVE_STATUSES.has(e.session.status)),
    [allRemoteSessions],
  );
  const activeLocalSessions = useMemo(
    () => localSessions.filter((s) => s.status === "running"),
    [localSessions],
  );

  // Determine if the active session is local or remote
  const isLocalSession = useMemo(
    () => localSessions.some((s) => s.id === activeSessionId),
    [localSessions, activeSessionId],
  );

  // Find the active remote session's host for baseUrl
  const activeRemoteEntry = useMemo(
    () => allRemoteSessions.find((e) => e.session.id === activeSessionId) ?? null,
    [allRemoteSessions, activeSessionId],
  );
  const activeRemoteHost = activeRemoteEntry
    ? hosts.find((h) => h.id === activeRemoteEntry.hostId) ?? null
    : null;
  const activeBaseUrl = activeRemoteHost?.url ?? "http://localhost:8787";
```

- [ ] **Step 3: Update hook calls to use active host's baseUrl**

```typescript
  // Use both hooks — only the relevant one gets a sessionId
  const {
    sendInput: remoteSendInput,
    resize: remoteResize,
    terminate,
    sessionMeta: remoteSessionMeta,
  } = useTerminalSocket({
    baseUrl: activeBaseUrl,
    sessionId: isLocalSession ? null : activeSessionId,
    terminalRef,
    onSessionStatusChange: onRemoteSessionStatusChange,
    onError: setError,
  });

  const {
    sendInput: localSendInput,
    resize: localResize,
    kill: localKill,
    sessionMeta: localSessionMeta,
  } = useLocalTerminal({
    sessionId: isLocalSession ? activeSessionId : null,
    terminalRef,
    onSessionStatusChange: onLocalSessionStatusChange,
    onError: setError,
  });

  // Unified interface
  const sendInput = isLocalSession
    ? (d: string) => localSendInput(d)
    : (d: string) => remoteSendInput(d, false);
  const resize = useCallback(
    (cols: number, rows: number) => (isLocalSession ? localResize : remoteResize)(cols, rows),
    [isLocalSession, localResize, remoteResize],
  );
  const sessionMeta = isLocalSession ? localSessionMeta : remoteSessionMeta;

  const sendInputRef = useRef(sendInput);
  useEffect(() => { sendInputRef.current = sendInput; }, [sendInput]);

  const isTerminated = sessionMeta?.status === "terminated" || sessionMeta?.status === "exited" || sessionMeta?.status === "error";
```

- [ ] **Step 4: Update dropdown close handler**

```typescript
  // Close add menu on outside click
  useEffect(() => {
    if (!showAddMenu) return;
    function handleClick(e: MouseEvent) {
      if (addMenuRef.current && !addMenuRef.current.contains(e.target as Node)) {
        setShowAddMenu(false);
      }
    }
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [showAddMenu]);
```

- [ ] **Step 5: Keep terminal init/resize effects unchanged**

The `useEffect` blocks for xterm initialization (line 142-182), re-fit on visible (line 184-196), and focus on session change (line 198-207) stay exactly as they are.

- [ ] **Step 6: Update the render JSX**

Replace the full return JSX:

```typescript
  const activeSession = useMemo(
    () => allRemoteSessions.find((e) => e.session.id === activeSessionId)?.session ?? null,
    [allRemoteSessions, activeSessionId],
  );

  const SESSION_DOT_COLORS: Record<string, string> = {
    running: "#10b981",
    created: "#3b82f6",
    exited: "#6b7280",
    terminated: "#ef4444",
    error: "#ef4444",
  };

  return (
    <section className="terminal-workspace">
      {/* Header */}
      <div className="terminal-header">
        <div>
          <h2>Terminal</h2>
          <p className="muted">Agent execution environment</p>
        </div>
      </div>

      {/* Session tabs — local first, then remote grouped by host */}
      <div className="terminal-tabs">
        {/* Local sessions */}
        {activeLocalSessions.map((session) => (
          <button
            key={session.id}
            className={`terminal-tab ${session.id === activeSessionId ? "active" : ""}`}
            onClick={() => onSelect(session.id)}
          >
            <span className="terminal-tab-dot" style={{ background: "#10b981" }} />
            <span className="terminal-tab-source">Local</span>
            {" · shell"}
            <span
              className="terminal-tab-close"
              onClick={(e) => { e.stopPropagation(); onKillLocalSession(session.id); }}
              title="Kill session"
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </span>
          </button>
        ))}

        {/* Remote sessions grouped by host */}
        {activeRemoteSessions.map((entry) => (
          <button
            key={entry.session.id}
            className={`terminal-tab ${entry.session.id === activeSessionId ? "active" : ""}`}
            onClick={() => onSelect(entry.session.id)}
          >
            <span
              className="terminal-tab-dot"
              style={{ background: SESSION_DOT_COLORS[entry.session.status] ?? "#6b7280" }}
            />
            <span className="terminal-tab-source">{entry.hostName}</span>
            {" · "}{entry.session.name || entry.session.mode.replace("_", " ")}
            <span
              className="terminal-tab-close"
              onClick={(e) => { e.stopPropagation(); onKillRemoteSession(entry.session.id); }}
              title="Kill session"
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </span>
          </button>
        ))}

        {/* Add session dropdown */}
        <div className="terminal-add-wrapper" ref={addMenuRef}>
          <button
            className="terminal-tab-add"
            onClick={() => setShowAddMenu(!showAddMenu)}
            title="New session"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <line x1="12" y1="5" x2="12" y2="19" />
              <line x1="5" y1="12" x2="19" y2="12" />
            </svg>
          </button>
          {showAddMenu && (
            <div className="terminal-add-menu">
              <button
                className="terminal-add-menu-item"
                onClick={() => { onCreateLocalSession(); setShowAddMenu(false); }}
              >
                Local shell
              </button>
              {hosts.map((host) => {
                const conn = hostConnections.get(host.id);
                const isConnected = conn?.state === "connected";
                return (
                  <div key={host.id} className="terminal-add-menu-group">
                    <div className={`terminal-add-menu-host ${isConnected ? "" : "disabled"}`}>
                      <span className={`status-dot ${conn?.state ?? "idle"}`} />
                      {host.name}
                      {!isConnected && " (unreachable)"}
                    </div>
                    {isConnected && (
                      <>
                        <button
                          className="terminal-add-menu-item terminal-add-menu-sub"
                          onClick={() => { onCreateRemoteSession(host.id, "rescue_shell"); setShowAddMenu(false); }}
                        >
                          rescue shell
                        </button>
                        <button
                          className="terminal-add-menu-item terminal-add-menu-sub"
                          onClick={() => { onCreateRemoteSession(host.id, "agent"); setShowAddMenu(false); }}
                        >
                          agent
                        </button>
                        <button
                          className="terminal-add-menu-item terminal-add-menu-sub"
                          onClick={() => { onCreateRemoteSession(host.id, "project"); setShowAddMenu(false); }}
                        >
                          project
                        </button>
                      </>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>

      {error ? <div className="error-banner">{error}</div> : null}

      {/* Terminal area */}
      <div className="terminal-main">
        <div ref={terminalHostRef} className="terminal-host" />
      </div>

      {/* Status bar */}
      <div className="terminal-statusbar">
        <div className="terminal-statusbar-info">
          <span>
            {isLocalSession ? "Local" : activeRemoteEntry?.hostName ?? "Remote"}
            {" · "}
            {activeSession?.name || activeSession?.mode || (isLocalSession ? "shell" : "none")}
          </span>
          <span className="terminal-statusbar-sep" />
          <span>Uptime: {formatUptime(activeSession?.startedAt || localSessionMeta?.createdAt)}</span>
        </div>
        <div className="terminal-statusbar-actions">
          {!isTerminated && activeSessionId && (
            <button
              className="terminal-stop-btn"
              onClick={isLocalSession ? localKill : terminate}
              title="Terminate session"
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" />
              </svg>
            </button>
          )}
        </div>
      </div>
    </section>
  );
}
```

- [ ] **Step 7: Commit**

```bash
git add desktop/src/components/TerminalWorkspace.tsx
git commit -m "feat: update TerminalWorkspace for multi-host session dropdown"
```

---

## Task 7: Update LogViewer for Nullable baseUrl

**Files:**
- Modify: `desktop/src/components/LogViewer.tsx`

- [ ] **Step 1: Make baseUrl nullable and add empty state**

Change the `Props` type to accept `string | null`:

```typescript
type Props = {
  // ... other props stay the same
  baseUrl: string | null;
};
```

At the start of the component body, add an early return for null baseUrl:

```typescript
export function LogViewer({ baseUrl }: Props) {
  // ... existing state declarations

  useEffect(() => {
    if (!baseUrl) return;
    // ... rest of existing fetch logic, unchanged
  }, [baseUrl]);

  if (!baseUrl) {
    return (
      <div className="log-viewer">
        <div className="log-viewer-header">
          <h2>Logs</h2>
          <p className="muted">Select a remote terminal tab to view logs</p>
        </div>
      </div>
    );
  }

  // ... rest of existing render, unchanged
```

- [ ] **Step 2: Commit**

```bash
git add desktop/src/components/LogViewer.tsx
git commit -m "feat: handle null baseUrl in LogViewer for local-only mode"
```

---

## Task 8: Add CSS for Multi-Host UI

**Files:**
- Modify: `desktop/src/App.css`

- [ ] **Step 1: Replace `.sidebar-connection` styles with host list styles**

Find and replace the `.sidebar-connection` block in `App.css`:

```css
/* Remove:
.sidebar-connection {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 0.78rem;
  color: var(--text-secondary);
  margin-bottom: 12px;
}
*/

/* Add these in its place: */
.sidebar-hosts {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin-bottom: 12px;
}
.sidebar-hosts-header {
  font-size: 0.72rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--text-muted);
  margin-bottom: 4px;
}
.sidebar-hosts-empty {
  font-size: 0.78rem;
  color: var(--text-muted);
  padding: 4px 0;
}
.sidebar-host-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 0.78rem;
  padding: 4px 0;
  position: relative;
}
.sidebar-host-name {
  color: var(--text-primary);
  font-weight: 500;
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.sidebar-host-status {
  color: var(--text-muted);
  font-size: 0.72rem;
}
.sidebar-host-remove {
  display: none;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  padding: 0;
  border: none;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  border-radius: 4px;
  flex-shrink: 0;
}
.sidebar-host-row:hover .sidebar-host-remove {
  display: inline-flex;
}
.sidebar-host-remove:hover {
  background: rgba(239, 68, 68, 0.12);
  color: var(--accent-red);
}
.sidebar-add-host-toggle {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.78rem;
  color: var(--text-secondary);
  background: transparent;
  border: none;
  cursor: pointer;
  padding: 4px 0;
  margin-top: 4px;
}
.sidebar-add-host-toggle:hover {
  color: var(--text-primary);
}
.sidebar-add-host-form {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: 6px;
}
.sidebar-add-host-input {
  font-size: 0.78rem;
  padding: 6px 8px;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: var(--bg-input);
  color: var(--text-primary);
  outline: none;
}
.sidebar-add-host-input:focus {
  border-color: var(--accent-blue);
}
.sidebar-add-host-actions {
  display: flex;
  gap: 6px;
}
.sidebar-add-host-btn {
  flex: 1;
  font-size: 0.72rem;
  padding: 4px 8px;
}
```

- [ ] **Step 2: Add "+" dropdown menu styles**

Add after the `.terminal-tab-source` block:

```css
.terminal-add-wrapper {
  position: relative;
  display: inline-flex;
}
.terminal-add-menu {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  min-width: 200px;
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  box-shadow: var(--shadow-md);
  z-index: 50;
  padding: 4px 0;
}
.terminal-add-menu-item {
  display: block;
  width: 100%;
  text-align: left;
  padding: 8px 12px;
  border: none;
  background: transparent;
  color: var(--text-primary);
  font-size: 0.82rem;
  cursor: pointer;
}
.terminal-add-menu-item:hover {
  background: var(--bg-elevated);
}
.terminal-add-menu-sub {
  padding-left: 28px;
  font-size: 0.78rem;
  color: var(--text-secondary);
}
.terminal-add-menu-sub:hover {
  color: var(--text-primary);
}
.terminal-add-menu-group {
  border-top: 1px solid var(--border);
}
.terminal-add-menu-group:first-child {
  border-top: none;
}
.terminal-add-menu-host {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  font-size: 0.78rem;
  font-weight: 600;
  color: var(--text-primary);
}
.terminal-add-menu-host.disabled {
  color: var(--text-muted);
}
```

- [ ] **Step 3: Commit**

```bash
git add desktop/src/App.css
git commit -m "feat: add CSS for multi-host sidebar and session dropdown"
```

---

## Task 9: Compile Check & Fix

**Files:**
- Possibly any of the above

- [ ] **Step 1: Run TypeScript check**

```bash
cd desktop && npx tsc --noEmit
```

- [ ] **Step 2: Fix any compilation errors**

Address any type mismatches between the new prop interfaces. Common issues:
- `LogViewer` expecting `string` but now receiving `string | null`
- `TerminalWorkspace` prop name changes
- Missing imports

- [ ] **Step 3: Verify Rust still compiles**

```bash
cd desktop/src-tauri && cargo check
```

- [ ] **Step 4: Commit any fixes**

```bash
git add -A
git commit -m "fix: resolve TypeScript compilation errors from multi-host refactor"
```

---

## Task 10: End-to-End Smoke Test

- [ ] **Step 1: Build and launch**

```bash
cd desktop && npm run tauri dev
```

Expected: app compiles and launches. A local terminal auto-spawns.

- [ ] **Step 2: Verify sidebar**

- Sidebar should show "Hosts" section
- If migrating from existing baseUrl: one host "Default" should appear with connection status
- If fresh install: empty hosts with "Add a remote host to connect" text
- "+ Add host" button should show/hide the inline form

- [ ] **Step 3: Verify "+" terminal dropdown**

- Click "+" button in tab bar
- Should show dropdown with "Local shell" and any connected hosts with mode options
- Create a local shell session
- If a remote host is connected, create a remote session

- [ ] **Step 4: Verify inspector context switching**

- Select a local terminal tab → inspector header shows "Local — Observability"
- Select a remote terminal tab → inspector header shows "HostName — Observability"

- [ ] **Step 5: Verify host add/remove**

- Add a new host via sidebar form (can use a dummy URL that fails)
- Should appear in sidebar with "unreachable" status
- Hover and click "x" to remove → should disappear

- [ ] **Step 6: Commit any final fixes**

```bash
git add -A
git commit -m "fix: address smoke test issues from multi-host integration"
```

---

Plan complete and saved to `docs/superpowers/plans/2026-04-04-phase2-multi-host.md`. Two execution options:

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

Which approach?