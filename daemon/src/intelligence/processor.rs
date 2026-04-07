use tracing::{info, warn};
use uuid::Uuid;

use crate::store::Store;

use super::config::IntelligenceConfig;
use super::provider::{IntelligenceProvider, Message, ProviderError};

// ── Extraction types ───────────────────────────────────────────────────────

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

#[derive(Debug)]
pub enum ProcessError {
    Provider(ProviderError),
    Parse(String),
    Store(String),
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessError::Provider(e) => write!(f, "provider error: {}", e),
            ProcessError::Parse(s) => write!(f, "parse error: {}", s),
            ProcessError::Store(s) => write!(f, "store error: {}", s),
        }
    }
}

// ── System prompt ──────────────────────────────────────────────────────────

const EXTRACTION_SYSTEM_PROMPT: &str = r#"You are a session analysis engine for an AI agent system called Ghost Protocol.

Analyze the provided session transcript and return a structured JSON object with the following schema:

{
  "summary": "Brief description of what happened in this session (1-3 sentences)",
  "intent": "The primary goal or task the agent was trying to accomplish",
  "outcome": "success | failure | partial | unclear",
  "error_type": "Category of error if applicable (e.g., 'permission_denied', 'network_timeout', 'config_error'), or null",
  "tags": ["relevant", "topic", "tags"],
  "memories": [
    {
      "category": "error | decision | discovery | tool_use | pattern | config | workflow",
      "title": "Short descriptive title",
      "content": "Detailed description of what was learned or observed",
      "importance": 0.0,
      "lesson": "When {situation}, use ghost_recall to {action} — {reason}"
    }
  ],
  "metadata": {}
}

Guidelines:
- Extract 1-5 memories that capture the most useful information for future sessions
- importance is a float between 0.0 and 1.0 (higher = more important)
- For lessons, use the format: "When {situation}, use ghost_recall to {action} — {reason}"
  Example: "When encountering CORS errors in development, use ghost_recall to check proxy config — local dev servers often need explicit CORS headers"
- Only include a lesson if there is a genuine behavioral insight that would help in a similar future situation
- Return ONLY the JSON object, no surrounding text or markdown code blocks
"#;

// ── Main processing function ───────────────────────────────────────────────

pub async fn process_session(
    store: &Store,
    provider: &dyn IntelligenceProvider,
    config: &IntelligenceConfig,
    ctx: SessionContext,
) -> Result<(), ProcessError> {
    // 1. Skip if already processed
    let already_processed = store
        .has_memory_for_session(&ctx.session_id)
        .map_err(|e| ProcessError::Store(e.to_string()))?;

    if already_processed {
        info!(session_id = %ctx.session_id, "session already processed, skipping");
        return Ok(());
    }

    // 2. Skip if duration is too short
    if let Some(duration) = ctx.duration_secs {
        if duration < config.min_session_duration as f64 {
            info!(
                session_id = %ctx.session_id,
                duration_secs = duration,
                min_required = config.min_session_duration,
                "session too short, skipping"
            );
            return Ok(());
        }
    }

    // 3. Truncate transcript to token limit (take tail)
    let transcript = truncate_to_tokens(&ctx.transcript, config.processing_transcript_limit);

    if transcript.is_empty() {
        info!(session_id = %ctx.session_id, "empty transcript, skipping");
        return Ok(());
    }

    // 4. Build messages and call the provider
    let user_content = format!(
        "{}\n\n---\n\nSession transcript (type: {}, machine: {}):\n\n{}",
        EXTRACTION_SYSTEM_PROMPT, ctx.session_type, ctx.machine, transcript
    );

    let messages = vec![Message { role: "user".to_string(), content: user_content }];

    let raw_response =
        provider.complete(messages).await.map_err(ProcessError::Provider)?;

    // 5. Parse the response
    let extraction: ExtractionResult = parse_extraction(&raw_response)?;

    // 6. Store each extracted memory
    let memory_count = extraction.memories.len();
    for mem in &extraction.memories {
        let memory_id = Uuid::new_v4().to_string();
        let metadata = serde_json::json!({
            "agent": ctx.agent_id,
            "machine": ctx.machine,
            "intent": extraction.intent,
            "outcome": extraction.outcome,
            "error_type": extraction.error_type,
            "session_type": ctx.session_type,
            "tags": extraction.tags,
        });
        let metadata_str = metadata.to_string();

        store
            .create_memory(
                &memory_id,
                ctx.project_id.as_deref(),
                Some(&ctx.session_id),
                &mem.category,
                &mem.title,
                &mem.content,
                mem.lesson.as_deref(),
                &metadata_str,
                mem.importance,
            )
            .map_err(|e| ProcessError::Store(e.to_string()))?;
    }

    // 7. Log result
    info!(
        session_id = %ctx.session_id,
        summary = %extraction.summary,
        outcome = ?extraction.outcome,
        memories_stored = memory_count,
        "session processed successfully"
    );

    Ok(())
}

// ── Helper functions ───────────────────────────────────────────────────────

/// Build a transcript string from chat messages for a session.
pub fn build_transcript_from_chat(store: &Store, session_id: &str) -> String {
    match store.list_chat_messages(session_id, None, 10_000) {
        Ok(messages) => messages
            .iter()
            .map(|m| format!("[{}] {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n"),
        Err(e) => {
            warn!(session_id = %session_id, error = %e, "failed to list chat messages");
            String::new()
        }
    }
}

/// Build a transcript string from terminal chunks for a session.
pub fn build_transcript_from_chunks(store: &Store, session_id: &str) -> String {
    match store.list_terminal_chunks(session_id, None, 100_000) {
        Ok(chunks) => chunks.iter().map(|c| c.chunk.as_str()).collect::<Vec<_>>().join(""),
        Err(e) => {
            warn!(session_id = %session_id, error = %e, "failed to list terminal chunks");
            String::new()
        }
    }
}

/// Truncate text to approximately `max_tokens` tokens (rough: 4 chars/token).
/// Takes the tail (most recent content) when truncation is needed.
pub fn truncate_to_tokens(text: &str, max_tokens: usize) -> String {
    let max_chars = max_tokens * 4;
    if text.len() <= max_chars {
        return text.to_string();
    }
    // Take the tail — most recent content is more relevant
    let start = text.len() - max_chars;
    // Find a valid UTF-8 char boundary
    let start = text
        .char_indices()
        .map(|(i, _)| i)
        .find(|&i| i >= start)
        .unwrap_or(start);
    text[start..].to_string()
}

/// Parse the LLM response into an ExtractionResult, stripping any markdown fences.
fn parse_extraction(raw: &str) -> Result<ExtractionResult, ProcessError> {
    let trimmed = raw.trim();

    // Strip ```json ... ``` or ``` ... ``` wrappers if present
    let json_str = if trimmed.starts_with("```") {
        let inner = trimmed.trim_start_matches('`');
        // Strip optional "json" tag
        let inner = inner.strip_prefix("json").unwrap_or(inner);
        // Find closing ```
        let end = inner.rfind("```").unwrap_or(inner.len());
        inner[..end].trim()
    } else {
        trimmed
    };

    serde_json::from_str::<ExtractionResult>(json_str)
        .map_err(|e| ProcessError::Parse(format!("failed to deserialize ExtractionResult: {} (raw: {})", e, &json_str[..json_str.len().min(200)])))
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_preserves_short_text() {
        let text = "hello world";
        let result = truncate_to_tokens(text, 100);
        assert_eq!(result, text);
    }

    #[test]
    fn truncate_takes_tail() {
        // 8 chars/token * 1 token = 8 chars limit, so with max_tokens=1 we get 4 chars
        // Use a clearly longer text to verify tail is kept
        let text = "AAAA_BBBB_CCCC_DDDD"; // 19 chars
        // max_tokens=2 → max_chars=8 → take last 8 chars
        let result = truncate_to_tokens(text, 2);
        assert_eq!(result.len(), 8);
        assert!(result.ends_with("CC_DDDD") || result.ends_with("_CCCC_DD") || result.ends_with("CC_DDDD"));
        // The key property: tail is preserved, not head
        assert!(result.contains("DDDD"), "tail should be preserved");
        assert!(!result.starts_with("AAAA"), "head should be dropped");
    }

    #[test]
    fn extraction_result_deserializes() {
        let json = r#"{
            "summary": "The agent set up a Rust project and added dependencies.",
            "intent": "Initialize a new Rust workspace",
            "outcome": "success",
            "error_type": null,
            "tags": ["rust", "setup", "cargo"],
            "memories": [
                {
                    "category": "workflow",
                    "title": "Cargo workspace initialization",
                    "content": "Used cargo init followed by cargo add to set up dependencies.",
                    "importance": 0.7,
                    "lesson": "When setting up a new Rust project, use ghost_recall to check workspace config — nested workspaces require explicit member paths in Cargo.toml"
                }
            ],
            "metadata": {}
        }"#;

        let result: ExtractionResult = serde_json::from_str(json).unwrap();

        assert_eq!(result.summary, "The agent set up a Rust project and added dependencies.");
        assert_eq!(result.intent.as_deref(), Some("Initialize a new Rust workspace"));
        assert_eq!(result.outcome.as_deref(), Some("success"));
        assert!(result.error_type.is_none());
        assert_eq!(result.tags, vec!["rust", "setup", "cargo"]);
        assert_eq!(result.memories.len(), 1);

        let mem = &result.memories[0];
        assert_eq!(mem.category, "workflow");
        assert_eq!(mem.importance, 0.7);
        assert!(mem.lesson.is_some());
    }
}
