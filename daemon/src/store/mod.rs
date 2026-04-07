pub mod chat;
pub mod chunks;
pub mod delegations;
pub mod discoveries;
pub mod hosts;
pub mod outcomes;
pub mod permissions;
pub mod projects;
pub mod sessions;
pub mod skills;

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};
use std::error::Error;
use std::path::Path;
use std::sync::{Arc, Mutex};

const MIGRATIONS_SLICE: &[M<'static>] = &[
    M::up(include_str!("../../migrations/001_initial.sql")),
    M::up(include_str!("../../migrations/002_known_hosts.sql")),
    M::up(include_str!("../../migrations/003_peer_permissions.sql")),
    M::up(include_str!("../../migrations/004_discovered_peers.sql")),
    M::up(include_str!("../../migrations/005_outcome_log.sql")),
    M::up(include_str!("../../migrations/006_projects_and_chat.sql")),
    M::up(include_str!("../../migrations/007_session_metadata.sql")),
    M::up(include_str!("../../migrations/008_session_delegation.sql")),
    M::up(include_str!("../../migrations/009_supervisor_core.sql")),
    M::up(include_str!("../../migrations/010_code_server.sql")),
    M::up(include_str!("../../migrations/011_intelligence.sql")),
];

const MIGRATIONS: Migrations<'static> = Migrations::from_slice(MIGRATIONS_SLICE);
#[cfg(test)]
const LATEST_SCHEMA_VERSION: usize = MIGRATIONS_SLICE.len();

pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

impl Store {
    pub fn open(db_path: &Path) -> Result<Self, Box<dyn Error>> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        MIGRATIONS.to_latest(&mut conn)?;

        Ok(Store {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("db lock poisoned")
    }
}

impl Clone for Store {
    fn clone(&self) -> Self {
        Store {
            conn: Arc::clone(&self.conn),
        }
    }
}

#[cfg(test)]
pub fn test_store() -> Store {
    Store::open(Path::new(":memory:")).expect("in-memory db")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroUsize;

    #[test]
    fn migrations_validate() {
        MIGRATIONS
            .validate()
            .expect("migration set should be valid");
    }

    #[test]
    fn open_sets_schema_version_on_fresh_db() {
        let store = test_store();
        let conn = store.conn();

        assert_eq!(
            MIGRATIONS.current_version(&conn).expect("schema version"),
            rusqlite_migration::SchemaVersion::Inside(
                NonZeroUsize::new(LATEST_SCHEMA_VERSION).expect("non-zero schema version"),
            )
        );
        assert!(column_exists(&conn, "terminal_sessions", "session_type"));
        assert!(column_exists(&conn, "terminal_sessions", "project_id"));
        assert!(column_exists(
            &conn,
            "terminal_sessions",
            "parent_session_id"
        ));
        assert!(column_exists(&conn, "terminal_sessions", "root_session_id"));
        assert!(column_exists(&conn, "terminal_sessions", "port"));
        assert!(table_exists(&conn, "delegation_contracts"));
        assert!(table_exists(&conn, "agent_messages"));
        assert!(table_exists(&conn, "skill_candidates"));
    }

    fn table_exists(conn: &Connection, table_name: &str) -> bool {
        conn.query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1",
            [table_name],
            |row| row.get::<_, i64>(0),
        )
        .ok()
        .is_some()
    }

    fn column_exists(conn: &Connection, table_name: &str, column_name: &str) -> bool {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table_name})"))
            .expect("prepare table info");
        let mut rows = stmt.query([]).expect("query table info");

        while let Some(row) = rows.next().expect("next row") {
            if row.get::<_, String>(1).expect("column name") == column_name {
                return true;
            }
        }

        false
    }
}
