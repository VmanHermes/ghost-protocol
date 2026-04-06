use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::info;

use crate::chat::manager::ChatProcessManager;
use crate::host::logs::LogBuffer;
use crate::middleware::permissions::{
    ClientIp, CurrentPeerTier, OptionalNeedsApproval, RequireFullAccess, RequireLocalhostOnly,
    RequireReadOnly,
};
use crate::middleware::permissions::PeerTier;
use crate::store::delegations::{AgentMessageRecord, CreateAgentMessage, CreateDelegationContract, DelegationContractRecord};
use crate::store::permissions::{PeerPermission, PendingApproval};
use crate::store::sessions::WorkSessionRecord;
use crate::store::skills::{CreateSkillCandidate, SkillCandidateRecord};
use crate::store::Store;
use crate::supervisor;
use crate::terminal::manager::TerminalManager;

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub manager: TerminalManager,
    pub chat_manager: ChatProcessManager,
    pub supervisor_tx: broadcast::Sender<supervisor::SupervisorEvent>,
    pub log_buffer: LogBuffer,
    pub bind_address: String,
    pub bind_port: u16,
    pub allowed_cidrs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkSessionViews {
    pub chat: bool,
    pub terminal: bool,
    pub logs: bool,
    pub artifacts: bool,
    pub approvals: bool,
    pub delegation: bool,
    pub open_companion_terminal: bool,
    pub reopen_as_terminal: bool,
    pub safe_mode_switch: bool,
}

fn emit_supervisor_event(
    sender: &broadcast::Sender<supervisor::SupervisorEvent>,
    event_type: &str,
    session_id: Option<&str>,
    contract_id: Option<&str>,
    payload: serde_json::Value,
) {
    let _ = sender.send(supervisor::SupervisorEvent {
        event_type: event_type.to_string(),
        session_id: session_id.map(str::to_string),
        contract_id: contract_id.map(str::to_string),
        ts: chrono::Utc::now().to_rfc3339(),
        payload,
    });
}

fn resolve_agent(agent_id: Option<&str>) -> Option<crate::hardware::agents::AgentInfo> {
    agent_id.and_then(|aid| {
        crate::hardware::agents::detect_agents()
            .into_iter()
            .find(|agent| agent.id == aid)
    })
}

fn derive_work_session_views(session: &crate::store::sessions::TerminalSessionRecord) -> WorkSessionViews {
    let is_structured_chat = matches!(
        session.driver_kind.as_str(),
        supervisor::DRIVER_STRUCTURED_CHAT | supervisor::DRIVER_API
    );
    let supports_chat = if is_structured_chat {
        true
    } else {
        supervisor::supports_capability(&session.capabilities, supervisor::CAP_CHAT_VIEW)
    };
    let supports_terminal = if is_structured_chat {
        false
    } else {
        supervisor::supports_capability(&session.capabilities, supervisor::CAP_TERMINAL_VIEW)
    };
    let safe_mode_switch = if is_structured_chat {
        false
    } else {
        supervisor::supports_capability(&session.capabilities, supervisor::CAP_SAFE_MODE_SWITCH)
    };

    WorkSessionViews {
        chat: supports_chat,
        terminal: supports_terminal,
        logs: true,
        artifacts: false,
        approvals: true,
        delegation: supervisor::supports_capability(&session.capabilities, supervisor::CAP_DELEGATION)
            || supervisor::supports_capability(&session.capabilities, supervisor::CAP_MAILBOX),
        open_companion_terminal: is_structured_chat && session.agent_id.is_some(),
        reopen_as_terminal: !supports_terminal && session.agent_id.is_some(),
        safe_mode_switch,
    }
}

fn normalize_project_config_json(config: &serde_json::Value) -> String {
    serde_json::to_string(&supervisor::normalize_project_config(config)).unwrap_or_else(|_| "{}".to_string())
}

fn build_ghost_mcp_config(bind_port: u16) -> Option<String> {
    let daemon_path = std::env::current_exe().ok()?;
    let command = daemon_path.to_str()?.to_string();
    Some(serde_json::json!({
        "mcpServers": {
            "ghost-daemon": {
                "type": "stdio",
                "command": command,
                "args": ["--bind-port", bind_port.to_string(), "mcp"],
            }
        }
    }).to_string())
}

fn ghost_mcp_allowed_tools() -> Vec<String> {
    let server_names = ["ghost-daemon", "ghost_daemon"];
    let tool_names = [
        "ghost_report_outcome",
        "ghost_check_mesh",
        "ghost_list_machines",
        "ghost_list_agents",
        "ghost_spawn_remote_session",
    ];

    server_names
        .into_iter()
        .flat_map(|server| {
            tool_names
                .iter()
                .map(move |tool| format!("mcp__{server}__{tool}"))
        })
        .collect()
}

fn build_chat_system_prompt(
    session: &crate::store::sessions::TerminalSessionRecord,
    project: Option<&crate::store::projects::ProjectRecord>,
    mcp_attached: bool,
) -> String {
    let mut lines = vec![
        "You are running inside Ghost Protocol, a secure supervisor and desktop harness for agent sessions across a Tailscale mesh.".to_string(),
        "Use the Ghost context provided by this session when answering questions about Ghost Protocol, mesh routing, projects, approvals, observability, or available agents.".to_string(),
        format!("Current Ghost work session id: {}", session.id),
        format!("Current working directory: {}", session.workdir),
    ];

    if let Some(project) = project {
        lines.push(format!("Current Ghost project: {} ({})", project.name, project.workdir));
        let config = supervisor::parse_project_config(&project.config_json);
        lines.push(format!("Project communication policy: {}", config.communication_policy));
        lines.push(format!(
            "Experimental multi-agent mode: {}",
            if config.experimental_multi_agent { "enabled" } else { "disabled" }
        ));
    } else {
        lines.push("No registered Ghost project is attached to this session.".to_string());
    }

    if mcp_attached {
        lines.push("A Ghost Protocol MCP server named 'ghost-daemon' is configured for this session. Use it for mesh context, machines, sessions, agents, outcomes, and Ghost tools when relevant.".to_string());
    } else {
        lines.push("A Ghost Protocol MCP server could not be attached for this session. Rely on the provided session and project context, and be explicit about missing runtime capabilities.".to_string());
    }

    lines.push("Do not say you are unfamiliar with Ghost Protocol. If a feature is not exposed in this session, say that precisely instead.".to_string());
    lines.push("Be precise about limits: this session does not automatically have direct inter-agent communication unless Ghost exposes an explicit tool, mailbox, or contract for it.".to_string());
    lines.push("Distinguish clearly between Ghost-managed work sessions and your own runtime-internal helper agents or subagents. Internal helper agents are not Ghost work sessions and are not expected to appear on the Ghost mesh by default.".to_string());
    lines.push("When discussing whether an agent is visible in Ghost Protocol, treat only supervisor-managed sessions, delegations, and mailbox/contract events as Ghost-visible by default.".to_string());
    lines.push("Some approval prompts may still come from the agent runtime itself rather than the Ghost approvals panel. Explain that clearly when it happens.".to_string());

    lines.join("\n")
}

fn session_depth(state: &AppState, session: &crate::store::sessions::TerminalSessionRecord) -> usize {
    let mut depth = 0usize;
    let mut current = session.parent_session_id.clone();
    while let Some(parent_id) = current {
        depth += 1;
        current = state
            .store
            .get_terminal_session(&parent_id)
            .ok()
            .flatten()
            .and_then(|parent| parent.parent_session_id);
    }
    depth
}

// ---------------------------------------------------------------------------
// GET /health
// ---------------------------------------------------------------------------

pub async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true }))
}

// ---------------------------------------------------------------------------
// GET /api/system/status
// ---------------------------------------------------------------------------

pub async fn system_status(
    _tier: RequireReadOnly,
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let sessions = state.store.list_terminal_sessions().unwrap_or_default();
    let active_count = sessions.iter().filter(|s| s.status == "running").count();

    Json(serde_json::json!({
        "activeTerminalSessions": active_count,
        "connection": {
            "bindHost": state.bind_address,
            "allowedCidrs": state.allowed_cidrs,
        }
    }))
}

// ---------------------------------------------------------------------------
// GET /api/system/logs?limit=200&level=INFO
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct LogsQuery {
    #[serde(default = "default_log_limit")]
    pub limit: usize,
    pub level: Option<String>,
}

fn default_log_limit() -> usize {
    200
}

pub async fn system_logs(
    _tier: RequireReadOnly,
    State(state): State<AppState>,
    Query(params): Query<LogsQuery>,
) -> Json<Vec<crate::host::logs::LogEntry>> {
    let entries = state
        .log_buffer
        .entries(params.limit, params.level.as_deref());
    Json(entries)
}

// ---------------------------------------------------------------------------
// GET /api/terminal/sessions
// ---------------------------------------------------------------------------

pub async fn list_sessions(
    _tier: RequireReadOnly,
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::store::sessions::TerminalSessionRecord>>, (StatusCode, Json<serde_json::Value>)>
{
    state.store.list_terminal_sessions().map(Json).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("db error: {e}") })),
        )
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorkSessionBody {
    pub driver_kind: Option<String>,
    pub mode: Option<String>,
    pub name: Option<String>,
    pub workdir: Option<String>,
    pub agent_id: Option<String>,
    pub project_id: Option<String>,
    pub parent_session_id: Option<String>,
    pub root_session_id: Option<String>,
}

pub async fn create_work_session(
    _tier: RequireFullAccess,
    needs_approval: OptionalNeedsApproval,
    client_ip: ClientIp,
    State(state): State<AppState>,
    Json(body): Json<CreateWorkSessionBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    let driver_kind = body
        .driver_kind
        .clone()
        .unwrap_or_else(|| supervisor::DRIVER_TERMINAL.to_string());

    let compat_body = CreateSessionBody {
        mode: body.mode.unwrap_or_else(default_mode),
        name: body.name.clone(),
        workdir: body.workdir.unwrap_or_else(default_workdir),
        agent_id: body.agent_id.clone(),
        project_id: body.project_id.clone(),
        parent_session_id: body.parent_session_id.clone(),
        root_session_id: body.root_session_id.clone(),
        driver_kind: Some(driver_kind.clone()),
    };

    if driver_kind == supervisor::DRIVER_STRUCTURED_CHAT || driver_kind == supervisor::DRIVER_API {
        let chat_body = CreateChatSessionBody {
            agent_id: compat_body.agent_id.clone().unwrap_or_default(),
            project_id: compat_body.project_id.clone(),
            workdir: Some(compat_body.workdir.clone()),
            parent_session_id: compat_body.parent_session_id.clone(),
            root_session_id: compat_body.root_session_id.clone(),
        };
        let (status, json) = create_chat_session(
            RequireFullAccess,
            needs_approval,
            client_ip,
            State(state),
            Json(chat_body),
        ).await?;
        return Ok((status, json));
    }

    create_session(
        RequireFullAccess,
        needs_approval,
        client_ip,
        State(state),
        Json(compat_body),
    ).await
}

pub async fn get_work_session(
    _tier: RequireReadOnly,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<WorkSessionRecord>, (StatusCode, Json<serde_json::Value>)> {
    state
        .store
        .get_work_session(&id)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": format!("work session {id} not found") })),
            )
        })
        .map(Json)
}

pub async fn get_work_session_views(
    _tier: RequireReadOnly,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let session = state
        .store
        .get_terminal_session(&id)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": format!("work session {id} not found") })),
            )
        })?;

    let views = derive_work_session_views(&session);
    Ok(Json(serde_json::json!({
        "session": session.as_work_session(),
        "views": views,
    })))
}

pub async fn create_companion_terminal(
    _tier: RequireFullAccess,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<crate::store::sessions::TerminalSessionRecord>), (StatusCode, Json<serde_json::Value>)> {
    let session = state
        .store
        .get_terminal_session(&id)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": format!("work session {id} not found") })),
            )
        })?;

    let companion = state
        .manager
        .create_session(
            "terminal",
            Some("Companion shell"),
            &session.workdir,
            None,
            crate::terminal::manager::CreateSessionOptions {
                project_id: session.project_id.clone(),
                parent_session_id: Some(session.id.clone()),
                root_session_id: session.root_session_id.clone().or_else(|| Some(session.id.clone())),
                host_id: session.host_id.clone(),
                host_name: session.host_name.clone(),
                agent_id: session.agent_id.clone(),
                driver_kind: Some(supervisor::DRIVER_TERMINAL.to_string()),
                capabilities: supervisor::driver_capabilities(supervisor::DRIVER_TERMINAL, true, false),
            },
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        })?;

    emit_supervisor_event(
        &state.supervisor_tx,
        "work_session_companion_created",
        Some(&companion.id),
        None,
        serde_json::json!({
            "sourceSessionId": session.id,
            "rootSessionId": companion.root_session_id,
        }),
    );

    Ok((StatusCode::CREATED, Json(companion)))
}

pub async fn reopen_work_session(
    _tier: RequireFullAccess,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<crate::store::sessions::TerminalSessionRecord>), (StatusCode, Json<serde_json::Value>)> {
    let session = state
        .store
        .get_terminal_session(&id)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": format!("work session {id} not found") })),
            )
        })?;

    let reopened = reopen_session_record(&state, &session).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e })),
        )
    })?;

    emit_supervisor_event(
        &state.supervisor_tx,
        "work_session_reopened",
        Some(&reopened.id),
        None,
        serde_json::json!({
            "sourceSessionId": session.id,
            "driverKind": reopened.driver_kind,
            "mode": reopened.mode,
            "rootSessionId": reopened.root_session_id,
        }),
    );

    Ok((StatusCode::CREATED, Json(reopened)))
}

// ---------------------------------------------------------------------------
// POST /api/terminal/sessions
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateSessionBody {
    #[serde(default = "default_mode")]
    pub mode: String,
    pub name: Option<String>,
    #[serde(default = "default_workdir")]
    pub workdir: String,
    #[serde(rename = "agentId")]
    pub agent_id: Option<String>,
    #[serde(rename = "projectId")]
    pub project_id: Option<String>,
    #[serde(rename = "parentSessionId")]
    pub parent_session_id: Option<String>,
    #[serde(rename = "rootSessionId")]
    pub root_session_id: Option<String>,
    #[serde(rename = "driverKind")]
    pub driver_kind: Option<String>,
}

fn default_mode() -> String {
    "local".to_string()
}

fn default_workdir() -> String {
    crate::workdir::default_home_dir()
}

async fn create_terminal_driver_session(
    state: &AppState,
    body: &CreateSessionBody,
) -> Result<crate::store::sessions::TerminalSessionRecord, String> {
    let workdir = crate::workdir::expand_workdir(&body.workdir);
    let agent = resolve_agent(body.agent_id.as_deref());
    let session_name = body.name.as_deref().or(agent.as_ref().map(|a| a.name.as_str()));
    let command_override = agent.as_ref().map(|a| a.command.as_str());
    let driver_kind = body
        .driver_kind
        .clone()
        .unwrap_or_else(|| supervisor::DRIVER_TERMINAL.to_string());
    let capabilities = supervisor::driver_capabilities(&driver_kind, true, agent.is_some());

    state
        .manager
        .create_session(
            &body.mode,
            session_name,
            &workdir,
            command_override,
            crate::terminal::manager::CreateSessionOptions {
                project_id: body.project_id.clone(),
                parent_session_id: body.parent_session_id.clone(),
                root_session_id: body.root_session_id.clone(),
                host_id: None,
                host_name: None,
                agent_id: body.agent_id.clone(),
                driver_kind: Some(driver_kind),
                capabilities,
            },
        )
        .await
}

pub async fn create_session(
    _tier: RequireFullAccess,
    needs_approval: OptionalNeedsApproval,
    client_ip: ClientIp,
    State(state): State<AppState>,
    Json(body): Json<CreateSessionBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)>
{
    if needs_approval.0 {
        let host_id = state
            .store
            .resolve_host_id_by_ip(&client_ip.0)
            .ok()
            .flatten()
            .unwrap_or_default();
        let id = uuid::Uuid::new_v4().to_string();
        let expires_at =
            (chrono::Utc::now() + chrono::Duration::seconds(120)).to_rfc3339();
        let body_json = serde_json::to_string(&serde_json::json!({
            "mode": body.mode,
            "name": body.name,
            "workdir": body.workdir,
            "agentId": body.agent_id,
            "projectId": body.project_id,
            "parentSessionId": body.parent_session_id,
            "rootSessionId": body.root_session_id,
            "driverKind": body.driver_kind,
        }))
        .ok();
        if let Ok(approval) = state.store.create_approval(
            &id,
            &host_id,
            "POST",
            "/api/terminal/sessions",
            body_json.as_deref(),
            &expires_at,
        ) {
            return Ok((
                StatusCode::ACCEPTED,
                Json(serde_json::json!({
                    "approvalRequired": true,
                    "approvalId": approval.id,
                    "expiresAt": approval.expires_at,
                })),
            ));
        }
    }

    let rec = create_terminal_driver_session(&state, &body)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        })?;

    emit_supervisor_event(
        &state.supervisor_tx,
        "work_session_created",
        Some(&rec.id),
        None,
        serde_json::json!({
            "driverKind": rec.driver_kind,
            "capabilities": rec.capabilities,
            "mode": rec.mode,
            "projectId": rec.project_id,
            "parentSessionId": rec.parent_session_id,
            "rootSessionId": rec.root_session_id,
        }),
    );

    // Auto-capture outcome
    let source_host_id = state.store.resolve_host_id_by_ip(&client_ip.0).ok().flatten();
    let metadata = serde_json::json!({ "mode": body.mode, "workdir": body.workdir });
    state.store.create_outcome(
        &uuid::Uuid::new_v4().to_string(),
        "daemon",
        source_host_id.as_deref(),
        "terminal",
        "session_created",
        body.name.as_deref(),
        None,
        "success",
        None,
        None,
        Some(&serde_json::to_string(&metadata).unwrap_or_default()),
    ).ok(); // fire-and-forget

    Ok((StatusCode::CREATED, Json(serde_json::to_value(rec).unwrap_or_default())))
}

// ---------------------------------------------------------------------------
// GET /api/terminal/sessions/{id}
// ---------------------------------------------------------------------------

pub async fn get_session(
    _tier: RequireReadOnly,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let session = state
        .store
        .get_terminal_session(&id)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": format!("session {id} not found") })),
            )
        })?;

    if session.driver_kind == supervisor::DRIVER_TERMINAL {
        state
            .manager
            .ensure_attached(&id)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e })),
                )
            })?;
    }

    let chunks = state
        .store
        .list_terminal_chunks(&id, None, 10_000)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })?;

    Ok(Json(serde_json::json!({
        "session": session,
        "chunks": chunks,
    })))
}

// ---------------------------------------------------------------------------
// POST /api/terminal/sessions/{id}/input
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputBody {
    pub input: String,
    #[serde(default = "default_append_newline_rest")]
    pub append_newline: bool,
}

fn default_append_newline_rest() -> bool {
    true
}

pub async fn send_input(
    _tier: RequireFullAccess,
    needs_approval: OptionalNeedsApproval,
    client_ip: ClientIp,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<InputBody>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    if needs_approval.0 {
        let host_id = state
            .store
            .resolve_host_id_by_ip(&client_ip.0)
            .ok()
            .flatten()
            .unwrap_or_default();
        let approval_id = uuid::Uuid::new_v4().to_string();
        let expires_at =
            (chrono::Utc::now() + chrono::Duration::seconds(120)).to_rfc3339();
        let body_json = serde_json::to_string(&serde_json::json!({
            "input": body.input,
            "appendNewline": body.append_newline,
        }))
        .ok();
        let path = format!("/api/terminal/sessions/{id}/input");
        if state
            .store
            .create_approval(
                &approval_id,
                &host_id,
                "POST",
                &path,
                body_json.as_deref(),
                &expires_at,
            )
            .is_ok()
        {
            return Ok(StatusCode::ACCEPTED);
        }
    }

    state
        .manager
        .send_input(&id, body.input.as_bytes(), body.append_newline)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        })
}

// ---------------------------------------------------------------------------
// POST /api/terminal/sessions/{id}/resize
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ResizeBody {
    pub cols: u16,
    pub rows: u16,
}

pub async fn resize_session(
    _tier: RequireFullAccess,
    needs_approval: OptionalNeedsApproval,
    client_ip: ClientIp,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ResizeBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)>
{
    if needs_approval.0 {
        let host_id = state
            .store
            .resolve_host_id_by_ip(&client_ip.0)
            .ok()
            .flatten()
            .unwrap_or_default();
        let approval_id = uuid::Uuid::new_v4().to_string();
        let expires_at =
            (chrono::Utc::now() + chrono::Duration::seconds(120)).to_rfc3339();
        let body_json = serde_json::to_string(&serde_json::json!({
            "cols": body.cols,
            "rows": body.rows,
        }))
        .ok();
        let path = format!("/api/terminal/sessions/{id}/resize");
        if let Ok(approval) = state.store.create_approval(
            &approval_id,
            &host_id,
            "POST",
            &path,
            body_json.as_deref(),
            &expires_at,
        ) {
            return Ok(Json(serde_json::json!({
                "approvalRequired": true,
                "approvalId": approval.id,
                "expiresAt": approval.expires_at,
            })));
        }
    }

    state
        .manager
        .resize_session(&id, body.cols, body.rows)
        .await
        .map(|rec| Json(serde_json::to_value(rec).unwrap_or_default()))
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        })
}

// ---------------------------------------------------------------------------
// POST /api/terminal/sessions/{id}/terminate
// ---------------------------------------------------------------------------

pub async fn terminate_session(
    _tier: RequireFullAccess,
    needs_approval: OptionalNeedsApproval,
    client_ip: ClientIp,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)>
{
    if needs_approval.0 {
        let host_id = state
            .store
            .resolve_host_id_by_ip(&client_ip.0)
            .ok()
            .flatten()
            .unwrap_or_default();
        let approval_id = uuid::Uuid::new_v4().to_string();
        let expires_at =
            (chrono::Utc::now() + chrono::Duration::seconds(120)).to_rfc3339();
        let path = format!("/api/terminal/sessions/{id}/terminate");
        if let Ok(approval) = state.store.create_approval(
            &approval_id,
            &host_id,
            "POST",
            &path,
            None,
            &expires_at,
        ) {
            return Ok(Json(serde_json::json!({
                "approvalRequired": true,
                "approvalId": approval.id,
                "expiresAt": approval.expires_at,
            })));
        }
    }

    let session = state
        .store
        .get_terminal_session(&id)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": format!("session {id} not found") })),
            )
        })?;

    let result = if session.driver_kind == supervisor::DRIVER_TERMINAL {
        state
            .manager
            .terminate_session(&id)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e })),
                )
            })?
    } else {
        state.chat_manager.kill_session(&id).await.ok();
        let now = chrono::Utc::now().to_rfc3339();
        state
            .store
            .update_terminal_session(&id, Some("terminated"), None, Some(&now), None, None, None)
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("db error: {e}") })),
                )
            })?;
        state
            .store
            .get_terminal_session(&id)
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("db error: {e}") })),
                )
            })?
            .ok_or_else(|| {
                (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({ "error": format!("session {id} not found") })),
                )
            })?
    };

    // Auto-capture outcome
    let source_host_id = state.store.resolve_host_id_by_ip(&client_ip.0).ok().flatten();
    let duration_secs = chrono::DateTime::parse_from_rfc3339(&result.created_at).ok()
        .map(|created| (chrono::Utc::now() - created.with_timezone(&chrono::Utc)).num_seconds() as f64);
    state.store.create_outcome(
        &uuid::Uuid::new_v4().to_string(),
        "daemon",
        source_host_id.as_deref(),
        "terminal",
        "session_terminated",
        None,
        None,
        "cancelled",
        None,
        duration_secs,
        None,
    ).ok(); // fire-and-forget

    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}

// ---------------------------------------------------------------------------
// GET /api/system/hardware
// ---------------------------------------------------------------------------

pub async fn system_hardware(
    _tier: RequireReadOnly,
) -> Json<crate::hardware::MachineInfo> {
    Json(crate::hardware::collect_machine_info())
}

// ---------------------------------------------------------------------------
// GET /api/system/hardware/status
// ---------------------------------------------------------------------------

pub async fn system_hardware_status(
    _tier: RequireReadOnly,
    State(state): State<AppState>,
) -> Json<crate::hardware::MachineStatus> {
    let sessions = state.store.list_terminal_sessions().unwrap_or_default();
    let active = sessions.iter().filter(|s| s.status == "running").count();
    Json(crate::hardware::collect_machine_status(active))
}

// ---------------------------------------------------------------------------
// GET /api/hosts
// ---------------------------------------------------------------------------

pub async fn list_hosts(
    _tier: RequireReadOnly,
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::store::hosts::KnownHost>>, (StatusCode, Json<serde_json::Value>)> {
    state.store.list_known_hosts().map(Json).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("db error: {e}") })),
        )
    })
}

// ---------------------------------------------------------------------------
// POST /api/hosts
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddHostBody {
    pub name: String,
    pub tailscale_ip: String,
}

pub async fn add_host(
    _tier: RequireFullAccess,
    needs_approval: OptionalNeedsApproval,
    client_ip: ClientIp,
    State(state): State<AppState>,
    Json(body): Json<AddHostBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)>
{
    if needs_approval.0 {
        let host_id = state
            .store
            .resolve_host_id_by_ip(&client_ip.0)
            .ok()
            .flatten()
            .unwrap_or_default();
        let approval_id = uuid::Uuid::new_v4().to_string();
        let expires_at =
            (chrono::Utc::now() + chrono::Duration::seconds(120)).to_rfc3339();
        let body_json = serde_json::to_string(&serde_json::json!({
            "name": body.name,
            "tailscaleIp": body.tailscale_ip,
        }))
        .ok();
        if let Ok(approval) = state.store.create_approval(
            &approval_id,
            &host_id,
            "POST",
            "/api/hosts",
            body_json.as_deref(),
            &expires_at,
        ) {
            return Ok((
                StatusCode::ACCEPTED,
                Json(serde_json::json!({
                    "approvalRequired": true,
                    "approvalId": approval.id,
                    "expiresAt": approval.expires_at,
                })),
            ));
        }
    }

    let id = uuid::Uuid::new_v4().to_string();
    let url = format!("http://{}:8787", body.tailscale_ip);
    state
        .store
        .add_known_host(&id, &body.name, &body.tailscale_ip, &url)
        .map(|h| {
            info!(name = %body.name, ip = %body.tailscale_ip, "host added");
            (StatusCode::CREATED, Json(serde_json::to_value(h).unwrap_or_default()))
        })
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })
}

// ---------------------------------------------------------------------------
// DELETE /api/hosts/{id}
// ---------------------------------------------------------------------------

pub async fn remove_host(
    _tier: RequireFullAccess,
    needs_approval: OptionalNeedsApproval,
    client_ip: ClientIp,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    if needs_approval.0 {
        let host_id = state
            .store
            .resolve_host_id_by_ip(&client_ip.0)
            .ok()
            .flatten()
            .unwrap_or_default();
        let approval_id = uuid::Uuid::new_v4().to_string();
        let expires_at =
            (chrono::Utc::now() + chrono::Duration::seconds(120)).to_rfc3339();
        let path = format!("/api/hosts/{id}");
        if state
            .store
            .create_approval(
                &approval_id,
                &host_id,
                "DELETE",
                &path,
                None,
                &expires_at,
            )
            .is_ok()
        {
            return Ok(StatusCode::ACCEPTED);
        }
    }

    state
        .store
        .remove_known_host(&id)
        .map(|_| {
            info!(host_id = %id, "host removed");
            StatusCode::NO_CONTENT
        })
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })
}

// ---------------------------------------------------------------------------
// GET /api/permissions  (localhost-only)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionWithHost {
    pub host_id: String,
    pub host_name: String,
    pub tailscale_ip: String,
    pub tier: String,
    pub updated_at: String,
}

pub async fn list_permissions(
    _: RequireLocalhostOnly,
    State(state): State<AppState>,
) -> Result<Json<Vec<PermissionWithHost>>, (StatusCode, Json<serde_json::Value>)> {
    let hosts = state.store.list_known_hosts().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("db error: {e}") })),
        )
    })?;

    let permissions = state.store.list_peer_permissions().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("db error: {e}") })),
        )
    })?;

    // Build a lookup map from host_id -> PeerPermission
    let perm_map: std::collections::HashMap<String, PeerPermission> = permissions
        .into_iter()
        .map(|p| (p.host_id.clone(), p))
        .collect();

    let result: Vec<PermissionWithHost> = hosts
        .into_iter()
        .map(|h| {
            let (tier, updated_at) = perm_map
                .get(&h.id)
                .map(|p| (p.tier.clone(), p.updated_at.clone()))
                .unwrap_or_else(|| ("no-access".to_string(), String::new()));
            PermissionWithHost {
                host_id: h.id,
                host_name: h.name,
                tailscale_ip: h.tailscale_ip,
                tier,
                updated_at,
            }
        })
        .collect();

    Ok(Json(result))
}

// ---------------------------------------------------------------------------
// PUT /api/hosts/{id}/permissions  (localhost-only)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct SetPermissionBody {
    pub tier: String,
}

pub async fn set_permission(
    _: RequireLocalhostOnly,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SetPermissionBody>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let valid_tiers = ["full-access", "approval-required", "read-only", "no-access"];
    if !valid_tiers.contains(&body.tier.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!(
                    "invalid tier '{}'; must be one of: {}",
                    body.tier,
                    valid_tiers.join(", ")
                )
            })),
        ));
    }

    state
        .store
        .set_peer_permission(&id, &body.tier)
        .map(|_| {
            info!(host_id = %id, tier = %body.tier, "permission changed");
            StatusCode::NO_CONTENT
        })
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })
}

// ---------------------------------------------------------------------------
// GET /api/approvals  (localhost-only)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ApprovalsQuery {
    pub status: Option<String>,
}

pub async fn list_approvals(
    _: RequireLocalhostOnly,
    State(state): State<AppState>,
    Query(params): Query<ApprovalsQuery>,
) -> Result<Json<Vec<PendingApproval>>, (StatusCode, Json<serde_json::Value>)> {
    state
        .store
        .list_approvals(params.status.as_deref())
        .map(Json)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })
}

// ---------------------------------------------------------------------------
// GET /api/approvals/{id}  (localhost-only)
// ---------------------------------------------------------------------------

pub async fn get_approval(
    _: RequireLocalhostOnly,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<PendingApproval>, (StatusCode, Json<serde_json::Value>)> {
    state
        .store
        .get_approval(&id)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": format!("approval {id} not found") })),
            )
        })
        .map(Json)
}

// ---------------------------------------------------------------------------
// replay_request helper
// ---------------------------------------------------------------------------

async fn replay_request(
    state: &AppState,
    method: &str,
    path: &str,
    body_json: Option<&str>,
) -> Result<serde_json::Value, String> {
    // Segment the path to match patterns
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();

    match (method, segments.as_slice()) {
        // POST /api/terminal/sessions
        ("POST", ["api", "terminal", "sessions"]) => {
            let body: CreateSessionBody = body_json
                .and_then(|b| serde_json::from_str(b).ok())
                .unwrap_or_else(|| CreateSessionBody {
                    mode: default_mode(),
                    name: None,
                    workdir: default_workdir(),
                    agent_id: None,
                    project_id: None,
                    parent_session_id: None,
                    root_session_id: None,
                    driver_kind: None,
                });
            let rec = state
                .manager
                .create_session(
                    &body.mode,
                    body.name.as_deref(),
                    &body.workdir,
                    None,
                    crate::terminal::manager::CreateSessionOptions {
                        project_id: body.project_id.clone(),
                        parent_session_id: body.parent_session_id.clone(),
                        root_session_id: body.root_session_id.clone(),
                        host_id: None,
                        host_name: None,
                        agent_id: body.agent_id.clone(),
                        driver_kind: body.driver_kind.clone(),
                        capabilities: Vec::new(),
                    },
                )
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(rec).map_err(|e| e.to_string())
        }

        // POST /api/terminal/sessions/{id}/input
        ("POST", ["api", "terminal", "sessions", id, "input"]) => {
            let body: InputBody = body_json
                .and_then(|b| serde_json::from_str(b).ok())
                .ok_or("missing or invalid input body")?;
            state
                .manager
                .send_input(id, body.input.as_bytes(), body.append_newline)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({}))
        }

        // POST /api/terminal/sessions/{id}/resize
        ("POST", ["api", "terminal", "sessions", id, "resize"]) => {
            let body: ResizeBody = body_json
                .and_then(|b| serde_json::from_str(b).ok())
                .ok_or("missing or invalid resize body")?;
            let rec = state
                .manager
                .resize_session(id, body.cols, body.rows)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(rec).map_err(|e| e.to_string())
        }

        // POST /api/terminal/sessions/{id}/terminate
        ("POST", ["api", "terminal", "sessions", id, "terminate"]) => {
            let rec = state
                .manager
                .terminate_session(id)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(rec).map_err(|e| e.to_string())
        }

        // POST /api/hosts
        ("POST", ["api", "hosts"]) => {
            let body: AddHostBody = body_json
                .and_then(|b| serde_json::from_str(b).ok())
                .ok_or("missing or invalid host body")?;
            let id = uuid::Uuid::new_v4().to_string();
            let url = format!("http://{}:8787", body.tailscale_ip);
            let host = state
                .store
                .add_known_host(&id, &body.name, &body.tailscale_ip, &url)
                .map_err(|e| e.to_string())?;
            serde_json::to_value(host).map_err(|e| e.to_string())
        }

        // DELETE /api/hosts/{id}
        ("DELETE", ["api", "hosts", id]) => {
            state
                .store
                .remove_known_host(id)
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({}))
        }

        _ => Err(format!("no handler for {method} {path}")),
    }
}

// ---------------------------------------------------------------------------
// PUT /api/approvals/{id}/approve  (localhost-only)
// ---------------------------------------------------------------------------

pub async fn approve_approval(
    _: RequireLocalhostOnly,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let approval = state
        .store
        .get_approval(&id)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": format!("approval {id} not found") })),
            )
        })?;

    if approval.status != "pending" {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": format!("approval is already '{}'", approval.status)
            })),
        ));
    }

    let result = replay_request(
        &state,
        &approval.method,
        &approval.path,
        approval.body_json.as_deref(),
    )
    .await;

    let (result_json, result_value) = match result {
        Ok(val) => (
            serde_json::to_string(&val).ok(),
            val,
        ),
        Err(err) => {
            let err_val = serde_json::json!({ "error": err });
            (serde_json::to_string(&err_val).ok(), err_val)
        }
    };

    state
        .store
        .resolve_approval(&id, "approved", result_json.as_deref())
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })?;

    info!(approval_id = %id, method = %approval.method, path = %approval.path, "approval granted and replayed");
    Ok(Json(serde_json::json!({ "result": result_value })))
}

// ---------------------------------------------------------------------------
// GET /api/discoveries (localhost-only)
// ---------------------------------------------------------------------------

pub async fn list_discoveries(
    _guard: RequireLocalhostOnly,
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::store::discoveries::DiscoveredPeer>>, (StatusCode, Json<serde_json::Value>)> {
    state
        .store
        .list_pending_discoveries()
        .map(Json)
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") })))
        })
}

// ---------------------------------------------------------------------------
// PUT /api/discoveries/{ip}/accept (localhost-only)
// ---------------------------------------------------------------------------

pub async fn accept_discovery(
    _guard: RequireLocalhostOnly,
    State(state): State<AppState>,
    Path(ip): Path<String>,
) -> Result<(StatusCode, Json<crate::store::hosts::KnownHost>), (StatusCode, Json<serde_json::Value>)> {
    let peer = state.store.get_discovery(&ip).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") })))
    })?.ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "discovery not found" })))
    })?;

    let id = uuid::Uuid::new_v4().to_string();
    let url = format!("http://{}:8787", ip);
    let host = state.store.add_known_host(&id, &peer.name, &ip, &url).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") })))
    })?;

    state.store.set_peer_permission(&id, "no-access").map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") })))
    })?;

    state.store.accept_discovery(&ip).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") })))
    })?;

    Ok((StatusCode::CREATED, Json(host)))
}

// ---------------------------------------------------------------------------
// PUT /api/discoveries/{ip}/dismiss (localhost-only)
// ---------------------------------------------------------------------------

pub async fn dismiss_discovery(
    _guard: RequireLocalhostOnly,
    State(state): State<AppState>,
    Path(ip): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    state.store.dismiss_discovery(&ip).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") })))
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// POST /api/outcomes (read-only and above)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateOutcomeBody {
    pub category: String,
    pub action: String,
    pub description: Option<String>,
    pub target_machine: Option<String>,
    pub status: String,
    pub exit_code: Option<i32>,
    pub duration_secs: Option<f64>,
    pub metadata: Option<serde_json::Value>,
}

pub async fn create_outcome(
    _tier: RequireReadOnly,
    client_ip: ClientIp,
    State(state): State<AppState>,
    Json(body): Json<CreateOutcomeBody>,
) -> Result<(StatusCode, Json<crate::store::outcomes::OutcomeRecord>), (StatusCode, Json<serde_json::Value>)> {
    let id = uuid::Uuid::new_v4().to_string();
    let source_host_id = state.store.resolve_host_id_by_ip(&client_ip.0).ok().flatten();
    let metadata_json = body.metadata.as_ref().map(|m| serde_json::to_string(m).unwrap_or_default());

    let record = state.store.create_outcome(
        &id,
        "agent",
        source_host_id.as_deref(),
        &body.category,
        &body.action,
        body.description.as_deref(),
        body.target_machine.as_deref(),
        &body.status,
        body.exit_code,
        body.duration_secs,
        metadata_json.as_deref(),
    ).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") })))
    })?;

    if let Some(metadata) = body.metadata.as_ref() {
        if let (Some(source_session_id), Some(proposed_change)) = (
            metadata.get("sourceSessionId").and_then(|v| v.as_str()),
            metadata.get("proposedChange").and_then(|v| v.as_str()),
        ) {
            let trace_refs = metadata
                .get("traceRefs")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([]));
            let risk_level = metadata
                .get("riskLevel")
                .and_then(|v| v.as_str())
                .unwrap_or("medium");

            let trace_refs_json = serde_json::to_string(&trace_refs).unwrap_or_else(|_| "[]".to_string());
            let candidate_id = uuid::Uuid::new_v4().to_string();
            if let Ok(candidate) = state.store.create_skill_candidate(CreateSkillCandidate {
                id: &candidate_id,
                source_session_id,
                trace_refs_json: &trace_refs_json,
                proposed_change,
                risk_level,
            }) {
                emit_supervisor_event(
                    &state.supervisor_tx,
                    "skill_candidate_created",
                    Some(source_session_id),
                    None,
                    serde_json::json!({
                        "candidateId": candidate.id,
                        "riskLevel": candidate.risk_level,
                    }),
                );
            }
        }
    }

    Ok((StatusCode::CREATED, Json(record)))
}

// ---------------------------------------------------------------------------
// GET /api/outcomes (localhost-only)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct OutcomesQuery {
    #[serde(default = "default_outcomes_limit")]
    pub limit: usize,
    pub category: Option<String>,
    pub status: Option<String>,
}

fn default_outcomes_limit() -> usize {
    50
}

pub async fn list_outcomes(
    _guard: RequireLocalhostOnly,
    State(state): State<AppState>,
    Query(params): Query<OutcomesQuery>,
) -> Result<Json<Vec<crate::store::outcomes::OutcomeRecord>>, (StatusCode, Json<serde_json::Value>)> {
    state
        .store
        .list_outcomes(params.limit, params.category.as_deref(), params.status.as_deref())
        .map(Json)
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") })))
        })
}

// ---------------------------------------------------------------------------
// POST /api/projects (localhost-only)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectBody {
    pub name: String,
    pub workdir: String,
    pub config: serde_json::Value,
}

pub async fn create_project(
    _guard: RequireLocalhostOnly,
    State(state): State<AppState>,
    Json(body): Json<CreateProjectBody>,
) -> Result<(StatusCode, Json<crate::store::projects::ProjectRecord>), (StatusCode, Json<serde_json::Value>)> {
    let id = uuid::Uuid::new_v4().to_string();
    let config_json = normalize_project_config_json(&body.config);
    state.store.create_project(&id, &body.name, &body.workdir, &config_json)
        .map(|p| (StatusCode::CREATED, Json(p)))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") }))))
}

// ---------------------------------------------------------------------------
// GET /api/projects (localhost-only)
// ---------------------------------------------------------------------------

pub async fn list_projects(
    _guard: RequireLocalhostOnly,
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::store::projects::ProjectRecord>>, (StatusCode, Json<serde_json::Value>)> {
    state.store.list_projects().map(Json).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") })))
    })
}

// ---------------------------------------------------------------------------
// GET /api/projects/{id} (localhost-only)
// ---------------------------------------------------------------------------

pub async fn get_project(
    _guard: RequireLocalhostOnly,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<crate::store::projects::ProjectRecord>, (StatusCode, Json<serde_json::Value>)> {
    state.store.get_project(&id).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") })))
    })?.ok_or_else(|| (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "project not found" })))).map(Json)
}

// ---------------------------------------------------------------------------
// PUT /api/projects/{id} (localhost-only)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct UpdateProjectBody {
    pub config: serde_json::Value,
}

pub async fn update_project(
    _guard: RequireLocalhostOnly,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateProjectBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let config_json = normalize_project_config_json(&body.config);
    state.store.update_project(&id, &config_json).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") })))
    })?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// DELETE /api/projects/{id} (localhost-only)
// ---------------------------------------------------------------------------

pub async fn remove_project(
    _guard: RequireLocalhostOnly,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    state.store.remove_project(&id).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") })))
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// GET /api/agents (localhost-only)
// ---------------------------------------------------------------------------

pub async fn list_agents(
    _guard: RequireLocalhostOnly,
) -> Json<Vec<crate::hardware::agents::AgentInfo>> {
    Json(crate::hardware::agents::detect_agents())
}

// ---------------------------------------------------------------------------
// POST /api/chat/sessions
// ---------------------------------------------------------------------------

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateChatSessionBody {
    pub agent_id: String,
    pub project_id: Option<String>,
    pub workdir: Option<String>,
    pub parent_session_id: Option<String>,
    pub root_session_id: Option<String>,
}

async fn create_structured_chat_driver_session(
    state: &AppState,
    body: &CreateChatSessionBody,
) -> Result<(crate::store::sessions::TerminalSessionRecord, crate::hardware::agents::AgentInfo), String> {
    let agents = crate::hardware::agents::detect_agents();
    let agent = agents
        .iter()
        .find(|a| a.id == body.agent_id)
        .ok_or_else(|| format!("agent '{}' not found", body.agent_id))?
        .clone();

    let workdir = crate::workdir::expand_workdir(
        body.workdir.as_deref().unwrap_or(&default_workdir()),
    );
    let id = uuid::Uuid::new_v4().to_string();
    let driver_kind = if agent.agent_type == "api" {
        supervisor::DRIVER_API
    } else {
        supervisor::DRIVER_STRUCTURED_CHAT
    };
    let capabilities = supervisor::driver_capabilities(driver_kind, agent.persistent, true);
    let root_session_id = body
        .root_session_id
        .clone()
        .or_else(|| body.parent_session_id.clone());

    let cmd = vec![agent.command.clone()];
    let mut session = state
        .store
        .create_work_session(crate::store::sessions::CreateWorkSessionParams {
            id: &id,
            mode: "chat",
            name: Some(&agent.name),
            workdir: &workdir,
            command: &cmd,
            session_type: "chat",
            project_id: body.project_id.as_deref(),
            parent_session_id: body.parent_session_id.as_deref(),
            root_session_id: root_session_id.as_deref(),
            host_id: None,
            host_name: None,
            agent_id: Some(&body.agent_id),
            driver_kind,
            capabilities: &capabilities,
        })
        .map_err(|e| format!("db error: {e}"))?;

    let project = body
        .project_id
        .as_deref()
        .and_then(|project_id| state.store.get_project(project_id).ok().flatten());
    let mcp_config = if agent.id == "claude-code" || agent.id.starts_with("claude") {
        build_ghost_mcp_config(state.bind_port)
    } else {
        None
    };
    let system_prompt = if agent.id == "claude-code" || agent.id.starts_with("claude") {
        Some(build_chat_system_prompt(&session, project.as_ref(), mcp_config.is_some()))
    } else {
        None
    };
    let allowed_tools = if agent.id == "claude-code" || agent.id.starts_with("claude") {
        ghost_mcp_allowed_tools()
    } else {
        Vec::new()
    };

    let now = chrono::Utc::now().to_rfc3339();
    state
        .store
        .update_terminal_session(&id, Some("running"), Some(&now), None, None, None, None)
        .map_err(|e| format!("db error: {e}"))?;
    session.status = "running".to_string();
    session.started_at = Some(now);

    if let Err(error) = state
        .chat_manager
        .spawn_session(
            &id,
            &agent,
            &workdir,
            crate::chat::manager::ChatSessionLaunchConfig {
                system_prompt,
                mcp_config,
                allowed_tools,
            },
        )
        .await
    {
        let finished_at = chrono::Utc::now().to_rfc3339();
        let _ = state.store.update_terminal_session(
            &id,
            Some("error"),
            None,
            Some(&finished_at),
            None,
            None,
            None,
        );
        let _ = state.store.create_chat_message(
            &uuid::Uuid::new_v4().to_string(),
            &id,
            "system",
            &format!("Failed to start {}: {error}", agent.name),
        );
        tracing::error!(
            session_id = %id,
            agent = %agent.name,
            workdir = %workdir,
            error = %error,
            "structured chat session failed to start"
        );
        return Err(error);
    }

    let mut startup_lines = vec![format!("Chat session started with {} in {}", agent.name, workdir)];
    if let Some(project) = project.as_ref() {
        startup_lines.push(format!("Ghost project: {}", project.name));
    }
    if agent.id == "claude-code" || agent.id.starts_with("claude") {
        startup_lines.push("Ghost context loaded for the agent.".to_string());
        startup_lines.push("Ghost MCP configured: ghost-daemon".to_string());
        startup_lines.push("Ghost MCP tools are pre-approved for this chat session.".to_string());
    }

    state
        .store
        .create_chat_message(
            &uuid::Uuid::new_v4().to_string(),
            &session.id,
            "system",
            &startup_lines.join("\n"),
        )
        .ok();

    Ok((session, agent))
}

async fn reopen_session_record(
    state: &AppState,
    session: &crate::store::sessions::TerminalSessionRecord,
) -> Result<crate::store::sessions::TerminalSessionRecord, String> {
    let root_session_id = session
        .root_session_id
        .clone()
        .or_else(|| Some(session.id.clone()));

    match session.driver_kind.as_str() {
        supervisor::DRIVER_STRUCTURED_CHAT | supervisor::DRIVER_API => {
            let agent_id = session
                .agent_id
                .clone()
                .ok_or_else(|| "session has no agentId and cannot be reopened".to_string())?;
            let (replacement, _) = create_structured_chat_driver_session(
                state,
                &CreateChatSessionBody {
                    agent_id,
                    project_id: session.project_id.clone(),
                    workdir: Some(session.workdir.clone()),
                    parent_session_id: session.parent_session_id.clone(),
                    root_session_id,
                },
            )
            .await?;
            Ok(replacement)
        }
        _ => {
            create_terminal_driver_session(
                state,
                &CreateSessionBody {
                    mode: session.mode.clone(),
                    name: session.name.clone(),
                    workdir: session.workdir.clone(),
                    agent_id: session.agent_id.clone(),
                    project_id: session.project_id.clone(),
                    parent_session_id: session.parent_session_id.clone(),
                    root_session_id,
                    driver_kind: Some(session.driver_kind.clone()),
                },
            )
            .await
        }
    }
}

pub async fn create_chat_session(
    _tier: RequireFullAccess,
    needs_approval: OptionalNeedsApproval,
    client_ip: ClientIp,
    State(state): State<AppState>,
    Json(body): Json<CreateChatSessionBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    // Approval check
    if needs_approval.0 {
        let host_id = state.store.resolve_host_id_by_ip(&client_ip.0).ok().flatten().unwrap_or_default();
        let id = uuid::Uuid::new_v4().to_string();
        let expires_at = (chrono::Utc::now() + chrono::TimeDelta::seconds(120)).to_rfc3339();
        let body_json = serde_json::to_string(&body).ok();
        if let Ok(approval) = state.store.create_approval(&id, &host_id, "POST", "/api/chat/sessions", body_json.as_deref(), &expires_at) {
            return Err((StatusCode::ACCEPTED, Json(serde_json::json!({
                "approvalRequired": true, "approvalId": approval.id, "expiresAt": approval.expires_at
            }))));
        }
    }

    let (session, agent) = create_structured_chat_driver_session(&state, &body)
        .await
        .map_err(|e| {
            if e.contains("agent '") {
                (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": e })),
                )
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e })),
                )
            }
        })?;

    // Outcome log
    let source_host_id = state.store.resolve_host_id_by_ip(&client_ip.0).ok().flatten();
    state.store.create_outcome(
        &uuid::Uuid::new_v4().to_string(), "daemon", source_host_id.as_deref(),
        "chat", "chat_session_created", Some(&agent.name), None, "success", None, None,
        Some(&serde_json::json!({"agentId": body.agent_id, "workdir": session.workdir, "driverKind": session.driver_kind}).to_string()),
    ).ok();

    emit_supervisor_event(
        &state.supervisor_tx,
        "work_session_created",
        Some(&session.id),
        None,
        serde_json::json!({
            "driverKind": session.driver_kind,
            "capabilities": session.capabilities,
            "projectId": session.project_id,
            "parentSessionId": session.parent_session_id,
            "rootSessionId": session.root_session_id,
        }),
    );

    Ok((StatusCode::CREATED, Json(serde_json::json!({ "session": session, "agent": agent }))))
}

// ---------------------------------------------------------------------------
// GET /api/chat/sessions/{id}/messages
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ChatMessagesQuery {
    pub after: Option<String>,
    pub limit: Option<usize>,
}

pub async fn list_chat_messages(
    _tier: RequireReadOnly,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<ChatMessagesQuery>,
) -> Result<Json<Vec<crate::store::chat::ChatMessage>>, (StatusCode, Json<serde_json::Value>)> {
    state.store.list_chat_messages(&id, params.after.as_deref(), params.limit.unwrap_or(100))
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") }))))
}

// ---------------------------------------------------------------------------
// Session auto-naming helper
// ---------------------------------------------------------------------------

fn generate_session_title(content: &str) -> String {
    // Take the first line, strip whitespace, truncate to 50 chars on a word boundary
    let first_line = content.lines().next().unwrap_or(content).trim();
    if first_line.len() <= 50 {
        return first_line.to_string();
    }
    // Truncate on word boundary
    let truncated = &first_line[..50];
    match truncated.rfind(' ') {
        Some(pos) if pos > 20 => format!("{}...", &truncated[..pos]),
        _ => format!("{truncated}..."),
    }
}

// ---------------------------------------------------------------------------
// POST /api/chat/sessions/{id}/message
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct SendChatMessageBody {
    pub content: String,
}

pub async fn send_chat_message(
    _tier: RequireFullAccess,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SendChatMessageBody>,
) -> Result<Json<crate::store::chat::ChatMessage>, (StatusCode, Json<serde_json::Value>)> {
    // Auto-name session on first user message
    let is_first = state.store.count_user_messages(&id).unwrap_or(1) == 0;

    let msg = state.store.create_chat_message(
        &uuid::Uuid::new_v4().to_string(), &id, "user", &body.content,
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") }))))?;

    if is_first {
        let title = generate_session_title(&body.content);
        if let Ok(()) = state.store.update_session_name(&id, &title) {
            if let Some(bc) = state.chat_manager.get_broadcaster(&id).await {
                bc.send(crate::chat::broadcaster::ChatEvent::SessionRenamed {
                    session_id: id.clone(),
                    name: title,
                });
            }
        }
    }

    // Try chat process first, fall back to terminal manager
    if state.chat_manager.has_session(&id).await {
        state.chat_manager.send_input(&id, &body.content).await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e }))))?;
    } else {
        state.manager.send_input(&id, body.content.as_bytes(), true).await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e }))))?;
    }

    Ok(Json(msg))
}

// ---------------------------------------------------------------------------
// POST /api/sessions/{id}/switch-mode
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct SwitchModeBody {
    pub mode: String,
    #[serde(default)]
    pub confirmed: bool,
}

pub async fn switch_session_mode(
    _tier: RequireFullAccess,
    _client_ip: ClientIp,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SwitchModeBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let session = state.store.get_terminal_session(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") }))))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "session not found" }))))?;

    if session.mode == body.mode {
        return Ok(Json(serde_json::json!({ "session": session })));
    }

    let views = derive_work_session_views(&session);
    let safe_mode_switch = views.safe_mode_switch;

    if !safe_mode_switch && !body.confirmed {
        return Ok(Json(serde_json::json!({
            "warning": "This runtime cannot switch views losslessly. Open a companion terminal or reopen in the requested mode instead.",
            "needsConfirmation": true,
            "companionAvailable": views.open_companion_terminal,
            "reopenSupported": views.reopen_as_terminal || session.agent_id.is_some(),
        })));
    }

    if !safe_mode_switch {
        if body.mode == "terminal" && views.open_companion_terminal {
            let companion = state
                .manager
                .create_session(
                    "terminal",
                    Some("Companion shell"),
                    &session.workdir,
                    None,
                    crate::terminal::manager::CreateSessionOptions {
                        project_id: session.project_id.clone(),
                        parent_session_id: Some(session.id.clone()),
                        root_session_id: session.root_session_id.clone().or_else(|| Some(session.id.clone())),
                        host_id: session.host_id.clone(),
                        host_name: session.host_name.clone(),
                        agent_id: session.agent_id.clone(),
                        driver_kind: Some(supervisor::DRIVER_TERMINAL.to_string()),
                        capabilities: supervisor::driver_capabilities(supervisor::DRIVER_TERMINAL, true, false),
                    },
                )
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": e })),
                    )
                })?;

            emit_supervisor_event(
                &state.supervisor_tx,
                "work_session_companion_created",
                Some(&companion.id),
                None,
                serde_json::json!({ "sourceSessionId": session.id }),
            );
            return Ok(Json(serde_json::json!({ "session": companion, "openedCompanion": true })));
        }

        if body.mode == "terminal" {
            let replacement_agent = resolve_agent(session.agent_id.as_deref());
            let replacement = state
                .manager
                .create_session(
                    "terminal",
                    session.name.as_deref(),
                    &session.workdir,
                    replacement_agent.as_ref().map(|a| a.command.as_str()),
                    crate::terminal::manager::CreateSessionOptions {
                        project_id: session.project_id.clone(),
                        parent_session_id: session.parent_session_id.clone(),
                        root_session_id: session.root_session_id.clone().or_else(|| Some(session.id.clone())),
                        host_id: session.host_id.clone(),
                        host_name: session.host_name.clone(),
                        agent_id: session.agent_id.clone(),
                        driver_kind: Some(supervisor::DRIVER_TERMINAL.to_string()),
                        capabilities: supervisor::driver_capabilities(supervisor::DRIVER_TERMINAL, true, session.agent_id.is_some()),
                    },
                )
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": e })),
                    )
                })?;
            if session.mode == "chat" {
                state.chat_manager.kill_session(&id).await.ok();
                state.store.update_terminal_session(&id, Some("terminated"), None, Some(&chrono::Utc::now().to_rfc3339()), None, None, None).ok();
            } else {
                state.manager.terminate_session(&id).await.ok();
            }
            return Ok(Json(serde_json::json!({ "session": replacement, "replacedSessionId": session.id })));
        }

        if body.mode == "chat" {
            let agent_id = session.agent_id.clone().ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "session has no agentId and cannot be reopened as chat" })),
                )
            })?;
            let (replacement, _agent) = create_structured_chat_driver_session(
                &state,
                &CreateChatSessionBody {
                    agent_id,
                    project_id: session.project_id.clone(),
                    workdir: Some(session.workdir.clone()),
                    parent_session_id: session.parent_session_id.clone(),
                    root_session_id: session.root_session_id.clone().or_else(|| Some(session.id.clone())),
                },
            )
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e })),
                )
            })?;
            state.manager.terminate_session(&id).await.ok();
            return Ok(Json(serde_json::json!({ "session": replacement, "replacedSessionId": session.id })));
        }
    }

    let agents = crate::hardware::agents::detect_agents();
    let agent = agents.iter().find(|a| Some(a.name.as_str()) == session.name.as_deref());

    // Update mode in DB for safe in-place switches only
    {
        let conn = state.store.conn();
        conn.execute(
            "UPDATE terminal_sessions SET mode = ?1, status = 'running' WHERE id = ?2",
            rusqlite::params![body.mode, id],
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") }))))?;
    }

    // Spawn in new mode
    if let Some(agent) = agent {
        if body.mode == "chat" {
            state.chat_manager.spawn_session(
                &id,
                agent,
                &session.workdir,
                crate::chat::manager::ChatSessionLaunchConfig::default(),
            ).await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e }))))?;
        } else {
            crate::terminal::tmux::new_session(&id, &session.workdir, &agent.command)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e }))))?;
        }
    }

    let updated = state.store.get_terminal_session(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("db error: {e}") }))))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "session not found" }))))?;

    emit_supervisor_event(
        &state.supervisor_tx,
        "work_session_mode_switched",
        Some(&updated.id),
        None,
        serde_json::json!({ "mode": updated.mode }),
    );

    Ok(Json(serde_json::json!({ "session": updated })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDelegationBody {
    pub parent_session_id: String,
    pub requester_agent_id: Option<String>,
    pub target_host_id: Option<String>,
    pub target_agent_id: String,
    pub task: String,
    #[serde(default)]
    pub allowed_skills: Vec<String>,
    #[serde(default)]
    pub tool_allowlist: Vec<String>,
    #[serde(default)]
    pub artifact_inputs: Vec<String>,
    pub budget_tokens: Option<i64>,
    pub budget_secs: Option<f64>,
    #[serde(default = "default_approval_mode")]
    pub approval_mode: String,
    #[serde(default)]
    pub experimental_comm_enabled: bool,
}

fn default_approval_mode() -> String {
    "restricted".to_string()
}

pub async fn create_delegation(
    _tier: RequireFullAccess,
    CurrentPeerTier(current_tier): CurrentPeerTier,
    State(state): State<AppState>,
    Json(body): Json<CreateDelegationBody>,
) -> Result<(StatusCode, Json<DelegationContractRecord>), (StatusCode, Json<serde_json::Value>)> {
    if current_tier != PeerTier::FullAccess {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "delegation requires full-access tier" })),
        ));
    }

    let parent_session = state
        .store
        .get_terminal_session(&body.parent_session_id)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "parent session not found" })),
            )
        })?;

    let mut project_config = supervisor::ProjectConfig::default();
    if let Some(project_id) = parent_session.project_id.as_deref() {
        if let Some(project) = state.store.get_project(project_id).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })? {
            project_config = supervisor::parse_project_config(&project.config_json);
        }
    }

    let depth = session_depth(&state, &parent_session);
    if depth as u32 >= project_config.delegation_limits.max_depth {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "delegation depth limit exceeded" })),
        ));
    }

    let existing_children = state
        .store
        .list_delegation_contracts_for_parent(&body.parent_session_id)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })?;
    if existing_children.len() as u32 >= project_config.delegation_limits.max_children {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "delegation fan-out limit exceeded" })),
        ));
    }

    if let Some(max_tokens) = project_config.delegation_limits.budget_tokens {
        if body.budget_tokens.unwrap_or(max_tokens as i64) > max_tokens as i64 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "budgetTokens exceeds project delegation limit" })),
            ));
        }
    }
    if body.budget_secs.unwrap_or(project_config.delegation_limits.budget_secs)
        > project_config.delegation_limits.budget_secs
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "budgetSecs exceeds project delegation limit" })),
        ));
    }

    if body.experimental_comm_enabled && !project_config.experimental_multi_agent {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "experimental multi-agent communication is not enabled for this project" })),
        ));
    }

    let contract_id = uuid::Uuid::new_v4().to_string();
    let allowed_skills_json = serde_json::to_string(&body.allowed_skills).unwrap_or_else(|_| "[]".to_string());
    let tool_allowlist_json = serde_json::to_string(&body.tool_allowlist).unwrap_or_else(|_| "[]".to_string());
    let artifact_inputs_json = serde_json::to_string(&body.artifact_inputs).unwrap_or_else(|_| "[]".to_string());

    let contract = state
        .store
        .create_delegation_contract(CreateDelegationContract {
            id: &contract_id,
            parent_session_id: &body.parent_session_id,
            requester_agent_id: body.requester_agent_id.as_deref(),
            target_host_id: body.target_host_id.as_deref(),
            target_agent_id: &body.target_agent_id,
            task: &body.task,
            allowed_skills_json: &allowed_skills_json,
            tool_allowlist_json: &tool_allowlist_json,
            artifact_inputs_json: &artifact_inputs_json,
            budget_tokens: body.budget_tokens,
            budget_secs: body.budget_secs,
            approval_mode: &body.approval_mode,
            experimental_comm_enabled: body.experimental_comm_enabled,
        })
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })?;

    emit_supervisor_event(
        &state.supervisor_tx,
        "delegation_created",
        Some(&body.parent_session_id),
        Some(&contract.id),
        serde_json::json!({
            "targetAgentId": contract.target_agent_id,
            "targetHostId": contract.target_host_id,
            "experimentalCommEnabled": contract.experimental_comm_enabled,
        }),
    );

    Ok((StatusCode::CREATED, Json(contract)))
}

pub async fn get_delegation(
    _tier: RequireReadOnly,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DelegationContractRecord>, (StatusCode, Json<serde_json::Value>)> {
    state
        .store
        .get_delegation_contract(&id)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "delegation not found" })),
            )
        })
        .map(Json)
}

pub async fn list_delegation_messages(
    _tier: RequireReadOnly,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<AgentMessageRecord>>, (StatusCode, Json<serde_json::Value>)> {
    state
        .store
        .list_delegation_messages(&id)
        .map(Json)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDelegationMessageBody {
    pub from_session_id: String,
    pub to_session_id: String,
    pub kind: String,
    pub content: String,
    #[serde(default = "default_visibility")]
    pub visibility: String,
    pub correlation_id: Option<String>,
}

fn default_visibility() -> String {
    "supervisor".to_string()
}

pub async fn create_delegation_message(
    _tier: RequireFullAccess,
    CurrentPeerTier(current_tier): CurrentPeerTier,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CreateDelegationMessageBody>,
) -> Result<(StatusCode, Json<AgentMessageRecord>), (StatusCode, Json<serde_json::Value>)> {
    if current_tier != PeerTier::FullAccess {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "mailbox messaging requires full-access tier" })),
        ));
    }

    let contract = state
        .store
        .get_delegation_contract(&id)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "delegation not found" })),
            )
        })?;

    if body.visibility != "supervisor" && !contract.experimental_comm_enabled {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "non-supervisor visibility requires experimental communication mode" })),
        ));
    }

    let message_id = uuid::Uuid::new_v4().to_string();
    let message = state
        .store
        .create_agent_message(CreateAgentMessage {
            id: &message_id,
            contract_id: &id,
            from_session_id: &body.from_session_id,
            to_session_id: &body.to_session_id,
            kind: &body.kind,
            content: &body.content,
            visibility: &body.visibility,
            correlation_id: body.correlation_id.as_deref(),
        })
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })?;

    emit_supervisor_event(
        &state.supervisor_tx,
        "agent_message_created",
        Some(&body.to_session_id),
        Some(&id),
        serde_json::json!({
            "messageId": message.id,
            "fromSessionId": message.from_session_id,
            "toSessionId": message.to_session_id,
            "kind": message.kind,
            "visibility": message.visibility,
        }),
    );

    Ok((StatusCode::CREATED, Json(message)))
}

pub async fn list_skill_candidates(
    _guard: RequireLocalhostOnly,
    State(state): State<AppState>,
) -> Result<Json<Vec<SkillCandidateRecord>>, (StatusCode, Json<serde_json::Value>)> {
    state
        .store
        .list_skill_candidates()
        .map(Json)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromoteSkillCandidateBody {
    pub reviewer: Option<String>,
    pub promoted_skill_version: Option<String>,
}

pub async fn promote_skill_candidate(
    _guard: RequireLocalhostOnly,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PromoteSkillCandidateBody>,
) -> Result<Json<SkillCandidateRecord>, (StatusCode, Json<serde_json::Value>)> {
    let promoted = state
        .store
        .promote_skill_candidate(
            &id,
            body.reviewer.as_deref().unwrap_or("owner"),
            body.promoted_skill_version.as_deref().unwrap_or("skill-v1"),
        )
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "skill candidate not found" })),
            )
        })?;

    emit_supervisor_event(
        &state.supervisor_tx,
        "skill_candidate_promoted",
        Some(&promoted.source_session_id),
        None,
        serde_json::json!({
            "candidateId": promoted.id,
            "promotedSkillVersion": promoted.promoted_skill_version,
        }),
    );

    Ok(Json(promoted))
}

// ---------------------------------------------------------------------------
// PUT /api/approvals/{id}/deny  (localhost-only)
// ---------------------------------------------------------------------------

pub async fn deny_approval(
    _: RequireLocalhostOnly,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let approval = state
        .store
        .get_approval(&id)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": format!("approval {id} not found") })),
            )
        })?;

    if approval.status != "pending" {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": format!("approval is already '{}'", approval.status)
            })),
        ));
    }

    state
        .store
        .resolve_approval(&id, "denied", None)
        .map(|_| {
            info!(approval_id = %id, "approval denied");
            StatusCode::NO_CONTENT
        })
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })
}
