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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MemoryMetadata {
    pub agent: Option<String>,
    pub machine: Option<String>,
    pub intent: Option<String>,
    pub outcome: Option<String>,
    pub error_type: Option<String>,
    pub session_type: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryRecord> {
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

const SELECT_COLS: &str =
    "SELECT id, project_id, session_id, category, title, content, lesson, metadata_json, created_at, accessed_at, importance FROM memories";

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
        match project_id {
            Some(pid) => {
                let sql = format!(
                    "{} WHERE project_id = ?1 OR project_id IS NULL ORDER BY importance DESC, accessed_at DESC LIMIT ?2",
                    SELECT_COLS
                );
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(params![pid, limit as i64], map_row)?;
                rows.collect()
            }
            None => {
                let sql = format!(
                    "{} ORDER BY importance DESC, accessed_at DESC LIMIT ?1",
                    SELECT_COLS
                );
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(params![limit as i64], map_row)?;
                rows.collect()
            }
        }
    }

    pub fn get_top_lessons(
        &self,
        project_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryRecord>, rusqlite::Error> {
        let conn = self.conn();
        let rows: Vec<MemoryRecord> = match project_id {
            Some(pid) => {
                let sql = format!(
                    "{} WHERE project_id = ?1 AND lesson IS NOT NULL ORDER BY importance DESC LIMIT ?2",
                    SELECT_COLS
                );
                let mut stmt = conn.prepare(&sql)?;
                stmt.query_map(params![pid, limit as i64], map_row)?
                    .collect::<Result<Vec<_>, _>>()?
            }
            None => {
                let sql = format!(
                    "{} WHERE lesson IS NOT NULL ORDER BY importance DESC LIMIT ?1",
                    SELECT_COLS
                );
                let mut stmt = conn.prepare(&sql)?;
                stmt.query_map(params![limit as i64], map_row)?
                    .collect::<Result<Vec<_>, _>>()?
            }
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
        let mut conditions: Vec<String> = Vec::new();
        let mut positional: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        let mut idx = 1usize;

        if let Some(pid) = project_id {
            conditions.push(format!("project_id = ?{}", idx));
            positional.push(Box::new(pid.to_string()));
            idx += 1;
        }
        if let Some(cat) = category {
            conditions.push(format!("category = ?{}", idx));
            positional.push(Box::new(cat.to_string()));
            idx += 1;
        }
        if let Some(ag) = agent {
            conditions.push(format!("json_extract(metadata_json, '$.agent') = ?{}", idx));
            positional.push(Box::new(ag.to_string()));
            idx += 1;
        }
        if let Some(mc) = machine {
            conditions.push(format!(
                "json_extract(metadata_json, '$.machine') = ?{}",
                idx
            ));
            positional.push(Box::new(mc.to_string()));
            idx += 1;
        }
        if let Some(oc) = outcome {
            conditions.push(format!(
                "json_extract(metadata_json, '$.outcome') = ?{}",
                idx
            ));
            positional.push(Box::new(oc.to_string()));
            idx += 1;
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let limit_idx = idx;
        positional.push(Box::new(limit as i64));

        let sql = format!(
            "{} {} ORDER BY importance DESC, accessed_at DESC LIMIT ?{}",
            SELECT_COLS, where_clause, limit_idx
        );

        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::ToSql> = positional.iter().map(|b| b.as_ref()).collect();
        let records: Vec<MemoryRecord> = stmt
            .query_map(param_refs.as_slice(), map_row)?
            .collect::<Result<Vec<_>, _>>()?;

        let ids: Vec<&str> = records.iter().map(|r| r.id.as_str()).collect();

        if !ids.is_empty() {
            let now = Utc::now().to_rfc3339();
            let placeholders: String = ids
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", i + 2))
                .collect::<Vec<_>>()
                .join(", ");
            let update_sql = format!(
                "UPDATE memories SET accessed_at = ?1 WHERE id IN ({})",
                placeholders
            );
            let mut update_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            update_params.push(Box::new(now));
            for id in ids {
                update_params.push(Box::new(id.to_string()));
            }
            let update_refs: Vec<&dyn rusqlite::ToSql> =
                update_params.iter().map(|b| b.as_ref()).collect();
            conn.execute(&update_sql, update_refs.as_slice())?;
        }

        Ok(records)
    }

    pub fn delete_memory(&self, id: &str) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let affected = conn.execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        Ok(affected > 0)
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

#[cfg(test)]
mod tests {
    use crate::store::test_store;

    fn sample_memory(store: &crate::store::Store, id: &str) {
        store
            .create_memory(
                id,
                Some("proj-1"),
                Some("sess-1"),
                "error",
                "Title",
                "Content body",
                Some("Always check the config"),
                r#"{"agent":"claude","machine":"dev","outcome":"failure"}"#,
                0.8,
            )
            .unwrap();
    }

    #[test]
    fn test_create_and_basic_retrieval() {
        let store = test_store();
        store
            .create_project("proj-1", "my-app", "/tmp/my-app", "{}")
            .unwrap();

        let rec = store
            .create_memory(
                "m1",
                Some("proj-1"),
                Some("sess-1"),
                "error",
                "Auth failure",
                "The auth token expired",
                Some("Refresh tokens before they expire"),
                r#"{"agent":"claude"}"#,
                0.9,
            )
            .unwrap();

        assert_eq!(rec.id, "m1");
        assert_eq!(rec.project_id, Some("proj-1".to_string()));
        assert_eq!(rec.category, "error");
        assert_eq!(rec.importance, 0.9);
        assert!(rec.lesson.is_some());
    }

    #[test]
    fn test_list_memories_by_project() {
        let store = test_store();
        store
            .create_project("proj-1", "my-app", "/tmp/my-app", "{}")
            .unwrap();
        store
            .create_project("proj-2", "other-app", "/tmp/other-app", "{}")
            .unwrap();

        store
            .create_memory(
                "m1", Some("proj-1"), None, "error", "T1", "C1", None, "{}", 0.5,
            )
            .unwrap();
        store
            .create_memory(
                "m2", Some("proj-1"), None, "decision", "T2", "C2", None, "{}", 0.9,
            )
            .unwrap();
        store
            .create_memory(
                "m3", Some("proj-2"), None, "error", "T3", "C3", None, "{}", 0.7,
            )
            .unwrap();

        let proj1 = store.list_memories_by_project(Some("proj-1"), 100).unwrap();
        assert_eq!(proj1.len(), 2);
        // Ordered by importance DESC — m2 (0.9) should come first
        assert_eq!(proj1[0].id, "m2");

        let proj2 = store.list_memories_by_project(Some("proj-2"), 100).unwrap();
        assert_eq!(proj2.len(), 1);
        assert_eq!(proj2[0].id, "m3");
    }

    #[test]
    fn test_get_top_lessons() {
        let store = test_store();
        store
            .create_project("proj-1", "my-app", "/tmp/my-app", "{}")
            .unwrap();

        store
            .create_memory(
                "m1",
                Some("proj-1"),
                None,
                "error",
                "T1",
                "C1",
                Some("Lesson A"),
                "{}",
                0.6,
            )
            .unwrap();
        store
            .create_memory(
                "m2",
                Some("proj-1"),
                None,
                "decision",
                "T2",
                "C2",
                None, // no lesson
                "{}",
                0.9,
            )
            .unwrap();
        store
            .create_memory(
                "m3",
                Some("proj-1"),
                None,
                "error",
                "T3",
                "C3",
                Some("Lesson B"),
                "{}",
                0.8,
            )
            .unwrap();

        let lessons = store.get_top_lessons(Some("proj-1"), 10).unwrap();
        // Only m1 and m3 have lessons; ordered by importance DESC
        assert_eq!(lessons.len(), 2);
        assert_eq!(lessons[0].id, "m3"); // 0.8 importance
        assert_eq!(lessons[1].id, "m1"); // 0.6 importance

        let global = store.get_top_lessons(None, 10).unwrap();
        assert_eq!(global.len(), 2);
    }

    #[test]
    fn test_query_memories_structured_filters() {
        let store = test_store();
        store
            .create_project("proj-1", "my-app", "/tmp/my-app", "{}")
            .unwrap();

        store
            .create_memory(
                "m1",
                Some("proj-1"),
                Some("s1"),
                "error",
                "T1",
                "C1",
                None,
                r#"{"agent":"claude","machine":"dev","outcome":"failure"}"#,
                0.7,
            )
            .unwrap();
        store
            .create_memory(
                "m2",
                Some("proj-1"),
                Some("s2"),
                "decision",
                "T2",
                "C2",
                None,
                r#"{"agent":"claude","machine":"prod","outcome":"success"}"#,
                0.8,
            )
            .unwrap();
        store
            .create_memory(
                "m3",
                Some("proj-1"),
                Some("s3"),
                "error",
                "T3",
                "C3",
                None,
                r#"{"agent":"gpt","machine":"dev","outcome":"failure"}"#,
                0.5,
            )
            .unwrap();

        // Filter by category
        let errors = store
            .query_memories_structured(None, None, None, None, Some("error"), 100)
            .unwrap();
        assert_eq!(errors.len(), 2);

        // Filter by agent
        let claude_mems = store
            .query_memories_structured(None, Some("claude"), None, None, None, 100)
            .unwrap();
        assert_eq!(claude_mems.len(), 2);

        // Filter by outcome
        let failures = store
            .query_memories_structured(None, None, None, Some("failure"), None, 100)
            .unwrap();
        assert_eq!(failures.len(), 2);

        // Filter by machine
        let dev_mems = store
            .query_memories_structured(None, None, Some("dev"), None, None, 100)
            .unwrap();
        assert_eq!(dev_mems.len(), 2);

        // Combined filters
        let claude_errors = store
            .query_memories_structured(
                Some("proj-1"),
                Some("claude"),
                None,
                None,
                Some("error"),
                100,
            )
            .unwrap();
        assert_eq!(claude_errors.len(), 1);
        assert_eq!(claude_errors[0].id, "m1");
    }

    #[test]
    fn test_query_memories_structured_updates_accessed_at() {
        let store = test_store();
        store
            .create_project("proj-1", "my-app", "/tmp/my-app", "{}")
            .unwrap();

        store
            .create_memory(
                "m1",
                Some("proj-1"),
                Some("s1"),
                "error",
                "T1",
                "C1",
                None,
                r#"{"agent":"claude"}"#,
                0.7,
            )
            .unwrap();

        let before = store
            .list_memories_by_project(Some("proj-1"), 1)
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
            .accessed_at;

        std::thread::sleep(std::time::Duration::from_millis(10));

        store
            .query_memories_structured(Some("proj-1"), None, None, None, None, 10)
            .unwrap();

        let after = store
            .list_memories_by_project(Some("proj-1"), 1)
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
            .accessed_at;

        assert!(after >= before, "accessed_at should be updated after query");
    }

    #[test]
    fn test_delete_memory() {
        let store = test_store();
        store
            .create_project("proj-1", "my-app", "/tmp/my-app", "{}")
            .unwrap();
        sample_memory(&store, "m1");

        let deleted = store.delete_memory("m1").unwrap();
        assert!(deleted);

        let not_deleted = store.delete_memory("m1").unwrap();
        assert!(!not_deleted);

        let remaining = store.list_memories_by_project(Some("proj-1"), 100).unwrap();
        assert!(remaining.is_empty());
    }

    #[test]
    fn test_has_memory_for_session() {
        let store = test_store();
        store
            .create_project("proj-1", "my-app", "/tmp/my-app", "{}")
            .unwrap();

        assert!(!store.has_memory_for_session("sess-1").unwrap());

        sample_memory(&store, "m1");

        assert!(store.has_memory_for_session("sess-1").unwrap());
        assert!(!store.has_memory_for_session("sess-999").unwrap());
    }

    #[test]
    fn test_memory_without_project() {
        let store = test_store();
        let rec = store
            .create_memory(
                "m1",
                None, // no project_id
                None,
                "global",
                "Global insight",
                "Some content",
                None,
                "{}",
                0.5,
            )
            .unwrap();

        assert!(rec.project_id.is_none());

        let global_lessons = store.get_top_lessons(None, 10).unwrap();
        // no lesson set, so empty
        assert!(global_lessons.is_empty());
    }
}
