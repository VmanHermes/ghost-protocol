use serde_json::{Value, json};
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::resources::ResourceBuilder;

pub async fn run_stdio(port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let builder = ResourceBuilder::new(port);
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let err = json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": { "code": -32700, "message": format!("parse error: {e}") }
                });
                write_response(&mut stdout, &err).await?;
                continue;
            }
        };

        let id = request["id"].clone();
        let method = request["method"].as_str().unwrap_or("");

        let response = match method {
            "initialize" => {
                let briefing = builder.context_briefing().await.unwrap_or_default();
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": "2025-03-26",
                        "capabilities": {
                            "resources": {},
                            "tools": {}
                        },
                        "serverInfo": {
                            "name": "ghost-protocol",
                            "version": env!("CARGO_PKG_VERSION")
                        },
                        "instructions": briefing
                    }
                })
            }
            "notifications/initialized" => continue,
            "resources/list" => {
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "resources": ResourceBuilder::resource_list()
                    }
                })
            }
            "resources/read" => {
                let uri = request["params"]["uri"].as_str().unwrap_or("");
                match read_resource(&builder, uri).await {
                    Ok(content) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "contents": [content]
                        }
                    }),
                    Err(e) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32602, "message": format!("resource error: {e}") }
                    }),
                }
            }
            "tools/list" => {
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "tools": tool_definitions()
                    }
                })
            }
            "tools/call" => {
                let name = request["params"]["name"].as_str().unwrap_or("");
                let arguments = &request["params"]["arguments"];
                match call_tool(&builder, name, arguments).await {
                    Ok(text) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{
                                "type": "text",
                                "text": text
                            }]
                        }
                    }),
                    Err(e) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32602, "message": format!("tool error: {e}") }
                    }),
                }
            }
            "ping" => {
                json!({ "jsonrpc": "2.0", "id": id, "result": {} })
            }
            _ => {
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32601, "message": format!("method not found: {method}") }
                })
            }
        };

        write_response(&mut stdout, &response).await?;
    }

    Ok(())
}

async fn read_resource(
    builder: &ResourceBuilder,
    uri: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    match uri {
        "ghost://machine/info" => {
            let data = builder.machine_info().await?;
            Ok(json!({
                "uri": uri,
                "mimeType": "application/json",
                "text": serde_json::to_string_pretty(&data)?
            }))
        }
        "ghost://machine/status" => {
            let data = builder.machine_status().await?;
            Ok(json!({
                "uri": uri,
                "mimeType": "application/json",
                "text": serde_json::to_string_pretty(&data)?
            }))
        }
        "ghost://network/hosts" => {
            let data = builder.network_hosts().await?;
            Ok(json!({
                "uri": uri,
                "mimeType": "application/json",
                "text": serde_json::to_string_pretty(&data)?
            }))
        }
        "ghost://terminal/sessions" => {
            let data = builder.terminal_sessions().await?;
            Ok(json!({
                "uri": uri,
                "mimeType": "application/json",
                "text": serde_json::to_string_pretty(&data)?
            }))
        }
        "ghost://agent/hints" => {
            let data = builder.agent_hints().await?;
            Ok(json!({
                "uri": uri,
                "mimeType": "application/json",
                "text": serde_json::to_string_pretty(&data)?
            }))
        }
        "ghost://context/briefing" => {
            let text = builder.context_briefing().await?;
            Ok(json!({
                "uri": uri,
                "mimeType": "text/plain",
                "text": text
            }))
        }
        "ghost://outcomes/recent" => {
            let data = builder.recent_outcomes().await?;
            Ok(json!({
                "uri": uri,
                "mimeType": "application/json",
                "text": serde_json::to_string_pretty(&data)?
            }))
        }
        "ghost://agents/available" => {
            let data = builder.available_agents().await?;
            Ok(json!({
                "uri": uri,
                "mimeType": "application/json",
                "text": serde_json::to_string_pretty(&data)?
            }))
        }
        _ => Err(format!("unknown resource: {uri}").into()),
    }
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "ghost_report_outcome",
            "description": "Report the outcome of work you performed. Call this after completing builds, deployments, inference, or other significant tasks. Helps the mesh learn which machines are best for which work.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "category": { "type": "string", "description": "Type of work: build, inference, deploy, test, custom" },
                    "action": { "type": "string", "description": "What you did: 'cargo build --release', 'ollama run llama3', etc." },
                    "status": { "type": "string", "enum": ["success", "failure", "timeout", "cancelled"], "description": "Outcome" },
                    "description": { "type": "string", "description": "Optional context about what you were trying to accomplish" },
                    "targetMachine": { "type": "string", "description": "Which machine the work ran on (hostname or IP)" },
                    "exitCode": { "type": "integer", "description": "Process exit code if applicable" },
                    "durationSecs": { "type": "number", "description": "How long the work took in seconds" },
                    "metadata": { "type": "object", "description": "Any additional structured data" }
                },
                "required": ["category", "action", "status"]
            }
        },
        {
            "name": "ghost_check_mesh",
            "description": "Get current mesh state: machines, active sessions, recent activity, and permission levels. Use this to understand what's available before routing work.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        },
        {
            "name": "ghost_list_machines",
            "description": "Get structured machine data for routing decisions: name, IP, online status, GPU, RAM, capabilities, and your permission tier on each machine.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        },
        {
            "name": "ghost_list_agents",
            "description": "List available agent runtimes across the mesh. Shows which agents (Hermes, Ollama models, Claude Code, etc.) are available on which machines.",
            "inputSchema": { "type": "object", "properties": {}, "required": [] }
        },
        {
            "name": "ghost_spawn_remote_session",
            "description": "Spawn an agent session on a remote machine in the mesh. Creates a fire-and-forget chat session. Returns the session ID and status.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "targetMachine": { "type": "string", "description": "Name or IP of the target machine" },
                    "agentId": { "type": "string", "description": "Agent ID to spawn (e.g., 'hermes', 'ollama:gemma4')" },
                    "task": { "type": "string", "description": "Task description / initial message for the agent" },
                    "workdir": { "type": "string", "description": "Working directory on the remote machine" }
                },
                "required": ["targetMachine", "agentId", "task"]
            }
        },
        {
            "name": "ghost_recall",
            "description": "Search project memory and history. Use before starting unfamiliar work, after hitting errors, or when deciding which machine to use.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language question or keyword search"
                    },
                    "filters": {
                        "type": "object",
                        "properties": {
                            "project": { "type": "string" },
                            "agent": { "type": "string" },
                            "machine": { "type": "string" },
                            "outcome": { "type": "string", "enum": ["success", "failed", "partial_success"] },
                            "category": { "type": "string", "enum": ["summary", "insight", "error_pattern", "preference", "machine_knowledge"] },
                            "tags": { "type": "array", "items": { "type": "string" } }
                        }
                    },
                    "limit": { "type": "integer", "default": 5, "maximum": 10 }
                },
                "required": []
            }
        }
    ])
}

pub async fn call_tool(
    builder: &ResourceBuilder,
    name: &str,
    arguments: &Value,
) -> Result<String, Box<dyn std::error::Error>> {
    match name {
        "ghost_report_outcome" => {
            let client = builder.client();
            let resp = client
                .post(format!("{}/api/outcomes", builder.base()))
                .json(arguments)
                .send()
                .await?;
            if resp.status().is_success() {
                let body: Value = resp.json().await?;
                let id = body["id"].as_str().unwrap_or("?");
                let created = body["createdAt"].as_str().unwrap_or("?");
                Ok(format!("Outcome recorded (id: {id}, created: {created})"))
            } else {
                let text = resp.text().await?;
                Err(format!("failed to report outcome: {text}").into())
            }
        }
        "ghost_check_mesh" => {
            let briefing = builder.context_briefing().await?;
            Ok(briefing)
        }
        "ghost_list_machines" => {
            let data = builder.list_machines().await?;
            Ok(serde_json::to_string_pretty(&data)?)
        }
        "ghost_list_agents" => {
            let data = builder.available_agents().await?;
            Ok(serde_json::to_string_pretty(&data)?)
        }
        "ghost_spawn_remote_session" => {
            let target_machine = arguments["targetMachine"]
                .as_str()
                .unwrap_or("")
                .to_string();
            let agent_id = arguments["agentId"].as_str().unwrap_or("").to_string();
            let task = arguments["task"].as_str().unwrap_or("").to_string();
            let workdir = arguments["workdir"].as_str().map(|s| s.to_string());

            // Fetch known hosts from the local daemon
            let client = builder.client();
            let hosts_resp: Value = client
                .get(format!("{}/api/hosts", builder.base()))
                .send()
                .await?
                .json()
                .await?;

            let hosts = hosts_resp.as_array().cloned().unwrap_or_default();

            // Find the target host by name or tailscaleIp or url
            let host = hosts.iter().find(|h| {
                h["name"].as_str() == Some(target_machine.as_str())
                    || h["tailscaleIp"].as_str() == Some(target_machine.as_str())
                    || h["url"].as_str() == Some(target_machine.as_str())
            });

            let host = match host {
                Some(h) => h.clone(),
                None => {
                    let known: Vec<String> = hosts
                        .iter()
                        .filter_map(|h| h["name"].as_str().map(|s| s.to_string()))
                        .collect();
                    return Err(format!(
                        "Machine '{}' not found. Known machines: [{}]",
                        target_machine,
                        known.join(", ")
                    )
                    .into());
                }
            };

            let host_url = host["url"].as_str().unwrap_or("").to_string();
            let host_name = host["name"].as_str().unwrap_or(&target_machine).to_string();

            // POST to create a chat session on the remote machine
            let mut session_body = json!({ "agentId": agent_id });
            if let Some(ref wd) = workdir {
                session_body["workdir"] = json!(wd);
            }

            let session_resp = client
                .post(format!("{}/api/chat/sessions", host_url))
                .json(&session_body)
                .send()
                .await
                .map_err(|e| format!("failed to reach {host_name} ({host_url}): {e}"))?;

            if !session_resp.status().is_success() {
                let status = session_resp.status();
                let text = session_resp.text().await.unwrap_or_default();
                return Err(format!(
                    "failed to create session on {host_name}: HTTP {status} — {text}"
                )
                .into());
            }

            let session: Value = session_resp.json().await?;
            let session_id = session["id"].as_str().unwrap_or("?").to_string();

            // Send the initial task message
            let msg_resp = client
                .post(format!(
                    "{}/api/chat/sessions/{}/message",
                    host_url, session_id
                ))
                .json(&json!({ "content": task }))
                .send()
                .await
                .map_err(|e| {
                    format!(
                        "session {session_id} created on {host_name} but failed to send task: {e}"
                    )
                })?;

            if !msg_resp.status().is_success() {
                let status = msg_resp.status();
                let text = msg_resp.text().await.unwrap_or_default();
                return Err(format!(
                    "session {session_id} created on {host_name} but task message failed: HTTP {status} — {text}"
                )
                .into());
            }

            Ok(format!(
                "Remote session spawned on {host_name}. session_id={session_id}, agent={agent_id}, status=running"
            ))
        }
        "ghost_recall" => {
            let client = builder.client();
            let resp = client
                .post(format!("{}/api/intelligence/recall", builder.base()))
                .json(arguments)
                .send()
                .await?;
            if resp.status().is_success() {
                let body: serde_json::Value = resp.json().await?;
                Ok(serde_json::to_string_pretty(&body)?)
            } else {
                let text = resp.text().await?;
                Err(format!("recall failed: {text}").into())
            }
        }
        _ => Err(format!("unknown tool: {name}").into()),
    }
}

async fn write_response(
    stdout: &mut io::Stdout,
    response: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = serde_json::to_vec(response)?;
    bytes.push(b'\n');
    stdout.write_all(&bytes).await?;
    stdout.flush().await?;
    Ok(())
}
