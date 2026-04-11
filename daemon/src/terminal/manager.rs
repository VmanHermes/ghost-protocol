use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::store::sessions::TerminalSessionRecord;
use crate::store::Store;
use crate::supervisor;
use crate::terminal::broadcaster::SessionBroadcaster;
use crate::terminal::session;
use crate::terminal::tmux;

/// Manages the lifecycle of all terminal sessions: creation, input, resize,
/// interrupt, termination, idle-timeout detach, and crash recovery.
#[derive(Clone)]
pub struct TerminalManager {
    sessions: Arc<Mutex<HashMap<String, ManagedSessionEntry>>>,
    broadcasters: Arc<Mutex<HashMap<String, Arc<SessionBroadcaster>>>>,
    store: Store,
}

/// Wraps ManagedSession so it can live inside the HashMap.
/// (We use a separate type alias for clarity.)
type ManagedSessionEntry = session::ManagedSession;

#[derive(Debug, Clone, Default)]
pub struct CreateSessionOptions {
    pub project_id: Option<String>,
    pub parent_session_id: Option<String>,
    pub root_session_id: Option<String>,
    pub host_id: Option<String>,
    pub host_name: Option<String>,
    pub agent_id: Option<String>,
    pub driver_kind: Option<String>,
    pub capabilities: Vec<String>,
}

impl TerminalManager {
    pub fn new(store: Store) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            broadcasters: Arc::new(Mutex::new(HashMap::new())),
            store,
        }
    }

    /// Creates a new terminal session: DB record, tmux session, PTY attach.
    pub async fn create_session(
        &self,
        mode: &str,
        name: Option<&str>,
        workdir: &str,
        command_override: Option<&str>,
        options: CreateSessionOptions,
    ) -> Result<TerminalSessionRecord, String> {
        let workdir = crate::workdir::expand_workdir(workdir);
        let id = Uuid::new_v4().to_string();
        let shell = command_override
            .map(|c| c.to_string())
            .unwrap_or_else(|| std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string()));
        let cmd = tmux::attach_command(&id);
        let driver_kind = options
            .driver_kind
            .clone()
            .unwrap_or_else(|| supervisor::DRIVER_TERMINAL.to_string());
        let capabilities = if options.capabilities.is_empty() {
            supervisor::driver_capabilities(&driver_kind, true, options.agent_id.is_some())
        } else {
            options.capabilities.clone()
        };
        let root_session_id = options
            .root_session_id
            .clone()
            .or_else(|| options.parent_session_id.clone());

        // 1. Create DB record
        let mut record = self
            .store
            .create_work_session(crate::store::sessions::CreateWorkSessionParams {
                id: &id,
                mode,
                name,
                workdir: &workdir,
                command: &cmd,
                session_type: "terminal",
                project_id: options.project_id.as_deref(),
                parent_session_id: options.parent_session_id.as_deref(),
                root_session_id: root_session_id.as_deref(),
                host_id: options.host_id.as_deref(),
                host_name: options.host_name.as_deref(),
                agent_id: options.agent_id.as_deref(),
                driver_kind: &driver_kind,
                capabilities: &capabilities,
                port: None,
                url: None,
                adopted: false,
            })
            .map_err(|e| format!("db error creating session: {e}"))?;

        // 2. Create tmux session
        if let Err(error) = tmux::new_session(&id, &workdir, &shell) {
            tracing::error!(session_id = %id, error = %error, "failed to create tmux session");
            let now = Utc::now().to_rfc3339();
            let _ = self.store.update_terminal_session(
                &id,
                Some("error"),
                None,
                Some(&now),
                None,
                None,
                None,
            );
            return Err(error);
        }

        // 3. Update DB to running
        let now = Utc::now().to_rfc3339();
        self.store
            .update_terminal_session(&id, Some("running"), Some(&now), None, None, None, None)
            .map_err(|e| format!("db error updating session: {e}"))?;
        record.status = "running".to_string();
        record.started_at = Some(now);

        // 4. Create broadcaster
        let broadcaster = Arc::new(SessionBroadcaster::new());
        self.broadcasters
            .lock()
            .await
            .insert(id.clone(), Arc::clone(&broadcaster));

        // 5. Spawn PTY attach
        let managed = session::spawn_attach(id.clone(), self.store.clone(), broadcaster)?;

        // 6. Store session
        self.sessions.lock().await.insert(id.clone(), managed);

        self.spawn_exit_monitor(id.clone());

        // Inject welcome message for terminal sessions
        if mode != "chat" {
            let sys_info = crate::host::detect::get_system_info();
            let hostname = &sys_info.hostname;
            let ip = sys_info.tailscale_ip.as_deref().unwrap_or("local");
            let version = env!("CARGO_PKG_VERSION");
            let welcome = format!(
                "\x1b[2m\
Ghost Protocol v{version} — {hostname} ({ip})\n\
\n\
Commands:\n\
  ghost init          Set up a project in this directory\n\
  ghost status        Mesh overview (machines, sessions)\n\
  ghost agents        Available agents across the mesh\n\
  ghost chat <agent>  Start a chat with an agent\n\
  ghost projects      Registered projects\n\
  ghost help          Full command reference\n\
\x1b[0m\n"
            );
            if let Ok(chunk) = self.store.append_terminal_chunk(&id, "stdout", &welcome) {
                if let Some(bc) = self.broadcasters.lock().await.get(&id) {
                    bc.send(chunk);
                }
            }
        }

        info!(session_id = %id, "terminal session created");
        Ok(record)
    }

    fn spawn_exit_monitor(&self, session_id: String) {
        let store = self.store.clone();
        let sessions = Arc::clone(&self.sessions);
        let broadcasters = Arc::clone(&self.broadcasters);

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;

                let current = match store.get_terminal_session(&session_id) {
                    Ok(Some(record)) => record,
                    Ok(None) => break,
                    Err(_) => break,
                };

                if current.status != "created" && current.status != "running" {
                    break;
                }

                // Session gone entirely (tmux crashed or was killed externally)
                if !tmux::has_session(&session_id) {
                    info!(session_id = %session_id, "terminal session disappeared");
                    let now = Utc::now().to_rfc3339();
                    let _ = store.update_terminal_session(
                        &session_id,
                        Some("exited"),
                        None,
                        Some(&now),
                        None,
                        None,
                        None,
                    );
                    Self::log_session_exit_outcome(&store, &current, None, &now);
                    Self::notify_session_exit(&store, &session_id, None, &broadcasters).await;
                    sessions.lock().await.remove(&session_id);
                    broadcasters.lock().await.remove(&session_id);
                    break;
                }

                // Pane still alive — keep polling
                if !tmux::is_pane_dead(&session_id) {
                    continue;
                }

                // Pane exited — capture exit code before destroying the session
                let exit_code = tmux::pane_exit_code(&session_id);
                let exit_status = match exit_code {
                    Some(0) => "exited",
                    Some(_) => "error",
                    None => "exited",
                };

                info!(
                    session_id = %session_id,
                    exit_code = ?exit_code,
                    "terminal session exited"
                );

                let now = Utc::now().to_rfc3339();
                let _ = store.update_terminal_session(
                    &session_id,
                    Some(exit_status),
                    None,
                    Some(&now),
                    None,
                    None,
                    exit_code,
                );

                // Kill the tmux session now that we've captured the exit code
                tmux::kill_session(&session_id);

                Self::log_session_exit_outcome(&store, &current, exit_code, &now);
                Self::notify_session_exit(&store, &session_id, exit_code, &broadcasters).await;

                sessions.lock().await.remove(&session_id);
                broadcasters.lock().await.remove(&session_id);
                break;
            }
        });
    }

    /// Computes session duration from started_at to finished_at.
    fn compute_duration(started_at: Option<&str>, finished_at: &str) -> Option<f64> {
        let start = started_at?;
        let start_dt = chrono::DateTime::parse_from_rfc3339(start).ok()?;
        let end_dt = chrono::DateTime::parse_from_rfc3339(finished_at).ok()?;
        Some((end_dt - start_dt).num_seconds() as f64)
    }

    /// Creates an outcome record for a terminal session exit.
    fn log_session_exit_outcome(
        store: &Store,
        session: &TerminalSessionRecord,
        exit_code: Option<i32>,
        finished_at: &str,
    ) {
        let status = match exit_code {
            Some(0) => "success",
            Some(_) => "failed",
            None => "success",
        };
        let duration = Self::compute_duration(session.started_at.as_deref(), finished_at);
        let metadata = serde_json::json!({
            "sessionId": session.id,
            "workdir": session.workdir,
            "agentId": session.agent_id,
            "projectId": session.project_id,
        });

        if let Err(e) = store.create_outcome(
            &Uuid::new_v4().to_string(),
            "daemon",
            None,
            "terminal",
            "session_exited",
            Some(&format!("Terminal session exited in {}", session.workdir)),
            None,
            status,
            exit_code,
            duration,
            Some(&metadata.to_string()),
        ) {
            warn!(session_id = %session.id, error = %e, "failed to log session exit outcome");
        }
    }

    /// Sends exit notification to connected WebSocket clients.
    async fn notify_session_exit(
        store: &Store,
        session_id: &str,
        exit_code: Option<i32>,
        broadcasters: &Arc<Mutex<HashMap<String, Arc<SessionBroadcaster>>>>,
    ) {
        let msg = match exit_code {
            Some(code) => format!("\r\n[session exited with code {code}]\r\n"),
            None => "\r\n[session exited]\r\n".to_string(),
        };
        if let Ok(chunk) = store.append_terminal_chunk(session_id, "system", &msg) {
            let broadcaster = {
                let guard = broadcasters.lock().await;
                guard.get(session_id).cloned()
            };
            if let Some(bc) = broadcaster {
                bc.send(chunk);
            }
        }
    }

    /// Ensures a PTY attach process is running for the given session.
    /// If already attached, this is a no-op.
    pub async fn ensure_attached(&self, session_id: &str) -> Result<(), String> {
        // Already attached?
        if self.sessions.lock().await.contains_key(session_id) {
            return Ok(());
        }

        // Verify tmux session still exists
        if !tmux::has_session(session_id) {
            return Err(format!("tmux session not found for {session_id}"));
        }

        // Get or create broadcaster
        let broadcaster = {
            let mut bcs = self.broadcasters.lock().await;
            bcs.entry(session_id.to_string())
                .or_insert_with(|| Arc::new(SessionBroadcaster::new()))
                .clone()
        };

        // Spawn attach
        let managed =
            session::spawn_attach(session_id.to_string(), self.store.clone(), broadcaster)?;
        self.sessions
            .lock()
            .await
            .insert(session_id.to_string(), managed);

        debug!(session_id, "re-attached to session");
        Ok(())
    }

    /// Sends input data to a terminal session.
    pub async fn send_input(
        &self,
        session_id: &str,
        data: &[u8],
        append_newline: bool,
    ) -> Result<(), String> {
        self.ensure_attached(session_id).await?;

        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| format!("session {session_id} not found after attach"))?;

        session.write_input(data)?;
        if append_newline {
            session.write_input(b"\n")?;
        }
        Ok(())
    }

    /// Resizes a terminal session and returns the updated DB record.
    pub async fn resize_session(
        &self,
        session_id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<TerminalSessionRecord, String> {
        self.ensure_attached(session_id).await?;

        {
            let sessions = self.sessions.lock().await;
            let session = sessions
                .get(session_id)
                .ok_or_else(|| format!("session {session_id} not found"))?;
            session.resize(cols, rows);
        }

        self.store
            .get_terminal_session(session_id)
            .map_err(|e| format!("db error: {e}"))?
            .ok_or_else(|| format!("session {session_id} not in db"))
    }

    /// Sends Ctrl+C (interrupt) to a terminal session.
    pub async fn interrupt_session(&self, session_id: &str) -> Result<(), String> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| format!("session {session_id} not found"))?;
        session.interrupt()
    }

    /// Terminates a terminal session: removes PTY, kills tmux, updates DB.
    pub async fn terminate_session(
        &self,
        session_id: &str,
    ) -> Result<TerminalSessionRecord, String> {
        // Remove from sessions (drops PTY fd, kills attach process)
        self.sessions.lock().await.remove(session_id);

        // Remove broadcaster
        self.broadcasters.lock().await.remove(session_id);

        // Kill tmux session
        tmux::kill_session(session_id);

        // Update DB
        let now = Utc::now().to_rfc3339();
        self.store
            .update_terminal_session(
                session_id,
                Some("terminated"),
                None,
                Some(&now),
                None,
                None,
                None,
            )
            .map_err(|e| format!("db error: {e}"))?;

        info!(session_id = %session_id, "terminal session terminated");
        self.store
            .get_terminal_session(session_id)
            .map_err(|e| format!("db error: {e}"))?
            .ok_or_else(|| format!("session {session_id} not in db"))
    }

    /// Returns the broadcaster for a session, if one exists.
    pub async fn get_broadcaster(
        &self,
        session_id: &str,
    ) -> Option<Arc<SessionBroadcaster>> {
        self.broadcasters.lock().await.get(session_id).cloned()
    }

    /// Called when a client unsubscribes from a session's output stream.
    /// If no subscribers remain, schedules a delayed detach (120s) to free
    /// resources while keeping the tmux session alive.
    pub fn on_unsubscribe(&self, session_id: &str) {
        let session_id = session_id.to_string();
        let sessions = Arc::clone(&self.sessions);
        let broadcasters = Arc::clone(&self.broadcasters);

        tokio::spawn(async move {
            // Check current subscriber count
            let count = {
                let bcs = broadcasters.lock().await;
                match bcs.get(&session_id) {
                    Some(bc) => bc.subscriber_count(),
                    None => return,
                }
            };

            if count > 0 {
                return;
            }

            debug!(session_id = %session_id, "no subscribers, scheduling idle detach in 120s");
            tokio::time::sleep(std::time::Duration::from_secs(120)).await;

            // Re-check after delay
            let still_zero = {
                let bcs = broadcasters.lock().await;
                match bcs.get(&session_id) {
                    Some(bc) => bc.subscriber_count() == 0,
                    None => return,
                }
            };

            if still_zero {
                info!(session_id = %session_id, "detaching idle session (tmux kept alive)");
                sessions.lock().await.remove(&session_id);
                // Note: we keep the broadcaster so it can be reused on re-attach
            }
        });
    }

    /// Recovers terminal sessions after daemon restart.
    ///
    /// - If tmux is unavailable, marks all incomplete DB sessions as terminated.
    /// - Otherwise, cross-references live `ghost-*` tmux sessions with DB records
    ///   and creates broadcasters for matches (lazy attach on first subscribe).
    /// - Orphaned DB records are marked terminated; orphaned tmux sessions are killed.
    pub async fn recover(&self) {
        info!("recovering terminal sessions");

        if !tmux::is_available() {
            warn!("tmux not available — terminating all incomplete sessions");
            match self.store.terminate_incomplete_sessions() {
                Ok(count) => info!(count, "terminated incomplete sessions"),
                Err(e) => warn!(error = %e, "failed to terminate incomplete sessions"),
            }
            return;
        }

        // Get live tmux sessions
        let live_tmux = tmux::list_ghost_sessions();
        debug!(count = live_tmux.len(), "found live ghost tmux sessions");

        // Get DB sessions that are still running/created
        let db_sessions = match self.store.list_terminal_sessions() {
            Ok(sessions) => sessions,
            Err(e) => {
                warn!(error = %e, "failed to list sessions from db during recovery");
                return;
            }
        };

        let active_db: Vec<&TerminalSessionRecord> = db_sessions
            .iter()
            .filter(|s| s.status == "running" || s.status == "created")
            .collect();

        // Build set of live tmux session names for quick lookup
        let live_set: std::collections::HashSet<String> = live_tmux.into_iter().collect();

        let mut broadcasters = self.broadcasters.lock().await;

        for record in &active_db {
            let tmux_name = tmux::session_name(&record.id);

            if live_set.contains(&tmux_name) {
                // Tmux session alive — create broadcaster for lazy re-attach
                debug!(session_id = %record.id, "recovered session, broadcaster ready");
                broadcasters
                    .entry(record.id.clone())
                    .or_insert_with(|| Arc::new(SessionBroadcaster::new()));
            } else {
                // DB says running but tmux is gone — mark terminated
                info!(session_id = %record.id, "orphaned db record — marking terminated");
                let now = Utc::now().to_rfc3339();
                if let Err(e) = self.store.update_terminal_session(
                    &record.id,
                    Some("terminated"),
                    None,
                    Some(&now),
                    None,
                    None,
                    None,
                ) {
                    warn!(session_id = %record.id, error = %e, "failed to terminate orphaned session");
                }
            }
        }

        // Kill orphaned tmux sessions (live in tmux but not in DB as active)
        let active_tmux_names: std::collections::HashSet<String> = active_db
            .iter()
            .map(|s| tmux::session_name(&s.id))
            .collect();

        for tmux_name in &live_set {
            if !active_tmux_names.contains(tmux_name) {
                info!(tmux_session = %tmux_name, "killing orphaned tmux session");
                // We need to reverse-lookup the session id from the tmux name.
                // tmux name is "ghost-{uuid_no_dashes}", but kill_session expects the
                // original session_id. Since we can't reverse that easily, use the raw
                // tmux kill command directly.
                let _ = std::process::Command::new("tmux")
                    .args(["kill-session", "-t", tmux_name])
                    .output();
            }
        }

        info!("session recovery complete");
    }
}
