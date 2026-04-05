use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::host::logs::LogBuffer;
use crate::middleware::permissions::{
    ClientIp, OptionalNeedsApproval, RequireFullAccess, RequireLocalhostOnly, RequireReadOnly,
};
use crate::store::permissions::{PeerPermission, PendingApproval};
use crate::store::Store;
use crate::terminal::manager::TerminalManager;

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub manager: TerminalManager,
    pub log_buffer: LogBuffer,
    pub bind_address: String,
    pub allowed_cidrs: Vec<String>,
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
}

fn default_mode() -> String {
    "local".to_string()
}

fn default_workdir() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string())
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

    let rec = state
        .manager
        .create_session(&body.mode, body.name.as_deref(), &body.workdir)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        })?;

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
    // Ensure attached
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

    let result = state
        .manager
        .terminate_session(&id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        })?;

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
        .map(|h| (StatusCode::CREATED, Json(serde_json::to_value(h).unwrap_or_default())))
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
        .map(|_| StatusCode::NO_CONTENT)
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
        .map(|_| StatusCode::NO_CONTENT)
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
                });
            let rec = state
                .manager
                .create_session(&body.mode, body.name.as_deref(), &body.workdir)
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
    let config_json = serde_json::to_string(&body.config).unwrap_or_default();
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
    let config_json = serde_json::to_string(&body.config).unwrap_or_default();
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
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("db error: {e}") })),
            )
        })
}
