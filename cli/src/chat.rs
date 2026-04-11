use std::io::Write;

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio_tungstenite::tungstenite::Message;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentInfo {
    id: String,
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateSessionResponse {
    session: SessionRecord,
    #[allow(dead_code)]
    agent: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionRecord {
    id: String,
}

async fn resolve_agent(daemon_url: &str, agent_id: Option<&str>) -> Result<AgentInfo, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{daemon_url}/api/agents"))
        .send()
        .await
        .map_err(|e| format!("Cannot connect to Ghost Protocol daemon at {daemon_url}: {e}"))?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Failed to fetch agents: {body}"));
    }
    let agents: Vec<AgentInfo> = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse agents: {e}"))?;

    if agents.is_empty() {
        return Err("No agents available on this machine.".into());
    }

    match agent_id {
        Some(id) => agents
            .into_iter()
            .find(|a| a.id == id)
            .ok_or_else(|| {
                format!("Agent '{id}' not found. Run 'ghost agents' to list available agents.")
            }),
        None => {
            println!("Available agents:");
            for (i, a) in agents.iter().enumerate() {
                println!("  {}. {} ({})", i + 1, a.name, a.id);
            }
            let choice: String = dialoguer::Input::new()
                .with_prompt("Pick an agent (number or ID)")
                .interact_text()
                .map_err(|e| format!("Input error: {e}"))?;
            if let Ok(num) = choice.parse::<usize>() {
                if num >= 1 && num <= agents.len() {
                    return Ok(agents.into_iter().nth(num - 1).unwrap());
                }
            }
            agents
                .into_iter()
                .find(|a| a.id == choice)
                .ok_or_else(|| format!("Agent '{choice}' not found."))
        }
    }
}

async fn create_session(daemon_url: &str, agent_id: &str) -> Result<String, String> {
    let workdir = std::env::current_dir()
        .map_err(|e| format!("Cannot determine working directory: {e}"))?;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{daemon_url}/api/chat/sessions"))
        .json(&serde_json::json!({
            "agentId": agent_id,
            "workdir": workdir.to_string_lossy(),
        }))
        .send()
        .await
        .map_err(|e| format!("Failed to create chat session: {e}"))?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Failed to create chat session: {body}"));
    }
    let parsed: CreateSessionResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse session response: {e}"))?;
    Ok(parsed.session.id)
}

pub async fn run(daemon_url: &str, agent: Option<&str>) -> Result<(), String> {
    let agent_info = resolve_agent(daemon_url, agent).await?;
    let session_id = create_session(daemon_url, &agent_info.id).await?;
    println!("Ghost Protocol — chatting with {} (session {})", agent_info.name, session_id.get(..8).unwrap_or(&session_id));
    println!("Type /exit or Ctrl+C to end session.\n");

    run_event_loop(daemon_url, &session_id).await
}

async fn run_event_loop(daemon_url: &str, session_id: &str) -> Result<(), String> {
    let ws_url = if daemon_url.starts_with("https://") {
        daemon_url.replacen("https://", "wss://", 1)
    } else {
        daemon_url.replacen("http://", "ws://", 1)
    };
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("{ws_url}/ws"))
        .await
        .map_err(|e| format!("WebSocket connection failed: {e}"))?;

    // Subscribe to chat events
    let subscribe = serde_json::json!({
        "op": "subscribe_chat",
        "sessionId": session_id,
    });
    ws.send(Message::Text(subscribe.to_string().into()))
        .await
        .map_err(|e| format!("Failed to subscribe: {e}"))?;

    // Split for concurrent read/write
    let (_ws_tx, mut ws_rx) = ws.split();

    // Input channel — readline thread sends user messages here
    let (input_tx, mut input_rx) = tokio::sync::mpsc::channel::<String>(16);

    // Spawn readline input thread
    std::thread::spawn(move || {
        read_input_loop(input_tx);
    });

    // HTTP client for sending messages (created once, reused per message)
    let client = reqwest::Client::new();

    // Track whether we're mid-response (for blank line separator)
    let mut in_response = false;
    let daemon_url_owned = daemon_url.to_string();
    let session_id_owned = session_id.to_string();

    loop {
        tokio::select! {
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let event: serde_json::Value = match serde_json::from_str(&text) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        match event["op"].as_str() {
                            Some("chat_delta") => {
                                let delta = event["delta"].as_str().unwrap_or("");
                                print!("{delta}");
                                std::io::stdout().flush().ok();
                                in_response = true;
                            }
                            Some("chat_status") => {
                                let status = event["status"].as_str().unwrap_or("");
                                match status {
                                    "thinking" => {
                                        print_dim("[thinking...]");
                                    }
                                    "tool_use" => {
                                        print_dim("[using tool...]");
                                    }
                                    "idle" => {
                                        if in_response {
                                            println!("\n");
                                            in_response = false;
                                        }
                                    }
                                    "exited" | "error" => {
                                        if in_response {
                                            println!();
                                        }
                                        println!("[session ended]");
                                        return Ok(());
                                    }
                                    _ => {}
                                }
                            }
                            Some("chat_message") => {
                                // Complete messages arrive during history replay on subscribe.
                                // During live streaming we get deltas instead, so skip replay messages.
                            }
                            Some("subscribed_chat") => {}
                            Some("heartbeat") => {}
                            Some("error") => {
                                let msg = event["message"].as_str().unwrap_or("unknown error");
                                return Err(format!("Server error: {msg}"));
                            }
                            _ => {}
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        println!("[connection closed]");
                        return Ok(());
                    }
                    Some(Err(e)) => {
                        return Err(format!("WebSocket error: {e}"));
                    }
                    _ => {}
                }
            }
            input = input_rx.recv() => {
                match input {
                    Some(text) if text == "/exit" => {
                        println!("Session ended.");
                        return Ok(());
                    }
                    Some(text) => {
                        // Send via HTTP (more reliable than WS for messages)
                        if let Err(e) = send_message(&client, &daemon_url_owned, &session_id_owned, &text).await {
                            eprintln!("Failed to send message: {e}");
                        }
                    }
                    None => return Ok(()), // Input channel closed
                }
            }
        }
    }
}

async fn send_message(client: &reqwest::Client, daemon_url: &str, session_id: &str, content: &str) -> Result<(), String> {
    let resp = client
        .post(format!("{daemon_url}/api/chat/sessions/{session_id}/message"))
        .json(&serde_json::json!({"content": content}))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(body);
    }
    Ok(())
}

fn print_dim(text: &str) {
    println!("\x1b[2m{text}\x1b[0m");
}

fn read_input_loop(tx: tokio::sync::mpsc::Sender<String>) {
    let mut rl = match rustyline::DefaultEditor::new() {
        Ok(rl) => rl,
        Err(e) => {
            eprintln!("Failed to initialize input: {e}");
            return;
        }
    };
    loop {
        match rl.readline("you> ") {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                // Multi-line: if line ends with \, continue reading
                let full_input = if trimmed.ends_with('\\') {
                    let mut buf = trimmed.strip_suffix('\\').unwrap().to_string();
                    loop {
                        match rl.readline("...> ") {
                            Ok(cont) => {
                                let cont = cont.trim_end();
                                if cont.ends_with('\\') {
                                    buf.push('\n');
                                    buf.push_str(cont.strip_suffix('\\').unwrap());
                                } else {
                                    buf.push('\n');
                                    buf.push_str(cont);
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    buf
                } else {
                    trimmed.to_string()
                };

                let _ = rl.add_history_entry(&full_input);

                if tx.blocking_send(full_input).is_err() {
                    break; // Channel closed, main loop exited
                }
            }
            Err(rustyline::error::ReadlineError::Interrupted) | // Ctrl+C
            Err(rustyline::error::ReadlineError::Eof) => {       // Ctrl+D
                let _ = tx.blocking_send("/exit".into());
                break;
            }
            Err(e) => {
                eprintln!("Input error: {e}");
                break;
            }
        }
    }
}
