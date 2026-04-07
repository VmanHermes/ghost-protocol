UPDATE known_hosts
SET
    status = :status,
    last_seen = :last_seen,
    capabilities_json = :capabilities_json
WHERE id = :id;
