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

    // 2. Create memories with metadata
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
    assert_eq!(all[0].id, "m1"); // higher importance first

    // 4. Get top lessons
    let lessons = store.get_top_lessons(None, 3).unwrap();
    assert_eq!(lessons.len(), 1);
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
