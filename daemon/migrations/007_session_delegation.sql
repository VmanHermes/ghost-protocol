-- 007_session_delegation.sql
-- Parent-child session tracking and host identity
ALTER TABLE terminal_sessions ADD COLUMN parent_session_id TEXT REFERENCES terminal_sessions(id) ON DELETE SET NULL;
ALTER TABLE terminal_sessions ADD COLUMN host_id TEXT;
ALTER TABLE terminal_sessions ADD COLUMN host_name TEXT;
