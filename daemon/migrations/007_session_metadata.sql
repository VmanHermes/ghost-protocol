ALTER TABLE terminal_sessions ADD COLUMN session_type TEXT NOT NULL DEFAULT 'terminal';
ALTER TABLE terminal_sessions ADD COLUMN project_id TEXT REFERENCES projects(id);
