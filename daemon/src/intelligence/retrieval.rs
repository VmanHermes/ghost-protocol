use serde::{Deserialize, Serialize};

use crate::intelligence::memory::{MemoryMetadata, MemoryRecord};
use crate::store::Store;

fn default_limit() -> usize {
    5
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecallQuery {
    pub query: Option<String>,
    pub filters: Option<RecallFilters>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

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
    pub fn from_record(record: &MemoryRecord) -> Self {
        let metadata: MemoryMetadata =
            serde_json::from_str(&record.metadata_json).unwrap_or_default();
        RecallMemory {
            title: record.title.clone(),
            content: record.content.clone(),
            category: record.category.clone(),
            agent: metadata.agent,
            machine: metadata.machine,
            created: record.created_at.clone(),
        }
    }
}

pub fn recall(
    store: &Store,
    query: &RecallQuery,
    current_project_id: Option<&str>,
) -> RecallResponse {
    let limit = query.limit.min(10);

    let filters = query.filters.as_ref();
    let project_id = filters
        .and_then(|f| f.project.as_deref())
        .or(current_project_id);
    let agent = filters.and_then(|f| f.agent.as_deref());
    let machine = filters.and_then(|f| f.machine.as_deref());
    let outcome = filters.and_then(|f| f.outcome.as_deref());
    let category = filters.and_then(|f| f.category.as_deref());

    let has_any_filter = agent.is_some()
        || machine.is_some()
        || outcome.is_some()
        || category.is_some()
        || project_id.is_some();

    // Case: no filters and no query — return top memories by importance
    if !has_any_filter && query.query.is_none() {
        let records = store
            .list_memories_by_project(None, limit)
            .unwrap_or_default();
        let memories: Vec<RecallMemory> = records.iter().map(RecallMemory::from_record).collect();
        let total = memories.len();
        return RecallResponse {
            memories,
            total_available: total,
            search_method: "structured".to_string(),
        };
    }

    // Structured pass
    let structured = store
        .query_memories_structured(project_id, agent, machine, outcome, category, limit)
        .unwrap_or_default();

    if structured.len() >= limit {
        let memories: Vec<RecallMemory> = structured.iter().map(RecallMemory::from_record).collect();
        let total = memories.len();
        return RecallResponse {
            memories,
            total_available: total,
            search_method: "structured".to_string(),
        };
    }

    // Keyword fallback
    if let Some(ref q) = query.query {
        let q_lower = q.to_lowercase();
        let all_records = store
            .list_memories_by_project(project_id, 1000)
            .unwrap_or_default();

        let structured_ids: std::collections::HashSet<&str> =
            structured.iter().map(|r| r.id.as_str()).collect();

        let mut merged: Vec<MemoryRecord> = structured.clone();

        for record in &all_records {
            if structured_ids.contains(record.id.as_str()) {
                continue;
            }
            if record.title.to_lowercase().contains(&q_lower)
                || record.content.to_lowercase().contains(&q_lower)
                || record.metadata_json.to_lowercase().contains(&q_lower)
            {
                merged.push(record.clone());
                if merged.len() >= limit {
                    break;
                }
            }
        }

        let search_method = if structured.is_empty() {
            "keyword".to_string()
        } else {
            "hybrid".to_string()
        };

        let memories: Vec<RecallMemory> = merged.iter().map(RecallMemory::from_record).collect();
        let total = memories.len();
        return RecallResponse {
            memories,
            total_available: total,
            search_method,
        };
    }

    // Structured results only (< limit, no keyword query)
    let memories: Vec<RecallMemory> = structured.iter().map(RecallMemory::from_record).collect();
    let total = memories.len();
    RecallResponse {
        memories,
        total_available: total,
        search_method: "structured".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_store;

    fn make_memory(
        store: &Store,
        id: &str,
        project_id: Option<&str>,
        title: &str,
        content: &str,
        metadata_json: &str,
    ) {
        store
            .create_memory(
                id,
                project_id,
                None,
                "general",
                title,
                content,
                None,
                metadata_json,
                0.5,
            )
            .unwrap();
    }

    #[test]
    fn recall_with_no_memories() {
        let store = test_store();
        let query = RecallQuery {
            query: None,
            filters: None,
            limit: 5,
        };
        let response = recall(&store, &query, None);
        assert!(response.memories.is_empty());
        assert_eq!(response.total_available, 0);
    }

    #[test]
    fn recall_with_structured_filter() {
        let store = test_store();
        store
            .create_project("proj-1", "my-app", "/tmp/my-app", "{}")
            .unwrap();

        make_memory(
            &store,
            "m1",
            Some("proj-1"),
            "Memory on dev",
            "Something happened on dev",
            r#"{"agent":"claude","machine":"dev-box"}"#,
        );
        make_memory(
            &store,
            "m2",
            Some("proj-1"),
            "Memory on prod",
            "Something happened on prod",
            r#"{"agent":"claude","machine":"prod-box"}"#,
        );

        let query = RecallQuery {
            query: None,
            filters: Some(RecallFilters {
                project: Some("proj-1".to_string()),
                agent: None,
                machine: Some("dev-box".to_string()),
                outcome: None,
                category: None,
                tags: None,
            }),
            limit: 5,
        };

        let response = recall(&store, &query, None);
        assert_eq!(response.memories.len(), 1);
        assert_eq!(response.memories[0].title, "Memory on dev");
        assert_eq!(response.memories[0].machine, Some("dev-box".to_string()));
    }

    #[test]
    fn recall_with_keyword_search() {
        let store = test_store();
        store
            .create_project("proj-1", "my-app", "/tmp/my-app", "{}")
            .unwrap();

        make_memory(
            &store,
            "m1",
            Some("proj-1"),
            "Authentication issue",
            "The token was expired and caused login failure",
            "{}",
        );
        make_memory(
            &store,
            "m2",
            Some("proj-1"),
            "Database timeout",
            "Connection pool exhausted under load",
            "{}",
        );

        let query = RecallQuery {
            query: Some("expired".to_string()),
            filters: Some(RecallFilters {
                project: Some("proj-1".to_string()),
                agent: None,
                machine: None,
                outcome: None,
                category: None,
                tags: None,
            }),
            limit: 5,
        };

        let response = recall(&store, &query, None);
        assert!(!response.memories.is_empty());
        assert!(response.memories.iter().any(|m| m.title == "Authentication issue"));
    }

    #[test]
    fn recall_limit_is_capped_at_10() {
        let store = test_store();
        store
            .create_project("proj-1", "my-app", "/tmp/my-app", "{}")
            .unwrap();

        for i in 0..15 {
            make_memory(
                &store,
                &format!("m{}", i),
                Some("proj-1"),
                &format!("Memory {}", i),
                &format!("Content for memory {}", i),
                "{}",
            );
        }

        let query = RecallQuery {
            query: None,
            filters: Some(RecallFilters {
                project: Some("proj-1".to_string()),
                agent: None,
                machine: None,
                outcome: None,
                category: None,
                tags: None,
            }),
            limit: 100,
        };

        let response = recall(&store, &query, None);
        assert!(
            response.memories.len() <= 10,
            "Expected at most 10 memories, got {}",
            response.memories.len()
        );
    }
}
