SELECT
    id,
    name,
    tailscale_ip,
    url,
    status,
    last_seen,
    capabilities_json AS capabilities
FROM known_hosts
ORDER BY name ASC;
