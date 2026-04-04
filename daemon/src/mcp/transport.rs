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
                            "resources": {}
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
        _ => Err(format!("unknown resource: {uri}").into()),
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
