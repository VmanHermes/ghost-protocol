# Outcome Log Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an outcome log that captures terminal session lifecycle events automatically and accepts agent-reported outcomes via API, exposed through MCP for agent context awareness.

**Architecture:** New `outcome_log` SQLite table with store CRUD. Daemon auto-captures terminal create/terminate events in existing handlers. Agents report richer outcomes via POST /api/outcomes. MCP exposes recent outcomes as a resource and adds activity summary to the context briefing.

**Tech Stack:** Rust (axum, rusqlite, tokio, serde, chrono, uuid)

**Spec:** `docs/superpowers/specs/2026-04-05-outcome-log-design.md`

---

## File Structure

### New Files

| File | Responsibility |
|---|---|
| `daemon/migrations/005_outcome_log.sql` | Schema for outcome_log table |
| `daemon/src/store/outcomes.rs` | CRUD for outcome_log |

### Modified Files

| File | Change |
|---|---|
| `daemon/src/store/mod.rs` | Register outcomes module, run migration |
| `daemon/src/transport/http.rs` | Add POST/GET /api/outcomes, auto-capture in create_session and terminate_session |
| `daemon/src/server.rs` | Register new routes |
| `daemon/src/mcp/resources.rs` | Add ghost://outcomes/recent resource, add activity summary to briefing |

---

## Task 1: Database Migration & Outcomes Store

**Files:**
- Create: `daemon/migrations/005_outcome_log.sql`
- Create: `daemon/src/store/outcomes.rs`
- Modify: `daemon/src/store/mod.rs`

- [ ] **Step 1: Write the migration SQL**

Create `daemon/migrations/005_outcome_log.sql`:

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

- [ ] **Step 2: Register migration in store/mod.rs**

Add `pub mod outcomes;` after `pub mod discoveries;` (line 5).

After migration_004 block, add:
```rust
let migration_005 = include_str!("../../migrations/005_outcome_log.sql");
conn.execute_batch(migration_005)?;
```

- [ ] **Step 3: Write tests and implement outcomes store**

Create `daemon/src/store/outcomes.rs` with:

**Types:**
```rust
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use super::Store;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutcomeRecord {
    pub id: String,
    pub source: String,
    pub source_host_id: Option<String>,
    pub category: String,
    pub action: String,
    pub description: Option<String>,
    pub target_machine: Option<String>,
    pub status: String,
    pub exit_code: Option<i32>,
    pub duration_secs: Option<f64>,
    pub metadata_json: Option<String>,
    pub created_at: String,
}
```

**Store methods on `impl Store`:**

- `create_outcome(id, source, source_host_id, category, action, description, target_machine, status, exit_code, duration_secs, metadata_json)` — INSERT, returns OutcomeRecord
- `list_outcomes(limit, category_filter, status_filter)` — SELECT with optional filters, ORDER BY created_at DESC
- `get_outcome(id)` — SELECT by id, returns Option

**Tests (3):**
- test_create_and_get_outcome — create one, retrieve it, verify all fields
- test_list_outcomes_with_filters — create 3 with different categories/statuses, filter by category, filter by status, verify counts
- test_list_outcomes_limit — create 5, request limit 2, verify only 2 returned (newest first)

- [ ] **Step 4: Run tests**

Run: `cd daemon && cargo test store::outcomes`
Expected: All 3 tests PASS

- [ ] **Step 5: Commit**

```bash
git add daemon/migrations/005_outcome_log.sql daemon/src/store/outcomes.rs daemon/src/store/mod.rs
git commit -m "feat(daemon): add outcome_log store for action tracking"
```

---

## Task 2: HTTP Endpoints for Outcomes

**Files:**
- Modify: `daemon/src/transport/http.rs`
- Modify: `daemon/src/server.rs`

**Depends on:** Task 1

- [ ] **Step 1: Add outcome handlers to http.rs**

Add these imports (if not already present):
```rust
use crate::store::outcomes::OutcomeRecord;
```

Add these handlers at the end of http.rs:

```rust
// ---------------------------------------------------------------------------
// POST /api/outcomes (read-only and above)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateOutcomeBody {
    pub category: String,
    pub action: String,
    pub description: Option<String>,
    pub target_machine: Option<String>,
    pub status: String,
    pub exit_code: Option<i32>,
    pub duration_secs: Option<f64>,
    pub metadata: Option<serde_json::Value>,
}

pub async fn create_outcome(
    _tier: RequireReadOnly,
    client_ip: ClientIp,
    State(state): State<AppState>,
    Json(body): Json<CreateOutcomeBody>,
) -> Result<(StatusCode, Json<OutcomeRecord>), (StatusCode, Json<serde_json::Value>)> {
    let id = uuid::Uuid::new_v4().to_string();
    let source_host_id = state.store.resolve_host_id_by_ip(&client_ip.0).ok().flatten();
    let metadata_json = body.metadata.as_ref().map(|m| serde_json::to_string(m).unwrap_or_default());

    let record = state.store.create_outcome(
        &id,
        "agent",
        source_host_id.as_deref(),
        &body.category,
        &body.action,
        body.description.as_deref(),
        body.target_machine.as_deref(),
        &body.status,
        body.exit_code,
        body.duration_secs,
        metadata_json.as_deref(),
    ).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") })))
    })?;

    Ok((StatusCode::CREATED, Json(record)))
}

// ---------------------------------------------------------------------------
// GET /api/outcomes (localhost-only)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct OutcomesQuery {
    #[serde(default = "default_outcomes_limit")]
    pub limit: usize,
    pub category: Option<String>,
    pub status: Option<String>,
}

fn default_outcomes_limit() -> usize {
    50
}

pub async fn list_outcomes(
    _guard: RequireLocalhostOnly,
    State(state): State<AppState>,
    Query(params): Query<OutcomesQuery>,
) -> Result<Json<Vec<OutcomeRecord>>, (StatusCode, Json<serde_json::Value>)> {
    state
        .store
        .list_outcomes(params.limit, params.category.as_deref(), params.status.as_deref())
        .map(Json)
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") })))
        })
}
```

- [ ] **Step 2: Register routes in server.rs**

After the discovery routes, add:
```rust
.route("/api/outcomes", get(http::list_outcomes).post(http::create_outcome))
```

- [ ] **Step 3: Verify compilation**

Run: `cd daemon && cargo build`

- [ ] **Step 4: Commit**

```bash
git add daemon/src/transport/http.rs daemon/src/server.rs
git commit -m "feat(daemon): add POST/GET /api/outcomes endpoints"
```

---

## Task 3: Auto-Capture in Terminal Handlers

**Files:**
- Modify: `daemon/src/transport/http.rs`

**Depends on:** Task 1

- [ ] **Step 1: Add auto-capture to create_session handler**

In the `create_session` handler, after the successful session creation (after the `.map(|rec| (StatusCode::CREATED, Json(rec)))` line), add outcome logging. The cleanest way: extract the success path to log before returning.

Restructure `create_session` so that on success, before returning, it writes an outcome:

```rust
pub async fn create_session(
    _tier: RequireFullAccess,
    needs_approval: OptionalNeedsApproval,
    client_ip: ClientIp,
    State(state): State<AppState>,
    Json(body): Json<CreateSessionBody>,
) -> Result<(StatusCode, Json<crate::store::sessions::TerminalSessionRecord>), (StatusCode, Json<serde_json::Value>)>
{
    // ... existing approval check ...

    let result = state
        .manager
        .create_session(&body.mode, body.name.as_deref(), &body.workdir)
        .await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e })))
        })?;

    // Auto-capture outcome
    let source_host_id = state.store.resolve_host_id_by_ip(&client_ip.0).ok().flatten();
    let metadata = serde_json::json!({ "mode": body.mode, "workdir": body.workdir });
    state.store.create_outcome(
        &uuid::Uuid::new_v4().to_string(),
        "daemon",
        source_host_id.as_deref(),
        "terminal",
        "session_created",
        body.name.as_deref(),
        None,
        "success",
        None,
        None,
        Some(&serde_json::to_string(&metadata).unwrap_or_default()),
    ).ok(); // fire-and-forget

    Ok((StatusCode::CREATED, Json(result)))
}
```

- [ ] **Step 2: Add auto-capture to terminate_session handler**

In the `terminate_session` handler, after the successful termination, log an outcome:

```rust
    let result = state
        .manager
        .terminate_session(&id)
        .await
        .map_err(|e| { ... })?;

    // Auto-capture outcome
    let source_host_id = state.store.resolve_host_id_by_ip(&client_ip.0).ok().flatten();
    // Calculate duration from session creation
    let duration_secs = result.created_at.as_deref()
        .and_then(|created| chrono::DateTime::parse_from_rfc3339(created).ok())
        .map(|created| (chrono::Utc::now() - created.with_timezone(&chrono::Utc)).num_seconds() as f64);
    state.store.create_outcome(
        &uuid::Uuid::new_v4().to_string(),
        "daemon",
        source_host_id.as_deref(),
        "terminal",
        "session_terminated",
        None,
        None,
        "cancelled",
        None,
        duration_secs,
        None,
    ).ok(); // fire-and-forget

    Ok(Json(result))
```

Note: `terminate_session` needs `client_ip: ClientIp` added as a parameter if not already present.

- [ ] **Step 3: Verify compilation and tests**

Run: `cd daemon && cargo build && cargo test`

- [ ] **Step 4: Commit**

```bash
git add daemon/src/transport/http.rs
git commit -m "feat(daemon): auto-capture terminal lifecycle in outcome log"
```

---

## Task 4: MCP Resource & Briefing Enhancement

**Files:**
- Modify: `daemon/src/mcp/resources.rs`

**Depends on:** Task 1

- [ ] **Step 1: Add ghost://outcomes/recent resource**

In the `ResourceBuilder` impl, add a new method:

```rust
pub async fn recent_outcomes(&self) -> Result<Value, Box<dyn std::error::Error>> {
    let resp: Value = self.client()
        .get(format!("{}/api/outcomes?limit=20", self.base()))
        .send()
        .await?
        .json()
        .await?;
    Ok(json!({ "outcomes": resp }))
}
```

Add to `resource_list()`:
```rust
json!({
    "uri": "ghost://outcomes/recent",
    "name": "Recent Outcomes",
    "description": "Recent action outcomes across the mesh: what was done, where, and whether it succeeded",
    "mimeType": "application/json"
}),
```

Add the resource read handler in `daemon/src/mcp/transport.rs` — find the match on URI and add:
```rust
"ghost://outcomes/recent" => builder.recent_outcomes().await,
```

- [ ] **Step 2: Add activity summary to context_briefing**

In the `context_briefing` method, after the permission notes section (near the end), add:

```rust
// Recent activity
let outcomes_data: Value = match self.client()
    .get(format!("{}/api/outcomes?limit=5", self.base()))
    .send()
    .await
{
    Ok(resp) => resp.json().await.unwrap_or(json!([])),
    Err(_) => json!([]),
};

if let Some(outcomes) = outcomes_data.as_array() {
    if !outcomes.is_empty() {
        lines.push("\nRecent activity:".to_string());
        for o in outcomes {
            let action = o["action"].as_str().unwrap_or("?");
            let target = o["targetMachine"].as_str().unwrap_or("local");
            let status = o["status"].as_str().unwrap_or("?");
            let duration = o["durationSecs"].as_f64()
                .map(|d| format!(" ({d:.0}s)"))
                .unwrap_or_default();
            lines.push(format!("  - {action} on {target}: {status}{duration}"));
        }
    }
}
```

- [ ] **Step 3: Verify compilation**

Run: `cd daemon && cargo build`

- [ ] **Step 4: Commit**

```bash
git add daemon/src/mcp/resources.rs daemon/src/mcp/transport.rs
git commit -m "feat(daemon): add outcomes MCP resource and activity summary in briefing"
```

---

## Verification

After all tasks:

1. **Tests:** `cd daemon && cargo test store::outcomes` — all pass
2. **Build:** `cd daemon && cargo build` — clean
3. **Manual test:**
   - Start daemon: `cargo run -- serve`
   - Report an outcome: `curl -X POST localhost:8787/api/outcomes -H 'Content-Type: application/json' -d '{"category":"build","action":"cargo build","status":"success","durationSecs":12.5}'`
   - List outcomes: `curl localhost:8787/api/outcomes`
   - Check MCP briefing: `echo '{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"ghost://outcomes/recent"}}' | cargo run -- mcp`
