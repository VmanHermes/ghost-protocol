# Outcome Log

**Date:** 2026-04-05
**Status:** Draft
**Phase:** 2f (extends Phase 2: The Context Layer)

## Context

Ghost Protocol agents work across a mesh of machines but have no memory of past actions or results. Without outcome history, agents can't learn which machines are best for which tasks, can't detect patterns of failure, and can't make informed routing decisions. The outcome log is the foundation for the future Distribution Advisor and Behavioral Oversight features.

## Goals

1. Capture terminal session lifecycle events automatically (daemon-side, deterministic)
2. Provide an API for agents to report richer outcomes (intent + result)
3. Expose outcome history via MCP so agents have awareness of recent activity
4. Build the data foundation for future intelligence layers

## Non-Goals

- Interpreting outcomes (no "smart" routing yet — just data collection)
- Enforcing outcome reporting (agents can choose not to report)
- Aggregation or analytics dashboards (future work)

---

## Data Model

### Migration: `005_outcome_log.sql`

```sql
CREATE TABLE IF NOT EXISTS outcome_log (
    id TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    source_host_id TEXT,
    category TEXT NOT NULL,
    action TEXT NOT NULL,
    description TEXT,
    target_machine TEXT,
    status TEXT NOT NULL,
    exit_code INTEGER,
    duration_secs REAL,
    metadata_json TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_outcome_log_created ON outcome_log(created_at);
CREATE INDEX IF NOT EXISTS idx_outcome_log_category ON outcome_log(category);
```

### Fields

| Field | Type | Description |
|---|---|---|
| `id` | TEXT PK | UUID, generated on insert |
| `source` | TEXT | `"daemon"` (auto-captured) or `"agent"` (reported via API) |
| `source_host_id` | TEXT | Peer that triggered/reported this. NULL for localhost |
| `category` | TEXT | Free-form: `"terminal"`, `"build"`, `"inference"`, `"deploy"`, `"custom"`, etc. |
| `action` | TEXT | What happened: `"session_created"`, `"cargo build --release"`, `"ollama run llama3"`, etc. |
| `description` | TEXT | Optional human-readable context |
| `target_machine` | TEXT | Which machine the work ran on (hostname or IP) |
| `status` | TEXT | `"success"`, `"failure"`, `"timeout"`, `"cancelled"` |
| `exit_code` | INTEGER | Optional numeric exit code |
| `duration_secs` | REAL | How long the action took |
| `metadata_json` | TEXT | Flexible JSON blob for extra data |
| `created_at` | TEXT | ISO 8601 timestamp |

### Design Decisions

- **Free-form category/action** — no fixed taxonomy. Agents choose their own labels. Future intelligence layers will cluster and learn from whatever is reported.
- **`source` field distinguishes daemon vs agent** — daemon entries are guaranteed (deterministic), agent entries are richer but optional.
- **`metadata_json` for extensibility** — avoids schema changes as we learn what data matters.

---

## Agent Reporting API

### `POST /api/outcomes` (all tiers)

Any peer with at least read-only access can report outcomes. `no-access` peers are still blocked by the tier middleware. Reporting what you observed doesn't require write access — `read-only` and above can POST outcomes.

**Request body:**
```json
{
  "category": "build",
  "action": "cargo build --release",
  "description": "Building daemon for v0.2.1 release",
  "targetMachine": "shared-host",
  "status": "success",
  "exitCode": 0,
  "durationSecs": 45.2,
  "metadata": {
    "workdir": "/home/vman/projects/ghost-protocol/daemon",
    "binary_size_mb": 12.4
  }
}
```

**Required fields:** `category`, `action`, `status`

**Optional fields:** `description`, `targetMachine`, `exitCode`, `durationSecs`, `metadata`

**Response:** `201 Created` with the full outcome record (id, createdAt filled in)

The daemon sets `source: "agent"` and resolves `sourceHostId` from the request's `ClientIp` middleware extension.

### `GET /api/outcomes` (localhost-only)

**Query params:**
- `limit` — max results, default 50
- `category` — optional filter
- `status` — optional filter

Returns newest first.

---

## Daemon Auto-Capture

The daemon writes `source: "daemon"`, `category: "terminal"` entries automatically at three points:

### 1. Session Created

In the `create_session` handler, after successful creation:
- action: `"session_created"`
- status: `"success"`
- metadata: `{ "mode": "...", "workdir": "..." }`

### 2. Session Terminated

In the `terminate_session` handler, after successful termination:
- action: `"session_terminated"`
- status: `"cancelled"`
- duration_secs: calculated from session's `createdAt` to now

### 3. Session Exited

When the terminal manager detects a session ended naturally (PTY EOF):
- action: `"session_exited"`
- status: exit_code == 0 ? `"success"` : `"failure"`
- exit_code: from the session record
- duration_secs: calculated from session's `createdAt` to now

Auto-capture is fire-and-forget — if the outcome log write fails, the terminal operation still succeeds.

---

## MCP Integration

### New Resource: `ghost://outcomes/recent`

Returns the last 20 outcomes:

```json
{
  "outcomes": [
    {
      "id": "...",
      "source": "agent",
      "category": "build",
      "action": "cargo build --release",
      "targetMachine": "shared-host",
      "status": "success",
      "durationSecs": 45.2,
      "createdAt": "2026-04-05T14:30:00Z"
    }
  ]
}
```

### Context Briefing Enhancement

Append a "Recent activity" section to `ghost://context/briefing`:

```
Recent activity:
  - cargo build --release on shared-host: success (45s)
  - session exited on laptop: failure, exit code 1 (12s)
  - ollama inference on shared-host: success (8s)
```

Shows the last 5 outcomes in human-readable form. Gives agents immediate awareness before they start work.

---

## Files to Modify/Create

### Daemon (Rust)

| File | Change |
|---|---|
| `daemon/migrations/005_outcome_log.sql` | New migration |
| `daemon/src/store/outcomes.rs` | New — CRUD for outcome_log |
| `daemon/src/store/mod.rs` | Register outcomes module, run migration |
| `daemon/src/transport/http.rs` | Add POST/GET /api/outcomes endpoints, add auto-capture to create_session and terminate_session |
| `daemon/src/server.rs` | Register new routes |
| `daemon/src/terminal/manager.rs` | Add auto-capture on session exit detection |
| `daemon/src/mcp/resources.rs` | Add ghost://outcomes/recent resource, add activity summary to briefing |
