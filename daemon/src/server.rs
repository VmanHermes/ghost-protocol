use std::net::SocketAddr;
use std::sync::Arc;

use axum::middleware;
use axum::routing::{get, post};
use axum::{Extension, Router};
use tracing::info;

use crate::config::Settings;
use crate::host::logs::LogBuffer;
use crate::middleware::cors::cors_layer;
use crate::middleware::tailscale::tailscale_guard;
use crate::store::Store;
use crate::terminal::manager::TerminalManager;
use crate::transport::http::{self, AppState};
use crate::transport::ws;

pub async fn run(settings: Settings) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Open store
    let store = Store::open(&settings.db_path)?;

    // 2. Create terminal manager
    let manager = TerminalManager::new(store.clone());

    // 3. Create log buffer
    let log_buffer = LogBuffer::new();

    // 4. Recover sessions
    manager.recover().await;

    // 5. Build app state
    let state = AppState {
        store,
        manager,
        log_buffer,
        bind_address: settings.bind_hosts.join(","),
        allowed_cidrs: settings
            .allowed_cidrs
            .iter()
            .map(|c| c.to_string())
            .collect(),
    };

    // 6. Build router
    let app = Router::new()
        .route("/health", get(http::health))
        .route("/api/system/status", get(http::system_status))
        .route("/api/system/logs", get(http::system_logs))
        .route(
            "/api/terminal/sessions",
            get(http::list_sessions).post(http::create_session),
        )
        .route("/api/terminal/sessions/{id}", get(http::get_session))
        .route(
            "/api/terminal/sessions/{id}/input",
            post(http::send_input),
        )
        .route(
            "/api/terminal/sessions/{id}/resize",
            post(http::resize_session),
        )
        .route(
            "/api/terminal/sessions/{id}/terminate",
            post(http::terminate_session),
        )
        .route("/ws", get(ws::ws_upgrade))
        .with_state(state)
        .layer(middleware::from_fn(cors_layer))
        .layer(middleware::from_fn(tailscale_guard))
        .layer(Extension(Arc::new(settings.clone())));

    // 7. Bind to all configured hosts
    let mut handles = Vec::new();

    for host in &settings.bind_hosts {
        let addr: SocketAddr = format!("{host}:{}", settings.bind_port).parse()?;
        let listener = tokio::net::TcpListener::bind(addr).await?;
        info!(addr = %addr, "listening");

        let app = app.clone();
        let handle = tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
        });
        handles.push(handle);
    }

    // Wait on all listeners
    for handle in handles {
        handle.await??;
    }

    Ok(())
}
