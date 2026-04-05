use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::Store;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerPermission {
    pub host_id: String,
    pub tier: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingApproval {
    pub id: String,
    pub host_id: String,
    pub method: String,
    pub path: String,
    pub body_json: Option<String>,
    pub status: String,
    pub created_at: String,
    pub resolved_at: Option<String>,
    pub expires_at: String,
    pub result_json: Option<String>,
}

impl Store {
    /// UPSERT a tier for a given host_id into peer_permissions.
    pub fn set_peer_permission(&self, host_id: &str, tier: &str) -> Result<(), rusqlite::Error> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "INSERT INTO peer_permissions (host_id, tier, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(host_id) DO UPDATE SET tier = excluded.tier, updated_at = excluded.updated_at",
            params![host_id, tier, now],
        )?;
        Ok(())
    }

    /// Fetch the permission row for a host_id, or None if not yet set.
    pub fn get_peer_permission(
        &self,
        host_id: &str,
    ) -> Result<Option<PeerPermission>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT host_id, tier, updated_at FROM peer_permissions WHERE host_id = ?1",
        )?;
        let mut rows = stmt.query_map(params![host_id], |row| {
            Ok(PeerPermission {
                host_id: row.get(0)?,
                tier: row.get(1)?,
                updated_at: row.get(2)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// List all permission rows, newest first.
    pub fn list_peer_permissions(&self) -> Result<Vec<PeerPermission>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT host_id, tier, updated_at FROM peer_permissions ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(PeerPermission {
                host_id: row.get(0)?,
                tier: row.get(1)?,
                updated_at: row.get(2)?,
            })
        })?;
        rows.collect()
    }

    /// Return the tier string for the host whose tailscale_ip matches, or None.
    pub fn resolve_tier_by_ip(&self, ip: &str) -> Result<Option<String>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT pp.tier
             FROM peer_permissions pp
             JOIN known_hosts kh ON kh.id = pp.host_id
             WHERE kh.tailscale_ip = ?1",
        )?;
        let mut rows = stmt.query_map(params![ip], |row| row.get(0))?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// Return the known_hosts.id for the host whose tailscale_ip matches, or None.
    pub fn resolve_host_id_by_ip(&self, ip: &str) -> Result<Option<String>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt =
            conn.prepare("SELECT id FROM known_hosts WHERE tailscale_ip = ?1")?;
        let mut rows = stmt.query_map(params![ip], |row| row.get(0))?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// Insert a new pending approval request.
    pub fn create_approval(
        &self,
        id: &str,
        host_id: &str,
        method: &str,
        path: &str,
        body_json: Option<&str>,
        expires_at: &str,
    ) -> Result<PendingApproval, rusqlite::Error> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "INSERT INTO pending_approvals
             (id, host_id, method, path, body_json, status, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, ?7)",
            params![id, host_id, method, path, body_json, now, expires_at],
        )?;
        Ok(PendingApproval {
            id: id.to_string(),
            host_id: host_id.to_string(),
            method: method.to_string(),
            path: path.to_string(),
            body_json: body_json.map(|s| s.to_string()),
            status: "pending".to_string(),
            created_at: now,
            resolved_at: None,
            expires_at: expires_at.to_string(),
            result_json: None,
        })
    }

    /// Fetch a single approval by id, or None.
    pub fn get_approval(&self, id: &str) -> Result<Option<PendingApproval>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, host_id, method, path, body_json, status, created_at,
                    resolved_at, expires_at, result_json
             FROM pending_approvals WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(PendingApproval {
                id: row.get(0)?,
                host_id: row.get(1)?,
                method: row.get(2)?,
                path: row.get(3)?,
                body_json: row.get(4)?,
                status: row.get(5)?,
                created_at: row.get(6)?,
                resolved_at: row.get(7)?,
                expires_at: row.get(8)?,
                result_json: row.get(9)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// List approvals, optionally filtered by status.
    pub fn list_approvals(
        &self,
        status_filter: Option<&str>,
    ) -> Result<Vec<PendingApproval>, rusqlite::Error> {
        let conn = self.conn();
        let sql_base = "SELECT id, host_id, method, path, body_json, status, created_at,
                               resolved_at, expires_at, result_json
                        FROM pending_approvals";
        let map_row = |row: &rusqlite::Row<'_>| {
            Ok(PendingApproval {
                id: row.get(0)?,
                host_id: row.get(1)?,
                method: row.get(2)?,
                path: row.get(3)?,
                body_json: row.get(4)?,
                status: row.get(5)?,
                created_at: row.get(6)?,
                resolved_at: row.get(7)?,
                expires_at: row.get(8)?,
                result_json: row.get(9)?,
            })
        };

        if let Some(status) = status_filter {
            let mut stmt =
                conn.prepare(&format!("{} WHERE status = ?1 ORDER BY created_at DESC", sql_base))?;
            let rows = stmt.query_map(params![status], map_row)?;
            rows.collect()
        } else {
            let mut stmt = conn.prepare(&format!("{} ORDER BY created_at DESC", sql_base))?;
            let rows = stmt.query_map([], map_row)?;
            rows.collect()
        }
    }

    /// Set status (and optionally result_json) on an approval, recording resolved_at.
    pub fn resolve_approval(
        &self,
        id: &str,
        status: &str,
        result_json: Option<&str>,
    ) -> Result<(), rusqlite::Error> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "UPDATE pending_approvals
             SET status = ?1, resolved_at = ?2, result_json = ?3
             WHERE id = ?4",
            params![status, now, result_json, id],
        )?;
        Ok(())
    }

    /// Mark all 'pending' approvals whose expires_at is in the past as 'expired'.
    /// Returns the number of rows updated.
    pub fn expire_stale_approvals(&self) -> Result<usize, rusqlite::Error> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn();
        let count = conn.execute(
            "UPDATE pending_approvals
             SET status = 'expired'
             WHERE status = 'pending' AND expires_at < ?1",
            params![now],
        )?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_store;
    use super::*;

    fn add_host(store: &Store, id: &str, ip: &str) {
        store
            .add_known_host(id, id, ip, &format!("http://{}:8787", ip))
            .unwrap();
    }

    #[test]
    fn test_set_and_get_permission() {
        let store = test_store();
        add_host(&store, "h1", "100.64.1.1");

        store.set_peer_permission("h1", "read-only").unwrap();
        let perm = store.get_peer_permission("h1").unwrap().unwrap();
        assert_eq!(perm.host_id, "h1");
        assert_eq!(perm.tier, "read-only");
        assert!(!perm.updated_at.is_empty());
    }

    #[test]
    fn test_get_permission_default_no_access() {
        let store = test_store();
        add_host(&store, "h1", "100.64.1.1");

        // No row inserted — should return None
        let perm = store.get_peer_permission("h1").unwrap();
        assert!(perm.is_none());
    }

    #[test]
    fn test_list_permissions_with_hosts() {
        let store = test_store();
        add_host(&store, "h1", "100.64.1.1");
        add_host(&store, "h2", "100.64.1.2");

        store.set_peer_permission("h1", "read-only").unwrap();
        store.set_peer_permission("h2", "full-access").unwrap();

        let perms = store.list_peer_permissions().unwrap();
        assert_eq!(perms.len(), 2);
        // Ordered by updated_at DESC — both inserted nearly simultaneously; just
        // verify all host_ids are present.
        let ids: Vec<&str> = perms.iter().map(|p| p.host_id.as_str()).collect();
        assert!(ids.contains(&"h1"));
        assert!(ids.contains(&"h2"));
    }

    #[test]
    fn test_resolve_tier_by_ip() {
        let store = test_store();
        add_host(&store, "h1", "100.64.1.1");
        store.set_peer_permission("h1", "read-only").unwrap();

        // Known IP
        let tier = store.resolve_tier_by_ip("100.64.1.1").unwrap();
        assert_eq!(tier, Some("read-only".to_string()));

        // Unknown IP
        let tier = store.resolve_tier_by_ip("10.0.0.99").unwrap();
        assert!(tier.is_none());
    }

    #[test]
    fn test_create_and_resolve_approval() {
        let store = test_store();
        add_host(&store, "h1", "100.64.1.1");

        let future = "2099-01-01T00:00:00+00:00";
        store
            .create_approval("req-1", "h1", "POST", "/api/run", None, future)
            .unwrap();

        let approval = store.get_approval("req-1").unwrap().unwrap();
        assert_eq!(approval.id, "req-1");
        assert_eq!(approval.status, "pending");
        assert!(approval.resolved_at.is_none());

        store
            .resolve_approval("req-1", "approved", Some(r#"{"ok":true}"#))
            .unwrap();

        let approval = store.get_approval("req-1").unwrap().unwrap();
        assert_eq!(approval.status, "approved");
        assert!(approval.resolved_at.is_some());
        assert_eq!(approval.result_json.as_deref(), Some(r#"{"ok":true}"#));
    }

    #[test]
    fn test_expire_stale_approvals() {
        let store = test_store();
        add_host(&store, "h1", "100.64.1.1");

        let past = "2000-01-01T00:00:00+00:00";
        let future = "2099-01-01T00:00:00+00:00";

        store
            .create_approval("req-stale", "h1", "GET", "/api/info", None, past)
            .unwrap();
        store
            .create_approval("req-fresh", "h1", "GET", "/api/info", None, future)
            .unwrap();

        let count = store.expire_stale_approvals().unwrap();
        assert_eq!(count, 1);

        let stale = store.get_approval("req-stale").unwrap().unwrap();
        assert_eq!(stale.status, "expired");

        let fresh = store.get_approval("req-fresh").unwrap().unwrap();
        assert_eq!(fresh.status, "pending");
    }

    #[test]
    fn test_list_approvals_filter() {
        let store = test_store();
        add_host(&store, "h1", "100.64.1.1");

        let future = "2099-01-01T00:00:00+00:00";
        store
            .create_approval("req-1", "h1", "GET", "/a", None, future)
            .unwrap();
        store
            .create_approval("req-2", "h1", "POST", "/b", None, future)
            .unwrap();
        store.resolve_approval("req-2", "approved", None).unwrap();

        // Filter by pending
        let pending = store.list_approvals(Some("pending")).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, "req-1");

        // Filter by approved
        let approved = store.list_approvals(Some("approved")).unwrap();
        assert_eq!(approved.len(), 1);
        assert_eq!(approved[0].id, "req-2");

        // No filter — all
        let all = store.list_approvals(None).unwrap();
        assert_eq!(all.len(), 2);
    }
}
