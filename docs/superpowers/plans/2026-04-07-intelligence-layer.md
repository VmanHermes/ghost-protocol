# Intelligence Layer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an embedded intelligence layer to the Ghost Protocol daemon that learns from agent sessions — enriching future sessions with lessons, extracting memories post-session, and providing on-demand recall via MCP tool.

**Architecture:** New `daemon/src/intelligence/` module with provider abstraction (Anthropic API + Ollama), SQLite memory store with sqlite-vec for vector search, minimal pre-session enrichment (~200 tokens), post-session processing pipeline, and `ghost_recall` MCP tool. Off by default — activated when user configures a provider.

**Tech Stack:** Rust, rusqlite + sqlite-vec, reqwest (LLM API calls), serde_json (structured extraction), existing daemon infrastructure (Store, MCP transport, chat manager)

---

### Task 1: Migration and Memory Store Types

**Files:**
- Create: `daemon/migrations/011_intelligence.sql`
- Create: `daemon/src/intelligence/mod.rs`
- Create: `daemon/src/intelligence/memory.rs`
- Modify: `daemon/src/store/mod.rs`

- [ ] **Step 1: Write the migration file**

Create `daemon/migrations/011_intelligence.sql`:

```sql
CREATE TABLE IF NOT EXISTS memories (
    id TEXT PRIMARY KEY,
    project_id TEXT REFERENCES projects(id),
    session_id TEXT,
    category TEXT NOT NULL,
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    lesson TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    accessed_at TEXT NOT NULL,
    importance REAL NOT NULL DEFAULT 0.5
);

CREATE INDEX IF NOT EXISTS idx_memories_project ON memories(project_id);
CREATE INDEX IF NOT EXISTS idx_memories_category ON memories(category);
CREATE INDEX IF NOT EXISTS idx_memories_importance ON memories(importance DESC);
```

- [ ] **Step 2: Register the migration in store/mod.rs**

Add to `MIGRATIONS_SLICE` in `daemon/src/store/mod.rs`:

```rust
M::up(include_str!("../../migrations/011_intelligence.sql")),
```

- [ ] **Step 3: Create the intelligence module with memory types**

Create `daemon/src/intelligence/mod.rs`:

```rust
pub mod memory;
```

Create `daemon/src/intelligence/memory.rs`:

```rust
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::store::Store;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRecord {
    pub id: String,
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub category: String,
    pub title: String,
    pub content: String,
    pub lesson: Option<String>,
    pub metadata_json: String,
    pub created_at: String,
    pub accessed_at: String,
    pub importance: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_type: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

impl Store {
    pub fn create_memory(
        &self,
        id: &str,
        project_id: Option<&str>,
        session_id: Option<&str>,
        category: &str,
        title: &str,
        content: &str,
        lesson: Option<&str>,
        metadata_json: &str,
        importance: f64,
    ) -> Result<MemoryRecord, rusqlite::Error> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "INSERT INTO memories (id, project_id, session_id, category, title, content, lesson, metadata_json, created_at, accessed_at, importance)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![id, project_id, session_id, category, title, content, lesson, metadata_json, now, now, importance],
        )?;
        Ok(MemoryRecord {
            id: id.to_string(),
            project_id: project_id.map(|s| s.to_string()),
            session_id: session_id.map(|s| s.to_string()),
            category: category.to_string(),
            title: title.to_string(),
            content: content.to_string(),
            lesson: lesson.map(|s| s.to_string()),
            metadata_json: metadata_json.to_string(),
            created_at: now.clone(),
            accessed_at: now,
            importance,
        })
    }

    pub fn list_memories_by_project(
        &self,
        project_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryRecord>, rusqlite::Error> {
        let conn = self.conn();
        let rows = if let Some(pid) = project_id {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, session_id, category, title, content, lesson, metadata_json, created_at, accessed_at, importance
                 FROM memories WHERE project_id = ?1 OR project_id IS NULL
                 ORDER BY importance DESC LIMIT ?2",
            )?;
            stmt.query_map(params![pid, limit as i64], map_memory_row)?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, session_id, category, title, content, lesson, metadata_json, created_at, accessed_at, importance
                 FROM memories ORDER BY importance DESC LIMIT ?1",
            )?;
            stmt.query_map(params![limit as i64], map_memory_row)?
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(rows)
    }

    pub fn get_top_lessons(
        &self,
        project_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryRecord>, rusqlite::Error> {
        let conn = self.conn();
        let rows = if let Some(pid) = project_id {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, session_id, category, title, content, lesson, metadata_json, created_at, accessed_at, importance
                 FROM memories WHERE lesson IS NOT NULL AND (project_id = ?1 OR project_id IS NULL)
                 ORDER BY importance DESC LIMIT ?2",
            )?;
            stmt.query_map(params![pid, limit as i64], map_memory_row)?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, session_id, category, title, content, lesson, metadata_json, created_at, accessed_at, importance
                 FROM memories WHERE lesson IS NOT NULL
                 ORDER BY importance DESC LIMIT ?1",
            )?;
            stmt.query_map(params![limit as i64], map_memory_row)?
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(rows)
    }

    pub fn query_memories_structured(
        &self,
        project_id: Option<&str>,
        agent: Option<&str>,
        machine: Option<&str>,
        outcome: Option<&str>,
        category: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryRecord>, rusqlite::Error> {
        let conn = self.conn();
        let mut conditions = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut idx = 1;

        if let Some(pid) = project_id {
            conditions.push(format!("(project_id = ?{idx} OR project_id IS NULL)"));
            param_values.push(Box::new(pid.to_string()));
            idx += 1;
        }
        if let Some(a) = agent {
            conditions.push(format!("json_extract(metadata_json, '$.agent') = ?{idx}"));
            param_values.push(Box::new(a.to_string()));
            idx += 1;
        }
        if let Some(m) = machine {
            conditions.push(format!("json_extract(metadata_json, '$.machine') = ?{idx}"));
            param_values.push(Box::new(m.to_string()));
            idx += 1;
        }
        if let Some(o) = outcome {
            conditions.push(format!("json_extract(metadata_json, '$.outcome') = ?{idx}"));
            param_values.push(Box::new(o.to_string()));
            idx += 1;
        }
        if let Some(c) = category {
            conditions.push(format!("category = ?{idx}"));
            param_values.push(Box::new(c.to_string()));
            idx += 1;
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let sql = format!(
            "SELECT id, project_id, session_id, category, title, content, lesson, metadata_json, created_at, accessed_at, importance
             FROM memories {where_clause} ORDER BY importance DESC LIMIT ?{idx}"
        );

        param_values.push(Box::new(limit as i64));

        let mut stmt = conn.prepare(&sql)?;
        let params_ref: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|b| b.as_ref()).collect();
        let rows = stmt.query_map(params_ref.as_slice(), map_memory_row)?
            .collect::<Result<Vec<_>, _>>()?;

        // Touch accessed_at for returned memories
        let now = Utc::now().to_rfc3339();
        for mem in &rows {
            conn.execute(
                "UPDATE memories SET accessed_at = ?1 WHERE id = ?2",
                params![now, mem.id],
            ).ok();
        }

        Ok(rows)
    }

    pub fn delete_memory(&self, id: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn has_memory_for_session(&self, session_id: &str) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memories WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }
}

fn map_memory_row(row: &rusqlite::Row<'_>) -> Result<MemoryRecord, rusqlite::Error> {
    Ok(MemoryRecord {
        id: row.get(0)?,
        project_id: row.get(1)?,
        session_id: row.get(2)?,
        category: row.get(3)?,
        title: row.get(4)?,
        content: row.get(5)?,
        lesson: row.get(6)?,
        metadata_json: row.get(7)?,
        created_at: row.get(8)?,
        accessed_at: row.get(9)?,
        importance: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use crate::store::test_store;

    #[test]
    fn create_and_list_memories() {
        let store = test_store();
        store.create_memory(
            "m1", None, None, "insight", "test insight", "some content",
            Some("use ghost_recall when testing"), r#"{"agent":"claude-code"}"#, 0.8,
        ).unwrap();
        store.create_memory(
            "m2", None, None, "error_pattern", "build failure", "OOM on laptop",
            None, r#"{"agent":"claude-code","outcome":"failed"}"#, 0.6,
        ).unwrap();

        let all = store.list_memories_by_project(None, 10).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, "m1"); // higher importance first
    }

    #[test]
    fn get_top_lessons_only_returns_memories_with_lessons() {
        let store = test_store();
        store.create_memory(
            "m1", None, None, "insight", "with lesson", "content",
            Some("check ghost_recall"), "{}", 0.9,
        ).unwrap();
        store.create_memory(
            "m2", None, None, "insight", "no lesson", "content",
            None, "{}", 0.95,
        ).unwrap();

        let lessons = store.get_top_lessons(None, 10).unwrap();
        assert_eq!(lessons.len(), 1);
        assert_eq!(lessons[0].id, "m1");
    }

    #[test]
    fn query_memories_structured_filters() {
        let store = test_store();
        store.create_memory(
            "m1", None, None, "error_pattern", "OOM", "content",
            None, r#"{"agent":"claude-code","machine":"laptop","outcome":"failed"}"#, 0.7,
        ).unwrap();
        store.create_memory(
            "m2", None, None, "summary", "build ok", "content",
            None, r#"{"agent":"claude-code","machine":"shared-host","outcome":"success"}"#, 0.5,
        ).unwrap();

        let failed = store.query_memories_structured(None, None, None, Some("failed"), None, 10).unwrap();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].id, "m1");

        let laptop = store.query_memories_structured(None, None, Some("laptop"), None, None, 10).unwrap();
        assert_eq!(laptop.len(), 1);
        assert_eq!(laptop[0].id, "m1");
    }

    #[test]
    fn has_memory_for_session() {
        let store = test_store();
        assert!(!store.has_memory_for_session("s1").unwrap());

        store.create_memory(
            "m1", None, Some("s1"), "summary", "title", "content",
            None, "{}", 0.5,
        ).unwrap();
        assert!(store.has_memory_for_session("s1").unwrap());
    }
}
```

- [ ] **Step 4: Register intelligence module in daemon main**

Add `mod intelligence;` to `daemon/src/main.rs` (alongside existing module declarations).

- [ ] **Step 5: Run tests to verify migration and memory CRUD**

Run: `cd daemon && cargo test -- intelligence::memory`

Expected: All 4 tests pass. The `test_store()` uses `:memory:` DB so migration runs automatically.

- [ ] **Step 6: Commit**

```bash
git add daemon/migrations/011_intelligence.sql daemon/src/intelligence/ daemon/src/store/mod.rs daemon/src/main.rs
git commit -m "feat(intelligence): add memories table and memory store CRUD"
```

---

### Task 2: Intelligence Configuration

**Files:**
- Create: `daemon/src/intelligence/config.rs`
- Modify: `daemon/src/intelligence/mod.rs`

- [ ] **Step 1: Write tests for config parsing**

Create `daemon/src/intelligence/config.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntelligenceConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub embedding_provider: Option<String>,
    #[serde(default)]
    pub embedding_model: Option<String>,
    #[serde(default = "default_max_lessons")]
    pub max_lessons: usize,
    #[serde(default = "default_processing_transcript_limit")]
    pub processing_transcript_limit: usize,
    #[serde(default = "default_min_session_duration")]
    pub min_session_duration: u64,
    #[serde(default = "default_min_session_chunks")]
    pub min_session_chunks: usize,
}

fn default_max_lessons() -> usize { 3 }
fn default_processing_transcript_limit() -> usize { 8000 }
fn default_min_session_duration() -> u64 { 10 }
fn default_min_session_chunks() -> usize { 5 }

impl Default for IntelligenceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: None,
            model: None,
            api_key_env: None,
            embedding_provider: None,
            embedding_model: None,
            max_lessons: default_max_lessons(),
            processing_transcript_limit: default_processing_transcript_limit(),
            min_session_duration: default_min_session_duration(),
            min_session_chunks: default_min_session_chunks(),
        }
    }
}

impl IntelligenceConfig {
    /// Resolve from project config JSON, falling back to daemon config file.
    /// Returns None (disabled) if no provider is configured.
    pub fn resolve(project_config_json: Option<&str>) -> Self {
        // Try project config first
        if let Some(json_str) = project_config_json {
            if let Ok(project_config) = serde_json::from_str::<serde_json::Value>(json_str) {
                if let Some(intel) = project_config.get("intelligence") {
                    if let Ok(config) = serde_json::from_value::<IntelligenceConfig>(intel.clone()) {
                        if config.enabled && config.provider.is_some() {
                            return config;
                        }
                    }
                }
            }
        }

        // Try daemon-level config file
        if let Some(config) = Self::load_daemon_config() {
            if config.enabled && config.provider.is_some() {
                return config;
            }
        }

        // Disabled by default
        Self::default()
    }

    fn load_daemon_config() -> Option<Self> {
        let config_dir = std::env::var("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
                std::path::PathBuf::from(home).join(".config")
            });
        let path = config_dir.join("ghost-protocol").join("intelligence.toml");
        let content = std::fs::read_to_string(path).ok()?;

        // Parse TOML manually — the daemon doesn't have a toml dependency,
        // so we use a minimal approach: read key-value pairs from [intelligence] section
        let mut config = Self::default();
        let mut in_section = false;
        let mut in_embedding = false;

        for line in content.lines() {
            let line = line.trim();
            if line == "[intelligence]" {
                in_section = true;
                in_embedding = false;
                continue;
            }
            if line == "[intelligence.embedding]" {
                in_embedding = true;
                continue;
            }
            if line.starts_with('[') {
                in_section = false;
                in_embedding = false;
                continue;
            }
            if !in_section && !in_embedding {
                continue;
            }

            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim().trim_matches('"');

                if in_embedding {
                    match key {
                        "provider" => config.embedding_provider = Some(value.to_string()),
                        "model" => config.embedding_model = Some(value.to_string()),
                        _ => {}
                    }
                } else {
                    match key {
                        "enabled" => config.enabled = value == "true",
                        "provider" => config.provider = Some(value.to_string()),
                        "model" => config.model = Some(value.to_string()),
                        "api_key_env" => config.api_key_env = Some(value.to_string()),
                        _ => {}
                    }
                }
            }
        }

        Some(config)
    }

    /// Resolve the API key from environment variables.
    /// Checks apiKeyEnv first, then well-known vars based on provider.
    pub fn resolve_api_key(&self) -> Option<String> {
        // Check explicit env var name first
        if let Some(env_name) = &self.api_key_env {
            if let Ok(key) = std::env::var(env_name) {
                if !key.is_empty() {
                    return Some(key);
                }
            }
        }

        // Fall back to well-known vars by provider
        match self.provider.as_deref() {
            Some("api") => {
                std::env::var("ANTHROPIC_API_KEY").ok()
                    .or_else(|| std::env::var("OPENAI_API_KEY").ok())
                    .filter(|k| !k.is_empty())
            }
            _ => None,
        }
    }

    pub fn is_active(&self) -> bool {
        self.enabled && self.provider.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_disabled() {
        let config = IntelligenceConfig::default();
        assert!(!config.enabled);
        assert!(!config.is_active());
        assert_eq!(config.max_lessons, 3);
    }

    #[test]
    fn resolve_from_project_json() {
        let json = r#"{"intelligence":{"enabled":true,"provider":"api","model":"claude-sonnet-4-20250514"}}"#;
        let config = IntelligenceConfig::resolve(Some(json));
        assert!(config.is_active());
        assert_eq!(config.provider.as_deref(), Some("api"));
        assert_eq!(config.model.as_deref(), Some("claude-sonnet-4-20250514"));
    }

    #[test]
    fn resolve_disabled_when_no_provider() {
        let json = r#"{"intelligence":{"enabled":true}}"#;
        let config = IntelligenceConfig::resolve(Some(json));
        assert!(!config.is_active());
    }

    #[test]
    fn resolve_falls_back_to_default_when_no_intelligence_block() {
        let json = r#"{"name":"my-project"}"#;
        let config = IntelligenceConfig::resolve(Some(json));
        assert!(!config.is_active());
    }
}
```

- [ ] **Step 2: Update intelligence mod.rs**

```rust
pub mod config;
pub mod memory;
```

- [ ] **Step 3: Run tests**

Run: `cd daemon && cargo test -- intelligence::config`

Expected: All 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add daemon/src/intelligence/config.rs daemon/src/intelligence/mod.rs
git commit -m "feat(intelligence): add configuration with project and daemon-level resolution"
```

---

### Task 3: Provider Abstraction

**Files:**
- Create: `daemon/src/intelligence/provider.rs`
- Modify: `daemon/src/intelligence/mod.rs`

- [ ] **Step 1: Create provider trait and implementations**

Create `daemon/src/intelligence/provider.rs`:

```rust
use serde::{Deserialize, Serialize};
use tracing::{error, warn};

use super::config::IntelligenceConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[async_trait::async_trait]
pub trait IntelligenceProvider: Send + Sync {
    async fn complete(&self, messages: Vec<Message>) -> Result<String, ProviderError>;
    async fn embed(&self, text: &str) -> Result<Vec<f32>, ProviderError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("HTTP request failed: {0}")]
    Http(String),
    #[error("API returned error: {0}")]
    Api(String),
    #[error("No API key configured")]
    NoApiKey,
    #[error("Provider not configured")]
    NotConfigured,
    #[error("Embedding not available: {0}")]
    EmbeddingUnavailable(String),
}

pub struct ApiProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    embedding_endpoint: Option<EmbeddingEndpoint>,
}

struct EmbeddingEndpoint {
    provider: String,
    model: String,
    api_key: Option<String>,
    base_url: String,
}

pub struct OllamaProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
    embedding_model: Option<String>,
}

impl ApiProvider {
    pub fn new(config: &IntelligenceConfig) -> Result<Self, ProviderError> {
        let api_key = config.resolve_api_key().ok_or(ProviderError::NoApiKey)?;
        let model = config.model.clone().unwrap_or_else(|| "claude-sonnet-4-20250514".to_string());
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| ProviderError::Http(e.to_string()))?;

        let embedding_endpoint = config.embedding_provider.as_ref().map(|ep| {
            match ep.as_str() {
                "ollama" => EmbeddingEndpoint {
                    provider: "ollama".to_string(),
                    model: config.embedding_model.clone().unwrap_or_else(|| "nomic-embed-text".to_string()),
                    api_key: None,
                    base_url: "http://localhost:11434".to_string(),
                },
                _ => EmbeddingEndpoint {
                    provider: "openai".to_string(),
                    model: config.embedding_model.clone().unwrap_or_else(|| "text-embedding-3-small".to_string()),
                    api_key: std::env::var("OPENAI_API_KEY").ok(),
                    base_url: "https://api.openai.com".to_string(),
                },
            }
        });

        Ok(Self { client, api_key, model, embedding_endpoint })
    }
}

#[async_trait::async_trait]
impl IntelligenceProvider for ApiProvider {
    async fn complete(&self, messages: Vec<Message>) -> Result<String, ProviderError> {
        let api_messages: Vec<serde_json::Value> = messages.iter().map(|m| {
            serde_json::json!({ "role": m.role, "content": m.content })
        }).collect();

        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 4096,
            "messages": api_messages,
        });

        let resp = self.client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api(format!("HTTP {status}: {text}")));
        }

        let data: serde_json::Value = resp.json().await
            .map_err(|e| ProviderError::Http(e.to_string()))?;

        let text = data["content"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|block| block["text"].as_str())
            .unwrap_or("")
            .to_string();

        Ok(text)
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>, ProviderError> {
        let endpoint = self.embedding_endpoint.as_ref()
            .ok_or_else(|| ProviderError::EmbeddingUnavailable("no embedding provider configured".to_string()))?;

        match endpoint.provider.as_str() {
            "ollama" => embed_ollama(&self.client, &endpoint.base_url, &endpoint.model, text).await,
            _ => embed_openai(&self.client, &endpoint.base_url, &endpoint.model, endpoint.api_key.as_deref(), text).await,
        }
    }
}

impl OllamaProvider {
    pub fn new(config: &IntelligenceConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .unwrap_or_default();
        let model = config.model.clone().unwrap_or_else(|| "llama3".to_string());
        let embedding_model = config.embedding_model.clone();
        Self {
            client,
            base_url: "http://localhost:11434".to_string(),
            model,
            embedding_model,
        }
    }
}

#[async_trait::async_trait]
impl IntelligenceProvider for OllamaProvider {
    async fn complete(&self, messages: Vec<Message>) -> Result<String, ProviderError> {
        let api_messages: Vec<serde_json::Value> = messages.iter().map(|m| {
            serde_json::json!({ "role": m.role, "content": m.content })
        }).collect();

        let body = serde_json::json!({
            "model": self.model,
            "messages": api_messages,
            "stream": false,
        });

        let resp = self.client
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api(format!("HTTP {status}: {text}")));
        }

        let data: serde_json::Value = resp.json().await
            .map_err(|e| ProviderError::Http(e.to_string()))?;

        Ok(data["message"]["content"].as_str().unwrap_or("").to_string())
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>, ProviderError> {
        let model = self.embedding_model.as_deref()
            .ok_or_else(|| ProviderError::EmbeddingUnavailable("no embedding model configured for Ollama".to_string()))?;
        embed_ollama(&self.client, &self.base_url, model, text).await
    }
}

async fn embed_ollama(client: &reqwest::Client, base_url: &str, model: &str, text: &str) -> Result<Vec<f32>, ProviderError> {
    let body = serde_json::json!({
        "model": model,
        "input": text,
    });

    let resp = client
        .post(format!("{base_url}/api/embed"))
        .json(&body)
        .send()
        .await
        .map_err(|e| ProviderError::Http(e.to_string()))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(ProviderError::Api(format!("Ollama embed HTTP {status}: {text}")));
    }

    let data: serde_json::Value = resp.json().await
        .map_err(|e| ProviderError::Http(e.to_string()))?;

    let embeddings = data["embeddings"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_array())
        .ok_or_else(|| ProviderError::Api("unexpected Ollama embed response format".to_string()))?;

    Ok(embeddings.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect())
}

async fn embed_openai(
    client: &reqwest::Client,
    base_url: &str,
    model: &str,
    api_key: Option<&str>,
    text: &str,
) -> Result<Vec<f32>, ProviderError> {
    let api_key = api_key.ok_or(ProviderError::NoApiKey)?;

    let body = serde_json::json!({
        "model": model,
        "input": text,
    });

    let resp = client
        .post(format!("{base_url}/v1/embeddings"))
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&body)
        .send()
        .await
        .map_err(|e| ProviderError::Http(e.to_string()))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(ProviderError::Api(format!("OpenAI embed HTTP {status}: {text}")));
    }

    let data: serde_json::Value = resp.json().await
        .map_err(|e| ProviderError::Http(e.to_string()))?;

    let embedding = data["data"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|item| item["embedding"].as_array())
        .ok_or_else(|| ProviderError::Api("unexpected OpenAI embed response format".to_string()))?;

    Ok(embedding.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect())
}

/// Create a provider from config. Returns None if intelligence is disabled.
pub fn create_provider(config: &IntelligenceConfig) -> Option<Box<dyn IntelligenceProvider>> {
    if !config.is_active() {
        return None;
    }
    match config.provider.as_deref() {
        Some("api") => match ApiProvider::new(config) {
            Ok(p) => Some(Box::new(p)),
            Err(e) => {
                warn!("intelligence layer: failed to create API provider: {e}");
                None
            }
        },
        Some("ollama") => Some(Box::new(OllamaProvider::new(config))),
        other => {
            warn!("intelligence layer: unknown provider {:?}", other);
            None
        }
    }
}
```

- [ ] **Step 2: Add dependencies to Cargo.toml**

Add to `[dependencies]` in `daemon/Cargo.toml`:

```toml
async-trait = "0.1"
thiserror = "2"
```

- [ ] **Step 3: Update intelligence mod.rs**

```rust
pub mod config;
pub mod memory;
pub mod provider;
```

- [ ] **Step 4: Verify it compiles**

Run: `cd daemon && cargo check`

Expected: No errors. Provider tests require live API endpoints so we only check compilation here.

- [ ] **Step 5: Commit**

```bash
git add daemon/src/intelligence/provider.rs daemon/src/intelligence/mod.rs daemon/Cargo.toml
git commit -m "feat(intelligence): add provider abstraction with API and Ollama backends"
```

---

### Task 4: Pre-Session Enricher

**Files:**
- Create: `daemon/src/intelligence/enricher.rs`
- Modify: `daemon/src/intelligence/mod.rs`

- [ ] **Step 1: Create enricher with tests**

Create `daemon/src/intelligence/enricher.rs`:

```rust
use crate::store::Store;
use super::config::IntelligenceConfig;
use super::memory::MemoryRecord;

pub struct EnrichmentResult {
    pub system_prompt: String,
}

/// Build a minimal enrichment prompt for an agent session.
/// No LLM calls — purely reads from the memory store.
pub fn enrich_session(
    store: &Store,
    config: &IntelligenceConfig,
    project_id: Option<&str>,
    project_name: Option<&str>,
    machine_name: &str,
    commands: Option<&ProjectCommands>,
) -> EnrichmentResult {
    let mut lines = Vec::new();

    lines.push("You are running inside Ghost Protocol, a mesh control plane that connects".to_string());
    lines.push("your session to other machines and agents on the network. Use the Ghost".to_string());
    lines.push("Protocol MCP tools to search memory, report outcomes, and check mesh state.".to_string());
    lines.push(String::new());

    // Project context
    if let Some(name) = project_name {
        let mut project_line = format!("Project: {name} on {machine_name}");
        lines.push(project_line);
    } else {
        lines.push(format!("Machine: {machine_name}"));
    }

    if let Some(cmds) = commands {
        let mut cmd_parts = Vec::new();
        if let Some(ref b) = cmds.build { cmd_parts.push(format!("build={b}")); }
        if let Some(ref t) = cmds.test { cmd_parts.push(format!("test={t}")); }
        if let Some(ref l) = cmds.lint { cmd_parts.push(format!("lint={l}")); }
        if !cmd_parts.is_empty() {
            lines.push(format!("Commands: {}", cmd_parts.join(", ")));
        }
    }

    // Key lessons
    let max_lessons = config.max_lessons;
    if let Ok(lessons) = store.get_top_lessons(project_id, max_lessons) {
        if !lessons.is_empty() {
            lines.push(String::new());
            lines.push("Key lessons:".to_string());
            for memory in &lessons {
                if let Some(ref lesson) = memory.lesson {
                    lines.push(format!("- {lesson}"));
                }
            }
        }
    }

    EnrichmentResult {
        system_prompt: lines.join("\n"),
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProjectCommands {
    pub build: Option<String>,
    pub test: Option<String>,
    pub lint: Option<String>,
    pub deploy: Option<String>,
}

impl ProjectCommands {
    pub fn from_config_json(config_json: &str) -> Self {
        let value: serde_json::Value = serde_json::from_str(config_json).unwrap_or_default();
        Self {
            build: value["commands"]["build"].as_str().map(|s| s.to_string()),
            test: value["commands"]["test"].as_str().map(|s| s.to_string()),
            lint: value["commands"]["lint"].as_str().map(|s| s.to_string()),
            deploy: value["commands"]["deploy"].as_str().map(|s| s.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_store;
    use crate::intelligence::config::IntelligenceConfig;

    #[test]
    fn enrichment_without_lessons() {
        let store = test_store();
        let config = IntelligenceConfig::default();

        let result = enrich_session(&store, &config, None, Some("my-app"), "laptop", None);
        assert!(result.system_prompt.contains("Ghost Protocol"));
        assert!(result.system_prompt.contains("my-app on laptop"));
        assert!(!result.system_prompt.contains("Key lessons"));
    }

    #[test]
    fn enrichment_with_lessons() {
        let store = test_store();
        let config = IntelligenceConfig::default();

        store.create_memory(
            "m1", None, None, "machine_knowledge", "OOM", "content",
            Some("Before running release builds, use ghost_recall to check machine capacity"),
            "{}", 0.9,
        ).unwrap();

        let result = enrich_session(&store, &config, None, Some("my-app"), "laptop", None);
        assert!(result.system_prompt.contains("Key lessons"));
        assert!(result.system_prompt.contains("ghost_recall"));
    }

    #[test]
    fn enrichment_with_commands() {
        let store = test_store();
        let config = IntelligenceConfig::default();
        let cmds = ProjectCommands {
            build: Some("cargo build".to_string()),
            test: Some("cargo test".to_string()),
            lint: None,
            deploy: None,
        };

        let result = enrich_session(&store, &config, None, Some("my-app"), "laptop", Some(&cmds));
        assert!(result.system_prompt.contains("build=cargo build"));
        assert!(result.system_prompt.contains("test=cargo test"));
    }

    #[test]
    fn project_commands_from_json() {
        let json = r#"{"commands":{"build":"cargo build --release","test":"cargo test"}}"#;
        let cmds = ProjectCommands::from_config_json(json);
        assert_eq!(cmds.build.as_deref(), Some("cargo build --release"));
        assert_eq!(cmds.test.as_deref(), Some("cargo test"));
        assert!(cmds.lint.is_none());
    }
}
```

- [ ] **Step 2: Update intelligence mod.rs**

```rust
pub mod config;
pub mod enricher;
pub mod memory;
pub mod provider;
```

- [ ] **Step 3: Run tests**

Run: `cd daemon && cargo test -- intelligence::enricher`

Expected: All 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add daemon/src/intelligence/enricher.rs daemon/src/intelligence/mod.rs
git commit -m "feat(intelligence): add pre-session enricher with lesson injection"
```

---

### Task 5: Post-Session Processor

**Files:**
- Create: `daemon/src/intelligence/processor.rs`
- Modify: `daemon/src/intelligence/mod.rs`

- [ ] **Step 1: Create processor module**

Create `daemon/src/intelligence/processor.rs`:

```rust
use tracing::{info, warn, error};
use uuid::Uuid;

use crate::store::Store;
use super::config::IntelligenceConfig;
use super::provider::{IntelligenceProvider, Message, ProviderError};

/// Extraction result from the LLM. Matches the structured JSON schema
/// described in the spec.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ExtractionResult {
    pub summary: String,
    pub intent: Option<String>,
    pub outcome: Option<String>,
    pub error_type: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub memories: Vec<ExtractedMemory>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ExtractedMemory {
    pub category: String,
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub importance: f64,
    pub lesson: Option<String>,
}

pub struct SessionContext {
    pub session_id: String,
    pub project_id: Option<String>,
    pub agent_id: Option<String>,
    pub machine: String,
    pub session_type: String,
    pub duration_secs: Option<f64>,
    pub transcript: String,
}

const EXTRACTION_SYSTEM_PROMPT: &str = r#"You are a session analyst for Ghost Protocol, a mesh control plane for AI agents.
Analyze the following agent session transcript and extract structured information.

Respond with a JSON object containing:
- "summary": One sentence describing what happened
- "intent": What the agent was trying to accomplish
- "outcome": "success", "failed", or "partial_success"
- "error_type": If failed, categorize the error (e.g., "compilation", "resource_exhaustion", "network", "permission")
- "tags": Array of relevant keywords
- "memories": Array of memory objects to store, each with:
  - "category": One of "summary", "insight", "error_pattern", "preference", "machine_knowledge"
  - "title": Short descriptor
  - "content": Full memory text
  - "importance": 0.0 to 1.0 (how likely this is useful in future sessions)
  - "lesson": A behavioral recall trigger in the format "When {situation}, use ghost_recall to {action} — {reason}". Only include if the memory contains an actionable pattern. Null otherwise.
- "metadata": Object with "agent", "machine", "intent", "outcome", "error_type", "session_type", "tags"

Focus on extracting actionable patterns: things that went wrong, machine-specific behaviors, commands that worked or failed, and insights that would help a future agent session.

Respond ONLY with the JSON object, no markdown fences or explanation."#;

/// Process a completed session: extract memories via LLM and store them.
pub async fn process_session(
    store: &Store,
    provider: &dyn IntelligenceProvider,
    config: &IntelligenceConfig,
    ctx: SessionContext,
) -> Result<(), ProcessError> {
    // Skip if already processed
    if store.has_memory_for_session(&ctx.session_id).unwrap_or(false) {
        info!(session_id = %ctx.session_id, "session already processed, skipping");
        return Ok(());
    }

    // Skip trivial sessions
    if let Some(duration) = ctx.duration_secs {
        if duration < config.min_session_duration as f64 {
            info!(session_id = %ctx.session_id, duration, "session too short, skipping");
            return Ok(());
        }
    }

    // Truncate transcript
    let transcript = truncate_to_tokens(&ctx.transcript, config.processing_transcript_limit);
    if transcript.is_empty() {
        info!(session_id = %ctx.session_id, "empty transcript, skipping");
        return Ok(());
    }

    let user_message = format!(
        "Session ID: {}\nAgent: {}\nMachine: {}\nType: {}\nDuration: {}s\n\nTranscript:\n{}",
        ctx.session_id,
        ctx.agent_id.as_deref().unwrap_or("unknown"),
        ctx.machine,
        ctx.session_type,
        ctx.duration_secs.unwrap_or(0.0),
        transcript,
    );

    let messages = vec![
        Message { role: "user".to_string(), content: user_message },
    ];

    // Prepend system prompt via a system message for Anthropic API
    let mut full_messages = vec![
        Message { role: "user".to_string(), content: format!("{EXTRACTION_SYSTEM_PROMPT}\n\n{}", messages[0].content) },
    ];

    let response = provider.complete(full_messages).await
        .map_err(|e| {
            warn!(session_id = %ctx.session_id, error = %e, "LLM extraction failed");
            ProcessError::Provider(e)
        })?;

    let extraction: ExtractionResult = serde_json::from_str(&response)
        .map_err(|e| {
            warn!(session_id = %ctx.session_id, error = %e, response = %response, "failed to parse extraction JSON");
            ProcessError::Parse(e.to_string())
        })?;

    // Store memories
    for mem in &extraction.memories {
        let id = Uuid::new_v4().to_string();
        let metadata = serde_json::json!({
            "agent": ctx.agent_id,
            "machine": ctx.machine,
            "intent": extraction.intent,
            "outcome": extraction.outcome,
            "error_type": extraction.error_type,
            "session_type": ctx.session_type,
            "tags": extraction.tags,
        });

        if let Err(e) = store.create_memory(
            &id,
            ctx.project_id.as_deref(),
            Some(&ctx.session_id),
            &mem.category,
            &mem.title,
            &mem.content,
            mem.lesson.as_deref(),
            &serde_json::to_string(&metadata).unwrap_or_else(|_| "{}".to_string()),
            mem.importance.max(0.0).min(1.0),
        ) {
            error!(session_id = %ctx.session_id, error = %e, "failed to store memory");
        }
    }

    info!(
        session_id = %ctx.session_id,
        memories_extracted = extraction.memories.len(),
        intent = ?extraction.intent,
        outcome = ?extraction.outcome,
        "post-session processing complete"
    );

    Ok(())
}

/// Build session transcript from chat messages or terminal chunks.
pub fn build_transcript_from_chat(store: &Store, session_id: &str) -> String {
    let messages = store.list_chat_messages(session_id, None, 1000).unwrap_or_default();
    messages.iter().map(|m| {
        format!("[{}] {}", m.role, m.content)
    }).collect::<Vec<_>>().join("\n")
}

pub fn build_transcript_from_chunks(store: &Store, session_id: &str) -> String {
    let chunks = store.list_terminal_chunks(session_id, None, 1000).unwrap_or_default();
    chunks.iter().map(|c| c.chunk.clone()).collect::<Vec<_>>().join("")
}

/// Rough token estimation: ~4 chars per token
fn truncate_to_tokens(text: &str, max_tokens: usize) -> String {
    let max_chars = max_tokens * 4;
    if text.len() <= max_chars {
        text.to_string()
    } else {
        // Take the tail (most recent content is most relevant)
        text[text.len() - max_chars..].to_string()
    }
}

#[derive(Debug)]
pub enum ProcessError {
    Provider(ProviderError),
    Parse(String),
    Store(String),
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessError::Provider(e) => write!(f, "provider error: {e}"),
            ProcessError::Parse(e) => write!(f, "parse error: {e}"),
            ProcessError::Store(e) => write!(f, "store error: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_preserves_short_text() {
        let text = "hello world";
        assert_eq!(truncate_to_tokens(text, 1000), "hello world");
    }

    #[test]
    fn truncate_takes_tail() {
        let text = "a".repeat(40000); // 40k chars = ~10k tokens
        let result = truncate_to_tokens(&text, 1000); // 1000 tokens = 4000 chars
        assert_eq!(result.len(), 4000);
    }

    #[test]
    fn extraction_result_deserializes() {
        let json = r#"{
            "summary": "Built the project",
            "intent": "build",
            "outcome": "success",
            "error_type": null,
            "tags": ["cargo", "build"],
            "memories": [{
                "category": "summary",
                "title": "build succeeded",
                "content": "cargo build completed in 30s",
                "importance": 0.5,
                "lesson": null
            }],
            "metadata": {"agent": "claude-code"}
        }"#;
        let result: ExtractionResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.summary, "Built the project");
        assert_eq!(result.memories.len(), 1);
        assert_eq!(result.outcome.as_deref(), Some("success"));
    }
}
```

- [ ] **Step 2: Add list_terminal_chunks to store if missing**

Check if `Store::list_terminal_chunks` exists. If not, add to `daemon/src/store/chunks.rs`:

```rust
pub fn list_terminal_chunks(
    &self,
    session_id: &str,
    after_id: Option<i64>,
    limit: usize,
) -> Result<Vec<TerminalChunkRecord>, rusqlite::Error> {
    let conn = self.conn();
    if let Some(cursor) = after_id {
        let mut stmt = conn.prepare(
            "SELECT id, session_id, stream, chunk, created_at
             FROM terminal_chunks WHERE session_id = ?1 AND id > ?2
             ORDER BY id ASC LIMIT ?3"
        )?;
        stmt.query_map(params![session_id, cursor, limit as i64], |row| {
            Ok(TerminalChunkRecord {
                id: row.get(0)?,
                session_id: row.get(1)?,
                stream: row.get(2)?,
                chunk: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?.collect()
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, session_id, stream, chunk, created_at
             FROM terminal_chunks WHERE session_id = ?1
             ORDER BY id ASC LIMIT ?2"
        )?;
        stmt.query_map(params![session_id, limit as i64], |row| {
            Ok(TerminalChunkRecord {
                id: row.get(0)?,
                session_id: row.get(1)?,
                stream: row.get(2)?,
                chunk: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?.collect()
    }
}
```

- [ ] **Step 3: Update intelligence mod.rs**

```rust
pub mod config;
pub mod enricher;
pub mod memory;
pub mod processor;
pub mod provider;
```

- [ ] **Step 4: Run tests**

Run: `cd daemon && cargo test -- intelligence::processor`

Expected: All 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add daemon/src/intelligence/processor.rs daemon/src/intelligence/mod.rs daemon/src/store/chunks.rs
git commit -m "feat(intelligence): add post-session processor with LLM extraction"
```

---

### Task 6: Retrieval Module

**Files:**
- Create: `daemon/src/intelligence/retrieval.rs`
- Modify: `daemon/src/intelligence/mod.rs`

- [ ] **Step 1: Create retrieval module**

Create `daemon/src/intelligence/retrieval.rs`:

```rust
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::store::Store;
use super::memory::MemoryRecord;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecallQuery {
    pub query: Option<String>,
    pub filters: Option<RecallFilters>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize { 5 }

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecallFilters {
    pub project: Option<String>,
    pub agent: Option<String>,
    pub machine: Option<String>,
    pub outcome: Option<String>,
    pub category: Option<String>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecallResponse {
    pub memories: Vec<RecallMemory>,
    pub total_available: usize,
    pub search_method: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecallMemory {
    pub title: String,
    pub content: String,
    pub category: String,
    pub agent: Option<String>,
    pub machine: Option<String>,
    pub created: String,
}

impl RecallMemory {
    fn from_record(record: &MemoryRecord) -> Self {
        let metadata: serde_json::Value = serde_json::from_str(&record.metadata_json).unwrap_or_default();
        Self {
            title: record.title.clone(),
            content: record.content.clone(),
            category: record.category.clone(),
            agent: metadata["agent"].as_str().map(|s| s.to_string()),
            machine: metadata["machine"].as_str().map(|s| s.to_string()),
            created: record.created_at.clone(),
        }
    }
}

/// Execute a recall query. Uses structured filtering first,
/// falls back to vector search if available and needed.
pub fn recall(
    store: &Store,
    query: &RecallQuery,
    current_project_id: Option<&str>,
) -> RecallResponse {
    let limit = query.limit.min(10);

    let project_id = query.filters.as_ref()
        .and_then(|f| f.project.as_deref())
        .or(current_project_id);

    let agent = query.filters.as_ref().and_then(|f| f.agent.as_deref());
    let machine = query.filters.as_ref().and_then(|f| f.machine.as_deref());
    let outcome = query.filters.as_ref().and_then(|f| f.outcome.as_deref());
    let category = query.filters.as_ref().and_then(|f| f.category.as_deref());

    let has_filters = agent.is_some() || machine.is_some() || outcome.is_some() || category.is_some();

    // Structured pass
    let records = store.query_memories_structured(
        project_id, agent, machine, outcome, category, limit,
    ).unwrap_or_default();

    let total = store.list_memories_by_project(project_id, 10000)
        .map(|r| r.len())
        .unwrap_or(0);

    // If structured results are sufficient, return them
    if records.len() >= limit || !has_filters && query.query.is_none() {
        return RecallResponse {
            memories: records.iter().map(RecallMemory::from_record).collect(),
            total_available: total,
            search_method: "structured".to_string(),
        };
    }

    // Vector pass would go here when sqlite-vec is integrated.
    // For now, if structured results are insufficient and a query is provided,
    // do a basic keyword search as a fallback.
    if let Some(ref query_text) = query.query {
        let all_memories = store.list_memories_by_project(project_id, 100).unwrap_or_default();
        let query_lower = query_text.to_lowercase();
        let mut keyword_matches: Vec<MemoryRecord> = all_memories.into_iter()
            .filter(|m| {
                m.title.to_lowercase().contains(&query_lower)
                    || m.content.to_lowercase().contains(&query_lower)
                    || m.metadata_json.to_lowercase().contains(&query_lower)
            })
            .collect();

        // Merge with structured results, dedup by id
        let existing_ids: std::collections::HashSet<String> = records.iter().map(|r| r.id.clone()).collect();
        keyword_matches.retain(|m| !existing_ids.contains(&m.id));

        let mut combined = records;
        combined.extend(keyword_matches);
        combined.truncate(limit);

        return RecallResponse {
            memories: combined.iter().map(RecallMemory::from_record).collect(),
            total_available: total,
            search_method: if has_filters { "hybrid".to_string() } else { "keyword".to_string() },
        };
    }

    // Default: return what we have from structured pass
    RecallResponse {
        memories: records.iter().map(RecallMemory::from_record).collect(),
        total_available: total,
        search_method: "structured".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_store;

    #[test]
    fn recall_with_no_memories() {
        let store = test_store();
        let query = RecallQuery { query: None, filters: None, limit: 5 };
        let result = recall(&store, &query, None);
        assert!(result.memories.is_empty());
        assert_eq!(result.search_method, "structured");
    }

    #[test]
    fn recall_with_structured_filter() {
        let store = test_store();
        store.create_memory(
            "m1", None, None, "error_pattern", "OOM", "laptop ran out of memory",
            None, r#"{"agent":"claude-code","machine":"laptop","outcome":"failed"}"#, 0.8,
        ).unwrap();
        store.create_memory(
            "m2", None, None, "summary", "build ok", "shared-host build succeeded",
            None, r#"{"agent":"claude-code","machine":"shared-host","outcome":"success"}"#, 0.5,
        ).unwrap();

        let query = RecallQuery {
            query: None,
            filters: Some(RecallFilters {
                project: None,
                agent: None,
                machine: Some("laptop".to_string()),
                outcome: None,
                category: None,
                tags: None,
            }),
            limit: 5,
        };
        let result = recall(&store, &query, None);
        assert_eq!(result.memories.len(), 1);
        assert_eq!(result.memories[0].title, "OOM");
    }

    #[test]
    fn recall_with_keyword_search() {
        let store = test_store();
        store.create_memory(
            "m1", None, None, "error_pattern", "OOM on release build", "linker ran out of memory during cargo build --release",
            None, r#"{"agent":"claude-code"}"#, 0.8,
        ).unwrap();
        store.create_memory(
            "m2", None, None, "summary", "test passed", "all tests green",
            None, r#"{"agent":"claude-code"}"#, 0.5,
        ).unwrap();

        let query = RecallQuery {
            query: Some("release build memory".to_string()),
            filters: None,
            limit: 5,
        };
        let result = recall(&store, &query, None);
        assert!(!result.memories.is_empty());
        // The OOM memory should match because it contains "release build" and "memory"
        assert!(result.memories.iter().any(|m| m.title.contains("OOM")));
    }

    #[test]
    fn recall_limit_is_capped_at_10() {
        let store = test_store();
        for i in 0..15 {
            store.create_memory(
                &format!("m{i}"), None, None, "summary", &format!("memory {i}"), "content",
                None, "{}", 0.5,
            ).unwrap();
        }

        let query = RecallQuery { query: None, filters: None, limit: 100 };
        let result = recall(&store, &query, None);
        assert!(result.memories.len() <= 10);
    }
}
```

- [ ] **Step 2: Update intelligence mod.rs**

```rust
pub mod config;
pub mod enricher;
pub mod memory;
pub mod processor;
pub mod provider;
pub mod retrieval;
```

- [ ] **Step 3: Run tests**

Run: `cd daemon && cargo test -- intelligence::retrieval`

Expected: All 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add daemon/src/intelligence/retrieval.rs daemon/src/intelligence/mod.rs
git commit -m "feat(intelligence): add retrieval module with structured and keyword search"
```

---

### Task 7: ghost_recall MCP Tool

**Files:**
- Modify: `daemon/src/mcp/transport.rs`

- [ ] **Step 1: Add ghost_recall to tool_definitions()**

In `daemon/src/mcp/transport.rs`, add to the `tool_definitions()` array:

```rust
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

- [ ] **Step 2: Add ghost_recall handler to call_tool()**

In the `call_tool` function's match block, add:

```rust
"ghost_recall" => {
    let query: crate::intelligence::retrieval::RecallQuery =
        serde_json::from_value(arguments.clone()).unwrap_or_else(|_| {
            crate::intelligence::retrieval::RecallQuery {
                query: arguments["query"].as_str().map(|s| s.to_string()),
                filters: None,
                limit: 5,
            }
        });

    let store = crate::store::Store::open(
        &std::path::PathBuf::from(
            std::env::var("GHOST_PROTOCOL_DB")
                .unwrap_or_else(|_| "./data/ghost_protocol.db".to_string())
        )
    ).map_err(|e| format!("failed to open store: {e}"))?;

    let result = crate::intelligence::retrieval::recall(&store, &query, None);
    Ok(serde_json::to_string_pretty(&result)?)
}
```

Note: The MCP transport currently uses `ResourceBuilder` which calls HTTP endpoints. For `ghost_recall`, we need direct store access since the MCP server runs as a separate process (stdio). A cleaner approach is to add an HTTP endpoint and route through it like the other tools.

- [ ] **Step 3: Add HTTP endpoint for recall**

In `daemon/src/transport/http.rs`, add:

```rust
pub async fn recall_memories(
    State(state): State<AppState>,
    axum::Json(body): axum::Json<serde_json::Value>,
) -> impl IntoResponse {
    let query: crate::intelligence::retrieval::RecallQuery =
        serde_json::from_value(body).unwrap_or_else(|_| {
            crate::intelligence::retrieval::RecallQuery {
                query: None,
                filters: None,
                limit: 5,
            }
        });
    let result = crate::intelligence::retrieval::recall(&state.store, &query, None);
    axum::Json(result)
}
```

- [ ] **Step 4: Register the route in server.rs**

Add to the router in `daemon/src/server.rs`:

```rust
.route("/api/intelligence/recall", post(http::recall_memories))
```

- [ ] **Step 5: Update ghost_recall in MCP to use HTTP**

Replace the direct store access in `call_tool` with:

```rust
"ghost_recall" => {
    let client = builder.client();
    let resp = client
        .post(format!("{}/api/intelligence/recall", builder.base()))
        .json(arguments)
        .send()
        .await?;
    if resp.status().is_success() {
        let body: serde_json::Value = resp.json().await?;
        Ok(serde_json::to_string_pretty(&body)?)
    } else {
        let text = resp.text().await?;
        Err(format!("recall failed: {text}").into())
    }
}
```

- [ ] **Step 6: Run full compilation check**

Run: `cd daemon && cargo check`

Expected: No errors.

- [ ] **Step 7: Commit**

```bash
git add daemon/src/mcp/transport.rs daemon/src/transport/http.rs daemon/src/server.rs
git commit -m "feat(intelligence): add ghost_recall MCP tool and HTTP endpoint"
```

---

### Task 8: Wire Enricher into Chat Manager

**Files:**
- Modify: `daemon/src/chat/manager.rs`
- Modify: `daemon/src/server.rs`

- [ ] **Step 1: Add enrichment call before session spawn**

In `daemon/src/chat/manager.rs`, modify `spawn_session` to accept optional enrichment and apply it:

At the top of `spawn_session`, before `build_chat_command`, add logic to enrich:

```rust
pub async fn spawn_session(
    &self,
    session_id: &str,
    agent: &AgentInfo,
    workdir: &str,
    mut launch: ChatSessionLaunchConfig,
) -> Result<(), String> {
    // Enrich session if intelligence layer is configured
    if launch.system_prompt.is_none() {
        let enrichment = self.try_enrich(session_id, workdir);
        if let Some(prompt) = enrichment {
            launch.system_prompt = Some(prompt);
        }
    }

    // ... rest of existing method
```

Add the helper method to `ChatProcessManager`:

```rust
fn try_enrich(&self, session_id: &str, workdir: &str) -> Option<String> {
    use crate::intelligence::config::IntelligenceConfig;
    use crate::intelligence::enricher::{enrich_session, ProjectCommands};

    // Look up project by workdir
    let project = self.store.get_project_by_workdir(workdir).ok().flatten();
    let config_json = project.as_ref().map(|p| p.config_json.as_str());
    let intel_config = IntelligenceConfig::resolve(config_json);

    // Only enrich if intelligence is active
    if !intel_config.is_active() {
        return None;
    }

    let project_id = project.as_ref().map(|p| p.id.as_str());
    let project_name = project.as_ref().map(|p| p.name.as_str());
    let machine_name = hostname::get().ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".to_string());

    let commands = config_json.map(|j| ProjectCommands::from_config_json(j));

    let result = enrich_session(
        &self.store,
        &intel_config,
        project_id,
        project_name,
        &machine_name,
        commands.as_ref(),
    );

    Some(result.system_prompt)
}
```

- [ ] **Step 2: Add post-processing call when session exits**

In the stdout reader task (inside `spawn_session`), after the session exit handling block (around line 270-300 in the current code), add:

```rust
// Post-session processing (intelligence layer)
{
    let store2 = store.clone();
    let session_id2 = session_id_read.clone();
    tokio::spawn(async move {
        use crate::intelligence::config::IntelligenceConfig;
        use crate::intelligence::processor::{self, SessionContext};
        use crate::intelligence::provider;

        let session = match store2.get_terminal_session(&session_id2) {
            Ok(Some(s)) => s,
            _ => return,
        };

        let config_json = session.project_id.as_ref()
            .and_then(|pid| store2.get_project(pid).ok().flatten())
            .map(|p| p.config_json);
        let config = IntelligenceConfig::resolve(config_json.as_deref());

        if !config.is_active() {
            return;
        }

        let prov = match provider::create_provider(&config) {
            Some(p) => p,
            None => return,
        };

        let transcript = processor::build_transcript_from_chat(&store2, &session_id2);
        let machine = hostname::get().ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "unknown".to_string());

        let duration = session.started_at.as_ref().and_then(|start| {
            let start = chrono::DateTime::parse_from_rfc3339(start).ok()?;
            let end = session.finished_at.as_ref()
                .and_then(|f| chrono::DateTime::parse_from_rfc3339(f).ok())
                .unwrap_or_else(|| chrono::Utc::now().into());
            Some((end - start).num_seconds() as f64)
        });

        let ctx = SessionContext {
            session_id: session_id2.clone(),
            project_id: session.project_id,
            agent_id: session.agent_id,
            machine,
            session_type: session.session_type,
            duration_secs: duration,
            transcript,
        };

        if let Err(e) = processor::process_session(&store2, prov.as_ref(), &config, ctx).await {
            tracing::warn!(session_id = %session_id2, error = %e, "post-session processing failed");
        }
    });
}
```

- [ ] **Step 3: Verify compilation**

Run: `cd daemon && cargo check`

Expected: No errors.

- [ ] **Step 4: Commit**

```bash
git add daemon/src/chat/manager.rs
git commit -m "feat(intelligence): wire enricher and post-processor into chat session lifecycle"
```

---

### Task 9: Update Context Briefing

**Files:**
- Modify: `daemon/src/mcp/resources.rs`

- [ ] **Step 1: Add ghost_recall mention to context briefing**

In `daemon/src/mcp/resources.rs`, in the `context_briefing()` method, update the tool instructions section (around line 371-377) to include `ghost_recall`:

Replace:
```rust
lines.push("\nAvailable Ghost Protocol tools:".to_string());
lines.push("  - ghost_report_outcome: Report what you did and the result after completing work".to_string());
lines.push("  - ghost_check_mesh: Check current mesh state (machines, sessions, activity)".to_string());
lines.push("  - ghost_list_machines: Get machine capabilities and permissions for routing decisions".to_string());
```

With:
```rust
lines.push("\nAvailable Ghost Protocol tools:".to_string());
lines.push("  - ghost_recall: Search project memory and history for relevant context before starting work".to_string());
lines.push("  - ghost_report_outcome: Report what you did and the result after completing work".to_string());
lines.push("  - ghost_check_mesh: Check current mesh state (machines, sessions, activity)".to_string());
lines.push("  - ghost_list_machines: Get machine capabilities and permissions for routing decisions".to_string());
```

Also update the instruction text (around line 376-378):

Replace:
```rust
lines.push("After completing significant work (builds, deployments, inference, file operations),".to_string());
lines.push("use ghost_report_outcome to log the result. This helps the mesh learn which machines".to_string());
lines.push("are best for which tasks.".to_string());
```

With:
```rust
lines.push("Before starting unfamiliar work, use ghost_recall to check for relevant history.".to_string());
lines.push("After completing significant work, use ghost_report_outcome to log the result.".to_string());
lines.push("This helps the mesh learn from experience.".to_string());
```

- [ ] **Step 2: Verify compilation**

Run: `cd daemon && cargo check`

Expected: No errors.

- [ ] **Step 3: Commit**

```bash
git add daemon/src/mcp/resources.rs
git commit -m "feat(intelligence): add ghost_recall to context briefing"
```

---

### Task 10: Integration Test

**Files:**
- Create: `daemon/tests/intelligence_integration.rs`

- [ ] **Step 1: Write integration test for the full memory lifecycle**

Create `daemon/tests/intelligence_integration.rs`:

```rust
//! Integration test for the intelligence layer memory lifecycle.
//! Tests memory CRUD, enrichment, and recall without live LLM calls.

use std::path::Path;

#[test]
fn memory_lifecycle() {
    let store = ghost_protocol_daemon::store::Store::open(Path::new(":memory:"))
        .expect("open in-memory store");

    // 1. No memories initially
    let memories = store.list_memories_by_project(None, 10).unwrap();
    assert!(memories.is_empty());

    // 2. Create memories
    store.create_memory(
        "m1", None, Some("s1"), "error_pattern",
        "laptop OOM on release builds",
        "Release build of ghost-protocol on laptop (16GB RAM) runs out of memory after ~8 minutes.",
        Some("When running release builds, use ghost_recall to check which machines have enough RAM"),
        r#"{"agent":"claude-code","machine":"laptop","outcome":"failed","error_type":"resource_exhaustion","tags":["cargo","release","oom"]}"#,
        0.8,
    ).unwrap();

    store.create_memory(
        "m2", None, Some("s2"), "summary",
        "shared-host build succeeded",
        "cargo build --release completed in 4 minutes on shared-host (64GB RAM).",
        None,
        r#"{"agent":"claude-code","machine":"shared-host","outcome":"success","tags":["cargo","release"]}"#,
        0.5,
    ).unwrap();

    // 3. List all
    let all = store.list_memories_by_project(None, 10).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].id, "m1"); // higher importance

    // 4. Get lessons
    let lessons = store.get_top_lessons(None, 3).unwrap();
    assert_eq!(lessons.len(), 1); // only m1 has a lesson
    assert!(lessons[0].lesson.as_ref().unwrap().contains("ghost_recall"));

    // 5. Structured query
    let failed = store.query_memories_structured(None, None, None, Some("failed"), None, 10).unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].id, "m1");

    let laptop = store.query_memories_structured(None, None, Some("laptop"), None, None, 10).unwrap();
    assert_eq!(laptop.len(), 1);

    // 6. Recall via retrieval module
    use ghost_protocol_daemon::intelligence::retrieval::{recall, RecallQuery, RecallFilters};

    let result = recall(&store, &RecallQuery {
        query: Some("release build memory".to_string()),
        filters: None,
        limit: 5,
    }, None);
    assert!(!result.memories.is_empty());

    let result = recall(&store, &RecallQuery {
        query: None,
        filters: Some(RecallFilters {
            project: None,
            agent: Some("claude-code".to_string()),
            machine: Some("laptop".to_string()),
            outcome: None,
            category: None,
            tags: None,
        }),
        limit: 5,
    }, None);
    assert_eq!(result.memories.len(), 1);
    assert_eq!(result.memories[0].title, "laptop OOM on release builds");

    // 7. Enrichment
    use ghost_protocol_daemon::intelligence::config::IntelligenceConfig;
    use ghost_protocol_daemon::intelligence::enricher::enrich_session;

    let config = IntelligenceConfig::default();
    let enrichment = enrich_session(&store, &config, None, Some("ghost-protocol"), "laptop", None);
    assert!(enrichment.system_prompt.contains("Ghost Protocol"));
    assert!(enrichment.system_prompt.contains("ghost-protocol on laptop"));
    assert!(enrichment.system_prompt.contains("Key lessons"));
    assert!(enrichment.system_prompt.contains("ghost_recall"));

    // 8. Session already processed check
    assert!(store.has_memory_for_session("s1").unwrap());
    assert!(!store.has_memory_for_session("nonexistent").unwrap());
}
```

- [ ] **Step 2: Run the integration test**

Run: `cd daemon && cargo test --test intelligence_integration`

Expected: PASS

- [ ] **Step 3: Run the full test suite**

Run: `cd daemon && cargo test`

Expected: All tests pass, including the existing ones and the new intelligence tests.

- [ ] **Step 4: Commit**

```bash
git add daemon/tests/intelligence_integration.rs
git commit -m "test: add intelligence layer integration test"
```

---

### Task 11: sqlite-vec Integration (Foundation)

**Files:**
- Modify: `daemon/Cargo.toml`
- Modify: `daemon/src/intelligence/memory.rs`
- Modify: `daemon/src/intelligence/mod.rs`

Note: sqlite-vec integration requires the `sqlite-vec` crate which provides the loadable extension for rusqlite. This task sets up the foundation — the actual vector table creation happens dynamically when the intelligence layer is enabled and an embedding provider is configured.

- [ ] **Step 1: Research sqlite-vec Rust integration**

Check if `sqlite-vec` has a Rust crate available. The options are:
1. `sqlite-vec` crate (if it exists on crates.io)
2. Building from source and loading as a rusqlite extension
3. Using the `sqlite_vec_static` approach

Run: `cargo search sqlite-vec` to check availability.

If no crate is available, add a compile-time note and skip vector integration for now — the keyword-based fallback in `retrieval.rs` handles the case. The structured metadata filtering is the primary retrieval path anyway.

- [ ] **Step 2: Add TODO comment in retrieval.rs for future vector integration**

At the top of `daemon/src/intelligence/retrieval.rs`, add:

```rust
// TODO: When sqlite-vec crate is available, replace keyword search fallback
// with proper vector similarity search. The structured metadata filtering
// handles the primary use case; vector search is the semantic fallback.
// See spec: docs/superpowers/specs/2026-04-07-intelligence-layer-design.md
```

- [ ] **Step 3: Commit**

```bash
git add daemon/src/intelligence/retrieval.rs
git commit -m "chore(intelligence): document sqlite-vec integration path for vector search"
```

---

### Task 12: Backfill Task

**Files:**
- Create: `daemon/src/intelligence/backfill.rs`
- Modify: `daemon/src/intelligence/mod.rs`
- Modify: `daemon/src/server.rs`

- [ ] **Step 1: Create backfill module**

Create `daemon/src/intelligence/backfill.rs`:

```rust
use tracing::{info, warn};

use crate::store::Store;
use super::config::IntelligenceConfig;
use super::processor::{self, SessionContext};
use super::provider::{self, IntelligenceProvider};

/// Run backfill for historical sessions that haven't been processed.
/// Called once on startup when intelligence layer is first enabled.
pub async fn run_backfill(
    store: Store,
    config: IntelligenceConfig,
) {
    let provider = match provider::create_provider(&config) {
        Some(p) => p,
        None => {
            warn!("backfill: no provider available, skipping");
            return;
        }
    };

    let sessions = match store.list_terminal_sessions() {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "backfill: failed to list sessions");
            return;
        }
    };

    let machine = hostname::get().ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".to_string());

    let mut processed = 0;
    let mut skipped = 0;

    for session in &sessions {
        // Only process completed sessions
        if session.status != "exited" && session.status != "terminated" && session.status != "error" {
            continue;
        }

        // Skip if already has memories
        if store.has_memory_for_session(&session.id).unwrap_or(true) {
            skipped += 1;
            continue;
        }

        // Skip trivial sessions
        let duration = match (&session.started_at, &session.finished_at) {
            (Some(start), Some(end)) => {
                let start = chrono::DateTime::parse_from_rfc3339(start).ok();
                let end = chrono::DateTime::parse_from_rfc3339(end).ok();
                match (start, end) {
                    (Some(s), Some(e)) => Some((e - s).num_seconds() as f64),
                    _ => None,
                }
            }
            _ => None,
        };

        if let Some(d) = duration {
            if d < config.min_session_duration as f64 {
                skipped += 1;
                continue;
            }
        }

        // Build transcript
        let transcript = if session.session_type == "chat" {
            processor::build_transcript_from_chat(&store, &session.id)
        } else {
            processor::build_transcript_from_chunks(&store, &session.id)
        };

        if transcript.is_empty() {
            skipped += 1;
            continue;
        }

        let ctx = SessionContext {
            session_id: session.id.clone(),
            project_id: session.project_id.clone(),
            agent_id: session.agent_id.clone(),
            machine: machine.clone(),
            session_type: session.session_type.clone(),
            duration_secs: duration,
            transcript,
        };

        match processor::process_session(&store, provider.as_ref(), &config, ctx).await {
            Ok(()) => processed += 1,
            Err(e) => {
                warn!(session_id = %session.id, error = %e, "backfill: failed to process session");
            }
        }

        // Rate limit: 1 session per 5 seconds
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }

    info!(processed, skipped, total = sessions.len(), "backfill complete");
}
```

- [ ] **Step 2: Update intelligence mod.rs**

```rust
pub mod backfill;
pub mod config;
pub mod enricher;
pub mod memory;
pub mod processor;
pub mod provider;
pub mod retrieval;
```

- [ ] **Step 3: Wire backfill into server startup**

In `daemon/src/server.rs`, after the approval expiry task (step 6), add:

```rust
// 6b. Start intelligence backfill if newly enabled
{
    let store = store.clone();
    tokio::spawn(async move {
        // Small delay to let daemon finish startup
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

        let config = crate::intelligence::config::IntelligenceConfig::resolve(None);
        if config.is_active() {
            tracing::info!("intelligence layer enabled, checking for backfill");
            crate::intelligence::backfill::run_backfill(store, config).await;
        }
    });
}
```

- [ ] **Step 4: Verify compilation**

Run: `cd daemon && cargo check`

Expected: No errors.

- [ ] **Step 5: Commit**

```bash
git add daemon/src/intelligence/backfill.rs daemon/src/intelligence/mod.rs daemon/src/server.rs
git commit -m "feat(intelligence): add backfill task for historical session processing"
```

---

### Task 13: Final Compilation and Full Test Suite

**Files:** None (verification only)

- [ ] **Step 1: Run full test suite**

Run: `cd daemon && cargo test`

Expected: All tests pass.

- [ ] **Step 2: Run clippy**

Run: `cd daemon && cargo clippy -- -D warnings`

Expected: No warnings. Fix any that appear.

- [ ] **Step 3: Build release binary**

Run: `cd daemon && cargo build --release`

Expected: Builds successfully.

- [ ] **Step 4: Final commit if any fixes were needed**

```bash
git add -A
git commit -m "chore(intelligence): fix clippy warnings and finalize intelligence layer"
```
