use chrono::Utc;
use rusqlite::params;
use serde::Serialize;

use super::Store;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSessionRecord {
    pub id: String,
    pub mode: String,
    pub status: String,
    pub name: Option<String>,
    pub workdir: String,
    pub command: Vec<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub last_chunk_at: Option<String>,
    pub pid: Option<i64>,
    pub exit_code: Option<i32>,
    pub session_type: String,
    pub project_id: Option<String>,
    pub parent_session_id: Option<String>,
    pub host_id: Option<String>,
    pub host_name: Option<String>,
}

impl Store {
    pub fn create_terminal_session(
        &self,
        id: &str,
        mode: &str,
        name: Option<&str>,
        workdir: &str,
        command: &[String],
        session_type: &str,
        project_id: Option<&str>,
    ) -> Result<TerminalSessionRecord, rusqlite::Error> {
        let now = Utc::now().to_rfc3339();
        let command_json = serde_json::to_string(command).unwrap();
        let conn = self.conn();
        conn.execute(
            "INSERT INTO terminal_sessions (id, mode, status, name, workdir, command_json, created_at, session_type, project_id)
             VALUES (?1, ?2, 'created', ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, mode, name, workdir, command_json, now, session_type, project_id],
        )?;
        Ok(TerminalSessionRecord {
            id: id.to_string(),
            mode: mode.to_string(),
            status: "created".to_string(),
            name: name.map(|s| s.to_string()),
            workdir: workdir.to_string(),
            command: command.to_vec(),
            created_at: now,
            started_at: None,
            finished_at: None,
            last_chunk_at: None,
            pid: None,
            exit_code: None,
            session_type: session_type.to_string(),
            project_id: project_id.map(|s| s.to_string()),
            parent_session_id: None,
            host_id: None,
            host_name: None,
        })
    }

    pub fn update_terminal_session(
        &self,
        session_id: &str,
        status: Option<&str>,
        started_at: Option<&str>,
        finished_at: Option<&str>,
        last_chunk_at: Option<&str>,
        pid: Option<i64>,
        exit_code: Option<i32>,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        if let Some(v) = status {
            conn.execute(
                "UPDATE terminal_sessions SET status = ?1 WHERE id = ?2",
                params![v, session_id],
            )?;
        }
        if let Some(v) = started_at {
            conn.execute(
                "UPDATE terminal_sessions SET started_at = ?1 WHERE id = ?2",
                params![v, session_id],
            )?;
        }
        if let Some(v) = finished_at {
            conn.execute(
                "UPDATE terminal_sessions SET finished_at = ?1 WHERE id = ?2",
                params![v, session_id],
            )?;
        }
        if let Some(v) = last_chunk_at {
            conn.execute(
                "UPDATE terminal_sessions SET last_chunk_at = ?1 WHERE id = ?2",
                params![v, session_id],
            )?;
        }
        if let Some(v) = pid {
            conn.execute(
                "UPDATE terminal_sessions SET pid = ?1 WHERE id = ?2",
                params![v, session_id],
            )?;
        }
        if let Some(v) = exit_code {
            conn.execute(
                "UPDATE terminal_sessions SET exit_code = ?1 WHERE id = ?2",
                params![v, session_id],
            )?;
        }
        Ok(())
    }

    pub fn get_terminal_session(
        &self,
        session_id: &str,
    ) -> Result<Option<TerminalSessionRecord>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, mode, status, name, workdir, command_json, created_at,
                    started_at, finished_at, last_chunk_at, pid, exit_code,
                    session_type, project_id, parent_session_id, host_id, host_name
             FROM terminal_sessions WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![session_id], |row| {
            let command_json: String = row.get(5)?;
            let command: Vec<String> =
                serde_json::from_str(&command_json).unwrap_or_default();
            Ok(TerminalSessionRecord {
                id: row.get(0)?,
                mode: row.get(1)?,
                status: row.get(2)?,
                name: row.get(3)?,
                workdir: row.get(4)?,
                command,
                created_at: row.get(6)?,
                started_at: row.get(7)?,
                finished_at: row.get(8)?,
                last_chunk_at: row.get(9)?,
                pid: row.get(10)?,
                exit_code: row.get(11)?,
                session_type: row.get(12)?,
                project_id: row.get(13)?,
                parent_session_id: row.get(14)?,
                host_id: row.get(15)?,
                host_name: row.get(16)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn list_terminal_sessions(
        &self,
    ) -> Result<Vec<TerminalSessionRecord>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, mode, status, name, workdir, command_json, created_at,
                    started_at, finished_at, last_chunk_at, pid, exit_code,
                    session_type, project_id, parent_session_id, host_id, host_name
             FROM terminal_sessions ORDER BY created_at DESC, id ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let command_json: String = row.get(5)?;
            let command: Vec<String> =
                serde_json::from_str(&command_json).unwrap_or_default();
            Ok(TerminalSessionRecord {
                id: row.get(0)?,
                mode: row.get(1)?,
                status: row.get(2)?,
                name: row.get(3)?,
                workdir: row.get(4)?,
                command,
                created_at: row.get(6)?,
                started_at: row.get(7)?,
                finished_at: row.get(8)?,
                last_chunk_at: row.get(9)?,
                pid: row.get(10)?,
                exit_code: row.get(11)?,
                session_type: row.get(12)?,
                project_id: row.get(13)?,
                parent_session_id: row.get(14)?,
                host_id: row.get(15)?,
                host_name: row.get(16)?,
            })
        })?;
        rows.collect()
    }

    pub fn terminate_incomplete_sessions(&self) -> Result<usize, rusqlite::Error> {
        let conn = self.conn();
        let count = conn.execute(
            "UPDATE terminal_sessions SET status = 'terminated'
             WHERE status IN ('created', 'running')",
            [],
        )?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_store;

    #[test]
    fn test_create_and_get_session() {
        let store = test_store();
        let cmd = vec!["bash".to_string(), "-c".to_string(), "echo hi".to_string()];
        let rec = store
            .create_terminal_session("s1", "local", Some("test"), "/tmp", &cmd, "terminal", None)
            .unwrap();
        assert_eq!(rec.id, "s1");
        assert_eq!(rec.status, "created");

        let fetched = store.get_terminal_session("s1").unwrap().unwrap();
        assert_eq!(fetched.id, "s1");
        assert_eq!(fetched.command, cmd);
        assert_eq!(fetched.name, Some("test".to_string()));
    }

    #[test]
    fn test_update_session_status() {
        let store = test_store();
        let cmd = vec!["bash".to_string()];
        store
            .create_terminal_session("s2", "local", None, "/tmp", &cmd, "terminal", None)
            .unwrap();
        store
            .update_terminal_session("s2", Some("running"), Some("2026-01-01T00:00:00Z"), None, None, Some(1234), None)
            .unwrap();
        let s = store.get_terminal_session("s2").unwrap().unwrap();
        assert_eq!(s.status, "running");
        assert_eq!(s.started_at, Some("2026-01-01T00:00:00Z".to_string()));
        assert_eq!(s.pid, Some(1234));
    }

    #[test]
    fn test_list_sessions() {
        let store = test_store();
        let cmd = vec!["bash".to_string()];
        store
            .create_terminal_session("a", "local", None, "/tmp", &cmd, "terminal", None)
            .unwrap();
        store
            .create_terminal_session("b", "local", None, "/tmp", &cmd, "terminal", None)
            .unwrap();
        let list = store.list_terminal_sessions().unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_terminate_incomplete_sessions() {
        let store = test_store();
        let cmd = vec!["bash".to_string()];
        store
            .create_terminal_session("t1", "local", None, "/tmp", &cmd, "terminal", None)
            .unwrap();
        store
            .create_terminal_session("t2", "local", None, "/tmp", &cmd, "terminal", None)
            .unwrap();
        store
            .update_terminal_session("t2", Some("running"), None, None, None, None, None)
            .unwrap();
        store
            .create_terminal_session("t3", "local", None, "/tmp", &cmd, "terminal", None)
            .unwrap();
        store
            .update_terminal_session("t3", Some("exited"), None, None, None, None, None)
            .unwrap();

        let count = store.terminate_incomplete_sessions().unwrap();
        assert_eq!(count, 2);

        let s1 = store.get_terminal_session("t1").unwrap().unwrap();
        assert_eq!(s1.status, "terminated");
        let s2 = store.get_terminal_session("t2").unwrap().unwrap();
        assert_eq!(s2.status, "terminated");
        let s3 = store.get_terminal_session("t3").unwrap().unwrap();
        assert_eq!(s3.status, "exited");
    }
}
