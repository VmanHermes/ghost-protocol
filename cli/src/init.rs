use dialoguer::{Input, MultiSelect};
use crate::detect;

pub async fn run(daemon_url: &str) -> Result<(), String> {
    let workdir = std::env::current_dir().map_err(|e| format!("failed to get cwd: {e}"))?;
    let dir_name = workdir.file_name().and_then(|n| n.to_str()).unwrap_or("project").to_string();

    println!("Initializing Ghost Protocol project...\n");

    let name: String = Input::new().with_prompt("Project name").default(dir_name).interact_text().map_err(|e| format!("input error: {e}"))?;

    println!("\nDetecting available agents...");
    let agents = detect::detect_local_agents();
    if agents.is_empty() {
        println!("  No agents detected.");
    } else {
        for a in &agents {
            let ver = a.version.as_deref().unwrap_or("");
            println!("  ✓ {} {}", a.name, ver);
        }
    }

    let selected = if !agents.is_empty() {
        let labels: Vec<String> = agents.iter().map(|a| a.name.clone()).collect();
        let selections = MultiSelect::new().with_prompt("Select agents").items(&labels).defaults(&vec![true; labels.len()]).interact().map_err(|e| format!("error: {e}"))?;
        selections.into_iter().map(|i| agents[i].clone()).collect::<Vec<_>>()
    } else {
        vec![]
    };

    let config = serde_json::json!({
        "name": name,
        "workdir": workdir.to_string_lossy(),
        "agents": selected.iter().map(|a| serde_json::json!({ "id": a.id, "enabled": true, "preferredMachine": null })).collect::<Vec<_>>(),
        "machines": {},
        "commands": { "build": null, "test": null, "lint": null, "deploy": null },
        "environment": {}
    });

    let ghost_dir = workdir.join(".ghost");
    std::fs::create_dir_all(&ghost_dir).map_err(|e| format!("failed to create .ghost/: {e}"))?;
    std::fs::write(ghost_dir.join("config.json"), serde_json::to_string_pretty(&config).unwrap()).map_err(|e| format!("write error: {e}"))?;
    println!("\nCreated .ghost/config.json");

    let client = reqwest::Client::new();
    match client.post(format!("{daemon_url}/api/projects")).json(&serde_json::json!({ "name": name, "workdir": workdir.to_string_lossy(), "config": config })).send().await {
        Ok(resp) if resp.status().is_success() => println!("Registered project with daemon."),
        Ok(resp) => { let t = resp.text().await.unwrap_or_default(); println!("Warning: daemon registration failed: {t}"); }
        Err(_) => println!("Warning: daemon not reachable. Config saved locally."),
    }

    println!("\nRun 'ghost chat <agent>' to start working.");
    Ok(())
}
