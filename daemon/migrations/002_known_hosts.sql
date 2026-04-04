CREATE TABLE IF NOT EXISTS known_hosts (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    tailscale_ip TEXT NOT NULL UNIQUE,
    url TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'unknown',
    last_seen TEXT,
    capabilities_json TEXT
);
