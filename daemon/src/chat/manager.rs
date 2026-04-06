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

    pub async fn spawn_session(
        &self,
        session_id: &str,
        agent: &AgentInfo,
        workdir: &str,
        launch: ChatSessionLaunchConfig,
    ) -> Result<(), String> {
        let workdir = crate::workdir::expand_workdir(workdir);
        let (program, args) = Self::build_chat_command(agent, session_id, &launch);

        let mut child = Command::new(&program)
            .args(&args)
            .current_dir(&workdir)
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

        // Stderr reader task
        let session_id_err = session_id.to_string();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        warn!(session_id = %session_id_err, stderr = %line.trim(), "chat process stderr");
                    }
                    Err(_) => break,
                }
            }
        });

        let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::channel::<String>(64);

        let broadcaster = Arc::new(ChatBroadcaster::new());
        self.broadcasters
            .lock().await
            .insert(session_id.to_string(), Arc::clone(&broadcaster));

        let managed = Arc::new(Mutex::new(ManagedChatProcess {
            child,
            stdin_tx: stdin_tx.clone(),
            agent_id: agent.id.clone(),
        }));
        self.processes
            .lock().await
            .insert(session_id.to_string(), managed);

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

        let formatted = if managed.agent_id == "claude-code" || managed.agent_id.starts_with("claude") {
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
