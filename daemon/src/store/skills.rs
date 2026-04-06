use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::Store;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCandidateRecord {
    pub id: String,
    pub source_session_id: String,
    pub trace_refs_json: String,
    pub proposed_change: String,
    pub risk_level: String,
    pub status: String,
    pub reviewer: Option<String>,
    pub promoted_skill_version: Option<String>,
    pub created_at: String,
    pub reviewed_at: Option<String>,
}

pub struct CreateSkillCandidate<'a> {
    pub id: &'a str,
    pub source_session_id: &'a str,
    pub trace_refs_json: &'a str,
    pub proposed_change: &'a str,
    pub risk_level: &'a str,
}

impl Store {
    pub fn create_skill_candidate(
        &self,
        input: CreateSkillCandidate<'_>,
    ) -> Result<SkillCandidateRecord, rusqlite::Error> {
        let created_at = Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "INSERT INTO skill_candidates (
                id, source_session_id, trace_refs_json, proposed_change, risk_level,
                status, reviewer, promoted_skill_version, created_at, reviewed_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'pending_review', NULL, NULL, ?6, NULL)",
            params![
                input.id,
                input.source_session_id,
                input.trace_refs_json,
                input.proposed_change,
                input.risk_level,
                created_at,
            ],
        )?;
        Ok(SkillCandidateRecord {
            id: input.id.to_string(),
            source_session_id: input.source_session_id.to_string(),
            trace_refs_json: input.trace_refs_json.to_string(),
            proposed_change: input.proposed_change.to_string(),
            risk_level: input.risk_level.to_string(),
            status: "pending_review".to_string(),
            reviewer: None,
            promoted_skill_version: None,
            created_at,
            reviewed_at: None,
        })
    }

    pub fn list_skill_candidates(&self) -> Result<Vec<SkillCandidateRecord>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, source_session_id, trace_refs_json, proposed_change, risk_level,
                    status, reviewer, promoted_skill_version, created_at, reviewed_at
             FROM skill_candidates
             ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(SkillCandidateRecord {
                id: row.get(0)?,
                source_session_id: row.get(1)?,
                trace_refs_json: row.get(2)?,
                proposed_change: row.get(3)?,
                risk_level: row.get(4)?,
                status: row.get(5)?,
                reviewer: row.get(6)?,
                promoted_skill_version: row.get(7)?,
                created_at: row.get(8)?,
                reviewed_at: row.get(9)?,
            })
        })?;
        rows.collect()
    }

    pub fn promote_skill_candidate(
        &self,
        id: &str,
        reviewer: &str,
        promoted_skill_version: &str,
    ) -> Result<Option<SkillCandidateRecord>, rusqlite::Error> {
        let reviewed_at = Utc::now().to_rfc3339();
        let conn = self.conn();
        conn.execute(
            "UPDATE skill_candidates
             SET status = 'promoted', reviewer = ?1, promoted_skill_version = ?2, reviewed_at = ?3
             WHERE id = ?4",
            params![reviewer, promoted_skill_version, reviewed_at, id],
        )?;

        let mut stmt = conn.prepare(
            "SELECT id, source_session_id, trace_refs_json, proposed_change, risk_level,
                    status, reviewer, promoted_skill_version, created_at, reviewed_at
             FROM skill_candidates WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(SkillCandidateRecord {
                id: row.get(0)?,
                source_session_id: row.get(1)?,
                trace_refs_json: row.get(2)?,
                proposed_change: row.get(3)?,
                risk_level: row.get(4)?,
                status: row.get(5)?,
                reviewer: row.get(6)?,
                promoted_skill_version: row.get(7)?,
                created_at: row.get(8)?,
                reviewed_at: row.get(9)?,
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
    use super::CreateSkillCandidate;

    fn setup_session(store: &super::Store, id: &str) {
        store
            .create_terminal_session(id, "chat", Some("session"), "/tmp", &["chat".to_string()], "chat", None)
            .unwrap();
    }

    #[test]
    fn test_create_and_promote_skill_candidate() {
        let store = test_store();
        setup_session(&store, "s1");

        let rec = store.create_skill_candidate(CreateSkillCandidate {
            id: "skill-1",
            source_session_id: "s1",
            trace_refs_json: "[\"trace-1\"]",
            proposed_change: "Prefer the test skill before retrying",
            risk_level: "low",
        }).unwrap();
        assert_eq!(rec.status, "pending_review");

        let promoted = store
            .promote_skill_candidate("skill-1", "owner", "skill-v2")
            .unwrap()
            .unwrap();
        assert_eq!(promoted.status, "promoted");
        assert_eq!(promoted.promoted_skill_version.as_deref(), Some("skill-v2"));
    }
}
