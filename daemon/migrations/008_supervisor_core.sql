ALTER TABLE terminal_sessions ADD COLUMN root_session_id TEXT REFERENCES terminal_sessions(id) ON DELETE SET NULL;
ALTER TABLE terminal_sessions ADD COLUMN agent_id TEXT;
ALTER TABLE terminal_sessions ADD COLUMN driver_kind TEXT NOT NULL DEFAULT 'terminal_driver';
ALTER TABLE terminal_sessions ADD COLUMN capabilities_json TEXT NOT NULL DEFAULT '[]';

CREATE TABLE IF NOT EXISTS delegation_contracts (
    id TEXT PRIMARY KEY,
    parent_session_id TEXT NOT NULL REFERENCES terminal_sessions(id) ON DELETE CASCADE,
    requester_agent_id TEXT,
    target_host_id TEXT REFERENCES known_hosts(id) ON DELETE SET NULL,
    target_agent_id TEXT NOT NULL,
    task TEXT NOT NULL,
    allowed_skills_json TEXT NOT NULL DEFAULT '[]',
    tool_allowlist_json TEXT NOT NULL DEFAULT '[]',
    artifact_inputs_json TEXT NOT NULL DEFAULT '[]',
    budget_tokens INTEGER,
    budget_secs REAL,
    approval_mode TEXT NOT NULL DEFAULT 'restricted',
    experimental_comm_enabled INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_delegation_contracts_parent_session
    ON delegation_contracts(parent_session_id, created_at DESC);

CREATE TABLE IF NOT EXISTS agent_messages (
    id TEXT PRIMARY KEY,
    contract_id TEXT NOT NULL REFERENCES delegation_contracts(id) ON DELETE CASCADE,
    from_session_id TEXT NOT NULL REFERENCES terminal_sessions(id) ON DELETE CASCADE,
    to_session_id TEXT NOT NULL REFERENCES terminal_sessions(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    content TEXT NOT NULL,
    visibility TEXT NOT NULL DEFAULT 'supervisor',
    correlation_id TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_agent_messages_contract
    ON agent_messages(contract_id, created_at ASC);

CREATE INDEX IF NOT EXISTS idx_agent_messages_to_session
    ON agent_messages(to_session_id, created_at ASC);

CREATE TABLE IF NOT EXISTS skill_candidates (
    id TEXT PRIMARY KEY,
    source_session_id TEXT NOT NULL REFERENCES terminal_sessions(id) ON DELETE CASCADE,
    trace_refs_json TEXT NOT NULL DEFAULT '[]',
    proposed_change TEXT NOT NULL,
    risk_level TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending_review',
    reviewer TEXT,
    promoted_skill_version TEXT,
    created_at TEXT NOT NULL,
    reviewed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_skill_candidates_status
    ON skill_candidates(status, created_at DESC);
