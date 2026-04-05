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

impl Store {
    pub fn create_outcome(
        &self,
        id: &str,
        source: &str,
        source_host_id: Option<&str>,
        category: &str,
        action: &str,
        description: Option<&str>,
        target_machine: Option<&str>,
        status: &str,
        exit_code: Option<i32>,
        duration_secs: Option<f64>,
        metadata_json: Option<&str>,
    ) -> Result<OutcomeRecord, rusqlite::Error> {
        let created_at = Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "INSERT INTO outcome_log (id, source, source_host_id, category, action, description,
             target_machine, status, exit_code, duration_secs, metadata_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                id,
                source,
                source_host_id,
                category,
                action,
                description,
                target_machine,
                status,
                exit_code,
                duration_secs,
                metadata_json,
                created_at,
            ],
        )?;
        Ok(OutcomeRecord {
            id: id.to_string(),
            source: source.to_string(),
            source_host_id: source_host_id.map(|s| s.to_string()),
            category: category.to_string(),
            action: action.to_string(),
            description: description.map(|s| s.to_string()),
            target_machine: target_machine.map(|s| s.to_string()),
            status: status.to_string(),
            exit_code,
            duration_secs,
            metadata_json: metadata_json.map(|s| s.to_string()),
            created_at,
        })
    }

    pub fn list_outcomes(
        &self,
        limit: usize,
        category_filter: Option<&str>,
        status_filter: Option<&str>,
    ) -> Result<Vec<OutcomeRecord>, rusqlite::Error> {
        let conn = self.conn();
        let limit_i64 = limit as i64;
        let select = "SELECT id, source, source_host_id, category, action, description,
                      target_machine, status, exit_code, duration_secs, metadata_json, created_at
                      FROM outcome_log";
        let order_limit = "ORDER BY created_at DESC LIMIT ?";

        let map_row = |row: &rusqlite::Row<'_>| {
            Ok(OutcomeRecord {
                id: row.get(0)?,
                source: row.get(1)?,
                source_host_id: row.get(2)?,
                category: row.get(3)?,
                action: row.get(4)?,
                description: row.get(5)?,
                target_machine: row.get(6)?,
                status: row.get(7)?,
                exit_code: row.get(8)?,
                duration_secs: row.get(9)?,
                metadata_json: row.get(10)?,
                created_at: row.get(11)?,
            })
        };

        let rows: Vec<OutcomeRecord> = match (category_filter, status_filter) {
            (Some(cat), Some(stat)) => {
                let sql = format!("{} WHERE category = ?1 AND status = ?2 {}", select, order_limit);
                let mut stmt = conn.prepare(&sql)?;
                stmt.query_map(params![cat, stat, limit_i64], map_row)?.collect::<Result<Vec<_>, _>>()?
            }
            (Some(cat), None) => {
                let sql = format!("{} WHERE category = ?1 {}", select, order_limit);
                let mut stmt = conn.prepare(&sql)?;
                stmt.query_map(params![cat, limit_i64], map_row)?.collect::<Result<Vec<_>, _>>()?
            }
            (None, Some(stat)) => {
                let sql = format!("{} WHERE status = ?1 {}", select, order_limit);
                let mut stmt = conn.prepare(&sql)?;
                stmt.query_map(params![stat, limit_i64], map_row)?.collect::<Result<Vec<_>, _>>()?
            }
            (None, None) => {
                let sql = format!("{} {}", select, order_limit);
                let mut stmt = conn.prepare(&sql)?;
                stmt.query_map(params![limit_i64], map_row)?.collect::<Result<Vec<_>, _>>()?
            }
        };
        Ok(rows)
    }

    pub fn get_outcome(&self, id: &str) -> Result<Option<OutcomeRecord>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, source, source_host_id, category, action, description,
             target_machine, status, exit_code, duration_secs, metadata_json, created_at
             FROM outcome_log WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(OutcomeRecord {
                id: row.get(0)?,
                source: row.get(1)?,
                source_host_id: row.get(2)?,
                category: row.get(3)?,
                action: row.get(4)?,
                description: row.get(5)?,
                target_machine: row.get(6)?,
                status: row.get(7)?,
                exit_code: row.get(8)?,
                duration_secs: row.get(9)?,
                metadata_json: row.get(10)?,
                created_at: row.get(11)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_store;

    #[test]
    fn test_create_and_get_outcome() {
        let store = test_store();
        let record = store
            .create_outcome(
                "o1",
                "local",
                Some("host-1"),
                "deploy",
                "run_script",
                Some("deployed app"),
                Some("server-1"),
                "success",
                Some(0),
                Some(1.5),
                None,
            )
            .unwrap();

        assert_eq!(record.id, "o1");
        assert_eq!(record.source, "local");
        assert_eq!(record.source_host_id, Some("host-1".to_string()));
        assert_eq!(record.category, "deploy");
        assert_eq!(record.action, "run_script");
        assert_eq!(record.status, "success");
        assert_eq!(record.exit_code, Some(0));
        assert_eq!(record.duration_secs, Some(1.5));

        let fetched = store.get_outcome("o1").unwrap().unwrap();
        assert_eq!(fetched.id, "o1");
        assert_eq!(fetched.description, Some("deployed app".to_string()));
        assert_eq!(fetched.target_machine, Some("server-1".to_string()));

        let missing = store.get_outcome("nonexistent").unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn test_list_outcomes_with_filters() {
        let store = test_store();

        store
            .create_outcome(
                "o1", "local", None, "deploy", "push", None, None, "success", None, None, None,
            )
            .unwrap();
        store
            .create_outcome(
                "o2", "local", None, "deploy", "push", None, None, "failure", None, None, None,
            )
            .unwrap();
        store
            .create_outcome(
                "o3", "local", None, "health", "check", None, None, "success", None, None, None,
            )
            .unwrap();

        // Filter by category
        let deploy_outcomes = store.list_outcomes(100, Some("deploy"), None).unwrap();
        assert_eq!(deploy_outcomes.len(), 2);
        assert!(deploy_outcomes.iter().all(|o| o.category == "deploy"));

        // Filter by status
        let success_outcomes = store.list_outcomes(100, None, Some("success")).unwrap();
        assert_eq!(success_outcomes.len(), 2);
        assert!(success_outcomes.iter().all(|o| o.status == "success"));

        // Filter by both category and status
        let deploy_failures = store
            .list_outcomes(100, Some("deploy"), Some("failure"))
            .unwrap();
        assert_eq!(deploy_failures.len(), 1);
        assert_eq!(deploy_failures[0].id, "o2");

        // No filter
        let all = store.list_outcomes(100, None, None).unwrap();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_list_outcomes_limit() {
        let store = test_store();

        for i in 1..=5 {
            store
                .create_outcome(
                    &format!("o{}", i),
                    "local",
                    None,
                    "deploy",
                    "push",
                    None,
                    None,
                    "success",
                    None,
                    Some(i as f64),
                    None,
                )
                .unwrap();
            // Small sleep to ensure distinct created_at timestamps
            std::thread::sleep(std::time::Duration::from_millis(2));
        }

        let limited = store.list_outcomes(2, None, None).unwrap();
        assert_eq!(limited.len(), 2);

        // Results should be newest-first (o5, o4)
        assert_eq!(limited[0].id, "o5");
        assert_eq!(limited[1].id, "o4");
    }
}
