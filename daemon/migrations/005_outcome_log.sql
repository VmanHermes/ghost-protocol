CREATE TABLE IF NOT EXISTS outcome_log (
    id TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    source_host_id TEXT,
    category TEXT NOT NULL,
    action TEXT NOT NULL,
    description TEXT,
    target_machine TEXT,
    status TEXT NOT NULL,
    exit_code INTEGER,
    duration_secs REAL,
    metadata_json TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_outcome_log_created ON outcome_log(created_at);
CREATE INDEX IF NOT EXISTS idx_outcome_log_category ON outcome_log(category);
