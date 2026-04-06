use std::collections::HashMap;

use serde_json::{json, Value};

fn extract_requester_tier(payload: &Value) -> Option<&str> {
    payload
        .get("peer")
        .and_then(|peer| peer.get("currentTier"))
        .and_then(Value::as_str)
}

fn local_grant_tier<'a>(perms_data: &'a Value, ip: &str) -> &'a str {
    perms_data
        .as_array()
        .and_then(|arr| arr.iter().find(|perm| perm["tailscaleIp"].as_str() == Some(ip)))
        .and_then(|perm| perm["tier"].as_str())
        .unwrap_or("no-access")
}

fn describe_host_status(status: &str) -> &str {
    match status {
        "permission-required" => "reachable, but permission required",
        other => other,
    }
}

fn describe_remote_access_tier(tier: Option<&str>) -> &'static str {
    match tier {
        Some("full-access") => "full-access",
        Some("approval-required") => "approval-required",
        Some("read-only") => "read-only",
        Some("no-access") => "no-access",
        Some(_) => "unknown",
        None => "unknown",
    }
}

pub struct ResourceBuilder {
    port: u16,
}

impl ResourceBuilder {
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    pub(crate) fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub(crate) fn client(&self) -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default()
    }

    async fn remote_access_tiers(&self, hosts_data: &Value) -> HashMap<String, Option<String>> {
        let mut tiers = HashMap::new();

        if let Some(hosts) = hosts_data["hosts"].as_array() {
            for host in hosts {
                let ip = host["tailscaleIp"].as_str().unwrap_or_default();
                if ip.is_empty() {
                    continue;
                }

                let base_url = host["url"]
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("http://{ip}:8787"));
                let tier = match self.client()
                    .get(format!("{base_url}/api/system/status"))
                    .send()
                    .await
                {
                    Ok(resp) if resp.status().is_success() => {
                        let body = resp.json::<Value>().await.unwrap_or_else(|_| json!({}));
                        extract_requester_tier(&body).map(str::to_string)
                    }
                    Ok(resp) if resp.status() == reqwest::StatusCode::FORBIDDEN => Some("no-access".to_string()),
                    _ => None,
                };

                tiers.insert(ip.to_string(), tier);
            }
        }

        tiers
    }

    pub async fn machine_info(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let resp: Value = self.client()
            .get(format!("{}/api/system/hardware", self.base()))
            .send()
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    pub async fn machine_status(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let resp: Value = self.client()
            .get(format!("{}/api/system/hardware/status", self.base()))
            .send()
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    pub async fn network_hosts(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let resp: Value = self.client()
            .get(format!("{}/api/hosts", self.base()))
            .send()
            .await?
            .json()
            .await?;
        let discoveries = match self.client()
            .get(format!("{}/api/discoveries", self.base()))
            .send()
            .await
        {
            Ok(resp) => resp.json().await.unwrap_or(json!([])),
            Err(_) => json!([]),
        };
        Ok(json!({
            "hosts": resp,
            "discoveries": discoveries,
        }))
    }

    pub async fn terminal_sessions(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let resp: Value = self.client()
            .get(format!("{}/api/terminal/sessions", self.base()))
            .send()
            .await?
            .json()
            .await?;
        Ok(json!({ "sessions": resp }))
    }

    pub async fn available_agents(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let local_info = self.machine_info().await?;
        let local_agents = local_info["tools"]["agents"].clone();
        let hosts_data = self.network_hosts().await?;

        let mut peers = serde_json::Map::new();
        if let Some(hosts) = hosts_data["hosts"].as_array() {
            for h in hosts {
                let name = h["name"].as_str().unwrap_or("?");
                let agents = h.get("capabilities")
                    .and_then(|c| c.get("agents"))
                    .cloned()
                    .unwrap_or(json!([]));
                peers.insert(name.to_string(), agents);
            }
        }

        Ok(json!({
            "local": local_agents,
            "peers": peers,
        }))
    }

    pub async fn agent_hints(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let info = self.machine_info().await?;

        let tailscale_ip = info["tailscaleIp"].as_str().unwrap_or("localhost");
        let ssh_user = info["tools"]["sshUser"].as_str().unwrap_or("user");

        let mut hints = json!({
            "ssh": format!("ssh {ssh_user}@{tailscale_ip}"),
            "cliCommands": [
                "ghost-protocol-daemon status",
                "ghost-protocol-daemon sessions",
                "ghost-protocol-daemon hosts",
                "ghost-protocol-daemon info"
            ],
        });

        if info["tools"]["ollama"].is_string() {
            hints["inference"] = json!({
                "ollama": format!("ssh {ssh_user}@{tailscale_ip} ollama run <model> '<prompt>'"),
                "ollamaApi": format!("http://{tailscale_ip}:11434/api/generate"),
            });
        }

        if let Some(hermes_path) = info["tools"]["hermes"].as_str() {
            hints["hermes"] = json!(format!("ssh {ssh_user}@{tailscale_ip} {hermes_path} run <task>"));
        }

        Ok(hints)
    }

    pub async fn context_briefing(&self) -> Result<String, Box<dyn std::error::Error>> {
        let info = self.machine_info().await?;
        let _status = self.machine_status().await?;
        let hosts_data = self.network_hosts().await?;
        let sessions_data = self.terminal_sessions().await?;

        let perms_data: Value = match self.client()
            .get(format!("{}/api/permissions", self.base()))
            .send()
            .await
        {
            Ok(resp) => resp.json().await.unwrap_or(json!([])),
            Err(_) => json!([]),
        };
        let remote_access = self.remote_access_tiers(&hosts_data).await;

        let hostname = info["hostname"].as_str().unwrap_or("this machine");
        let tailscale_ip = info["tailscaleIp"].as_str().unwrap_or("unknown");
        let ssh_user = info["tools"]["sshUser"].as_str().unwrap_or("user");

        let mut lines = Vec::new();

        // Machine list
        let hosts = hosts_data["hosts"].as_array();
        let host_count = hosts.map(|h| h.len()).unwrap_or(0) + 1;
        lines.push(format!(
            "You are connected to a Ghost Protocol mesh with {host_count} machine(s):\n"
        ));

        // Self
        let gpu_desc = info["gpu"]["model"]
            .as_str()
            .map(|m| {
                let vram = info["gpu"]["vramGb"].as_f64().unwrap_or(0.0);
                format!("{m} {vram:.0}GB")
            })
            .unwrap_or_else(|| "no GPU".to_string());
        let ram = info["ramGb"].as_f64().unwrap_or(0.0);
        let cores = info["cpu"]["cores"].as_u64().unwrap_or(0);
        lines.push(format!(
            "- {hostname} (this machine, {tailscale_ip}): {gpu_desc}, {ram:.0}GB RAM, {cores} cores"
        ));

        // Other hosts
        if let Some(hosts) = hosts {
            for h in hosts {
                let name = h["name"].as_str().unwrap_or("?");
                let ip = h["tailscaleIp"].as_str().unwrap_or("?");
                let hgpu = h["capabilities"]["gpu"].as_str().unwrap_or("no GPU");
                let hram = h["capabilities"]["ramGb"]
                    .as_f64()
                    .map(|r| format!("{r:.0}GB RAM"))
                    .unwrap_or_else(|| "? RAM".to_string());
                let hstatus = describe_host_status(h["status"].as_str().unwrap_or("unknown"));
                let outgoing_tier = local_grant_tier(&perms_data, ip);
                let incoming_tier = describe_remote_access_tier(
                    remote_access.get(ip).and_then(|tier| tier.as_deref()),
                );

                lines.push(format!(
                    "- {name} ({ip}): {hgpu}, {hram} [{hstatus}] — your access there: {incoming_tier}; its access here: {outgoing_tier}"
                ));
            }
        }

        if let Some(discoveries) = hosts_data["discoveries"].as_array() {
            if !discoveries.is_empty() {
                lines.push("\nPending discoveries:".to_string());
                for peer in discoveries {
                    let name = peer["name"].as_str().unwrap_or("?");
                    let ip = peer["tailscaleIp"].as_str().unwrap_or("?");
                    lines.push(format!("- {name} ({ip}) — discovered on the mesh but not added as a Ghost host yet"));
                }
                lines.push("Discovered peers appear in the sidebar with an Add button. Until they are added, they are not part of the Ghost host list and cannot be used for remote sessions.".to_string());
            }
        }

        // Ollama
        if info["tools"]["ollama"].is_string() {
            lines.push(format!("\nServices on {hostname}: Ollama on :11434"));
        }

        // Sessions
        if let Some(sessions) = sessions_data["sessions"].as_array() {
            let running: Vec<_> = sessions
                .iter()
                .filter(|s| s["status"].as_str() == Some("running"))
                .collect();
            if !running.is_empty() {
                lines.push(format!("\nActive terminal sessions on {hostname}:"));
                for s in &running {
                    let name = s["name"].as_str().unwrap_or("unnamed");
                    let workdir = s["workdir"].as_str().unwrap_or("?");
                    lines.push(format!("  - {name} ({workdir})"));
                }
            }
        }

        // Interaction hints
        lines.push(format!("\nTo interact with {hostname}:"));
        lines.push("  ghost-protocol-daemon status        # check machine state".to_string());
        lines.push("  ghost-protocol-daemon sessions      # list terminal sessions".to_string());

        if let Some(hosts) = hosts_data["hosts"].as_array() {
            for h in hosts {
                let name = h["name"].as_str().unwrap_or("?");
                let ip = h["tailscaleIp"].as_str().unwrap_or("?");
                if h["status"].as_str() == Some("online") {
                    lines.push(format!("\nTo interact with {name}:"));
                    lines.push(format!("  ssh {ssh_user}@{ip} ghost-protocol-daemon status"));
                    lines.push(format!("  ssh {ssh_user}@{ip} ghost-protocol-daemon sessions"));
                    if h["capabilities"]["ollama"].as_bool() == Some(true) {
                        lines.push(format!("  ssh {ssh_user}@{ip} ollama run <model>   # run inference on GPU"));
                    }
                }
            }
        }

        lines.push("\nPermission model:".to_string());
        lines.push("  - 'your access there' is what the peer machine grants this machine.".to_string());
        lines.push("  - 'its access here' is what this machine grants that peer.".to_string());
        lines.push("  - To create sessions on a peer, your access there must be 'approval-required' or 'full-access'.".to_string());
        lines.push("  - Changing this machine's permission for a peer does not upgrade what that peer allows this machine to do.".to_string());

        // Recent activity
        let outcomes_data: Value = match self.client()
            .get(format!("{}/api/outcomes?limit=5", self.base()))
            .send()
            .await
        {
            Ok(resp) => resp.json().await.unwrap_or(json!([])),
            Err(_) => json!([]),
        };

        if let Some(outcomes) = outcomes_data.as_array() {
            if !outcomes.is_empty() {
                lines.push("\nRecent activity:".to_string());
                for o in outcomes {
                    let action = o["action"].as_str().unwrap_or("?");
                    let target = o["targetMachine"].as_str().unwrap_or("local");
                    let status = o["status"].as_str().unwrap_or("?");
                    let duration = o["durationSecs"].as_f64()
                        .map(|d| format!(" ({d:.0}s)"))
                        .unwrap_or_default();
                    lines.push(format!("  - {action} on {target}: {status}{duration}"));
                }
            }
        }

        // Available agents on this machine
        let local_agents_data = self.machine_info().await.unwrap_or(json!({}));
        if let Some(agents) = local_agents_data["tools"]["agents"].as_array() {
            if !agents.is_empty() {
                let agent_names: Vec<&str> = agents.iter()
                    .filter_map(|a| a["name"].as_str())
                    .collect();
                lines.push(format!("\nAvailable agents on {hostname}: {}", agent_names.join(", ")));
            }
        }
        // Agents on peer machines
        if let Some(hosts) = hosts_data["hosts"].as_array() {
            for h in hosts {
                if let Some(peer_agents) = h.get("capabilities").and_then(|c| c["agents"].as_array()) {
                    if !peer_agents.is_empty() {
                        let name = h["name"].as_str().unwrap_or("?");
                        let agent_names: Vec<&str> = peer_agents.iter()
                            .filter_map(|a| a["name"].as_str())
                            .collect();
                        lines.push(format!("Available agents on {name}: {}", agent_names.join(", ")));
                    }
                }
            }
        }

        // Tool instructions
        lines.push("\nAvailable Ghost Protocol tools:".to_string());
        lines.push("  - ghost_report_outcome: Report what you did and the result after completing work".to_string());
        lines.push("  - ghost_check_mesh: Check current mesh state (machines, sessions, activity)".to_string());
        lines.push("  - ghost_list_machines: Get machine capabilities and permissions for routing decisions".to_string());
        lines.push("".to_string());
        lines.push("After completing significant work (builds, deployments, inference, file operations),".to_string());
        lines.push("use ghost_report_outcome to log the result. This helps the mesh learn which machines".to_string());
        lines.push("are best for which tasks.".to_string());

        Ok(lines.join("\n"))
    }

    pub async fn list_machines(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let local_info = self.machine_info().await?;
        let hosts_data = self.network_hosts().await?;
        let perms_data: Value = match self.client()
            .get(format!("{}/api/permissions", self.base()))
            .send()
            .await
        {
            Ok(resp) => resp.json().await.unwrap_or(json!([])),
            Err(_) => json!([]),
        };
        let remote_access = self.remote_access_tiers(&hosts_data).await;

        let mut peers = vec![];
        if let Some(hosts) = hosts_data["hosts"].as_array() {
            for h in hosts {
                let ip = h["tailscaleIp"].as_str().unwrap_or("?");
                let your_access_tier = describe_remote_access_tier(
                    remote_access.get(ip).and_then(|tier| tier.as_deref()),
                );
                let granted_by_local_tier = local_grant_tier(&perms_data, ip);

                peers.push(json!({
                    "name": h["name"],
                    "ip": ip,
                    "status": h["status"],
                    "gpu": h.get("capabilities").and_then(|c| c.get("gpu")).unwrap_or(&json!(null)),
                    "ramGb": h.get("capabilities").and_then(|c| c.get("ramGb")).unwrap_or(&json!(null)),
                    "capabilities": {
                        "hermes": h.get("capabilities").and_then(|c| c["hermes"].as_bool()).unwrap_or(false),
                        "ollama": h.get("capabilities").and_then(|c| c["ollama"].as_bool()).unwrap_or(false),
                    },
                    "permissionTier": your_access_tier,
                    "yourAccessTier": your_access_tier,
                    "grantedByLocalTier": granted_by_local_tier,
                }));
            }
        }

        let mut discoveries = vec![];
        if let Some(items) = hosts_data["discoveries"].as_array() {
            for peer in items {
                discoveries.push(json!({
                    "name": peer["name"],
                    "ip": peer["tailscaleIp"],
                    "status": "discovered",
                    "added": false,
                }));
            }
        }

        Ok(json!({
            "local": {
                "hostname": local_info["hostname"],
                "ip": local_info["tailscaleIp"],
                "cpu": local_info["cpu"]["model"],
                "ramGb": local_info["ramGb"],
                "gpu": local_info.get("gpu").and_then(|g| g.get("model")).unwrap_or(&json!(null)),
            },
            "peers": peers,
            "discoveries": discoveries,
        }))
    }

    pub async fn recent_outcomes(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let resp: Value = self.client()
            .get(format!("{}/api/outcomes?limit=20", self.base()))
            .send()
            .await?
            .json()
            .await?;
        Ok(json!({ "outcomes": resp }))
    }

    pub fn resource_list() -> Vec<Value> {
        vec![
            json!({
                "uri": "ghost://machine/info",
                "name": "Machine Info",
                "description": "Static hardware profile: hostname, CPU, RAM, GPU, installed tools",
                "mimeType": "application/json"
            }),
            json!({
                "uri": "ghost://machine/status",
                "name": "Machine Status",
                "description": "Live utilization: CPU, RAM, GPU usage, active sessions, notable processes",
                "mimeType": "application/json"
            }),
            json!({
                "uri": "ghost://network/hosts",
                "name": "Network Hosts",
                "description": "Known machines on the Tailscale mesh with status and capabilities",
                "mimeType": "application/json"
            }),
            json!({
                "uri": "ghost://terminal/sessions",
                "name": "Terminal Sessions",
                "description": "Active terminal sessions on this machine",
                "mimeType": "application/json"
            }),
            json!({
                "uri": "ghost://agent/hints",
                "name": "Agent Hints",
                "description": "SSH commands, CLI tools, and inference endpoints for interacting with this machine",
                "mimeType": "application/json"
            }),
            json!({
                "uri": "ghost://context/briefing",
                "name": "Context Briefing",
                "description": "Plain-language summary of the entire mesh: machines, sessions, and how to interact",
                "mimeType": "text/plain"
            }),
            json!({
                "uri": "ghost://outcomes/recent",
                "name": "Recent Outcomes",
                "description": "Recent action outcomes across the mesh: what was done, where, and whether it succeeded",
                "mimeType": "application/json"
            }),
            json!({
                "uri": "ghost://agents/available",
                "name": "Available Agents",
                "description": "Agent runtimes available across the mesh: which agents can run where",
                "mimeType": "application/json"
            }),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::{describe_remote_access_tier, extract_requester_tier, local_grant_tier};
    use serde_json::json;

    #[test]
    fn extracts_requester_tier_from_system_status_payload() {
        let payload = json!({
            "peer": {
                "currentTier": "full-access"
            }
        });

        assert_eq!(extract_requester_tier(&payload), Some("full-access"));
    }

    #[test]
    fn local_grant_defaults_to_no_access_when_missing() {
        let perms = json!([]);
        assert_eq!(local_grant_tier(&perms, "100.87.33.75"), "no-access");
    }

    #[test]
    fn unknown_remote_access_tier_is_described_honestly() {
        assert_eq!(describe_remote_access_tier(None), "unknown");
        assert_eq!(describe_remote_access_tier(Some("unexpected-tier")), "unknown");
    }
}
