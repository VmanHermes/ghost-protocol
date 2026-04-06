use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::Store;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegationContractRecord {
    pub id: String,
    pub parent_session_id: String,
    pub requester_agent_id: Option<String>,
    pub target_host_id: Option<String>,
    pub target_agent_id: String,
    pub task: String,
    pub allowed_skills_json: String,
    pub tool_allowlist_json: String,
    pub artifact_inputs_json: String,
    pub budget_tokens: Option<i64>,
    pub budget_secs: Option<f64>,
    pub approval_mode: String,
    pub experimental_comm_enabled: bool,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessageRecord {
    pub id: String,
    pub contract_id: String,
    pub from_session_id: String,
    pub to_session_id: String,
    pub kind: String,
    pub content: String,
    pub visibility: String,
    pub correlation_id: Option<String>,
    pub created_at: String,
}

pub struct CreateDelegationContract<'a> {
    pub id: &'a str,
    pub parent_session_id: &'a str,
    pub requester_agent_id: Option<&'a str>,
    pub target_host_id: Option<&'a str>,
    pub target_agent_id: &'a str,
    pub task: &'a str,
    pub allowed_skills_json: &'a str,
    pub tool_allowlist_json: &'a str,
    pub artifact_inputs_json: &'a str,
    pub budget_tokens: Option<i64>,
    pub budget_secs: Option<f64>,
    pub approval_mode: &'a str,
    pub experimental_comm_enabled: bool,
}

pub struct CreateAgentMessage<'a> {
    pub id: &'a str,
    pub contract_id: &'a str,
    pub from_session_id: &'a str,
    pub to_session_id: &'a str,
    pub kind: &'a str,
    pub content: &'a str,
    pub visibility: &'a str,
    pub correlation_id: Option<&'a str>,
}

impl Store {
    pub fn create_delegation_contract(
        &self,
        input: CreateDelegationContract<'_>,
    ) -> Result<DelegationContractRecord, rusqlite::Error> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "INSERT INTO delegation_contracts (
                id, parent_session_id, requester_agent_id, target_host_id, target_agent_id, task,
                allowed_skills_json, tool_allowlist_json, artifact_inputs_json, budget_tokens,
                budget_secs, approval_mode, experimental_comm_enabled, status, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 'pending', ?14, ?15)",
            params![
                input.id,
                input.parent_session_id,
                input.requester_agent_id,
                input.target_host_id,
                input.target_agent_id,
                input.task,
                input.allowed_skills_json,
                input.tool_allowlist_json,
                input.artifact_inputs_json,
                input.budget_tokens,
                input.budget_secs,
                input.approval_mode,
                if input.experimental_comm_enabled { 1 } else { 0 },
                now,
                now,
            ],
        )?;
        Ok(DelegationContractRecord {
            id: input.id.to_string(),
            parent_session_id: input.parent_session_id.to_string(),
            requester_agent_id: input.requester_agent_id.map(str::to_string),
            target_host_id: input.target_host_id.map(str::to_string),
            target_agent_id: input.target_agent_id.to_string(),
            task: input.task.to_string(),
            allowed_skills_json: input.allowed_skills_json.to_string(),
            tool_allowlist_json: input.tool_allowlist_json.to_string(),
            artifact_inputs_json: input.artifact_inputs_json.to_string(),
            budget_tokens: input.budget_tokens,
            budget_secs: input.budget_secs,
            approval_mode: input.approval_mode.to_string(),
            experimental_comm_enabled: input.experimental_comm_enabled,
            status: "pending".to_string(),
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn get_delegation_contract(
        &self,
        id: &str,
    ) -> Result<Option<DelegationContractRecord>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, parent_session_id, requester_agent_id, target_host_id, target_agent_id, task,
                    allowed_skills_json, tool_allowlist_json, artifact_inputs_json, budget_tokens,
                    budget_secs, approval_mode, experimental_comm_enabled, status, created_at, updated_at
             FROM delegation_contracts WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(DelegationContractRecord {
                id: row.get(0)?,
                parent_session_id: row.get(1)?,
                requester_agent_id: row.get(2)?,
                target_host_id: row.get(3)?,
                target_agent_id: row.get(4)?,
                task: row.get(5)?,
                allowed_skills_json: row.get(6)?,
                tool_allowlist_json: row.get(7)?,
                artifact_inputs_json: row.get(8)?,
                budget_tokens: row.get(9)?,
                budget_secs: row.get(10)?,
                approval_mode: row.get(11)?,
                experimental_comm_enabled: row.get::<_, i64>(12)? != 0,
                status: row.get(13)?,
                created_at: row.get(14)?,
                updated_at: row.get(15)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn update_delegation_contract_status(
        &self,
        id: &str,
        status: &str,
    ) -> Result<(), rusqlite::Error> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "UPDATE delegation_contracts SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status, now, id],
        )?;
        Ok(())
    }

    pub fn list_delegation_contracts_for_parent(
        &self,
        parent_session_id: &str,
    ) -> Result<Vec<DelegationContractRecord>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, parent_session_id, requester_agent_id, target_host_id, target_agent_id, task,
                    allowed_skills_json, tool_allowlist_json, artifact_inputs_json, budget_tokens,
                    budget_secs, approval_mode, experimental_comm_enabled, status, created_at, updated_at
             FROM delegation_contracts
             WHERE parent_session_id = ?1
             ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![parent_session_id], |row| {
            Ok(DelegationContractRecord {
                id: row.get(0)?,
                parent_session_id: row.get(1)?,
                requester_agent_id: row.get(2)?,
                target_host_id: row.get(3)?,
                target_agent_id: row.get(4)?,
                task: row.get(5)?,
                allowed_skills_json: row.get(6)?,
                tool_allowlist_json: row.get(7)?,
                artifact_inputs_json: row.get(8)?,
                budget_tokens: row.get(9)?,
                budget_secs: row.get(10)?,
                approval_mode: row.get(11)?,
                experimental_comm_enabled: row.get::<_, i64>(12)? != 0,
                status: row.get(13)?,
                created_at: row.get(14)?,
                updated_at: row.get(15)?,
            })
        })?;
        rows.collect()
    }

    pub fn list_delegation_messages(
        &self,
        contract_id: &str,
    ) -> Result<Vec<AgentMessageRecord>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, contract_id, from_session_id, to_session_id, kind, content,
                    visibility, correlation_id, created_at
             FROM agent_messages
             WHERE contract_id = ?1
             ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![contract_id], |row| {
            Ok(AgentMessageRecord {
                id: row.get(0)?,
                contract_id: row.get(1)?,
                from_session_id: row.get(2)?,
                to_session_id: row.get(3)?,
                kind: row.get(4)?,
                content: row.get(5)?,
                visibility: row.get(6)?,
                correlation_id: row.get(7)?,
                created_at: row.get(8)?,
            })
        })?;
        rows.collect()
    }

    pub fn create_agent_message(
        &self,
        input: CreateAgentMessage<'_>,
    ) -> Result<AgentMessageRecord, rusqlite::Error> {
        let created_at = Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "INSERT INTO agent_messages (
                id, contract_id, from_session_id, to_session_id, kind, content, visibility,
                correlation_id, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                input.id,
                input.contract_id,
                input.from_session_id,
                input.to_session_id,
                input.kind,
                input.content,
                input.visibility,
                input.correlation_id,
                created_at,
            ],
        )?;
        Ok(AgentMessageRecord {
            id: input.id.to_string(),
            contract_id: input.contract_id.to_string(),
            from_session_id: input.from_session_id.to_string(),
            to_session_id: input.to_session_id.to_string(),
            kind: input.kind.to_string(),
            content: input.content.to_string(),
            visibility: input.visibility.to_string(),
            correlation_id: input.correlation_id.map(str::to_string),
            created_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_store;
    use super::{CreateAgentMessage, CreateDelegationContract};

    fn setup_session(store: &super::Store, id: &str) {
        store
            .create_terminal_session(id, "terminal", Some("session"), "/tmp", &["bash".to_string()], "terminal", None)
            .unwrap();
    }

    #[test]
    fn test_create_and_get_delegation_contract() {
        let store = test_store();
        setup_session(&store, "parent");

        let rec = store.create_delegation_contract(CreateDelegationContract {
            id: "d1",
            parent_session_id: "parent",
            requester_agent_id: Some("claude-code"),
            target_host_id: None,
            target_agent_id: "ollama:llama3",
            task: "Run tests",
            allowed_skills_json: "[\"test\"]",
            tool_allowlist_json: "[\"cargo test\"]",
            artifact_inputs_json: "[]",
            budget_tokens: Some(1000),
            budget_secs: Some(60.0),
            approval_mode: "restricted",
            experimental_comm_enabled: false,
        }).unwrap();

        assert_eq!(rec.id, "d1");
        assert_eq!(rec.status, "pending");
        let fetched = store.get_delegation_contract("d1").unwrap().unwrap();
        assert_eq!(fetched.task, "Run tests");
    }

    #[test]
    fn test_create_and_list_agent_messages() {
        let store = test_store();
        setup_session(&store, "parent");
        setup_session(&store, "child");
        store.create_delegation_contract(CreateDelegationContract {
            id: "d1",
            parent_session_id: "parent",
            requester_agent_id: Some("claude-code"),
            target_host_id: None,
            target_agent_id: "ollama:llama3",
            task: "Run tests",
            allowed_skills_json: "[]",
            tool_allowlist_json: "[]",
            artifact_inputs_json: "[]",
            budget_tokens: None,
            budget_secs: None,
            approval_mode: "restricted",
            experimental_comm_enabled: false,
        }).unwrap();

        let msg = store.create_agent_message(CreateAgentMessage {
            id: "m1",
            contract_id: "d1",
            from_session_id: "parent",
            to_session_id: "child",
            kind: "instruction",
            content: "Please run the test suite",
            visibility: "supervisor",
            correlation_id: Some("corr-1"),
        }).unwrap();

        assert_eq!(msg.kind, "instruction");
        let messages = store.list_delegation_messages("d1").unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "Please run the test suite");
    }
}
