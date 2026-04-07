use std::net::SocketAddr;
use std::sync::Arc;

use axum::middleware;
use axum::routing::{get, post};
use axum::{Extension, Router};
use tracing::info;

use tower_http::services::{ServeDir, ServeFile};

use crate::config::Settings;
use crate::host::logs::LogBuffer;
use crate::middleware::cors::cors_layer;
use crate::middleware::tailscale::tailscale_guard;
use crate::store::Store;
use crate::terminal::manager::TerminalManager;
use crate::transport::http::{self, AppState};
use crate::transport::ws;

pub async fn run(
    settings: Settings,
    log_buffer: LogBuffer,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Open store
    info!(db_path = %settings.db_path.display(), "opening store");
    let store = Store::open(&settings.db_path)?;

    // 2. Create terminal manager
    let manager = TerminalManager::new(store.clone());

    // 3. Create chat process manager
    let chat_manager = crate::chat::manager::ChatProcessManager::new(store.clone());

    // 4. Recover sessions
    manager.recover().await;

    // Recover code-server sessions
    {
        let recovered_code_servers = crate::code_server::lifecycle::scan_running_code_servers();
        let recovered_adopted_pids: std::collections::HashSet<i64> = recovered_code_servers
            .iter()
            .map(|info| info.pid as i64)
            .collect();

        if let Ok(cs_sessions) = store.list_code_server_sessions() {
            for session in cs_sessions {
                if session.status != "running" && session.status != "created" {
                    continue;
                }
                if session.adopted {
                    if let Some(pid) = session.pid {
                        let proc_path = format!("/proc/{pid}");
                        if std::path::Path::new(&proc_path).exists()
                            && recovered_adopted_pids.contains(&pid)
                        {
                            tracing::info!(session_id = %session.id, pid, "recovered adopted code-server session");
                            let store2 = store.clone();
                            let sid = session.id.clone();
                            tokio::spawn(async move {
                                loop {
                                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                                    let proc_path = format!("/proc/{pid}");
                                    if !std::path::Path::new(&proc_path).exists() {
                                        let now = chrono::Utc::now().to_rfc3339();
                                        store2
                                            .update_terminal_session(
                                                &sid,
                                                Some("exited"),
                                                None,
                                                Some(&now),
                                                None,
                                                None,
                                                None,
                                            )
                                            .ok();
                                        break;
                                    }
                                }
                            });
                        } else {
                            store
                                .update_terminal_session(
                                    &session.id,
                                    Some("exited"),
                                    None,
                                    Some(&chrono::Utc::now().to_rfc3339()),
                                    None,
                                    None,
                                    None,
                                )
                                .ok();
                        }
                    } else {
                        store
                            .update_terminal_session(
                                &session.id,
                                Some("exited"),
                                None,
                                Some(&chrono::Utc::now().to_rfc3339()),
                                None,
                                None,
                                None,
                            )
                            .ok();
                    }
                } else {
                    // Spawned sessions: process is gone after daemon restart
                    store
                        .update_terminal_session(
                            &session.id,
                            Some("exited"),
                            None,
                            Some(&chrono::Utc::now().to_rfc3339()),
                            None,
                            None,
                            None,
                        )
                        .ok();
                }
            }
        }
    }

    // Create supervisor broadcast channel before background task so it can be shared
    let (supervisor_tx, _supervisor_rx) =
        tokio::sync::broadcast::channel::<crate::supervisor::SupervisorEvent>(256);

    // 5. Start background host health poller + discovery
    {
        let store = store.clone();
        let supervisor_tx_bg = supervisor_tx.clone();
        tokio::spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(3))
                .build()
                .unwrap_or_default();
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;

                // Phase 1: Discover new peers via Tailscale
                let peers = crate::host::detect::list_tailscale_peers();
                for peer in &peers {
                    if !peer.online {
                        continue;
                    }
                    if store.is_known_or_dismissed(&peer.ip).unwrap_or(true) {
                        continue;
                    }
                    let already_discovered = store.get_discovery(&peer.ip).ok().flatten().is_some();
                    let health_url = format!("http://{}:8787/health", peer.ip);
                    match client.get(&health_url).send().await {
                        Ok(resp) if resp.status().is_success() => {
                            store.upsert_discovered_peer(&peer.ip, &peer.name).ok();
                            if !already_discovered {
                                tracing::info!(peer = %peer.name, ip = %peer.ip, "discovered new Ghost Protocol peer");
                            }
                        }
                        _ => {}
                    }
                }

                // Phase 2: Poll known hosts (existing logic)
                if let Ok(hosts) = store.list_known_hosts() {
                    for host in hosts {
                        let old_status = &host.status;
                        let url = format!("{}/api/system/hardware", host.url);
                        let new_status = match client.get(&url).send().await {
                            Ok(resp) if resp.status().is_success() => {
                                let caps = resp.json::<serde_json::Value>().await.ok().map(|v| {
                                    let agents_data: Option<
                                        Vec<crate::hardware::agents::AgentInfo>,
                                    > = v["tools"]["agents"].as_array().map(|arr| {
                                        arr.iter()
                                            .filter_map(|a| serde_json::from_value(a.clone()).ok())
                                            .collect()
                                    });
                                    crate::store::hosts::HostCapabilities {
                                        gpu: v["gpu"]["model"].as_str().map(|s| s.to_string()),
                                        ram_gb: v["ramGb"].as_f64(),
                                        hermes: v["tools"]["hermes"].is_string(),
                                        ollama: v["tools"]["ollama"].is_string(),
                                        agents: agents_data,
                                    }
                                });
                                store
                                    .update_host_status(&host.id, "online", caps.as_ref())
                                    .ok();
                                "online"
                            }
                            Ok(resp) if resp.status() == reqwest::StatusCode::FORBIDDEN => {
                                store
                                    .update_host_status(&host.id, "permission-required", None)
                                    .ok();
                                "permission-required"
                            }
                            _ => {
                                store.update_host_status(&host.id, "offline", None).ok();
                                "offline"
                            }
                        };
                        if new_status != old_status {
                            tracing::info!(host = %host.name, old_status, new_status, "host status changed");
                        }
                    }
                }

                // Phase 3: Detect code-server instances
                {
                    let detected = crate::code_server::lifecycle::scan_running_code_servers();
                    let tracked_pids: std::collections::HashSet<i64> = store
                        .list_code_server_sessions()
                        .unwrap_or_default()
                        .iter()
                        .filter(|s| s.status == "running" || s.status == "created")
                        .filter_map(|s| s.pid)
                        .collect();

                    let untracked: Vec<_> = detected
                        .into_iter()
                        .filter(|cs| !tracked_pids.contains(&(cs.pid as i64)))
                        .collect();

                    if !untracked.is_empty() {
                        tracing::info!(
                            count = untracked.len(),
                            "detected untracked code-server instances"
                        );
                        let _ = supervisor_tx_bg.send(crate::supervisor::SupervisorEvent {
                            event_type: "code_server_detected".to_string(),
                            session_id: None,
                            contract_id: None,
                            ts: chrono::Utc::now().to_rfc3339(),
                            payload: serde_json::json!({ "instances": untracked }),
                        });
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
        chat_manager,
        supervisor_tx,
        log_buffer,
        bind_address: settings.bind_hosts.join(","),
        bind_port: settings.bind_port,
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
        .route(
            "/api/system/hardware/status",
            get(http::system_hardware_status),
        )
        .route(
            "/api/terminal/sessions",
            get(http::list_sessions).post(http::create_session),
        )
        .route("/api/work-sessions", post(http::create_work_session))
        .route("/api/work-sessions/{id}", get(http::get_work_session))
        .route(
            "/api/work-sessions/{id}/views",
            get(http::get_work_session_views),
        )
        .route(
            "/api/work-sessions/{id}/companion-terminal",
            post(http::create_companion_terminal),
        )
        .route(
            "/api/work-sessions/{id}/reopen",
            post(http::reopen_work_session),
        )
        .route("/api/terminal/sessions/{id}", get(http::get_session))
        .route("/api/terminal/sessions/{id}/input", post(http::send_input))
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
        .route(
            "/api/hosts/{id}/permissions",
            axum::routing::put(http::set_permission),
        )
        .route("/api/approvals", get(http::list_approvals))
        .route("/api/approvals/{id}", get(http::get_approval))
        .route(
            "/api/approvals/{id}/approve",
            axum::routing::put(http::approve_approval),
        )
        .route(
            "/api/approvals/{id}/deny",
            axum::routing::put(http::deny_approval),
        )
        .route("/api/discoveries", get(http::list_discoveries))
        .route(
            "/api/discoveries/{ip}/accept",
            axum::routing::put(http::accept_discovery),
        )
        .route(
            "/api/discoveries/{ip}/dismiss",
            axum::routing::put(http::dismiss_discovery),
        )
        .route(
            "/api/outcomes",
            get(http::list_outcomes).post(http::create_outcome),
        )
        .route(
            "/api/projects",
            get(http::list_projects).post(http::create_project),
        )
        .route(
            "/api/projects/{id}",
            get(http::get_project)
                .put(http::update_project)
                .delete(http::remove_project),
        )
        .route("/api/agents", get(http::list_agents))
        .route("/api/chat/sessions", post(http::create_chat_session))
        .route(
            "/api/chat/sessions/{id}/messages",
            get(http::list_chat_messages),
        )
        .route(
            "/api/chat/sessions/{id}/message",
            post(http::send_chat_message),
        )
        .route(
            "/api/sessions/{id}/switch-mode",
            post(http::switch_session_mode),
        )
        .route("/api/delegations", post(http::create_delegation))
        .route("/api/delegations/{id}", get(http::get_delegation))
        .route(
            "/api/delegations/{id}/messages",
            get(http::list_delegation_messages).post(http::create_delegation_message),
        )
        .route("/api/skills/candidates", get(http::list_skill_candidates))
        .route(
            "/api/skills/candidates/{id}/promote",
            post(http::promote_skill_candidate),
        )
        .route(
            "/api/code-server/sessions",
            post(http::create_code_server_session),
        )
        .route(
            "/api/code-server/sessions/{id}/terminate",
            post(http::terminate_code_server_session),
        )
        .route(
            "/api/code-server/detected",
            get(http::list_detected_code_servers),
        )
        .route("/api/code-server/adopt", post(http::adopt_code_server))
        .route("/api/intelligence/recall", post(http::recall_memories))
        .with_state(state)
        .layer(middleware::from_fn(cors_layer))
        .layer(middleware::from_fn_with_state(
            store_for_guard,
            tailscale_guard,
        ))
        .layer(Extension(Arc::new(settings.clone())));

    let app = {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));
        let web_dir = exe_dir
            .as_ref()
            .map(|d| d.join("resources/web"))
            .filter(|p| p.is_dir())
            .or_else(|| {
                let cwd = std::env::current_dir().ok()?;
                let p = cwd.join("web");
                p.is_dir().then_some(p)
            });

        if let Some(web_path) = web_dir {
            info!(path = %web_path.display(), "serving PWA frontend");
            app.fallback_service(
                ServeDir::new(&web_path).fallback(ServeFile::new(web_path.join("index.html"))),
            )
        } else {
            app
        }
    };

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
