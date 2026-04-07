ALTER TABLE terminal_sessions ADD COLUMN port INTEGER;
ALTER TABLE terminal_sessions ADD COLUMN url TEXT;
ALTER TABLE terminal_sessions ADD COLUMN adopted INTEGER NOT NULL DEFAULT 0;
