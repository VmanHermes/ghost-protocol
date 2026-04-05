use serde_json::{json, Value};
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
            "description": "List available agent runtimes across the mesh. Shows which agents (Claude Code, Ollama models, Hermes, etc.) are available on which machines.",
            "inputSchema": { "type": "object", "properties": {}, "required": [] }
        }
    ])
}

async fn call_tool(
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
