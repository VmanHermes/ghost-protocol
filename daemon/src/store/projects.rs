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
