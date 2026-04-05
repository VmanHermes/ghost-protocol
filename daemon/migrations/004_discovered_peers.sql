CREATE TABLE IF NOT EXISTS discovered_peers (
    tailscale_ip TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    discovered_at TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
);
