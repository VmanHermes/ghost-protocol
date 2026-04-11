use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::Store;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRecord {
    pub id: String,
    pub name: String,
    pub workdir: String,
    pub config_json: String,
    pub registered_at: String,
    pub updated_at: String,
}

impl Store {
    /// Look up a project by workdir; if none exists, auto-create one with defaults.
    /// If `config_json_override` is provided (e.g. from a `.ghost/config.json` file),
    /// use that instead of generating a minimal default config.
    pub fn get_or_create_project_for_workdir(
        &self,
        workdir: &str,
        config_json_override: Option<&str>,
    ) -> Result<ProjectRecord, rusqlite::Error> {
        if let Some(existing) = self.get_project_by_workdir(workdir)? {
            return Ok(existing);
        }

        let name = std::path::Path::new(workdir)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string();

        let config_json = match config_json_override {
            Some(json) => json.to_string(),
            None => serde_json::json!({
                "name": name,
                "workdir": workdir,
                "agents": [],
                "machines": {},
                "commands": { "build": null, "test": null, "lint": null, "deploy": null },
                "environment": {},
                "experimentalMultiAgent": false,
                "allowedDriverKinds": ["terminal_driver", "structured_chat_driver", "api_driver"],
                "defaultSkillSet": [],
                "delegationLimits": {
                    "maxDepth": 2,
                    "maxChildren": 4,
                    "budgetTokens": null,
                    "budgetSecs": 900
                },
                "communicationPolicy": "supervisor_mailbox"
            })
            .to_string(),
        };

        let id = uuid::Uuid::new_v4().to_string();
        self.create_project(&id, &name, workdir, &config_json)
    }

    pub fn create_project(
        &self,
        id: &str,
        name: &str,
        workdir: &str,
        config_json: &str,
    ) -> Result<ProjectRecord, rusqlite::Error> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "INSERT INTO projects (id, name, workdir, config_json, registered_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, name, workdir, config_json, now, now],
        )?;
        Ok(ProjectRecord {
            id: id.to_string(),
            name: name.to_string(),
            workdir: workdir.to_string(),
            config_json: config_json.to_string(),
            registered_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn list_projects(&self) -> Result<Vec<ProjectRecord>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, workdir, config_json, registered_at, updated_at
             FROM projects ORDER BY name ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ProjectRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                workdir: row.get(2)?,
                config_json: row.get(3)?,
                registered_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        rows.collect()
    }

    pub fn get_project(&self, id: &str) -> Result<Option<ProjectRecord>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, workdir, config_json, registered_at, updated_at
             FROM projects WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(ProjectRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                workdir: row.get(2)?,
                config_json: row.get(3)?,
                registered_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn get_project_by_workdir(
        &self,
        workdir: &str,
    ) -> Result<Option<ProjectRecord>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, workdir, config_json, registered_at, updated_at
             FROM projects WHERE workdir = ?1",
        )?;
        let mut rows = stmt.query_map(params![workdir], |row| {
            Ok(ProjectRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                workdir: row.get(2)?,
                config_json: row.get(3)?,
                registered_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn update_project(
        &self,
        id: &str,
        config_json: &str,
    ) -> Result<(), rusqlite::Error> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "UPDATE projects SET config_json = ?1, updated_at = ?2 WHERE id = ?3",
            params![config_json, now, id],
        )?;
        Ok(())
    }

    pub fn remove_project(&self, id: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_store;

    #[test]
    fn test_create_and_get_project() {
        let store = test_store();
        let project = store
            .create_project("p1", "my-app", "/home/user/my-app", "{}")
            .unwrap();
        assert_eq!(project.id, "p1");
        assert_eq!(project.name, "my-app");
        assert_eq!(project.workdir, "/home/user/my-app");
        assert_eq!(project.config_json, "{}");

        let fetched = store.get_project("p1").unwrap().unwrap();
        assert_eq!(fetched.id, "p1");
        assert_eq!(fetched.name, "my-app");
        assert_eq!(fetched.workdir, "/home/user/my-app");

        let missing = store.get_project("nonexistent").unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn test_get_project_by_workdir() {
        let store = test_store();
        store
            .create_project("p1", "my-app", "/home/user/my-app", "{}")
            .unwrap();

        let found = store
            .get_project_by_workdir("/home/user/my-app")
            .unwrap()
            .unwrap();
        assert_eq!(found.id, "p1");

        let missing = store
            .get_project_by_workdir("/home/user/other")
            .unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn test_list_projects() {
        let store = test_store();
        store
            .create_project("p2", "zebra-app", "/home/user/zebra-app", "{}")
            .unwrap();
        store
            .create_project("p1", "alpha-app", "/home/user/alpha-app", "{}")
            .unwrap();
        store
            .create_project("p3", "middle-app", "/home/user/middle-app", "{}")
            .unwrap();

        let projects = store.list_projects().unwrap();
        assert_eq!(projects.len(), 3);
        // Ordered by name ASC
        assert_eq!(projects[0].name, "alpha-app");
        assert_eq!(projects[1].name, "middle-app");
        assert_eq!(projects[2].name, "zebra-app");
    }

    #[test]
    fn test_get_or_create_for_workdir_creates_new() {
        let store = test_store();
        let result = store
            .get_or_create_project_for_workdir("/home/user/my-app", None)
            .unwrap();
        assert_eq!(result.name, "my-app");
        assert_eq!(result.workdir, "/home/user/my-app");
        // Should parse as valid JSON
        let config: serde_json::Value = serde_json::from_str(&result.config_json).unwrap();
        assert_eq!(config["name"], "my-app");
    }

    #[test]
    fn test_get_or_create_for_workdir_returns_existing() {
        let store = test_store();
        store
            .create_project("p1", "existing-app", "/home/user/my-app", r#"{"name":"existing-app"}"#)
            .unwrap();
        let result = store
            .get_or_create_project_for_workdir("/home/user/my-app", None)
            .unwrap();
        assert_eq!(result.id, "p1");
        assert_eq!(result.name, "existing-app");
    }

    #[test]
    fn test_get_or_create_with_custom_config() {
        let store = test_store();
        let config = r#"{"name":"custom","workdir":"/home/user/custom"}"#;
        let result = store
            .get_or_create_project_for_workdir("/home/user/custom", Some(config))
            .unwrap();
        assert_eq!(result.name, "custom");
        let parsed: serde_json::Value = serde_json::from_str(&result.config_json).unwrap();
        assert_eq!(parsed["name"], "custom");
    }

    #[test]
    fn test_update_and_remove_project() {
        let store = test_store();
        store
            .create_project("p1", "my-app", "/home/user/my-app", "{}")
            .unwrap();

        store
            .update_project("p1", r#"{"key":"value"}"#)
            .unwrap();

        let updated = store.get_project("p1").unwrap().unwrap();
        assert_eq!(updated.config_json, r#"{"key":"value"}"#);

        store.remove_project("p1").unwrap();
        let projects = store.list_projects().unwrap();
        assert_eq!(projects.len(), 0);
    }
}
