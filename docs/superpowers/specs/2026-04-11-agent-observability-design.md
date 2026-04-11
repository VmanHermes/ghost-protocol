# Agent Observability Dashboard (3c)

**Date:** 2026-04-11
**Status:** Draft
**Phase:** 3c (Agent Observability)

## Context

Ghost Protocol manages terminals, chat sessions, and code-server instances across a Tailscale mesh. The desktop app already aggregates sessions from all connected machines into a flat list, polls machine hardware status, and tracks outcomes. But there's no unified view of "what's happening right now across the mesh" — you have to click through individual sessions to understand the state of things.

The right panel currently only shows the approval queue. This spec extends it into a mesh observability dashboard.

## Goals

1. A stacked right panel with collapsible approvals and a mesh overview dashboard
2. Per-machine health cards showing online status, resource usage, and active session count
3. Active agent list with status, machine, workdir, and duration — clickable to navigate
4. Recent activity feed from the outcome log
5. Zero new daemon endpoints — everything derived from existing data

## Non-Goals

- Token usage display (would require subscribing to every chat session's WebSocket)
- Per-process CPU/RAM monitoring (would need new OS-level daemon work)
- Inline output preview in the right panel (the dashboard is for awareness, not interaction)
- Real-time WebSocket streaming for the dashboard (polling existing endpoints is sufficient)

---

## Right Panel Layout

The right panel becomes a vertically stacked dashboard with two top-level sections:

```
┌─────────────────────────┐
│ ⚠ Approvals (N)         │  Collapsible. Hidden entirely when count = 0.
│ [existing approval UI]  │  Unchanged behavior, extracted into own component.
├─────────────────────────┤
│ Mesh Overview           │  Always visible. Three sub-sections:
│                         │
│ ── Machines ──          │
│ ● hostname (ip)        │  Green/red dot for online/offline.
│   RAM 12/31 GB · GPU   │  Hardware stats from machineStatus.
│   2 active sessions    │  Count of running sessions on this machine.
│                         │
│ ── Active Agents ──     │
│ ● AgentName  machine   │  Status dot: green=running, red=error.
│   /path/to/workdir     │  Duration since startedAt.
│   Running · 12m        │  Click → navigate to session in main view.
│                         │
│ ── Recent Activity ──   │
│ Agent exited (0) · 45s  │  From outcome log. Last 10 entries.
│ Session created · 2m ago│  Relative timestamps.
└─────────────────────────┘
```

### Approvals Section Behavior

- When pending approvals exist: section is visible with count badge and full approval UI
- When no pending approvals: section is completely hidden (not collapsed, hidden) — the mesh overview gets the full panel height
- Approval UI behavior is unchanged from current ApprovalsTab

### Machine Cards

Each connected machine gets a card showing:
- Hostname and Tailscale IP
- Online/offline status dot (derived from connection state)
- RAM usage: used/total GB (from `machineStatus.ramUsedGb / ramTotalGb`)
- GPU name if present (from `machineInfo.gpu`)
- Active session count: number of running sessions on this machine

The local machine is always shown first. Remote machines follow, sorted by hostname.

Data source: `connections` map (already maintained in App.tsx) + `localMachineInfo` / `localMachineStatus` (already polled).

### Active Agents List

Shows all currently running sessions that have an `agentId`, across all machines:
- Agent name (looked up from agentId, or just the agentId if name unavailable)
- Machine hostname (from session's hostName, or "local")
- Workdir (shortened to last 2 path segments)
- Status dot: green for "running", red for "error"
- Duration: elapsed time since `startedAt`, formatted as "Xm" or "Xh Ym"
- Click action: calls `setActiveTerminalSessionId(session.id)` to navigate to the session in the main view

Sessions without agentId (plain terminal sessions) are excluded. Only `status === "running"` sessions are shown.

Data source: `allFlatSessions` (already computed in App.tsx), filtered.

### Recent Activity Feed

Shows the last 10 entries from the outcome log, newest first:
- Action description (e.g., "Terminal session exited", "Chat session created")
- Exit code if present (e.g., "(code 0)" or "(code 1)")
- Duration if present (e.g., "45s", "2m")
- Relative timestamp ("2m ago", "1h ago")

Data source: `GET /api/outcomes?limit=10` — new polling hook at 10s interval. This endpoint already exists.

---

## Components

### Modified Components

**RightPanel.tsx** — Refactored from single-component wrapper to stacked layout:
- Receives: `daemonUrl`, `activeSession`, machine data, sessions, connection state
- Renders: `ApprovalsSection` (conditionally) + `MeshOverview`
- Passes navigation callback for agent click-to-navigate

### New Components

**ApprovalsSection.tsx** — Extracted from current ApprovalsTab:
- Same polling logic, same UI, same behavior
- Adds: hide entirely when `pendingCount === 0` (currently the section always renders)
- No functional changes to approval logic

**MeshOverview.tsx** — The main dashboard component:
- Receives: machine data (local + remote), sessions, outcomes, navigation callback
- Renders three sub-sections: Machines, Active Agents, Recent Activity
- Pure display component — no data fetching, receives everything via props

**useOutcomes.ts** — New hook for polling outcomes:
- Polls `GET /api/outcomes?limit=10` every 10 seconds
- Returns `OutcomeRecord[]`
- Stops polling when component unmounts

### Type Additions

```typescript
// In types.ts
interface OutcomeRecord {
  id: string;
  source: string;
  sourceHostId: string | null;
  category: string;
  action: string;
  description: string | null;
  targetMachine: string | null;
  status: string;
  exitCode: number | null;
  durationSecs: number | null;
  metadataJson: string | null;
  createdAt: string;
}
```

---

## Data Flow

```
App.tsx
├── connections (Map<hostId, HostConnection>)  ──► RightPanel
├── localMachineInfo / localMachineStatus      ──► RightPanel
├── allFlatSessions                            ──► RightPanel
├── setActiveTerminalSessionId                 ──► RightPanel (for agent click)
└── daemonUrl                                  ──► RightPanel
                                                    │
                                                    ├── ApprovalsSection
                                                    │   └── polls listApprovals() @ 3s
                                                    │
                                                    └── MeshOverview
                                                        ├── Machine cards (from props)
                                                        ├── Active agents (filtered from sessions prop)
                                                        └── Recent activity
                                                            └── useOutcomes() polls /api/outcomes @ 10s
```

No new WebSocket subscriptions. No new daemon endpoints. All data flows through existing channels.

---

## Styling

The right panel uses the existing panel styling patterns (`.right-panel`, `.right-panel-header`, `.right-panel-content`). New sections use:

- `.mesh-section` — collapsible section with header and content
- `.mesh-section-header` — section title with optional count badge
- `.machine-card` — compact card for each machine
- `.agent-entry` — clickable row for each active agent
- `.activity-entry` — compact row for each outcome
- `.status-dot` — small colored circle (green/red/gray)

All styling follows existing conventions in the codebase (CSS classes, no CSS-in-JS).

---

## Edge Cases

- **No remote machines connected**: Machine section shows only local machine. Active agents shows only local agents.
- **No running agents**: Active agents section shows "No active agents" placeholder text.
- **No outcomes yet**: Recent activity section shows "No recent activity" placeholder text.
- **Machine goes offline**: Machine card shows red dot and "offline" status. Its sessions remain in the active agents list until next session refresh clears them.
- **Many active sessions**: Active agents list is not paginated — if you have 20+ running agents, it scrolls within the section. This is unlikely in practice.
