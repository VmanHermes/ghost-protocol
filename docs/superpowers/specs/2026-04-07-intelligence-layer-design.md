# Embedded Intelligence Layer (3a-next)

**Date:** 2026-04-07
**Status:** Draft
**Phase:** 3a-next (Intelligence Layer)

## Context

Ghost Protocol manages terminals, chat sessions, and code-server instances across a Tailscale mesh. The daemon already captures rich data — outcomes, session history, machine capabilities, agent detection — but agents start each session cold with no awareness of prior work. The intelligence layer makes the mesh learn from itself.

**Design philosophy:** Ghost Protocol doesn't just connect you to agents — it orchestrates them using an agent of your choice as the intelligence layer. The system gets smarter over time even if individual agents don't actively query history, because post-session processing distills lessons that shape future sessions.

## Goals

1. A provider abstraction supporting API (Anthropic/OpenAI) and local (Ollama) LLM backends
2. Hybrid memory — summarization for high-level recall, vector search (sqlite-vec) for specific retrieval
3. Minimal pre-session enrichment (~200 tokens) with behavioral recall triggers as lessons
4. Post-session processing that extracts memories, metadata, and lessons from session transcripts
5. `ghost_recall` MCP tool for on-demand memory retrieval with structured metadata filtering
6. All agent sessions (terminal + chat) processed, with smart filtering to skip trivial ones

## Non-Goals

- Mid-session context injection (the intelligence layer does not interject during a running session)
- Replacing the existing MCP tools/resources (the intelligence layer builds on top of them)
- Automated routing/delegation (future work — routing decisions surface as recall results, not autonomous actions)

---

## Core Architecture

A new `daemon/src/intelligence/` module with three entry points:

```
intelligence/
├── mod.rs          // public API: enrich(), process(), query()
├── provider.rs     // LLM provider abstraction (API + Ollama)
├── memory.rs       // memory store (SQLite + sqlite-vec)
├── enricher.rs     // pre-session prompt enrichment
├── processor.rs    // post-session processing pipeline
└── retrieval.rs    // metadata filtering + vector search
```

**Three entry points:**
1. `enrich(session) -> SystemPrompt` — called when a chat/agent session starts
2. `process(session) -> ()` — called when a session ends, extracts memories
3. `query(filter, text?) -> Vec<Memory>` — called by `ghost_recall` MCP tool and available via HTTP API

The intelligence layer lives inside the daemon, maintaining the "daemon is the source of truth" and "single binary" principles. Vector search uses `sqlite-vec` statically linked via `rusqlite`.

---

## Provider Abstraction

```rust
#[async_trait]
pub trait IntelligenceProvider: Send + Sync {
    async fn complete(&self, messages: Vec<Message>) -> Result<String>;
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
}
```

### Implementations

**`ApiProvider`** — calls Anthropic or OpenAI-compatible APIs via reqwest.
- Model and API key from project config or daemon-level settings
- For embeddings: uses OpenAI's `text-embedding-3-small` (1536 dims) or a configurable endpoint (Anthropic does not offer an embedding model)
- Rate limiting and retry built in

**`OllamaProvider`** — calls `localhost:11434` (or a remote Ollama on the mesh).
- Completion: `/api/chat`
- Embeddings: `/api/embed` (supports models like `nomic-embed-text`, 1024 dims)
- Embedding dimension is model-dependent, read from `/api/show` at startup

Completion and embedding providers can differ — e.g., Claude API for completions, local Ollama for embeddings to save cost.

---

## Memory Schema

Two tables in the existing SQLite database.

### `memories` table

```sql
CREATE TABLE IF NOT EXISTS memories (
    id TEXT PRIMARY KEY,
    project_id TEXT REFERENCES projects(id),  -- NULL for global/cross-project
    session_id TEXT,                           -- originating session, NULL for synthetic
    category TEXT NOT NULL,                    -- 'summary', 'insight', 'error_pattern', 'preference', 'machine_knowledge'
    title TEXT NOT NULL,                       -- short descriptor for listing/filtering
    content TEXT NOT NULL,                     -- full memory text
    lesson TEXT,                               -- behavioral recall trigger, NULL until generated
    metadata_json TEXT NOT NULL DEFAULT '{}',  -- structured metadata for filtering
    created_at TEXT NOT NULL,
    accessed_at TEXT NOT NULL,                 -- updated on retrieval, enables decay/pruning
    importance REAL NOT NULL DEFAULT 0.5       -- 0.0-1.0, set by intelligence layer
);

CREATE INDEX idx_memories_project ON memories(project_id);
CREATE INDEX idx_memories_category ON memories(category);
CREATE INDEX idx_memories_importance ON memories(importance DESC);
```

### `memory_embeddings` virtual table (sqlite-vec)

```sql
-- NOTE: This table is NOT created via the static migration file.
-- It is created dynamically at daemon startup by the intelligence module,
-- because the dimension depends on the configured embedding model.
-- Example for OpenAI text-embedding-3-small (1536 dims):
CREATE VIRTUAL TABLE memory_embeddings USING vec0(
    id TEXT PRIMARY KEY,
    embedding float[1536]
);
```

Dimension matches the configured embedding model (1536 for OpenAI `text-embedding-3-small`, 1024 for Ollama `nomic-embed-text`). Created dynamically at daemon startup — not in the static migration file — because the dimension is config-dependent.

IDs are generated in Rust (`Uuid::new_v4().to_string()`) before inserting into both tables, consistent with the existing codebase pattern.

### `metadata_json` structure

The structured filtering layer — all fields optional, extracted by the post-processor:

```json
{
    "agent": "claude-code",
    "machine": "shared-host",
    "intent": "build rust project",
    "outcome": "failed",
    "error_type": "compilation",
    "session_type": "chat",
    "tags": ["cargo", "release-build", "gpu"]
}
```

Queries filter on these fields first (fast, exact match via `json_extract`), then fall back to vector similarity when structured filters aren't sufficient.

---

## Pre-Session Enrichment

Minimal by default. The enricher targets ~200 tokens injected via the existing `ChatSessionLaunchConfig.system_prompt` field.

### Injected prompt

```
You are running inside Ghost Protocol, a mesh control plane that connects
your session to other machines and agents on the network. Use the Ghost
Protocol MCP tools to search memory, report outcomes, and check mesh state.

Project: {name} on {machine}
Commands: build={build}, test={test}

Key lessons:
- {lesson 1}
- {lesson 2}
- {lesson 3}
```

**Key lessons** are behavioral recall triggers, not static facts. They teach the agent *when to use the system* rather than giving it answers that may go stale.

Examples:
- "Before running resource-heavy builds, use ghost_recall to check which machine handles them best"
- "If compilation fails with OOM, use ghost_recall to find alternative machines with more RAM"
- "When deploying, use ghost_recall to review past deployment outcomes for this project"

Lessons are the top 3 by importance from the `memories` table where `lesson IS NOT NULL` and project matches. Simple `ORDER BY importance DESC LIMIT 3` query — no LLM call at enrichment time.

If no lessons exist yet (new project), the "Key lessons" section is omitted.

### Terminal sessions (non-chat)

For terminal sessions there's no system prompt injection point. The enricher writes a context file to `/tmp/ghost-context-{session_id}.md` and sets `GHOST_CONTEXT=/tmp/ghost-context-{session_id}.md` in the session environment. Agents that understand this env var can read it; others ignore it.

---

## Post-Session Processing

When an agent session ends (chat or terminal), the processor pipeline runs.

### Pipeline

**1. Collect session data:**
- Chat messages (from `chat_messages` table) or terminal chunks (from `terminal_chunks`)
- Session metadata: agent, machine, project, duration, exit code
- Any outcomes already reported by the agent during the session

**2. Call the intelligence provider for extraction:**

A single LLM call with the session transcript (truncated to the last ~8000 tokens if longer), requesting structured output:

```json
{
  "summary": "Attempted release build, hit OOM after 8 minutes, switched to debug build which succeeded",
  "intent": "build rust project",
  "outcome": "partial_success",
  "error_type": "resource_exhaustion",
  "tags": ["cargo", "release", "oom"],
  "memories": [
    {
      "category": "machine_knowledge",
      "title": "laptop OOM on release builds",
      "content": "Release build of ghost-protocol on laptop (16GB RAM) runs out of memory after ~8 minutes. Debug builds work fine. The linker phase is the bottleneck.",
      "importance": 0.8,
      "lesson": "When running release builds for this project, use ghost_recall to check which machines have enough RAM — this project's linker phase needs >16GB"
    }
  ],
  "metadata": {
    "agent": "claude-code",
    "machine": "laptop",
    "intent": "build rust project",
    "outcome": "partial_success",
    "error_type": "resource_exhaustion",
    "session_type": "chat",
    "tags": ["cargo", "release", "oom"]
  }
}
```

**3. Store results:**
- Insert memories into `memories` table
- Generate embeddings and insert into `memory_embeddings`
- Update the session's outcome record with enriched metadata
- Prune/merge if a near-duplicate memory already exists (check by structured metadata match first, vector similarity second)

**4. Re-rank lessons:**
After inserting new memories, the top 3 by importance (across all memories with non-null lessons for this project) become the ones injected into future sessions. No LLM call — just `ORDER BY importance DESC LIMIT 3`.

### Skip conditions

Processing is skipped if:
- Session lasted under 10 seconds
- Session produced fewer than 5 terminal chunks
- Intelligence layer is disabled in config

---

## Retrieval — `ghost_recall` MCP Tool

### Tool definition

```json
{
  "name": "ghost_recall",
  "description": "Search project memory and history. Use before starting unfamiliar work, after hitting errors, or when deciding which machine to use.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "query": {
        "type": "string",
        "description": "Natural language question or keyword search"
      },
      "filters": {
        "type": "object",
        "properties": {
          "project": { "type": "string" },
          "agent": { "type": "string" },
          "machine": { "type": "string" },
          "outcome": { "type": "string", "enum": ["success", "failed", "partial_success"] },
          "category": { "type": "string", "enum": ["summary", "insight", "error_pattern", "preference", "machine_knowledge"] },
          "tags": { "type": "array", "items": { "type": "string" } }
        }
      },
      "limit": { "type": "integer", "default": 5, "maximum": 10 }
    },
    "required": []
  }
}
```

### Retrieval strategy (`retrieval.rs`)

1. **Structured pass** — query `memories` using provided filters via `json_extract`. If results >= `limit`, return ranked by importance. No LLM call, no vector search. Fast and cheap.

2. **Vector pass** — if structured results < `limit` and a `query` string was provided, embed the query and search `memory_embeddings`. Results joined with `memories` and filtered by any structured filters provided. Combines precision (metadata) with recall (semantics).

3. **No filters, no query** — return top memories by importance for the current project. A "what should I know?" default.

### Response format

```json
{
  "memories": [
    {
      "title": "laptop OOM on release builds",
      "content": "Release build of ghost-protocol on laptop (16GB RAM) runs out of memory...",
      "category": "machine_knowledge",
      "agent": "claude-code",
      "machine": "laptop",
      "created": "2026-04-06T14:30:00Z"
    }
  ],
  "total_available": 12,
  "search_method": "structured"
}
```

`search_method` indicates which retrieval path was used (`structured`, `vector`, or `hybrid`) for observability.

---

## Configuration

### Project-level (`.ghost/config.json`)

New `intelligence` block in the existing project manifest:

```json
{
  "intelligence": {
    "enabled": true,
    "provider": "api",
    "model": "claude-sonnet-4-20250514",
    "apiKeyEnv": "ANTHROPIC_API_KEY",
    "embeddingProvider": "ollama",
    "embeddingModel": "nomic-embed-text",
    "maxLessons": 3,
    "processingTranscriptLimit": 8000,
    "minSessionDuration": 10,
    "minSessionChunks": 5
  }
}
```

`apiKeyEnv` names the environment variable containing the API key — the daemon reads the value from the environment at runtime. If omitted, falls back to well-known env vars by provider (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`). No keys ever stored in config files.

### Daemon-level fallback (`~/.config/ghost-protocol/intelligence.toml`)

Used for sessions outside a registered project:

```toml
[intelligence]
enabled = true
provider = "api"
model = "claude-sonnet-4-20250514"

[intelligence.embedding]
provider = "ollama"
model = "nomic-embed-text"
```

API keys resolved from well-known env vars only. No key fields in the file.

### Resolution order

Project config → daemon config → disabled. If neither specifies intelligence config, the layer is off. Sessions run exactly as they do today with no degradation.

### Embedding dimension handling

The daemon reads the embedding dimension from the provider at startup (Ollama's `/api/show`, or known per-model for OpenAI). The `memory_embeddings` virtual table is created with that dimension. If the embedding model changes, existing embeddings are invalidated — the daemon logs a warning and re-embeds on a background task.

---

## Files to Create/Modify

### New: Intelligence module

| File | Description |
|---|---|
| `daemon/src/intelligence/mod.rs` | Public API: `enrich()`, `process()`, `query()` |
| `daemon/src/intelligence/provider.rs` | `IntelligenceProvider` trait, `ApiProvider`, `OllamaProvider` |
| `daemon/src/intelligence/memory.rs` | Memory CRUD, embedding insert/delete, sqlite-vec integration |
| `daemon/src/intelligence/enricher.rs` | Pre-session prompt builder, lesson retrieval |
| `daemon/src/intelligence/processor.rs` | Post-session pipeline: collect → extract → store → rank |
| `daemon/src/intelligence/retrieval.rs` | Structured filtering + vector search, `ghost_recall` logic |

### Daemon modifications

| File | Change |
|---|---|
| `daemon/migrations/011_intelligence.sql` | `memories` table (the `memory_embeddings` virtual table is created dynamically at startup) |
| `daemon/Cargo.toml` | Add `sqlite-vec` feature/dependency |
| `daemon/src/store/mod.rs` | Run new migration |
| `daemon/src/server.rs` | Initialize intelligence module, wire into session lifecycle |
| `daemon/src/chat/manager.rs` | Call `enrich()` before session start, `process()` after end |
| `daemon/src/terminal/` | Call `process()` on terminal session end (for agent terminal sessions) |
| `daemon/src/mcp/transport.rs` | Register `ghost_recall` tool |
| `daemon/src/mcp/resources.rs` | Update context briefing to mention ghost_recall availability |
| `daemon/src/config.rs` | Parse intelligence config from project manifest and daemon toml |

### Configuration files

| File | Change |
|---|---|
| `.ghost/config.json` schema | Add `intelligence` block |
| `~/.config/ghost-protocol/intelligence.toml` | New daemon-level intelligence config |
