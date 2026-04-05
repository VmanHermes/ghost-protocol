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
    /// Insert or update a discovered peer. If the peer already exists with a
    /// non-pending status (dismissed/added), the status is preserved — only
    /// the name and discovered_at are updated when status is still 'pending'.
    pub fn upsert_discovered_peer(
        &self,
        ip: &str,
        name: &str,
    ) -> Result<(), rusqlite::Error> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "INSERT INTO discovered_peers (tailscale_ip, name, discovered_at, status)
             VALUES (?1, ?2, ?3, 'pending')
             ON CONFLICT(tailscale_ip) DO UPDATE SET
                 name = excluded.name,
                 discovered_at = excluded.discovered_at
             WHERE discovered_peers.status = 'pending'",
            params![ip, name, now],
        )?;
        Ok(())
    }

    /// List all peers with status = 'pending', newest first.
    pub fn list_pending_discoveries(&self) -> Result<Vec<DiscoveredPeer>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT tailscale_ip, name, discovered_at, status
             FROM discovered_peers
             WHERE status = 'pending'
             ORDER BY discovered_at DESC",
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

    /// Fetch a single discovered peer by IP, or None if not found.
    pub fn get_discovery(&self, ip: &str) -> Result<Option<DiscoveredPeer>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT tailscale_ip, name, discovered_at, status
             FROM discovered_peers WHERE tailscale_ip = ?1",
        )?;
        let mut rows = stmt.query_map(params![ip], |row| {
            Ok(DiscoveredPeer {
                tailscale_ip: row.get(0)?,
                name: row.get(1)?,
                discovered_at: row.get(2)?,
                status: row.get(3)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// Mark a discovered peer as 'added'.
    pub fn accept_discovery(&self, ip: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "UPDATE discovered_peers SET status = 'added' WHERE tailscale_ip = ?1",
            params![ip],
        )?;
        Ok(())
    }

    /// Mark a discovered peer as 'dismissed'.
    pub fn dismiss_discovery(&self, ip: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "UPDATE discovered_peers SET status = 'dismissed' WHERE tailscale_ip = ?1",
            params![ip],
        )?;
        Ok(())
    }

    /// Returns true if the IP is already in known_hosts OR in discovered_peers
    /// with status 'dismissed' or 'added'.
    pub fn is_known_or_dismissed(&self, ip: &str) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT 1 FROM known_hosts WHERE tailscale_ip = ?1
             UNION ALL
             SELECT 1 FROM discovered_peers
             WHERE tailscale_ip = ?1 AND status IN ('dismissed', 'added')
             LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![ip], |_row| Ok(()))?;
        Ok(rows.next().is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_store;

    #[test]
    fn test_upsert_and_list_pending() {
        let store = test_store();
        store.upsert_discovered_peer("100.64.1.10", "node-a").unwrap();
        store.upsert_discovered_peer("100.64.1.11", "node-b").unwrap();

        let pending = store.list_pending_discoveries().unwrap();
        assert_eq!(pending.len(), 2);
        assert!(pending.iter().all(|p| p.status == "pending"));
        let ips: Vec<&str> = pending.iter().map(|p| p.tailscale_ip.as_str()).collect();
        assert!(ips.contains(&"100.64.1.10"));
        assert!(ips.contains(&"100.64.1.11"));
    }

    #[test]
    fn test_upsert_does_not_overwrite_dismissed() {
        let store = test_store();
        store.upsert_discovered_peer("100.64.1.10", "node-a").unwrap();
        store.dismiss_discovery("100.64.1.10").unwrap();

        // Upsert again with a new name — status must remain 'dismissed'
        store.upsert_discovered_peer("100.64.1.10", "node-a-renamed").unwrap();

        let peer = store.get_discovery("100.64.1.10").unwrap().unwrap();
        assert_eq!(peer.status, "dismissed");

        // Should not appear in pending list
        let pending = store.list_pending_discoveries().unwrap();
        assert_eq!(pending.len(), 0);
    }

    #[test]
    fn test_accept_discovery() {
        let store = test_store();
        store.upsert_discovered_peer("100.64.1.20", "node-c").unwrap();

        store.accept_discovery("100.64.1.20").unwrap();

        let peer = store.get_discovery("100.64.1.20").unwrap().unwrap();
        assert_eq!(peer.status, "added");

        // Should not appear in pending list
        let pending = store.list_pending_discoveries().unwrap();
        assert_eq!(pending.len(), 0);
    }

    #[test]
    fn test_dismiss_discovery() {
        let store = test_store();
        store.upsert_discovered_peer("100.64.1.30", "node-d").unwrap();

        store.dismiss_discovery("100.64.1.30").unwrap();

        let peer = store.get_discovery("100.64.1.30").unwrap().unwrap();
        assert_eq!(peer.status, "dismissed");

        let pending = store.list_pending_discoveries().unwrap();
        assert_eq!(pending.len(), 0);
    }

    #[test]
    fn test_is_known_or_dismissed() {
        let store = test_store();

        // Completely unknown IP — false
        assert!(!store.is_known_or_dismissed("100.64.1.99").unwrap());

        // IP in known_hosts — true
        store
            .add_known_host("h1", "server", "100.64.1.50", "http://100.64.1.50:8787")
            .unwrap();
        assert!(store.is_known_or_dismissed("100.64.1.50").unwrap());

        // Pending discovery only — false
        store.upsert_discovered_peer("100.64.1.60", "node-e").unwrap();
        assert!(!store.is_known_or_dismissed("100.64.1.60").unwrap());

        // Dismissed discovery — true
        store.dismiss_discovery("100.64.1.60").unwrap();
        assert!(store.is_known_or_dismissed("100.64.1.60").unwrap());

        // Added (accepted) discovery — true
        store.upsert_discovered_peer("100.64.1.70", "node-f").unwrap();
        store.accept_discovery("100.64.1.70").unwrap();
        assert!(store.is_known_or_dismissed("100.64.1.70").unwrap());
    }
}
