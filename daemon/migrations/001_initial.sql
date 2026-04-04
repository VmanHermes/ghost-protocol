CREATE TABLE IF NOT EXISTS terminal_sessions (
    id TEXT PRIMARY KEY,
    mode TEXT NOT NULL,
    status TEXT NOT NULL,
    name TEXT,
    workdir TEXT NOT NULL,
    command_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT,
    last_chunk_at TEXT,
    pid INTEGER,
    exit_code INTEGER
);

CREATE TABLE IF NOT EXISTS terminal_chunks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    stream TEXT NOT NULL,
    chunk TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(session_id) REFERENCES terminal_sessions(id)
);

CREATE INDEX IF NOT EXISTS idx_chunks_session_id ON terminal_chunks(session_id, id);
