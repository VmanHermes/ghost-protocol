use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::Store;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

impl Store {
    pub fn create_chat_message(
        &self,
        id: &str,
        session_id: &str,
        role: &str,
        content: &str,
    ) -> Result<ChatMessage, rusqlite::Error> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "INSERT INTO chat_messages (id, session_id, role, content, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, session_id, role, content, now],
        )?;
        Ok(ChatMessage {
            id: id.to_string(),
            session_id: session_id.to_string(),
            role: role.to_string(),
            content: content.to_string(),
            created_at: now,
        })
    }

    pub fn list_chat_messages(
        &self,
        session_id: &str,
        after_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ChatMessage>, rusqlite::Error> {
        let conn = self.conn();

        let rows = if let Some(cursor_id) = after_id {
            let mut cursor_stmt = conn.prepare(
                "SELECT created_at FROM chat_messages WHERE id = ?1",
            )?;
            let cursor_ts: Option<String> = cursor_stmt
                .query_map(params![cursor_id], |row| row.get(0))?
                .next()
                .transpose()?;

            if let Some(ts) = cursor_ts {
                let mut stmt = conn.prepare(
                    "SELECT id, session_id, role, content, created_at
                     FROM chat_messages
                     WHERE session_id = ?1 AND created_at > ?2
                     ORDER BY created_at ASC
                     LIMIT ?3",
                )?;
                let rows = stmt.query_map(params![session_id, ts, limit as i64], |row| {
                    Ok(ChatMessage {
                        id: row.get(0)?,
                        session_id: row.get(1)?,
                        role: row.get(2)?,
                        content: row.get(3)?,
                        created_at: row.get(4)?,
                    })
                })?;
                rows.collect::<Result<Vec<_>, _>>()?
            } else {
                vec![]
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, session_id, role, content, created_at
                 FROM (
                     SELECT id, session_id, role, content, created_at
                     FROM chat_messages
                     WHERE session_id = ?1
                     ORDER BY created_at DESC
                     LIMIT ?2
                 )
                 ORDER BY created_at ASC",
            )?;
            let rows = stmt.query_map(params![session_id, limit as i64], |row| {
                Ok(ChatMessage {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    role: row.get(2)?,
                    content: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        Ok(rows)
    }

    pub fn count_user_messages(&self, session_id: &str) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.query_row(
            "SELECT COUNT(*) FROM chat_messages WHERE session_id = ?1 AND role = 'user'",
            params![session_id],
            |row| row.get(0),
        )
    }

    pub fn get_chat_message(&self, id: &str) -> Result<Option<ChatMessage>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, role, content, created_at
             FROM chat_messages WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(ChatMessage {
                id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                created_at: row.get(4)?,
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

    fn setup_session(store: &super::Store, session_id: &str) {
        store
            .create_terminal_session(session_id, "chat", None, "/tmp", &["chat".to_string()], "chat", None)
            .unwrap();
    }

    #[test]
    fn test_create_and_get_chat_message() {
        let store = test_store();
        setup_session(&store, "s1");

        let msg = store
            .create_chat_message("m1", "s1", "user", "Hello, world!")
            .unwrap();

        assert_eq!(msg.id, "m1");
        assert_eq!(msg.session_id, "s1");
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "Hello, world!");
        assert!(!msg.created_at.is_empty());

        let fetched = store.get_chat_message("m1").unwrap().unwrap();
        assert_eq!(fetched.id, "m1");
        assert_eq!(fetched.session_id, "s1");
        assert_eq!(fetched.role, "user");
        assert_eq!(fetched.content, "Hello, world!");
    }

    #[test]
    fn test_list_messages_for_session() {
        let store = test_store();
        setup_session(&store, "s1");

        store.create_chat_message("m1", "s1", "user", "First").unwrap();
        store.create_chat_message("m2", "s1", "assistant", "Second").unwrap();
        store.create_chat_message("m3", "s1", "user", "Third").unwrap();

        let messages = store.list_chat_messages("s1", None, 100).unwrap();
        assert_eq!(messages.len(), 3);

        // Verify ASC order by created_at
        assert_eq!(messages[0].id, "m1");
        assert_eq!(messages[1].id, "m2");
        assert_eq!(messages[2].id, "m3");

        // Verify roles
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[2].role, "user");
    }

    #[test]
    fn test_list_messages_with_cursor() {
        let store = test_store();
        setup_session(&store, "s1");

        // Insert messages with small sleeps to ensure distinct created_at timestamps
        store.create_chat_message("m1", "s1", "user", "One").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        store.create_chat_message("m2", "s1", "user", "Two").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        store.create_chat_message("m3", "s1", "user", "Three").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        store.create_chat_message("m4", "s1", "user", "Four").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        store.create_chat_message("m5", "s1", "user", "Five").unwrap();

        // List after m2 — should return m3, m4, m5
        let messages = store.list_chat_messages("s1", Some("m2"), 100).unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].id, "m3");
        assert_eq!(messages[1].id, "m4");
        assert_eq!(messages[2].id, "m5");
    }

    #[test]
    fn test_list_messages_without_cursor_returns_latest_window() {
        let store = test_store();
        setup_session(&store, "s1");

        store.create_chat_message("m1", "s1", "user", "One").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        store.create_chat_message("m2", "s1", "assistant", "Two").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        store.create_chat_message("m3", "s1", "user", "Three").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        store.create_chat_message("m4", "s1", "assistant", "Four").unwrap();

        let messages = store.list_chat_messages("s1", None, 2).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].id, "m3");
        assert_eq!(messages[1].id, "m4");
    }
}
