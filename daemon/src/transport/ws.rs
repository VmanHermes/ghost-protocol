use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{FromRequestParts, State, WebSocketUpgrade};
use axum::http::request::Parts;
use axum::response::IntoResponse;
use chrono::Utc;
use serde::Deserialize;
use tokio::sync::broadcast;
use tracing::{debug, warn};

use crate::chat::broadcaster::{ChatBroadcaster, ChatEvent};
use crate::middleware::permissions::PeerTier;
use crate::store::chunks::TerminalChunkRecord;
use crate::terminal::broadcaster::SessionBroadcaster;
use super::http::AppState;

// ---------------------------------------------------------------------------
// PeerTier extractor
// ---------------------------------------------------------------------------

pub(crate) struct ExtractedTier(PeerTier);

impl<S> FromRequestParts<S> for ExtractedTier
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let tier = parts
            .extensions
            .get::<PeerTier>()
            .copied()
            .unwrap_or(PeerTier::NoAccess);
        Ok(ExtractedTier(tier))
    }
}

// ---------------------------------------------------------------------------
// Incoming message shape
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct WsMessage {
    op: String,
    #[serde(default, rename = "sessionId")]
    session_id: Option<String>,
    #[serde(default, rename = "afterChunkId")]
    after_chunk_id: Option<i64>,
    #[serde(default)]
    input: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default, rename = "appendNewline")]
    append_newline: Option<bool>,
    #[serde(default)]
    cols: Option<u16>,
    #[serde(default)]
    rows: Option<u16>,
    #[serde(default)]
    ts: Option<String>,
}

// ---------------------------------------------------------------------------
// Upgrade handler
// ---------------------------------------------------------------------------

pub async fn ws_upgrade(
    State(state): State<AppState>,
    ExtractedTier(tier): ExtractedTier,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state, tier))
}

// ---------------------------------------------------------------------------
// Main WebSocket loop
// ---------------------------------------------------------------------------

async fn handle_ws(mut socket: WebSocket, state: AppState, tier: PeerTier) {
    if tier == PeerTier::NoAccess {
        let _ = send_error(&mut socket, "no-access: WebSocket connection denied").await;
        return;
    }

    let mut broadcast_rx: Option<broadcast::Receiver<TerminalChunkRecord>> = None;
    let mut current_session_id: Option<String> = None;
    let mut current_broadcaster: Option<Arc<SessionBroadcaster>> = None;
    let mut chat_rx: Option<broadcast::Receiver<ChatEvent>> = None;
    let mut _chat_broadcaster: Option<Arc<ChatBroadcaster>> = None;
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(20));

    loop {
        tokio::select! {
            biased;

            // Priority 1: forward broadcast chunks
            chunk_result = async {
                match broadcast_rx.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                match chunk_result {
                    Ok(chunk) => {
                        let msg = serde_json::json!({
                            "op": "terminal_chunk",
                            "chunk": chunk,
                        });
                        if send_json(&mut socket, &msg).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(lagged = n, "broadcast receiver lagged, some chunks lost");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        broadcast_rx = None;
                    }
                }
            }

            // Priority 2: forward chat events
            result = async {
                match chat_rx.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                match result {
                    Ok(event) => {
                        let msg = match &event {
                            ChatEvent::Delta { session_id, message_id, delta } => {
                                serde_json::json!({"op": "chat_delta", "sessionId": session_id, "messageId": message_id, "delta": delta})
                            }
                            ChatEvent::Message { message } => {
                                serde_json::json!({"op": "chat_message", "message": message})
                            }
                            ChatEvent::Status { session_id, status } => {
                                serde_json::json!({"op": "chat_status", "sessionId": session_id, "status": status})
                            }
                            ChatEvent::Meta { session_id, tokens, context_pct } => {
                                serde_json::json!({"op": "session_meta", "sessionId": session_id, "tokens": tokens, "contextPct": context_pct})
                            }
                        };
                        if send_json(&mut socket, &msg).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(lagged = n, "chat broadcast receiver lagged, some events lost");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        chat_rx = None;
                    }
                }
            }

            // Priority 3: heartbeat
            _ = heartbeat.tick() => {
                let msg = serde_json::json!({
                    "op": "heartbeat",
                    "ts": Utc::now().to_rfc3339(),
                });
                if send_json(&mut socket, &msg).await.is_err() {
                    break;
                }
            }

            // Priority 4: client messages
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let parsed: Result<WsMessage, _> = serde_json::from_str(&text);
                        match parsed {
                            Ok(ws_msg) => {
                                if handle_op(
                                    &mut socket,
                                    &state,
                                    ws_msg,
                                    &mut broadcast_rx,
                                    &mut current_session_id,
                                    &mut current_broadcaster,
                                    &mut chat_rx,
                                    &mut _chat_broadcaster,
                                    tier,
                                ).await.is_err() {
                                    break;
                                }
                            }
                            Err(e) => {
                                if send_error(&mut socket, &format!("invalid message: {e}")).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(data))) => {
                        if socket.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(_)) => {} // ignore binary, pong, etc.
                    Some(Err(_)) => break,
                }
            }
        }
    }

    // Cleanup
    cleanup(&state, current_session_id.as_deref(), current_broadcaster.as_ref());
    if let Some(ref bc) = _chat_broadcaster {
        bc.unsubscribe();
    }
    debug!("websocket disconnected");
}

// ---------------------------------------------------------------------------
// Op dispatcher
// ---------------------------------------------------------------------------

async fn handle_op(
    socket: &mut WebSocket,
    state: &AppState,
    msg: WsMessage,
    broadcast_rx: &mut Option<broadcast::Receiver<TerminalChunkRecord>>,
    current_session_id: &mut Option<String>,
    current_broadcaster: &mut Option<Arc<SessionBroadcaster>>,
    chat_rx: &mut Option<broadcast::Receiver<ChatEvent>>,
    chat_broadcaster: &mut Option<Arc<ChatBroadcaster>>,
    tier: PeerTier,
) -> Result<(), ()> {
    match msg.op.as_str() {
        "ping" => {
            let ts = msg.ts.unwrap_or_else(|| Utc::now().to_rfc3339());
            let reply = serde_json::json!({ "op": "heartbeat", "ts": ts });
            send_json(socket, &reply).await
        }

        "subscribe_terminal" => {
            let Some(session_id) = msg.session_id else {
                return send_error(socket, "subscribe_terminal requires sessionId").await;
            };

            // Unsubscribe from previous
            if let Some(prev_id) = current_session_id.take() {
                if let Some(bc) = current_broadcaster.take() {
                    bc.unsubscribe();
                    state.manager.on_unsubscribe(&prev_id);
                }
                *broadcast_rx = None;
            }

            // Ensure attached
            if let Err(e) = state.manager.ensure_attached(&session_id).await {
                return send_error(socket, &format!("failed to attach: {e}")).await;
            }

            // Replay chunks from DB
            let after = msg.after_chunk_id.unwrap_or(0);
            match state.store.list_terminal_chunks(&session_id, Some(after), 10_000) {
                Ok(chunks) => {
                    for chunk in &chunks {
                        let msg = serde_json::json!({
                            "op": "terminal_chunk",
                            "chunk": chunk,
                        });
                        send_json(socket, &msg).await?;
                    }
                }
                Err(e) => {
                    return send_error(socket, &format!("failed to load chunks: {e}")).await;
                }
            }

            // Get session record
            let session = match state.store.get_terminal_session(&session_id) {
                Ok(Some(s)) => s,
                Ok(None) => {
                    return send_error(socket, &format!("session {session_id} not found")).await;
                }
                Err(e) => {
                    return send_error(socket, &format!("db error: {e}")).await;
                }
            };

            // Send subscribed confirmation
            let reply = serde_json::json!({
                "op": "subscribed_terminal",
                "session": session,
            });
            send_json(socket, &reply).await?;

            // Subscribe to broadcaster
            if let Some(bc) = state.manager.get_broadcaster(&session_id).await {
                *broadcast_rx = Some(bc.subscribe());
                *current_broadcaster = Some(bc);
            }
            *current_session_id = Some(session_id);

            Ok(())
        }

        "terminal_input" => {
            if tier < PeerTier::FullAccess {
                return send_error(socket, "write operations require full-access tier").await;
            }
            let Some(session_id) = msg.session_id else {
                return send_error(socket, "terminal_input requires sessionId").await;
            };
            let Some(input) = msg.input else {
                return send_error(socket, "terminal_input requires input").await;
            };
            let append_newline = msg.append_newline.unwrap_or(false);

            if let Err(e) = state
                .manager
                .send_input(&session_id, input.as_bytes(), append_newline)
                .await
            {
                return send_error(socket, &format!("input error: {e}")).await;
            }
            Ok(())
        }

        "resize_terminal" => {
            if tier < PeerTier::FullAccess {
                return send_error(socket, "write operations require full-access tier").await;
            }
            let Some(session_id) = msg.session_id else {
                return send_error(socket, "resize_terminal requires sessionId").await;
            };
            let (Some(cols), Some(rows)) = (msg.cols, msg.rows) else {
                return send_error(socket, "resize_terminal requires cols and rows").await;
            };

            match state.manager.resize_session(&session_id, cols, rows).await {
                Ok(session) => {
                    let reply = serde_json::json!({
                        "op": "terminal_status",
                        "session": session,
                    });
                    send_json(socket, &reply).await
                }
                Err(e) => send_error(socket, &format!("resize error: {e}")).await,
            }
        }

        "interrupt_terminal" => {
            if tier < PeerTier::FullAccess {
                return send_error(socket, "write operations require full-access tier").await;
            }
            let Some(session_id) = msg.session_id else {
                return send_error(socket, "interrupt_terminal requires sessionId").await;
            };
            if let Err(e) = state.manager.interrupt_session(&session_id).await {
                return send_error(socket, &format!("interrupt error: {e}")).await;
            }
            Ok(())
        }

        "terminate_terminal" => {
            if tier < PeerTier::FullAccess {
                return send_error(socket, "write operations require full-access tier").await;
            }
            let Some(session_id) = msg.session_id else {
                return send_error(socket, "terminate_terminal requires sessionId").await;
            };

            match state.manager.terminate_session(&session_id).await {
                Ok(session) => {
                    let reply = serde_json::json!({
                        "op": "terminal_status",
                        "session": session,
                    });
                    send_json(socket, &reply).await
                }
                Err(e) => send_error(socket, &format!("terminate error: {e}")).await,
            }
        }

        "subscribe_chat" => {
            let Some(session_id) = msg.session_id else {
                return send_error(socket, "subscribe_chat requires sessionId").await;
            };

            // Unsubscribe from previous chat broadcaster
            if let Some(bc) = chat_broadcaster.take() {
                bc.unsubscribe();
            }
            *chat_rx = None;

            // Replay existing messages from DB
            match state.store.list_chat_messages(&session_id, None, 200) {
                Ok(messages) => {
                    for m in &messages {
                        let reply = serde_json::json!({ "op": "chat_message", "message": m });
                        send_json(socket, &reply).await?;
                    }
                }
                Err(e) => return send_error(socket, &format!("db error: {e}")).await,
            }

            // Subscribe to the ChatBroadcaster for live events
            if let Some(bc) = state.chat_manager.get_broadcaster(&session_id).await {
                *chat_rx = Some(bc.subscribe());
                *chat_broadcaster = Some(bc);
            }

            let reply = serde_json::json!({ "op": "subscribed_chat", "sessionId": session_id });
            send_json(socket, &reply).await
        }

        "send_chat_message" => {
            if tier < crate::middleware::permissions::PeerTier::FullAccess {
                return send_error(socket, "write operations require full-access tier").await;
            }
            let Some(session_id) = msg.session_id else {
                return send_error(socket, "send_chat_message requires sessionId").await;
            };
            let Some(content) = msg.content else {
                return send_error(socket, "send_chat_message requires content").await;
            };
            let msg_id = uuid::Uuid::new_v4().to_string();
            state.store.create_chat_message(&msg_id, &session_id, "user", &content).ok();

            // Try chat process first, fall back to terminal manager
            if state.chat_manager.has_session(&session_id).await {
                if let Err(e) = state.chat_manager.send_input(&session_id, &content).await {
                    return send_error(socket, &format!("input error: {e}")).await;
                }
            } else {
                if let Err(e) = state.manager.send_input(&session_id, content.as_bytes(), true).await {
                    return send_error(socket, &format!("input error: {e}")).await;
                }
            }
            Ok(())
        }

        other => {
            send_error(socket, &format!("unknown op: {other}")).await
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn send_json(socket: &mut WebSocket, value: &serde_json::Value) -> Result<(), ()> {
    let text = serde_json::to_string(value).unwrap();
    socket
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| ())
}

async fn send_error(socket: &mut WebSocket, message: &str) -> Result<(), ()> {
    let msg = serde_json::json!({ "op": "error", "message": message });
    send_json(socket, &msg).await
}

fn cleanup(
    state: &AppState,
    session_id: Option<&str>,
    broadcaster: Option<&Arc<SessionBroadcaster>>,
) {
    if let (Some(sid), Some(bc)) = (session_id, broadcaster) {
        bc.unsubscribe();
        state.manager.on_unsubscribe(sid);
    }
}
