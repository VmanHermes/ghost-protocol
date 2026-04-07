INSERT INTO known_hosts (id, name, tailscale_ip, url, status)
VALUES (:id, :name, :tailscale_ip, :url, 'unknown');
