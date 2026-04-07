use chrono::Utc;
use rusqlite::params;
use rusqlite::types::Type;
use serde::{Deserialize, Deserializer, Serialize};
use serde_rusqlite::{from_rows, to_params_named};

use super::Store;

const INSERT_KNOWN_HOST_SQL: &str = include_str!("../../sql/store/hosts/insert_known_host.sql");
const LIST_KNOWN_HOSTS_SQL: &str = include_str!("../../sql/store/hosts/list_known_hosts.sql");
const GET_KNOWN_HOST_SQL: &str = include_str!("../../sql/store/hosts/get_known_host.sql");
const UPDATE_HOST_STATUS_SQL: &str = include_str!("../../sql/store/hosts/update_host_status.sql");
const REMOVE_KNOWN_HOST_SQL: &str = include_str!("../../sql/store/hosts/remove_known_host.sql");

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
        let input = InsertKnownHostParams {
            id,
            name,
            tailscale_ip,
            url,
        };
        let params = to_params_named(&input).map_err(to_to_sql_error)?;
        conn.execute(INSERT_KNOWN_HOST_SQL, params.to_slice().as_slice())?;
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
        let mut stmt = conn.prepare(LIST_KNOWN_HOSTS_SQL)?;
        let rows = from_rows::<KnownHostRow>(stmt.query([])?);
        rows.map(|row| row.map(Into::into).map_err(to_from_sql_error))
            .collect()
    }

    pub fn get_known_host(&self, id: &str) -> Result<Option<KnownHost>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(GET_KNOWN_HOST_SQL)?;
        let params = to_params_named(&GetKnownHostParams { id }).map_err(to_to_sql_error)?;
        let mut rows = from_rows::<KnownHostRow>(stmt.query(params.to_slice().as_slice())?);
        match rows.next() {
            Some(row) => row.map(Into::into).map(Some).map_err(to_from_sql_error),
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
        let input = UpdateHostStatusParams {
            id,
            status,
            last_seen: &now,
            capabilities_json: capabilities
                .map(serde_json::to_string)
                .transpose()
                .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?,
        };
        let params = to_params_named(&input).map_err(to_to_sql_error)?;
        conn.execute(UPDATE_HOST_STATUS_SQL, params.to_slice().as_slice())?;
        Ok(())
    }

    pub fn remove_known_host(&self, id: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute(REMOVE_KNOWN_HOST_SQL, params![id])?;
        Ok(())
    }
}

#[derive(Serialize)]
struct InsertKnownHostParams<'a> {
    id: &'a str,
    name: &'a str,
    tailscale_ip: &'a str,
    url: &'a str,
}

#[derive(Serialize)]
struct GetKnownHostParams<'a> {
    id: &'a str,
}

#[derive(Serialize)]
struct UpdateHostStatusParams<'a> {
    id: &'a str,
    status: &'a str,
    last_seen: &'a str,
    capabilities_json: Option<String>,
}

#[derive(Deserialize)]
struct KnownHostRow {
    id: String,
    name: String,
    tailscale_ip: String,
    url: String,
    status: String,
    last_seen: Option<String>,
    #[serde(deserialize_with = "deserialize_capabilities")]
    capabilities: Option<HostCapabilities>,
}

impl From<KnownHostRow> for KnownHost {
    fn from(row: KnownHostRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            tailscale_ip: row.tailscale_ip,
            url: row.url,
            status: row.status,
            last_seen: row.last_seen,
            capabilities: row.capabilities,
        }
    }
}

fn deserialize_capabilities<'de, D>(deserializer: D) -> Result<Option<HostCapabilities>, D::Error>
where
    D: Deserializer<'de>,
{
    let caps_json = Option::<String>::deserialize(deserializer)?;
    caps_json
        .map(|json| serde_json::from_str(&json).map_err(serde::de::Error::custom))
        .transpose()
}

fn to_to_sql_error(err: serde_rusqlite::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(err))
}

fn to_from_sql_error(err: serde_rusqlite::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(err))
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
