use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::chat::adapters::{adapter_for_agent, AdapterEvent};
use crate::chat::broadcaster::{ChatBroadcaster, ChatEvent};
use crate::hardware::agents::AgentInfo;
use crate::store::Store;

struct ManagedChatProcess {
    child: Child,
    stdin_tx: tokio::sync::mpsc::Sender<String>,
    agent_id: String,
}

#[derive(Debug, Clone, Default)]
pub struct ChatSessionLaunchConfig {
    pub system_prompt: Option<String>,
    pub mcp_config: Option<String>,
    pub allowed_tools: Vec<String>,
    pub ghost_env: HashMap<String, String>,
    pub context_preamble: Option<String>,
}

#[derive(Clone)]
pub struct ChatProcessManager {
    processes: Arc<Mutex<HashMap<String, Arc<Mutex<ManagedChatProcess>>>>>,
    broadcasters: Arc<Mutex<HashMap<String, Arc<ChatBroadcaster>>>>,
    store: Store,
}

impl ChatProcessManager {
    pub fn new(store: Store) -> Self {
        Self {
            processes: Arc::new(Mutex::new(HashMap::new())),
            broadcasters: Arc::new(Mutex::new(HashMap::new())),
            store,
        }
    }

    fn build_chat_command(
        agent: &AgentInfo,
        session_id: &str,
        launch: &ChatSessionLaunchConfig,
    ) -> (String, Vec<String>) {
        match agent.id.as_str() {
            "claude-code" => {
                let program = "claude".to_string();
                let mut args = vec![
                    "-p".to_string(),
                    "--verbose".to_string(),
                    "--session-id".to_string(),
                    session_id.to_string(),
                    "--input-format".to_string(),
                    "stream-json".to_string(),
                    "--output-format".to_string(),
                    "stream-json".to_string(),
                    "--include-partial-messages".to_string(),
                ];
                if let Some(system_prompt) = launch.system_prompt.as_ref() {
                    args.push("--append-system-prompt".to_string());
                    args.push(system_prompt.clone());
                }
                if let Some(mcp_config) = launch.mcp_config.as_ref() {
                    args.push("--mcp-config".to_string());
                    args.push(mcp_config.clone());
                }
                if !launch.allowed_tools.is_empty() {
                    args.push("--allowedTools".to_string());
                    args.push(launch.allowed_tools.join(","));
                }
                (program, args)
            }
            _ if agent.command.contains(' ') => {
                let program = "bash".to_string();
                let args = vec!["-c".to_string(), agent.command.clone()];
                (program, args)
            }
            _ => {
                let program = agent.command.clone();
                let args = vec![];
                (program, args)
            }
        }
    }

    fn try_enrich(&self, _session_id: &str, workdir: &str) -> Option<String> {
        use crate::intelligence::config::IntelligenceConfig;
        use crate::intelligence::enricher::{enrich_session, ProjectCommands};

        // Look up project by workdir
        let project = self.store.get_project_by_workdir(workdir).ok().flatten();
        let config_json = project.as_ref().map(|p| p.config_json.as_str());
        let intel_config = IntelligenceConfig::resolve(config_json);

        if !intel_config.is_active() {
            return None;
        }

        let project_id = project.as_ref().map(|p| p.id.as_str());
        let project_name = project.as_ref().map(|p| p.name.as_str());
        let machine_name = hostname::get().ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "unknown".to_string());

        let commands = config_json.map(|j| ProjectCommands::from_config_json(j));

        let result = enrich_session(
            &self.store,
            &intel_config,
            project_id,
            project_name,
            &machine_name,
            commands.as_ref(),
        );

        Some(result.system_prompt)
    }

    pub async fn spawn_session(
        &self,
        session_id: &str,
        agent: &AgentInfo,
        workdir: &str,
        mut launch: ChatSessionLaunchConfig,
    ) -> Result<(), String> {
        let workdir = crate::workdir::expand_workdir(workdir);

        if launch.system_prompt.is_none() {
            if let Some(prompt) = self.try_enrich(session_id, &workdir) {
                launch.system_prompt = Some(prompt);
            }
        }

        let (program, args) = Self::build_chat_command(agent, session_id, &launch);

        let mut child = Command::new(&program)
            .args(&args)
            .current_dir(&workdir)
            .envs(&launch.ghost_env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                error!(
                    session_id = %session_id,
                    agent = %agent.name,
                    program = %program,
                    workdir = %workdir,
                    error = %e,
                    "failed to spawn chat session"
                );
                format!("failed to spawn {}: {e}", agent.name)
            })?;

        let stdin = child.stdin.take().ok_or("failed to capture stdin")?;
        let stdout = child.stdout.take().ok_or("failed to capture stdout")?;
        let stderr = child.stderr.take().ok_or("failed to capture stderr")?;

        let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::channel::<String>(64);

        let broadcaster = Arc::new(ChatBroadcaster::new());
        self.broadcasters
            .lock().await
            .insert(session_id.to_string(), Arc::clone(&broadcaster));

        // Stderr reader task — relay to chat UI as delta events
        let session_id_err = session_id.to_string();
        let bc_stderr: Arc<ChatBroadcaster> = Arc::clone(&broadcaster);
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        let trimmed = line.trim().to_string();
                        warn!(session_id = %session_id_err, stderr = %trimmed, "chat process stderr");
                        if !trimmed.is_empty() {
                            bc_stderr.send(ChatEvent::Delta {
                                session_id: session_id_err.clone(),
                                message_id: format!("stderr-{}", session_id_err),
                                delta: format!("{}\n", trimmed),
                            });
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let managed = Arc::new(Mutex::new(ManagedChatProcess {
            child,
            stdin_tx: stdin_tx.clone(),
            agent_id: agent.id.clone(),
        }));
        self.processes
            .lock().await
            .insert(session_id.to_string(), managed);

        // Send context preamble for non-MCP agents before any other input
        if let Some(preamble) = launch.context_preamble {
            let _ = stdin_tx.send(format!("{}\n", preamble)).await;
        }

        // Stdin writer task
        let session_id_stdin = session_id.to_string();
        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(data) = stdin_rx.recv().await {
                if let Err(e) = stdin.write_all(data.as_bytes()).await {
                    warn!(session_id = %session_id_stdin, error = %e, "stdin write failed");
                    break;
                }
                if let Err(e) = stdin.flush().await {
                    warn!(session_id = %session_id_stdin, error = %e, "stdin flush failed");
                    break;
                }
            }
        });

        // Stdout reader task
        let session_id_read = session_id.to_string();
        let agent_id = agent.id.clone();
        let store = self.store.clone();
        let bc = Arc::clone(&broadcaster);
        let processes = Arc::clone(&self.processes);
        let managed_for_wait = Arc::clone(
            self.processes
                .lock().await
                .get(session_id)
                .ok_or_else(|| format!("missing managed process for session {session_id}"))?,
        );

        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut adapter = adapter_for_agent(&agent_id);
            let mut line = String::new();
            let msg_id = Uuid::new_v4().to_string();

            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        let events = adapter.feed(&line);
                        for event in events {
                            match event {
                                AdapterEvent::Delta(text) => {
                                    bc.send(ChatEvent::Delta {
                                        session_id: session_id_read.clone(),
                                        message_id: msg_id.clone(),
                                        delta: text,
                                    });
                                }
                                AdapterEvent::Message(parsed) => {
                                    let id = Uuid::new_v4().to_string();
                                    if let Ok(chat_msg) = store.create_chat_message(
                                        &id, &session_id_read, &parsed.role, &parsed.content,
                                    ) {
                                        bc.send(ChatEvent::Message { message: chat_msg });
                                    }
                                }
                                AdapterEvent::Status(status) => {
                                    bc.send(ChatEvent::Status {
                                        session_id: session_id_read.clone(),
                                        status,
                                    });
                                }
                                AdapterEvent::Meta { tokens, context_pct } => {
                                    bc.send(ChatEvent::Meta {
                                        session_id: session_id_read.clone(),
                                        tokens,
                                        context_pct,
                                    });
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!(session_id = %session_id_read, error = %e, "stdout read error");
                        break;
                    }
                }
            }

            // Flush adapter on exit
            let events = adapter.flush();
            for event in events {
                if let AdapterEvent::Message(parsed) = event {
                    let id = Uuid::new_v4().to_string();
                    if let Ok(chat_msg) = store.create_chat_message(
                        &id, &session_id_read, &parsed.role, &parsed.content,
                    ) {
                        bc.send(ChatEvent::Message { message: chat_msg });
                    }
                }
            }

            let (mut exit_status, exit_code) = {
                let mut managed = managed_for_wait.lock().await;
                match managed.child.wait().await {
                    Ok(status) => {
                        let code = status.code();
                        let mapped = if status.success() { "exited" } else { "error" };
                        (mapped.to_string(), code)
                    }
                    Err(_) => ("error".to_string(), None),
                }
            };

            if let Ok(Some(existing)) = store.get_terminal_session(&session_id_read) {
                if existing.status == "terminated" {
                    exit_status = "terminated".to_string();
                }
            }

            let finished_at = chrono::Utc::now().to_rfc3339();
            store
                .update_terminal_session(
                    &session_id_read,
                    Some(&exit_status),
                    None,
                    Some(&finished_at),
                    None,
                    None,
                    exit_code,
                )
                .ok();

            let summary = match exit_code {
                Some(code) => format!("Session ended ({exit_status}, code {code})."),
                None => format!("Session ended ({exit_status})."),
            };
            if let Ok(chat_msg) = store.create_chat_message(
                &Uuid::new_v4().to_string(),
                &session_id_read,
                "system",
                &summary,
            ) {
                bc.send(ChatEvent::Message { message: chat_msg });
            }

            bc.send(ChatEvent::Status {
                session_id: session_id_read.clone(),
                status: exit_status,
            });

            processes.lock().await.remove(&session_id_read);
            info!(session_id = %session_id_read, "chat process exited");

            // Post-session processing (intelligence layer)
            {
                let store2 = store.clone();
                let session_id2 = session_id_read.clone();
                tokio::spawn(async move {
                    use crate::intelligence::config::IntelligenceConfig;
                    use crate::intelligence::processor::{self, SessionContext};
                    use crate::intelligence::provider;

                    let session = match store2.get_terminal_session(&session_id2) {
                        Ok(Some(s)) => s,
                        _ => return,
                    };

                    let config_json = session.project_id.as_ref()
                        .and_then(|pid| store2.get_project(pid).ok().flatten())
                        .map(|p| p.config_json);
                    let config = IntelligenceConfig::resolve(config_json.as_deref());

                    if !config.is_active() {
                        return;
                    }

                    let prov = match provider::create_provider(&config) {
                        Some(p) => p,
                        None => return,
                    };

                    let transcript = processor::build_transcript_from_chat(&store2, &session_id2);
                    let machine = hostname::get().ok()
                        .and_then(|h| h.into_string().ok())
                        .unwrap_or_else(|| "unknown".to_string());

                    let duration = session.started_at.as_ref().and_then(|start| {
                        let start = chrono::DateTime::parse_from_rfc3339(start).ok()?;
                        let end = session.finished_at.as_ref()
                            .and_then(|f| chrono::DateTime::parse_from_rfc3339(f).ok())
                            .unwrap_or_else(|| chrono::Utc::now().into());
                        Some((end - start).num_seconds() as f64)
                    });

                    let ctx = SessionContext {
                        session_id: session_id2.clone(),
                        project_id: session.project_id,
                        agent_id: session.agent_id,
                        machine,
                        session_type: session.session_type,
                        duration_secs: duration,
                        transcript,
                    };

                    if let Err(e) = processor::process_session(&store2, prov.as_ref(), &config, ctx).await {
                        tracing::warn!(session_id = %session_id2, error = %e, "post-session processing failed");
                    }
                });
            }
        });

        info!(session_id = %session_id, agent = %agent.name, "chat process spawned");
        Ok(())
    }

    pub async fn send_input(
        &self,
        session_id: &str,
        content: &str,
    ) -> Result<(), String> {
        let processes = self.processes.lock().await;
        let process = processes
            .get(session_id)
            .ok_or_else(|| format!("no chat process for session {session_id}"))?;

        let managed = process.lock().await;

        let formatted = if crate::hardware::agents::is_claude_protocol_agent(&managed.agent_id) {
            let msg = serde_json::json!({
                "type": "user",
                "message": { "role": "user", "content": content }
            });
            format!("{}\n", msg)
        } else {
            format!("{}\n", content)
        };

        managed.stdin_tx
            .send(formatted).await
            .map_err(|e| format!("stdin send failed: {e}"))
    }

    pub async fn get_broadcaster(
        &self,
        session_id: &str,
    ) -> Option<Arc<ChatBroadcaster>> {
        self.broadcasters.lock().await.get(session_id).cloned()
    }

    pub async fn kill_session(&self, session_id: &str) -> Result<(), String> {
        if let Some(process) = self.processes.lock().await.remove(session_id) {
            let mut managed = process.lock().await;
            managed.child.kill().await.map_err(|e| format!("kill failed: {e}"))?;
        }
        info!(session_id = %session_id, "chat session killed");
        self.broadcasters.lock().await.remove(session_id);
        let finished_at = chrono::Utc::now().to_rfc3339();
        self.store
            .update_terminal_session(
                session_id,
                Some("terminated"),
                None,
                Some(&finished_at),
                None,
                None,
                None,
            )
            .ok();
        Ok(())
    }

    pub async fn has_session(&self, session_id: &str) -> bool {
        self.processes.lock().await.contains_key(session_id)
    }
}
