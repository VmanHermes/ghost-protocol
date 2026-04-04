use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Request};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::config::Settings;

pub async fn tailscale_guard(request: Request, next: Next) -> Response {
    let settings = request
        .extensions()
        .get::<Arc<Settings>>()
        .cloned();

    let connect_info = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .cloned();

    let Some(settings) = settings else {
        return next.run(request).await;
    };

    let Some(ConnectInfo(addr)) = connect_info else {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({
                "error": "forbidden",
                "message": "unable to determine client address"
            })),
        )
            .into_response();
    };

    let ip = addr.ip();

    if settings.is_ip_allowed(ip) {
        return next.run(request).await;
    }

    (
        StatusCode::FORBIDDEN,
        axum::Json(serde_json::json!({
            "error": "forbidden",
            "message": format!("client {} is not in the configured private allowlist", ip)
        })),
    )
        .into_response()
}
