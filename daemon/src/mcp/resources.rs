use serde_json::{json, Value};

pub struct ResourceBuilder {
    port: u16,
}

impl ResourceBuilder {
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn client(&self) -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default()
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
        Ok(json!({ "hosts": resp }))
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
                let hstatus = h["status"].as_str().unwrap_or("unknown");

                // Find tier for this host from perms_data
                let tier = perms_data.as_array()
                    .and_then(|arr| arr.iter().find(|p| p["tailscaleIp"].as_str() == Some(ip)))
                    .and_then(|p| p["tier"].as_str())
                    .unwrap_or("no-access");

                lines.push(format!("- {name} ({ip}): {hgpu}, {hram} [{hstatus}] — {tier}"));
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

        // Permission notes
        if let Some(perms) = perms_data.as_array() {
            let restricted: Vec<String> = perms
                .iter()
                .filter(|p| p["tier"].as_str() != Some("full-access"))
                .filter_map(|p| {
                    let name = p["hostName"].as_str()?;
                    let tier = p["tier"].as_str()?;
                    Some(format!("  {name}: {tier}"))
                })
                .collect();

            if !restricted.is_empty() {
                lines.push("\nPermission restrictions:".to_string());
                for line in restricted {
                    lines.push(line);
                }
                lines.push("Peers with 'read-only' cannot create sessions or send input.".to_string());
                lines.push("Peers with 'approval-required' will have write operations queued for owner approval.".to_string());
            }
        }

        Ok(lines.join("\n"))
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
        ]
    }
}
