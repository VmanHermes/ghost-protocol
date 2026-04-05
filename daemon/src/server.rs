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

    // 5. Start background host health poller + discovery
    {
        let store = store.clone();
        tokio::spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(3))
                .build()
                .unwrap_or_default();
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;

                // Phase 1: Discover new peers via Tailscale
                let peers = crate::host::detect::list_tailscale_peers();
                for peer in &peers {
                    if !peer.online {
                        continue;
                    }
                    if store.is_known_or_dismissed(&peer.ip).unwrap_or(true) {
                        continue;
                    }
                    let health_url = format!("http://{}:8787/health", peer.ip);
                    match client.get(&health_url).send().await {
                        Ok(resp) if resp.status().is_success() => {
                            store.upsert_discovered_peer(&peer.ip, &peer.name).ok();
                            tracing::info!(peer = %peer.name, ip = %peer.ip, "discovered new Ghost Protocol peer");
                        }
                        _ => {}
                    }
                }

                // Phase 2: Poll known hosts (existing logic)
                if let Ok(hosts) = store.list_known_hosts() {
                    for host in hosts {
                        let url = format!("{}/api/system/hardware", host.url);
                        let status = match client.get(&url).send().await {
                            Ok(resp) if resp.status().is_success() => {
                                let caps = resp.json::<serde_json::Value>().await.ok().map(|v| {
                                    crate::store::hosts::HostCapabilities {
                                        gpu: v["gpu"]["model"].as_str().map(|s| s.to_string()),
                                        ram_gb: v["ramGb"].as_f64(),
                                        hermes: v["tools"]["hermes"].is_string(),
                                        ollama: v["tools"]["ollama"].is_string(),
                                    }
                                });
                                store.update_host_status(&host.id, "online", caps.as_ref()).ok();
                                "online"
                            }
                            _ => {
                                store.update_host_status(&host.id, "offline", None).ok();
                                "offline"
                            }
                        };
                        tracing::debug!(host = %host.name, status, "health poll");
                    }
                }
            }
        });
    }

    // 6. Spawn approval expiry background task
    {
        let store = store.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                if let Ok(count) = store.expire_stale_approvals() {
                    if count > 0 {
                        tracing::debug!(expired = count, "expired stale approvals");
                    }
                }
            }
        });
    }

    // 7. Build app state
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

    // 8. Build router
    let store_for_guard = state.store.clone();
    let app = Router::new()
        .route("/health", get(http::health))
        .route("/api/system/status", get(http::system_status))
        .route("/api/system/logs", get(http::system_logs))
        .route("/api/system/hardware", get(http::system_hardware))
        .route("/api/system/hardware/status", get(http::system_hardware_status))
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
        .route("/api/hosts", get(http::list_hosts).post(http::add_host))
        .route("/api/hosts/{id}", axum::routing::delete(http::remove_host))
        .route("/ws", get(ws::ws_upgrade))
        .route("/api/permissions", get(http::list_permissions))
        .route("/api/hosts/{id}/permissions", axum::routing::put(http::set_permission))
        .route("/api/approvals", get(http::list_approvals))
        .route("/api/approvals/{id}", get(http::get_approval))
        .route("/api/approvals/{id}/approve", axum::routing::put(http::approve_approval))
        .route("/api/approvals/{id}/deny", axum::routing::put(http::deny_approval))
        .route("/api/discoveries", get(http::list_discoveries))
        .route("/api/discoveries/{ip}/accept", axum::routing::put(http::accept_discovery))
        .route("/api/discoveries/{ip}/dismiss", axum::routing::put(http::dismiss_discovery))
        .with_state(state)
        .layer(middleware::from_fn(cors_layer))
        .layer(middleware::from_fn_with_state(store_for_guard, tailscale_guard))
        .layer(Extension(Arc::new(settings.clone())));

    // 9. Bind to all configured hosts
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
