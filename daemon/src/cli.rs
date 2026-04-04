use crate::config::CliCommand;

pub async fn run(command: CliCommand, port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    match command {
        CliCommand::Status { json } => cmd_status(&client, &base, json).await,
        CliCommand::Sessions { json } => cmd_sessions(&client, &base, json).await,
        CliCommand::Hosts { json } => cmd_hosts(&client, &base, json).await,
        CliCommand::Info => cmd_info(&client, &base).await,
        CliCommand::Serve => unreachable!("serve is handled in main"),
    }
}

async fn cmd_status(
    client: &reqwest::Client,
    base: &str,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let hardware: serde_json::Value = client
        .get(format!("{base}/api/system/hardware"))
        .send()
        .await?
        .json()
        .await?;
    let status: serde_json::Value = client
        .get(format!("{base}/api/system/hardware/status"))
        .send()
        .await?
        .json()
        .await?;

    if json {
        let combined = serde_json::json!({
            "hardware": hardware,
            "status": status,
        });
        println!("{}", serde_json::to_string_pretty(&combined)?);
        return Ok(());
    }

    let hostname = hardware["hostname"].as_str().unwrap_or("unknown");
    let sessions = status["activeSessions"].as_u64().unwrap_or(0);
    let ram_used = status["ramUsedGb"].as_f64().unwrap_or(0.0);
    let ram_total = status["ramTotalGb"].as_f64().unwrap_or(0.0);

    let gpu_part = match hardware["gpu"]["model"].as_str() {
        Some(model) => {
            let pct = status["gpuPercent"].as_u64().map(|p| format!("{p}%")).unwrap_or_else(|| "N/A".into());
            format!("GPU: {pct} ({model})")
        }
        None => "GPU: none".to_string(),
    };

    let ollama_part = if hardware["tools"]["ollama"].is_string() {
        "Ollama: running"
    } else {
        "Ollama: off"
    };

    println!(
        "{hostname} | {sessions} sessions | {gpu_part} | RAM: {ram_used:.0}/{ram_total:.0}GB | {ollama_part}"
    );
    Ok(())
}

async fn cmd_sessions(
    client: &reqwest::Client,
    base: &str,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let sessions: Vec<serde_json::Value> = client
        .get(format!("{base}/api/terminal/sessions"))
        .send()
        .await?
        .json()
        .await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&sessions)?);
        return Ok(());
    }

    if sessions.is_empty() {
        println!("No active sessions.");
        return Ok(());
    }

    println!(
        "{:<12} {:<16} {:<12} {:<8} {}",
        "ID", "NAME", "STATUS", "IDLE", "WORKDIR"
    );
    for s in &sessions {
        let id = s["id"].as_str().unwrap_or("?");
        let short_id = if id.len() > 8 { &id[..8] } else { id };
        let name = s["name"].as_str().unwrap_or("—");
        let status = s["status"].as_str().unwrap_or("?");
        let workdir = s["workdir"].as_str().unwrap_or("?");

        let idle = match s["lastChunkAt"].as_str() {
            Some(ts) => {
                if let Ok(t) = chrono::DateTime::parse_from_rfc3339(ts) {
                    let dur = chrono::Utc::now().signed_duration_since(t);
                    if dur.num_hours() > 0 {
                        format!("{}h", dur.num_hours())
                    } else {
                        format!("{}m", dur.num_minutes())
                    }
                } else {
                    "?".to_string()
                }
            }
            None => "—".to_string(),
        };

        println!("{short_id:<12} {name:<16} {status:<12} {idle:<8} {workdir}");
    }
    Ok(())
}

async fn cmd_hosts(
    client: &reqwest::Client,
    base: &str,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let hosts: Vec<serde_json::Value> = client
        .get(format!("{base}/api/hosts"))
        .send()
        .await?
        .json()
        .await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&hosts)?);
        return Ok(());
    }

    if hosts.is_empty() {
        println!("No known hosts.");
        return Ok(());
    }

    println!(
        "{:<16} {:<18} {:<10} {:<16} {}",
        "NAME", "IP", "STATUS", "GPU", "RAM"
    );
    for h in &hosts {
        let name = h["name"].as_str().unwrap_or("?");
        let ip = h["tailscaleIp"].as_str().unwrap_or("?");
        let status = h["status"].as_str().unwrap_or("?");
        let gpu = h["capabilities"]["gpu"]
            .as_str()
            .unwrap_or("none");
        let ram = h["capabilities"]["ramGb"]
            .as_f64()
            .map(|r| format!("{r:.0}GB"))
            .unwrap_or_else(|| "?".to_string());

        println!("{name:<16} {ip:<18} {status:<10} {gpu:<16} {ram}");
    }
    Ok(())
}

async fn cmd_info(
    client: &reqwest::Client,
    base: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let hardware: serde_json::Value = client
        .get(format!("{base}/api/system/hardware"))
        .send()
        .await?
        .json()
        .await?;
    let status: serde_json::Value = client
        .get(format!("{base}/api/system/hardware/status"))
        .send()
        .await?
        .json()
        .await?;

    let combined = serde_json::json!({
        "machine": hardware,
        "status": status,
    });
    println!("{}", serde_json::to_string_pretty(&combined)?);
    Ok(())
}
