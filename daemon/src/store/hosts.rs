use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::Store;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownHost {
    pub id: String,
    pub name: String,
    pub tailscale_ip: String,
    pub url: String,
    pub status: String,
    pub last_seen: Option<String>,
    pub capabilities: Option<HostCapabilities>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostCapabilities {
    pub gpu: Option<String>,
    pub ram_gb: Option<f64>,
    pub hermes: bool,
    pub ollama: bool,
    pub agents: Option<Vec<crate::hardware::agents::AgentInfo>>,
}

impl Store {
    pub fn add_known_host(
        &self,
        id: &str,
        name: &str,
        tailscale_ip: &str,
        url: &str,
    ) -> Result<KnownHost, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO known_hosts (id, name, tailscale_ip, url, status)
             VALUES (?1, ?2, ?3, ?4, 'unknown')",
            params![id, name, tailscale_ip, url],
        )?;
        Ok(KnownHost {
            id: id.to_string(),
            name: name.to_string(),
            tailscale_ip: tailscale_ip.to_string(),
            url: url.to_string(),
            status: "unknown".to_string(),
            last_seen: None,
            capabilities: None,
        })
    }

    pub fn list_known_hosts(&self) -> Result<Vec<KnownHost>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, tailscale_ip, url, status, last_seen, capabilities_json
             FROM known_hosts ORDER BY name ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let caps_json: Option<String> = row.get(6)?;
            let capabilities = caps_json
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok());
            Ok(KnownHost {
                id: row.get(0)?,
                name: row.get(1)?,
                tailscale_ip: row.get(2)?,
                url: row.get(3)?,
                status: row.get(4)?,
                last_seen: row.get(5)?,
                capabilities,
            })
        })?;
        rows.collect()
    }

    pub fn get_known_host(&self, id: &str) -> Result<Option<KnownHost>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, tailscale_ip, url, status, last_seen, capabilities_json
             FROM known_hosts WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            let caps_json: Option<String> = row.get(6)?;
            let capabilities = caps_json
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok());
            Ok(KnownHost {
                id: row.get(0)?,
                name: row.get(1)?,
                tailscale_ip: row.get(2)?,
                url: row.get(3)?,
                status: row.get(4)?,
                last_seen: row.get(5)?,
                capabilities,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn update_host_status(
        &self,
        id: &str,
        status: &str,
        capabilities: Option<&HostCapabilities>,
    ) -> Result<(), rusqlite::Error> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn();
        let caps_json = capabilities.map(|c| serde_json::to_string(c).unwrap());
        conn.execute(
            "UPDATE known_hosts SET status = ?1, last_seen = ?2, capabilities_json = ?3 WHERE id = ?4",
            params![status, now, caps_json, id],
        )?;
        Ok(())
    }

    pub fn remove_known_host(&self, id: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute("DELETE FROM known_hosts WHERE id = ?1", params![id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_store;
    use super::*;

    #[test]
    fn test_add_and_list_hosts() {
        let store = test_store();
        store
            .add_known_host("h1", "laptop", "100.64.1.2", "http://100.64.1.2:8787")
            .unwrap();
        store
            .add_known_host("h2", "server", "100.64.1.3", "http://100.64.1.3:8787")
            .unwrap();

        let hosts = store.list_known_hosts().unwrap();
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].name, "laptop");
    }

    #[test]
    fn test_update_host_status() {
        let store = test_store();
        store
            .add_known_host("h1", "laptop", "100.64.1.2", "http://100.64.1.2:8787")
            .unwrap();

        let caps = HostCapabilities {
            gpu: None,
            ram_gb: Some(16.0),
            hermes: false,
            ollama: false,
            agents: None,
        };
        store
            .update_host_status("h1", "online", Some(&caps))
            .unwrap();

        let host = store.get_known_host("h1").unwrap().unwrap();
        assert_eq!(host.status, "online");
        assert!(host.last_seen.is_some());
        let caps = host.capabilities.unwrap();
        assert_eq!(caps.ram_gb, Some(16.0));
    }

    #[test]
    fn test_remove_host() {
        let store = test_store();
        store
            .add_known_host("h1", "laptop", "100.64.1.2", "http://100.64.1.2:8787")
            .unwrap();
        store.remove_known_host("h1").unwrap();
        let hosts = store.list_known_hosts().unwrap();
        assert_eq!(hosts.len(), 0);
    }
}
