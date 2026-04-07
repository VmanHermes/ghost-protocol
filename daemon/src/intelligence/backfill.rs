use tracing::{info, warn};

use crate::store::Store;
use super::config::IntelligenceConfig;
use super::processor::{self, SessionContext};
use super::provider;

/// Run backfill for historical sessions that haven't been processed.
/// Called once on startup when intelligence layer is first enabled.
pub async fn run_backfill(store: Store, config: IntelligenceConfig) {
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

        // Skip trivial sessions (duration check)
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
