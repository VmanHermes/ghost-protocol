use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::Store;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredPeer {
    pub tailscale_ip: String,
    pub name: String,
    pub discovered_at: String,
    pub status: String,
}

impl Store {
    /// Insert or update a discovered peer.
    pub fn upsert_discovered_peer(
        &self,
        tailscale_ip: &str,
        name: &str,
    ) -> Result<(), rusqlite::Error> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "INSERT INTO discovered_peers (tailscale_ip, name, discovered_at, status)
             VALUES (?1, ?2, ?3, 'pending')
             ON CONFLICT(tailscale_ip) DO UPDATE SET name = excluded.name",
            params![tailscale_ip, name, now],
        )?;
        Ok(())
    }

    /// List all discovered peers.
    pub fn list_discovered_peers(&self) -> Result<Vec<DiscoveredPeer>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT tailscale_ip, name, discovered_at, status FROM discovered_peers ORDER BY discovered_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(DiscoveredPeer {
                tailscale_ip: row.get(0)?,
                name: row.get(1)?,
                discovered_at: row.get(2)?,
                status: row.get(3)?,
            })
        })?;
        rows.collect()
    }
}
