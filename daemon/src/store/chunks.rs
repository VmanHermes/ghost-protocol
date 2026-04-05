use chrono::Utc;
use rusqlite::params;
use serde::Serialize;

use super::Store;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalChunkRecord {
    pub id: i64,
    pub session_id: String,
    pub stream: String,
    pub chunk: String,
    pub created_at: String,
}

impl Store {
    pub fn append_terminal_chunk(
        &self,
        session_id: &str,
        stream: &str,
        chunk: &str,
    ) -> Result<TerminalChunkRecord, rusqlite::Error> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "INSERT INTO terminal_chunks (session_id, stream, chunk, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![session_id, stream, chunk, now],
        )?;
        let id = conn.last_insert_rowid();
        conn.execute(
            "UPDATE terminal_sessions SET last_chunk_at = ?1 WHERE id = ?2",
            params![now, session_id],
        )?;
        Ok(TerminalChunkRecord {
            id,
            session_id: session_id.to_string(),
            stream: stream.to_string(),
            chunk: chunk.to_string(),
            created_at: now,
        })
    }

    pub fn list_terminal_chunks(
        &self,
        session_id: &str,
        after_chunk_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<TerminalChunkRecord>, rusqlite::Error> {
        let conn = self.conn();
        let after = after_chunk_id.unwrap_or(0);
        let mut stmt = conn.prepare(
            "SELECT id, session_id, stream, chunk, created_at
             FROM terminal_chunks
             WHERE session_id = ?1 AND id > ?2
             ORDER BY id ASC
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![session_id, after, limit], |row| {
            Ok(TerminalChunkRecord {
                id: row.get(0)?,
                session_id: row.get(1)?,
                stream: row.get(2)?,
                chunk: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        rows.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_store;

    #[test]
    fn test_append_and_list_chunks() {
        let store = test_store();
        let cmd = vec!["bash".to_string()];
        store
            .create_terminal_session("s1", "local", None, "/tmp", &cmd, "terminal", None)
            .unwrap();

        let c1 = store.append_terminal_chunk("s1", "stdout", "hello ").unwrap();
        let c2 = store.append_terminal_chunk("s1", "stdout", "world").unwrap();
        assert_eq!(c1.id, 1);
        assert_eq!(c2.id, 2);

        let chunks = store.list_terminal_chunks("s1", None, 100).unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].chunk, "hello ");
        assert_eq!(chunks[1].chunk, "world");
    }

    #[test]
    fn test_list_chunks_after_id() {
        let store = test_store();
        let cmd = vec!["bash".to_string()];
        store
            .create_terminal_session("s1", "local", None, "/tmp", &cmd, "terminal", None)
            .unwrap();

        store.append_terminal_chunk("s1", "stdout", "a").unwrap();
        store.append_terminal_chunk("s1", "stdout", "b").unwrap();
        store.append_terminal_chunk("s1", "stdout", "c").unwrap();

        let chunks = store.list_terminal_chunks("s1", Some(1), 100).unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].chunk, "b");
        assert_eq!(chunks[1].chunk, "c");
    }

    #[test]
    fn test_append_updates_last_chunk_at() {
        let store = test_store();
        let cmd = vec!["bash".to_string()];
        store
            .create_terminal_session("s1", "local", None, "/tmp", &cmd, "terminal", None)
            .unwrap();

        let before = store.get_terminal_session("s1").unwrap().unwrap();
        assert!(before.last_chunk_at.is_none());

        store.append_terminal_chunk("s1", "stdout", "data").unwrap();

        let after = store.get_terminal_session("s1").unwrap().unwrap();
        assert!(after.last_chunk_at.is_some());
    }
}
