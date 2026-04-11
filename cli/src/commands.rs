pub async fn status(daemon_url: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let hw: serde_json::Value = client.get(format!("{daemon_url}/api/system/hardware")).send().await.map_err(|e| format!("daemon unreachable: {e}"))?.json().await.map_err(|e| format!("parse: {e}"))?;
    let hosts: Vec<serde_json::Value> = client.get(format!("{daemon_url}/api/hosts")).send().await.map_err(|e| format!("daemon unreachable: {e}"))?.json().await.map_err(|e| format!("parse: {e}"))?;

    let hostname = hw["hostname"].as_str().unwrap_or("?");
    let ip = hw["tailscaleIp"].as_str().unwrap_or("?");
    let online = hosts.iter().filter(|h| h["status"].as_str() == Some("online")).count();

    println!("Ghost Protocol — {hostname} ({ip})");
    println!("Mesh: {} machine(s), {} online\n", hosts.len() + 1, online + 1);
    for h in &hosts {
        let name = h["name"].as_str().unwrap_or("?");
        let st = h["status"].as_str().unwrap_or("?");
        let dot = if st == "online" { "●" } else { "○" };
        println!("  {dot} {name} [{st}]");
    }
    Ok(())
}

pub async fn agents(daemon_url: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let agents: Vec<crate::detect::AgentInfo> = client.get(format!("{daemon_url}/api/agents")).send().await.map_err(|e| format!("daemon unreachable: {e}"))?.json().await.map_err(|e| format!("parse: {e}"))?;
    if agents.is_empty() {
        println!("No agents detected.");
    } else {
        println!("Available agents:");
        for a in &agents {
            let ver = a.version.as_deref().map(|v| format!(" v{v}")).unwrap_or_default();
            println!("  {} ({}){}", a.name, a.id, ver);
        }
    }
    Ok(())
}

pub async fn projects(daemon_url: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let projects: Vec<serde_json::Value> = client.get(format!("{daemon_url}/api/projects")).send().await.map_err(|e| format!("daemon unreachable: {e}"))?.json().await.map_err(|e| format!("parse: {e}"))?;
    if projects.is_empty() {
        println!("No registered projects. Run 'ghost init' in a project directory.");
    } else {
        println!("Registered projects:");
        for p in &projects {
            let name = p["name"].as_str().unwrap_or("?");
            let workdir = p["workdir"].as_str().unwrap_or("?");
            println!("  {name} ({workdir})");
        }
    }
    Ok(())
}
