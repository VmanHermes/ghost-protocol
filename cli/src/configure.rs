use dialoguer::{Input, MultiSelect};
use crate::detect;

#[derive(Default)]
struct ExistingConfig {
    name: Option<String>,
    agents: Vec<ConfigAgent>,
    commands: ConfigCommands,
    environment: std::collections::HashMap<String, String>,
    /// Fields we preserve but don't prompt for
    extra: serde_json::Value,
}

#[derive(Clone)]
struct ConfigAgent {
    id: String,
    enabled: bool,
}

#[derive(Default)]
struct ConfigCommands {
    build: Option<String>,
    test: Option<String>,
    lint: Option<String>,
    deploy: Option<String>,
}

fn load_existing_config(workdir: &std::path::Path) -> Option<ExistingConfig> {
    let config_path = workdir.join(".ghost").join("config.json");
    let content = std::fs::read_to_string(&config_path).ok()?;
    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Warning: .ghost/config.json is malformed: {e}");
            let proceed: bool = dialoguer::Confirm::new()
                .with_prompt("Start fresh? (No to abort)")
                .default(true)
                .interact()
                .unwrap_or(false);
            if !proceed {
                std::process::exit(0);
            }
            return None;
        }
    };

    let name = json["name"].as_str().map(String::from);

    let agents = json["agents"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|a| {
                    let id = a["id"].as_str()?;
                    let enabled = a["enabled"].as_bool().unwrap_or(false);
                    Some(ConfigAgent { id: id.to_string(), enabled })
                })
                .collect()
        })
        .unwrap_or_default();

    let commands = ConfigCommands {
        build: json["commands"]["build"].as_str().map(String::from),
        test: json["commands"]["test"].as_str().map(String::from),
        lint: json["commands"]["lint"].as_str().map(String::from),
        deploy: json["commands"]["deploy"].as_str().map(String::from),
    };

    let environment = json["environment"]
        .as_object()
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default();

    // Preserve fields we don't prompt for
    let mut extra = json.clone();
    if let Some(obj) = extra.as_object_mut() {
        for key in &["name", "workdir", "agents", "commands", "environment", "machines"] {
            obj.remove(*key);
        }
    }

    Some(ExistingConfig { name, agents, commands, environment, extra })
}

pub async fn run(daemon_url: &str) -> Result<(), String> {
    let workdir = std::env::current_dir().map_err(|e| format!("failed to get cwd: {e}"))?;
    let dir_name = workdir.file_name().and_then(|n| n.to_str()).unwrap_or("project").to_string();

    let existing = load_existing_config(&workdir);
    let is_reconfigure = existing.is_some();

    if is_reconfigure {
        println!("Reconfiguring Ghost Protocol project...\n");
    } else {
        println!("Configuring Ghost Protocol project...\n");
    }

    let existing = existing.unwrap_or_default();

    // --- Project name ---
    let default_name = existing.name.unwrap_or(dir_name);
    let name: String = Input::new()
        .with_prompt("Project name")
        .default(default_name)
        .interact_text()
        .map_err(|e| format!("input error: {e}"))?;

    // --- Agent selection ---
    println!("\nDetecting available agents...");
    let detected = detect::detect_local_agents();
    if detected.is_empty() {
        println!("  No agents detected.");
    } else {
        for a in &detected {
            let ver = a.version.as_deref().unwrap_or("");
            println!("  ✓ {} {}", a.name, ver);
        }
    }

    // Build unified agent list: detected agents + any existing config agents not detected
    let mut agent_labels: Vec<String> = Vec::new();
    let mut agent_ids: Vec<String> = Vec::new();
    let mut defaults: Vec<bool> = Vec::new();

    for a in &detected {
        let was_enabled = existing.agents.iter().any(|ea| ea.id == a.id && ea.enabled);
        let is_new = !existing.agents.iter().any(|ea| ea.id == a.id);
        agent_labels.push(a.name.clone());
        agent_ids.push(a.id.clone());
        // Pre-check if: was enabled in existing config, or is new and this is first configure
        defaults.push(was_enabled || (is_new && !is_reconfigure));
    }
    for ea in &existing.agents {
        if !detected.iter().any(|d| d.id == ea.id) {
            agent_labels.push(format!("{} (not detected)", ea.id));
            agent_ids.push(ea.id.clone());
            defaults.push(ea.enabled);
        }
    }

    let selected_indices = if !agent_labels.is_empty() {
        MultiSelect::new()
            .with_prompt("Select agents")
            .items(&agent_labels)
            .defaults(&defaults)
            .interact()
            .map_err(|e| format!("error: {e}"))?
    } else {
        vec![]
    };

    // --- Commands ---
    println!("\nProject commands (Enter to keep, type to replace, - to clear):");
    let build = prompt_command("build", existing.commands.build.as_deref())?;
    let test = prompt_command("test", existing.commands.test.as_deref())?;
    let lint = prompt_command("lint", existing.commands.lint.as_deref())?;
    let deploy = prompt_command("deploy", existing.commands.deploy.as_deref())?;

    // --- Environment ---
    let environment = prompt_environment(&existing.environment)?;

    // --- Build final config ---
    let agents_json: Vec<serde_json::Value> = agent_ids.iter().enumerate().map(|(i, id)| {
        serde_json::json!({
            "id": id,
            "enabled": selected_indices.contains(&i),
            "preferredMachine": null
        })
    }).collect();

    let mut config = serde_json::json!({
        "name": name,
        "workdir": workdir.to_string_lossy(),
        "agents": agents_json,
        "machines": {},
        "commands": {
            "build": build,
            "test": test,
            "lint": lint,
            "deploy": deploy,
        },
        "environment": environment,
    });

    // Merge preserved fields from existing config
    if let (Some(config_obj), Some(extra_obj)) = (config.as_object_mut(), existing.extra.as_object()) {
        for (k, v) in extra_obj {
            config_obj.entry(k.clone()).or_insert(v.clone());
        }
    }

    // Ensure defaults for fields that should always exist
    let config_obj = config.as_object_mut().unwrap();
    config_obj.entry("experimentalMultiAgent".to_string()).or_insert(serde_json::json!(false));
    config_obj.entry("allowedDriverKinds".to_string()).or_insert(serde_json::json!(["terminal_driver", "structured_chat_driver", "api_driver"]));
    config_obj.entry("defaultSkillSet".to_string()).or_insert(serde_json::json!([]));
    config_obj.entry("delegationLimits".to_string()).or_insert(serde_json::json!({
        "maxDepth": 2,
        "maxChildren": 4,
        "budgetTokens": null,
        "budgetSecs": 900
    }));
    config_obj.entry("communicationPolicy".to_string()).or_insert(serde_json::json!("supervisor_mailbox"));

    // --- Write config ---
    let ghost_dir = workdir.join(".ghost");
    std::fs::create_dir_all(&ghost_dir).map_err(|e| format!("failed to create .ghost/: {e}"))?;
    std::fs::write(
        ghost_dir.join("config.json"),
        serde_json::to_string_pretty(&config).unwrap(),
    )
    .map_err(|e| format!("write error: {e}"))?;
    println!("\nSaved .ghost/config.json");

    // --- Register with daemon ---
    let client = reqwest::Client::new();
    match client
        .post(format!("{daemon_url}/api/projects"))
        .json(&serde_json::json!({ "name": name, "workdir": workdir.to_string_lossy(), "config": config }))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => println!("Registered project with daemon."),
        Ok(resp) => {
            let t = resp.text().await.unwrap_or_default();
            println!("Warning: daemon registration failed: {t}");
        }
        Err(_) => println!("Warning: daemon not reachable. Config saved locally."),
    }

    println!("\nRun 'ghost chat <agent>' to start working.");
    Ok(())
}

fn prompt_command(label: &str, existing: Option<&str>) -> Result<Option<String>, String> {
    let prompt = match existing {
        Some(val) => format!("  {label} [{val}]"),
        None => format!("  {label}"),
    };
    let default = existing.unwrap_or("").to_string();
    let input: String = Input::new()
        .with_prompt(&prompt)
        .default(default)
        .allow_empty(true)
        .interact_text()
        .map_err(|e| format!("input error: {e}"))?;

    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed == "-" {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

fn prompt_environment(
    existing: &std::collections::HashMap<String, String>,
) -> Result<std::collections::HashMap<String, String>, String> {
    let mut env = existing.clone();

    if !env.is_empty() {
        println!("\nCurrent environment:");
        for (k, v) in &env {
            println!("  {k}={v}");
        }
    }

    loop {
        let action: String = Input::new()
            .with_prompt("Environment (add KEY=VALUE, remove KEY, or Enter to finish)")
            .default("".into())
            .allow_empty(true)
            .interact_text()
            .map_err(|e| format!("input error: {e}"))?;

        let action = action.trim();
        if action.is_empty() {
            break;
        }

        if let Some((key, value)) = action.split_once('=') {
            env.insert(key.trim().to_string(), value.trim().to_string());
            println!("  Set {}", key.trim());
        } else if env.contains_key(action) {
            env.remove(action);
            println!("  Removed {action}");
        } else {
            println!("  Unknown key '{action}'. Use KEY=VALUE to add or KEY to remove.");
        }
    }

    Ok(env)
}
