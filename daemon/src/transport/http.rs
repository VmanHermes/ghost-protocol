use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::host::logs::LogBuffer;
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

pub async fn system_status(State(state): State<AppState>) -> Json<serde_json::Value> {
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
    State(state): State<AppState>,
    Json(body): Json<CreateSessionBody>,
) -> Result<(StatusCode, Json<crate::store::sessions::TerminalSessionRecord>), (StatusCode, Json<serde_json::Value>)>
{
    state
        .manager
        .create_session(&body.mode, body.name.as_deref(), &body.workdir)
        .await
        .map(|rec| (StatusCode::CREATED, Json(rec)))
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        })
}

// ---------------------------------------------------------------------------
// GET /api/terminal/sessions/{id}
// ---------------------------------------------------------------------------

pub async fn get_session(
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
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<InputBody>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
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
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ResizeBody>,
) -> Result<Json<crate::store::sessions::TerminalSessionRecord>, (StatusCode, Json<serde_json::Value>)>
{
    state
        .manager
        .resize_session(&id, body.cols, body.rows)
        .await
        .map(Json)
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
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<crate::store::sessions::TerminalSessionRecord>, (StatusCode, Json<serde_json::Value>)>
{
    state
        .manager
        .terminate_session(&id)
        .await
        .map(Json)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        })
}

// ---------------------------------------------------------------------------
// GET /api/system/hardware
// ---------------------------------------------------------------------------

pub async fn system_hardware() -> Json<crate::hardware::MachineInfo> {
    Json(crate::hardware::collect_machine_info())
}

// ---------------------------------------------------------------------------
// GET /api/system/hardware/status
// ---------------------------------------------------------------------------

pub async fn system_hardware_status(
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
    State(state): State<AppState>,
    Json(body): Json<AddHostBody>,
) -> Result<(StatusCode, Json<crate::store::hosts::KnownHost>), (StatusCode, Json<serde_json::Value>)>
{
    let id = uuid::Uuid::new_v4().to_string();
    let url = format!("http://{}:8787", body.tailscale_ip);
    state
        .store
        .add_known_host(&id, &body.name, &body.tailscale_ip, &url)
        .map(|h| (StatusCode::CREATED, Json(h)))
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
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
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
