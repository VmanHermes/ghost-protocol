pub mod sessions;
pub mod chunks;
pub mod hosts;
pub mod permissions;

use std::path::Path;
use std::sync::{Arc, Mutex};
use rusqlite::Connection;

pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

impl Store {
    pub fn open(db_path: &Path) -> Result<Self, rusqlite::Error> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let migration_001 = include_str!("../../migrations/001_initial.sql");
        conn.execute_batch(migration_001)?;
        let migration_002 = include_str!("../../migrations/002_known_hosts.sql");
        conn.execute_batch(migration_002)?;
        let migration_003 = include_str!("../../migrations/003_peer_permissions.sql");
        conn.execute_batch(migration_003)?;
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
