use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::config::Settings;
use crate::middleware::permissions::{ClientIp, IsLocalhost, PeerTier};
use crate::store::Store;

pub async fn tailscale_guard(
    State(store): State<Store>,
    mut request: Request,
    next: Next,
) -> Response {
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
        request.extensions_mut().insert(IsLocalhost(ip.is_loopback()));
        request.extensions_mut().insert(ClientIp(ip.to_string()));

        let tier = if ip.is_loopback() {
            PeerTier::FullAccess
        } else {
            store
                .resolve_tier_by_ip(&ip.to_string())
                .ok()
                .flatten()
                .map(|s| PeerTier::from_str(&s))
                .unwrap_or(PeerTier::NoAccess)
        };
        request.extensions_mut().insert(tier);

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
