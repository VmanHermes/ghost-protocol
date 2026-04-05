CREATE TABLE IF NOT EXISTS peer_permissions (
    host_id TEXT PRIMARY KEY REFERENCES known_hosts(id) ON DELETE CASCADE,
    tier TEXT NOT NULL DEFAULT 'no-access',
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS pending_approvals (
    id TEXT PRIMARY KEY,
    host_id TEXT NOT NULL REFERENCES known_hosts(id) ON DELETE CASCADE,
    method TEXT NOT NULL,
    path TEXT NOT NULL,
    body_json TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TEXT NOT NULL,
    resolved_at TEXT,
    expires_at TEXT NOT NULL,
    result_json TEXT
);
