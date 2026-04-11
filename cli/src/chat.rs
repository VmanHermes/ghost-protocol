use serde::Deserialize;

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
    let agents: Vec<AgentInfo> = client
        .get(format!("{daemon_url}/api/agents"))
        .send()
        .await
        .map_err(|e| format!("Cannot connect to Ghost Protocol daemon at {daemon_url}: {e}"))?
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
    let resp: CreateSessionResponse = client
        .post(format!("{daemon_url}/api/chat/sessions"))
        .json(&serde_json::json!({
            "agent_id": agent_id,
            "workdir": workdir.to_string_lossy(),
        }))
        .send()
        .await
        .map_err(|e| format!("Failed to create chat session: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse session response: {e}"))?;
    Ok(resp.session.id)
}

pub async fn run(daemon_url: &str, agent: Option<&str>) -> Result<(), String> {
    let agent_info = resolve_agent(daemon_url, agent).await?;
    let session_id = create_session(daemon_url, &agent_info.id).await?;
    println!("Ghost Protocol — chatting with {} (session {})", agent_info.name, &session_id[..8]);
    println!("Type /exit or Ctrl+C to end session.\n");

    // Placeholder — interactive loop added in next task
    println!("[chat loop not yet wired]");
    Ok(())
}
