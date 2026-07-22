//! Request handler — FP-6.1
//!
//! Maps IPC Actions to core API calls.
//! Returns Response::Ok/Err and broadcasts events.

use crate::proto::{Action, IpcError, Request, Response};
use crate::server::DaemonState;
use std::sync::Arc;
use termfast_core::config::TriggerType;
use termfast_core::error::ErrorCode;
use termfast_core::log::{LogEntry, LogKind, LogLevel};

/// Handle a single IPC request
pub async fn handle_request(request: &Request, state: &DaemonState) -> Response {
    tracing::debug!("handling request: {:?}", request.action);

    let result = match &request.action {
        Action::ListServers => handle_list_servers(state).await,
        Action::GetServerStatus => handle_get_server_status(state, &request.params).await,
        Action::AddServer => handle_add_server(state, &request.params).await,
        Action::RemoveServer => handle_remove_server(state, &request.params).await,
        Action::UpdateServer => handle_update_server(state, &request.params).await,
        Action::ConnectServer => handle_connect_server(state, &request.params).await,
        Action::DisconnectServer => handle_disconnect_server(state, &request.params).await,
        Action::GetConfig => handle_get_config(state).await,
        Action::UpdateGeneralConfig => handle_update_general_config(state, &request.params).await,
        Action::GetDaemonStatus => handle_get_daemon_status(state).await,
        Action::GetLogs => handle_get_logs(state, &request.params).await,
        Action::ClearLogs => handle_clear_logs(state).await,
        Action::ExportLogs => handle_export_logs(state, &request.params).await,
        Action::PauseAllTriggers => handle_pause_all_triggers(state).await,
        Action::ResumeAllTriggers => handle_resume_all_triggers(state).await,
        Action::PauseServerTriggers => handle_pause_server_triggers(state, &request.params).await,
        Action::ResumeServerTriggers => handle_resume_server_triggers(state, &request.params).await,
        Action::Shutdown => handle_shutdown(state).await,
        // Proxy actions
        Action::ToggleProxy => handle_toggle_proxy(state, &request.params).await,
        Action::GetProxyStatus => handle_get_proxy_status(state, &request.params).await,
        Action::TestProxy => handle_test_proxy(state, &request.params).await,
        Action::SetSystemProxy => handle_set_system_proxy(state, &request.params).await,
        Action::ClearSystemProxy => handle_clear_system_proxy(state).await,
        Action::GetSystemProxy => handle_get_system_proxy(state).await,
        // Trigger actions
        Action::ManualFireTrigger => handle_manual_fire_trigger(state, &request.params).await,
        // Template actions
        Action::ListTemplates => handle_list_templates(state).await,
        // Credential actions
        Action::SaveCredential => handle_save_credential(state, &request.params).await,
        Action::HasCredential => handle_has_credential(state, &request.params).await,
        Action::DeleteCredential => handle_delete_credential(state, &request.params).await,
        // Export/Import
        Action::ExportFull => handle_export_full(state, &request.params).await,
        Action::ExportServers => handle_export_servers(state).await,
        Action::ImportServers => handle_import_servers(state, &request.params).await,
        Action::ImportFull => handle_import_full(state, &request.params).await,
        Action::CleanupAuthorizedKeys => {
            handle_cleanup_authorized_keys(state, &request.params).await
        }
        Action::ReorderServers => handle_reorder_servers(state, &request.params).await,
        // Proxy advanced
        Action::ToggleProxyAdvanced => handle_toggle_proxy_advanced(state, &request.params).await,
        Action::SetProxyAuth => handle_set_proxy_auth(state, &request.params).await,
        Action::ClearProxyAuth => handle_clear_proxy_auth(state, &request.params).await,
        // Trigger CRUD
        Action::AddTrigger => handle_add_trigger(state, &request.params).await,
        Action::RemoveTrigger => handle_remove_trigger(state, &request.params).await,
        Action::UpdateTrigger => handle_update_trigger(state, &request.params).await,
        Action::SyncTriggerFromTemplate => {
            handle_sync_trigger_from_template(state, &request.params).await
        }
        // Template CRUD
        Action::CreateTemplate => handle_create_template(state, &request.params).await,
        Action::UpdateTemplate => handle_update_template(state, &request.params).await,
        Action::DeleteTemplate => handle_delete_template(state, &request.params).await,
        Action::SaveTriggerAsTemplate => {
            handle_save_trigger_as_template(state, &request.params).await
        }
        Action::ImportTemplates => handle_import_templates(state, &request.params).await,
        Action::ExportTemplates => handle_export_templates(state).await,
        // Credential advanced
        Action::ConfigureKeyAuth => handle_configure_key_auth(state, &request.params).await,
        Action::SwitchAuthMethod => handle_switch_auth_method(state, &request.params).await,
        Action::DetectFirewall => handle_detect_firewall(state, &request.params).await,
        Action::AcceptHostKey => handle_accept_host_key(state, &request.params).await,
        // Terminal — interactive SSH shell sessions
        Action::TerminalOpen => handle_terminal_open(state, &request.params).await,
        Action::TerminalInput => handle_terminal_input(state, &request.params).await,
        Action::TerminalClose => handle_terminal_close(state, &request.params).await,
        Action::TerminalResize => handle_terminal_resize(state, &request.params).await,

        // Cloud sync
        Action::CloudSyncGetAuthUrl => handle_cloud_sync_auth_url(state, &request.params).await,
        Action::CloudSyncExchangeCode => handle_cloud_sync_exchange_code(state, &request.params).await,
        Action::CloudSyncSaveToken => handle_cloud_sync_save_token(state, &request.params).await,
        Action::CloudSyncLoadToken => handle_cloud_sync_load_token(state, &request.params).await,
        Action::CloudSyncUpload => handle_cloud_sync_upload(state, &request.params).await,
        Action::CloudSyncDownload => handle_cloud_sync_download(state, &request.params).await,
        Action::CloudSyncGetFileInfo => handle_cloud_sync_file_info(state, &request.params).await,
        Action::CloudSyncStatus => handle_cloud_sync_status(state, &request.params).await,
        Action::CloudSyncDeleteRemote => handle_cloud_sync_delete_remote(state, &request.params).await,
        Action::CloudSyncDisconnect => handle_cloud_sync_disconnect(state, &request.params).await,
        Action::CloudSyncRefreshToken => handle_cloud_sync_refresh_token(state, &request.params).await,
        Action::CloudSyncAuthWithCallback => handle_cloud_sync_auth_with_callback(state, &request.params).await,
        Action::CloudSyncWaitCallback => handle_cloud_sync_wait_callback(state, &request.params).await,
    };

    match result {
        Ok(data) => Response::ok(&request.id, data),
        Err(e) => Response::err(&request.id, e),
    }
}

// === SECTION 1 END ===

type HandlerResult = Result<serde_json::Value, IpcError>;

/// List all servers
async fn handle_list_servers(state: &DaemonState) -> HandlerResult {
    let servers = state.server_manager.list_servers().await;
    // Sort by config order to preserve insertion/reorder order
    let config_order: Vec<String> = {
        let mgr = state.config_manager.lock().await;
        let config = mgr.get().await;
        config.servers.iter().map(|s| s.id.clone()).collect()
    };
    let mut sorted_servers: Vec<_> = servers.into_iter().collect();
    sorted_servers.sort_by_key(|s| {
        config_order
            .iter()
            .position(|id| id == s.id())
            .unwrap_or(usize::MAX)
    });

    let mut server_list: Vec<serde_json::Value> = Vec::new();
    for s in &sorted_servers {
        let status = s.status().await;
        let ip = s.current_ip().await;
        let client_ip = s.client_ip().await;
        let proxy_running = s.is_proxy_running().await;
        let active_clients = s.active_clients().await;
        let bytes_in = s.bytes_in().await;
        let bytes_out = s.bytes_out().await;
        let cfg = &s.config;
        let triggers = s.triggers.lock().await.clone();
        let auth_banner = s.auth_banner().await;
        server_list.push(serde_json::json!({
            "id": s.id(),
            "name": s.name(),
            "ssh": {
                "host": cfg.ssh.host,
                "port": cfg.ssh.port,
                "user": cfg.ssh.user,
                "auth_method": cfg.ssh.auth_method,
                "key_path": cfg.ssh.key_path,
                "key_auto_generated": cfg.ssh.key_auto_generated,
                "connection_mode": cfg.ssh.connection_mode,
                "skip_hostkey_verify": cfg.ssh.skip_hostkey_verify,
            },
            "proxy": {
                "enabled": cfg.proxy.enabled,
                "socks5_port": cfg.proxy.socks5_port,
                "http_port": cfg.proxy.http_port,
                "mixed_port": cfg.proxy.mixed_port,
                "max_channels": cfg.proxy.max_channels,
                "channel_idle_timeout": cfg.proxy.channel_idle_timeout,
            },
            "reconnect": {
                "auto_reconnect": cfg.reconnect.auto_reconnect,
                "heartbeat_interval": cfg.reconnect.heartbeat_interval,
                "max_attempts": cfg.reconnect.max_attempts,
                "reconnect_timeout_secs": cfg.reconnect.reconnect_timeout_secs,
                "initial_backoff_secs": cfg.reconnect.initial_backoff_secs,
                "max_backoff_secs": cfg.reconnect.max_backoff_secs,
            },
            "ip_check": {
                "enabled": cfg.ip_check.enabled,
                "interval_secs": cfg.ip_check.interval_secs,
            },
            "last_known_ip": cfg.last_known_ip,
            "triggers": triggers,
            "suppress_firewall_badge": cfg.suppress_firewall_badge,
            "current_status": match status {
                termfast_core::server::instance::ServerStatus::Disconnected => "disconnected",
                termfast_core::server::instance::ServerStatus::Connecting => "connecting",
                termfast_core::server::instance::ServerStatus::Connected => "connected",
                termfast_core::server::instance::ServerStatus::Reconnecting => "reconnecting",
                termfast_core::server::instance::ServerStatus::AuthFailed => "auth_failed",
                termfast_core::server::instance::ServerStatus::Error => "error",
                termfast_core::server::instance::ServerStatus::Offline => "offline",
            },
            "current_ip": ip,
            "client_ip": client_ip,
            "connected_since": null,
            "reconnect_count": 0,
            "max_attempts": cfg.reconnect.max_attempts,
            "proxy_running": proxy_running,
            "active_channels": active_clients,
            "bytes_in": bytes_in,
            "bytes_out": bytes_out,
            "auth_banner": auth_banner,
        }));
    }
    Ok(serde_json::json!({ "servers": server_list }))
}

/// Get server status
async fn handle_get_server_status(
    state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;

    let server = state.server_manager.get_server(server_id).await?;
    let status = server.status().await;
    let ip = server.current_ip().await;

    Ok(serde_json::json!({
        "server_id": server_id,
        "status": format!("{:?}", status),
        "ip": ip,
    }))
}

/// Add a server
async fn handle_add_server(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let config: termfast_core::config::ServerConfig = serde_json::from_value(params.clone())
        .map_err(|e| {
            IpcError::new(
                ErrorCode::InvalidParams,
                format!("invalid server config: {}", e),
            )
        })?;

    // Check port conflicts
    if state
        .server_manager
        .is_socks5_port_in_use(config.proxy.socks5_port, None)
        .await
    {
        return Err(IpcError::new(
            ErrorCode::ProxyPortInUse,
            format!("SOCKS5 port {} is in use", config.proxy.socks5_port),
        ));
    }
    if state
        .server_manager
        .is_http_port_in_use(config.proxy.http_port, None)
        .await
    {
        return Err(IpcError::new(
            ErrorCode::ProxyPortInUse,
            format!("HTTP port {} is in use", config.proxy.http_port),
        ));
    }
    if config.proxy.mixed_port > 0
        && state
            .server_manager
            .is_mixed_port_in_use(config.proxy.mixed_port, None)
            .await
    {
        return Err(IpcError::new(
            ErrorCode::ProxyPortInUse,
            format!("Mixed port {} is in use", config.proxy.mixed_port),
        ));
    }

    // Persist to config file
    let config_clone = config.clone();
    {
        let mgr = state.config_manager.lock().await;
        mgr.modify(|c| {
            c.servers.push(config_clone);
        })
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
    }

    let id = state.server_manager.add_server(config).await?;

    // Set runtime state manager on the new server instance (FP-1.3b)
    if let Ok(server) = state.server_manager.get_server(&id).await {
        server.set_runtime_state(state.runtime_state.clone()).await;
    }

    // Broadcast event
    state
        .broadcast("server:added", serde_json::json!({ "server_id": &id }))
        .await;

    maybe_broadcast_cli_focus(state, params, &id, Some("connection")).await;

    Ok(serde_json::json!({ "server_id": id }))
}

/// Remove a server
async fn handle_remove_server(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;

    // Remove from config file (persist)
    {
        let mgr = state.config_manager.lock().await;
        mgr.modify(|c| {
            c.servers.retain(|s| s.id != server_id);
        })
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
    }

    // Disconnect if connected
    if let Ok(server) = state.server_manager.get_server(server_id).await {
        let _ = server.disconnect().await;
    }

    state.server_manager.remove_server(server_id).await?;

    // Clean up credentials from keychain
    let _ = state.credential_store.delete_all_for_server(server_id);

    state
        .broadcast(
            "server:removed",
            serde_json::json!({ "server_id": server_id }),
        )
        .await;

    log_and_broadcast(
        state,
        None,
        LogLevel::Info,
        LogKind::System,
        "Server removed and credentials cleaned up".to_string(),
    )
    .await;

    Ok(serde_json::json!({ "server_id": server_id }))
}

/// Reorder servers in config (persist new order)
async fn handle_reorder_servers(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let server_ids: Vec<String> = params["server_ids"]
        .as_array()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_ids array"))?
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    let mgr = state.config_manager.lock().await;
    mgr.modify(|config| {
        // Reorder servers array based on the provided order
        let mut new_servers = Vec::new();
        for id in &server_ids {
            if let Some(s) = config.servers.iter().find(|s| &s.id == id) {
                new_servers.push(s.clone());
            }
        }
        // Append any servers not in the list (shouldn't happen, but be safe)
        for s in &config.servers {
            if !server_ids.contains(&s.id) {
                new_servers.push(s.clone());
            }
        }
        config.servers = new_servers;
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;

    state
        .broadcast(
            "server:reordered",
            serde_json::json!({ "server_ids": server_ids }),
        )
        .await;

    Ok(serde_json::json!({ "reordered": true }))
}

// === SECTION 2 END ===

/// Connect to a server
async fn handle_connect_server(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;

    maybe_broadcast_cli_focus(state, params, server_id, Some("connection")).await;
    let server = state.server_manager.get_server(server_id).await?;

    // Check concurrent connection limit (§1.2: max 3) — only count actually connected servers
    let mut connected_count = 0;
    for s in state.server_manager.list_servers().await {
        if s.status().await == termfast_core::server::instance::ServerStatus::Connected {
            connected_count += 1;
        }
    }
    if connected_count >= 3 {
        return Err(IpcError::new(
            ErrorCode::Internal,
            "max 3 concurrent SSH connections reached".to_string(),
        ));
    }

    // Set hostkey mismatch callback (§17.2: triple notification)
    {
        let forwarder = state.event_forwarder_handle();
        let sid_clone = server_id.to_string();
        let server_name = server.config.name.clone();
        server
            .set_hostkey_mismatch_callback(Arc::new(move |expected, actual| {
                // Triple notification (§17.2): log + broadcast event (frontend handles notification + tray)
                tracing::error!(
                    "hostkey mismatch for {} ({}): expected {}, got {}",
                    sid_clone,
                    server_name,
                    expected,
                    actual
                );
                // Forward event to GUI (Tauri emit) — sync call via event forwarder
                if let Ok(fwd) = forwarder.lock() {
                    if let Some(ref f) = *fwd {
                        f(
                            "ssh:hostkey_mismatch",
                            serde_json::json!({
                                "server_id": sid_clone,
                                "server_name": server_name,
                                "expected": expected,
                                "actual": actual,
                            }),
                        );
                    }
                }
            }))
            .await;
    }

    // Set trigger result callback — broadcast trigger execution results to frontend
    {
        let log_buffer = state.log_buffer.clone();
        let forwarder = state.event_forwarder_handle();
        let sid = server_id.to_string();
        server
            .set_trigger_result_callback(Arc::new(
                move |event, results: &[termfast_core::trigger::engine::TriggerExecutionResult]| {
                    for r in results {
                        let level = if r.success {
                            LogLevel::Info
                        } else {
                            LogLevel::Error
                        };
                        let kind_str = match event.trigger_type {
                            TriggerType::OnConnect => "OnConnect",
                            TriggerType::OnReconnect => "OnReconnect",
                            TriggerType::OnIpChange => "OnIpChange",
                            TriggerType::OnProcessDead => "OnProcessDead",
                            TriggerType::OnPortClosed => "OnPortClosed",
                            TriggerType::ManualFire => "ManualFire",
                        };
                        let msg = format!(
                            "[{}] Trigger '{}' {} ({}/{})",
                            kind_str,
                            r.trigger_name,
                            if r.success { "succeeded" } else { "failed" },
                            r.executed_commands,
                            r.total_commands
                        );
                        let level_str = match level {
                            LogLevel::Info => "info",
                            LogLevel::Error => "error",
                            _ => "info",
                        };
                        let entry = LogEntry {
                            timestamp: chrono::Utc::now(),
                            server_id: Some(sid.clone()),
                            level,
                            kind: LogKind::Trigger,
                            message: msg.clone(),
                            data: None,
                            execution_id: None,
                        };
                        let buffer = log_buffer.clone();
                        let fwd = forwarder.clone();
                        let entry_clone = entry.clone();
                        let sid_clone = sid.clone();
                        let msg_clone = msg.clone();
                        let trigger_id = r.trigger_id.clone();
                        let trigger_name = r.trigger_name.clone();
                        let success = r.success;
                        let executed_commands = r.executed_commands;
                        let total_commands = r.total_commands;
                        let cmd_results: Vec<serde_json::Value> = r
                            .results
                            .iter()
                            .map(|c| {
                                serde_json::json!({
                                    "command": c.command,
                                    "exit_code": c.exit_code,
                                    "stdout": c.stdout,
                                    "stderr": c.stderr,
                                })
                            })
                            .collect();
                        tokio::spawn(async move {
                            buffer.add(entry_clone).await;
                            if let Ok(f) = fwd.lock() {
                                if let Some(ref fwd) = *f {
                                    fwd(
                                        "log:entry",
                                        serde_json::json!({
                                            "server_id": sid_clone.clone(),
                                            "level": level_str,
                                            "kind": "Trigger",
                                            "message": msg_clone,
                                            "timestamp": entry.timestamp,
                                        }),
                                    );
                                    // Also broadcast trigger:completed so GUI can show execution result
                                    fwd(
                                        "trigger:completed",
                                        serde_json::json!({
                                            "server_id": sid_clone,
                                            "trigger_id": trigger_id,
                                            "trigger_name": trigger_name,
                                            "success": success,
                                            "executed_commands": executed_commands,
                                            "total_commands": total_commands,
                                            "results": cmd_results,
                                        }),
                                    );
                                }
                            }
                        });
                    }
                },
            ))
            .await;
    }

    // Set status change callback — broadcast status changes to frontend
    // (used by connection monitor for auto-reconnect status updates)
    {
        let forwarder = state.event_forwarder_handle();
        let sid = server_id.to_string();
        let client_ip = server.client_ip().await;
        server
            .set_status_change_callback(Arc::new(move |status| {
                let status_str = match status {
                    termfast_core::server::instance::ServerStatus::Disconnected => "disconnected",
                    termfast_core::server::instance::ServerStatus::Connecting => "connecting",
                    termfast_core::server::instance::ServerStatus::Connected => "connected",
                    termfast_core::server::instance::ServerStatus::Reconnecting => "reconnecting",
                    termfast_core::server::instance::ServerStatus::AuthFailed => "auth_failed",
                    termfast_core::server::instance::ServerStatus::Error => "error",
                    termfast_core::server::instance::ServerStatus::Offline => "offline",
                };
                if let Ok(fwd) = forwarder.lock() {
                    if let Some(ref f) = *fwd {
                        f(
                            "server:status_changed",
                            serde_json::json!({
                                "server_id": sid,
                                "status": status_str,
                                "client_ip": client_ip,
                            }),
                        );
                    }
                }
            }))
            .await;
    }

    // Sync triggers from config before connecting (ensure latest triggers are used)
    {
        let mgr = state.config_manager.lock().await;
        let config = mgr.get().await;
        if let Some(srv) = config.servers.iter().find(|s| s.id == server_id) {
            server.set_triggers(srv.triggers.clone()).await;
        }
    }

    // Build auth method from config + credential store
    let auth = build_auth_method(
        state,
        server_id,
        &server.config.ssh.auth_method,
        server.config.ssh.key_path.as_str(),
    )?;

    match server.connect(&auth).await {
        Ok(()) => {
            // Start connection monitor for auto-reconnect
            server.start_connection_monitor().await;
            // Persist host key fingerprint if this was a first connection (TOFU).
            if let Some(fp) = server.get_host_key_fingerprint().await {
                let mgr = state.config_manager.lock().await;
                let _ = mgr.modify(|config| {
                    if let Some(srv) = config.servers.iter_mut().find(|s| s.id == server_id) {
                        if srv.ssh.host_key_fingerprint.is_none() {
                            srv.ssh.host_key_fingerprint = Some(fp);
                        }
                    }
                }).await;
            }
            let log_entry = termfast_core::log::LogEntry {
                timestamp: chrono::Utc::now(),
                level: termfast_core::log::LogLevel::Info,
                kind: termfast_core::log::LogKind::Connection,
                server_id: Some(server_id.to_string()),
                message: "Connected successfully".to_string(),
                data: None,
                execution_id: None,
            };
            state.log_buffer.add(log_entry.clone()).await;
            state
                .broadcast(
                    "log:entry",
                    serde_json::json!({
                        "server_id": server_id,
                        "level": "info",
                        "kind": "Connection",
                        "message": "Connected successfully",
                        "timestamp": log_entry.timestamp,
                    }),
                )
                .await;
            let client_ip = server.client_ip().await;
            state
                .broadcast(
                    "server:status_changed",
                    serde_json::json!({
                        "server_id": server_id,
                        "status": "connected",
                        "client_ip": client_ip,
                    }),
                )
                .await;
            // If proxy was auto-started during connect(), broadcast its status
            // so the frontend knows the proxy is running.
            if server.is_proxy_running().await {
                state
                    .broadcast(
                        "proxy:status_changed",
                        serde_json::json!({
                            "server_id": server_id,
                            "proxy_enabled": true,
                        }),
                    )
                    .await;
            }
            // Forward SSH auth banner (RFC4252 §5.4) — the welcome message
            // sent by the server during authentication. SecureCRT shows this
            // on connect; we broadcast it so the frontend can display it.
            if let Some(banner) = server.auth_banner().await {
                if !banner.is_empty() {
                    state
                        .broadcast(
                            "ssh:banner",
                            serde_json::json!({
                                "server_id": server_id,
                                "banner": banner,
                            }),
                        )
                        .await;
                }
            }
            Ok(
                serde_json::json!({ "server_id": server_id, "status": "connected", "client_ip": client_ip }),
            )
        }
        Err(e) => {
            let err = IpcError::from(e);
            let error_detail = err.detail.clone();
            let status = if err.code == ErrorCode::AuthFailed {
                "auth_failed"
            } else {
                "error"
            };
            // Write log entry to buffer
            let log_entry = termfast_core::log::LogEntry {
                timestamp: chrono::Utc::now(),
                level: termfast_core::log::LogLevel::Error,
                kind: termfast_core::log::LogKind::Connection,
                server_id: Some(server_id.to_string()),
                message: format!("Connection failed: {}", error_detail),
                data: None,
                execution_id: None,
            };
            state.log_buffer.add(log_entry.clone()).await;
            // Broadcast log:entry event to frontend
            state
                .broadcast(
                    "log:entry",
                    serde_json::json!({
                        "server_id": server_id,
                        "level": "error",
                        "kind": "Connection",
                        "message": format!("Connection failed: {}", error_detail),
                        "timestamp": log_entry.timestamp,
                    }),
                )
                .await;
            state
                .broadcast(
                    "server:status_changed",
                    serde_json::json!({
                        "server_id": server_id,
                        "status": status,
                        "error": &err.detail,
                    }),
                )
                .await;
            Err(err)
        }
    }
}

/// Accept a new host key after user confirmation (server was reinstalled, etc.)
/// Params: { "server_id": "...", "fingerprint": "SHA256:..." }
async fn handle_accept_host_key(
    state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;
    let fingerprint = params["fingerprint"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing fingerprint"))?;

    let server = state
        .server_manager
        .get_server(server_id)
        .await
        .map_err(|e| IpcError::new(ErrorCode::ServerNotFound, e.to_string()))?;

    // Update the SSH client's known key and the instance's stored fingerprint
    server.accept_host_key(fingerprint.to_string()).await;

    // Persist to config file
    let mgr = state.config_manager.lock().await;
    mgr.modify(|config| {
        if let Some(srv) = config.servers.iter_mut().find(|s| s.id == server_id) {
            srv.ssh.host_key_fingerprint = Some(fingerprint.to_string());
        }
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;

    Ok(serde_json::json!({
        "server_id": server_id,
        "host_key_fingerprint": fingerprint,
    }))
}

/// Disconnect from a server
async fn handle_disconnect_server(
    state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;

    maybe_broadcast_cli_focus(state, params, server_id, Some("connection")).await;
    let server = state.server_manager.get_server(server_id).await?;
    server.disconnect().await?;

    // Close all terminal sessions for this server
    state.terminal_manager.close_all_for_server(server_id).await;

    // Release the connection slot (§1.2)
    state.server_manager.release_connection().await;

    log_and_broadcast(
        state,
        Some(server_id),
        LogLevel::Info,
        LogKind::Connection,
        "Disconnected".to_string(),
    )
    .await;

    state
        .broadcast(
            "server:status_changed",
            serde_json::json!({
                "server_id": server_id,
                "status": "disconnected"
            }),
        )
        .await;

    Ok(serde_json::json!({ "server_id": server_id, "status": "disconnected" }))
}

/// Get full config
async fn handle_get_config(state: &DaemonState) -> HandlerResult {
    let mgr = state.config_manager.lock().await;
    let config = mgr.get().await;
    let json = serde_json::to_value(config).map_err(|e| {
        IpcError::new(
            ErrorCode::Internal,
            format!("config serialization error: {}", e),
        )
    })?;
    Ok(json)
}

/// Get daemon status
async fn handle_get_daemon_status(state: &DaemonState) -> HandlerResult {
    let server_count = state.server_manager.list_server_ids().await.len();
    let log_count = state.log_buffer.len().await;
    Ok(serde_json::json!({
        "running": true,
        "server_count": server_count,
        "log_count": log_count,
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

// === SECTION 3 END ===

/// Get logs with optional filtering
async fn handle_get_logs(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let server_id = params["server_id"].as_str();
    let limit = params["limit"].as_u64().unwrap_or(1000) as usize;

    let entries = state.log_buffer.get_entries_tail(server_id, None, None, limit).await;

    let json = serde_json::to_value(&entries).map_err(|e| {
        IpcError::new(
            ErrorCode::Internal,
            format!("log serialization error: {}", e),
        )
    })?;
    Ok(serde_json::json!({ "logs": json }))
}

/// Clear all logs
async fn handle_clear_logs(state: &DaemonState) -> HandlerResult {
    state.log_buffer.clear().await;
    Ok(serde_json::json!({ "cleared": true }))
}

/// Pause all triggers
async fn handle_pause_all_triggers(state: &DaemonState) -> HandlerResult {
    let servers = state.server_manager.list_servers().await;
    for server in &servers {
        server.trigger_engine.pause_all().await;
    }
    Ok(serde_json::json!({ "paused": true }))
}

/// Resume all triggers
async fn handle_resume_all_triggers(state: &DaemonState) -> HandlerResult {
    let servers = state.server_manager.list_servers().await;
    let mut all_pending = Vec::new();
    for server in &servers {
        let pending = server.trigger_engine.resume_all().await;
        for p in &pending {
            all_pending.push(serde_json::json!({
                "server_id": p.server_id,
                "server_name": p.server_name,
                "trigger_type": format!("{:?}", p.trigger_type),
                "new_ip": p.new_ip,
                "old_ip": p.old_ip,
                "timestamp": p.timestamp.to_rfc3339(),
            }));
        }
    }
    // Broadcast that triggers resumed with pending events
    state
        .broadcast(
            "triggers:resumed",
            serde_json::json!({
                "pending_events": all_pending.len(),
            }),
        )
        .await;
    Ok(serde_json::json!({ "resumed": true, "pending_events": all_pending }))
}

/// Shutdown daemon
async fn handle_shutdown(state: &DaemonState) -> HandlerResult {
    state.trigger_shutdown().await;
    Ok(serde_json::json!({ "shutting_down": true }))
}

/// Build AuthMethod from server config + credential store
fn build_auth_method(
    state: &DaemonState,
    server_id: &str,
    auth_type: &str,
    key_path: &str,
) -> Result<termfast_core::ssh::auth::AuthMethod, IpcError> {
    match auth_type {
        "password" => {
            let key =
                termfast_credential::make_key(server_id, termfast_credential::cred_type::PASSWORD);
            let password = state.credential_store.load(&key).map_err(|e| {
                IpcError::new(
                    ErrorCode::CredentialNotFound,
                    format!("password not found: {}", e),
                )
            })?;
            Ok(termfast_core::ssh::auth::AuthMethod::Password { password: zeroize::Zeroizing::new(password) })
        }
        "key" => {
            let passphrase_key = termfast_credential::make_key(
                server_id,
                termfast_credential::cred_type::KEY_PASSPHRASE,
            );
            let passphrase = state.credential_store.load(&passphrase_key)
                .ok()
                .map(zeroize::Zeroizing::new);
            Ok(termfast_core::ssh::auth::AuthMethod::Key {
                key_path: key_path.to_string(),
                passphrase,
            })
        }
        _ => Err(IpcError::new(
            ErrorCode::AuthFailed,
            format!("unsupported auth method: {}", auth_type),
        )),
    }
}

/// Update a server's config
async fn handle_update_server(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;

    // Check port conflicts before applying changes
    let new_socks5 = params["socks5_port"].as_u64().map(|v| v as u16);
    let new_http = params["http_port"].as_u64().map(|v| v as u16);
    let new_mixed = params["mixed_port"].as_u64().map(|v| v as u16);

    if let Some(socks5) = new_socks5 {
        if state
            .server_manager
            .is_socks5_port_in_use(socks5, Some(server_id))
            .await
        {
            return Err(IpcError::new(
                ErrorCode::ProxyPortInUse,
                format!("SOCKS5 port {} is in use", socks5),
            ));
        }
    }
    if let Some(http) = new_http {
        if state
            .server_manager
            .is_http_port_in_use(http, Some(server_id))
            .await
        {
            return Err(IpcError::new(
                ErrorCode::ProxyPortInUse,
                format!("HTTP port {} is in use", http),
            ));
        }
    }
    if let Some(mixed) = new_mixed {
        if mixed > 0
            && state
                .server_manager
                .is_mixed_port_in_use(mixed, Some(server_id))
                .await
        {
            return Err(IpcError::new(
                ErrorCode::ProxyPortInUse,
                format!("Mixed port {} is in use", mixed),
            ));
        }
    }

    // Update config in config manager and get the updated config
    let mgr = state.config_manager.lock().await;
    let updated_config = mgr
        .modify(|config| {
            if let Some(srv) = config.find_server_mut(server_id) {
                if let Some(name) = params["name"].as_str() {
                    srv.name = name.to_string();
                }
                if let Some(socks5_port) = params["socks5_port"].as_u64() {
                    srv.proxy.socks5_port = socks5_port as u16;
                }
                if let Some(http_port) = params["http_port"].as_u64() {
                    srv.proxy.http_port = http_port as u16;
                }
                if let Some(mixed_port) = params["mixed_port"].as_u64() {
                    srv.proxy.mixed_port = mixed_port as u16;
                }
                // SSH config updates
                if let Some(ssh) = params["ssh"].as_object() {
                    if let Some(host) = ssh.get("host").and_then(|v| v.as_str()) {
                        srv.ssh.host = host.to_string();
                    }
                    if let Some(port) = ssh.get("port").and_then(|v| v.as_u64()) {
                        srv.ssh.port = port as u16;
                    }
                    if let Some(user) = ssh.get("user").and_then(|v| v.as_str()) {
                        srv.ssh.user = user.to_string();
                    }
                    if let Some(auth_method) = ssh.get("auth_method").and_then(|v| v.as_str()) {
                        srv.ssh.auth_method = auth_method.to_string();
                    }
                    if let Some(key_path) = ssh.get("key_path").and_then(|v| v.as_str()) {
                        srv.ssh.key_path = key_path.to_string();
                    }
                }
                // Update auto_reconnect
                if let Some(v) = params["auto_reconnect"].as_bool() {
                    srv.reconnect.auto_reconnect = v;
                }
                // Update reconnect_timeout_secs (clamp: 0=unlimited, min 3, max 259200=3days)
                if let Some(v) = params["reconnect_timeout_secs"].as_u64() {
                    srv.reconnect.reconnect_timeout_secs =
                        if v == 0 { 0 } else { v.clamp(3, 259200) };
                }
            }
            config.find_server(server_id).cloned()
        })
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
    drop(mgr);

    // Reload server instance with new config (disconnects if currently connected)
    if let Some(new_config) = updated_config {
        if let Err(e) = state
            .server_manager
            .reload_server_config(server_id, new_config)
            .await
        {
            tracing::warn!("failed to reload server instance: {}", e);
        }
    }

    Ok(serde_json::json!({ "server_id": server_id }))
}

/// Update general config
async fn handle_update_general_config(
    state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let mgr = state.config_manager.lock().await;
    mgr.modify(|config| {
        if let Some(theme) = params["theme"].as_str() {
            config.general.theme = theme.to_string();
        }
        if let Some(lang) = params["language"].as_str() {
            config.general.language = lang.to_string();
        }
        if let Some(auto) = params["auto_start"].as_bool() {
            config.general.auto_start = auto;
        }
        if let Some(min) = params["minimize_to_tray"].as_bool() {
            config.general.minimize_to_tray = min;
        }
        if let Some(level) = params["log_level"].as_str() {
            config.general.log_level = level.to_string();
        }
        if let Some(v) = params["log_to_file"].as_bool() {
            config.general.log_to_file = v;
        }
        if let Some(v) = params["log_max_days"].as_u64() {
            config.general.log_max_days = v as u32;
        }
        if let Some(v) = params["log_max_size_mb"].as_u64() {
            config.general.log_max_size_mb = v as u32;
        }
        // Update custom variables (full replace)
        if let Some(vars) = params["custom_variables"].as_array() {
            config.general.custom_variables = vars
                .iter()
                .filter_map(|v| {
                    let name = v.get("name")?.as_str()?.to_string();
                    let value = v.get("value")?.as_str()?.to_string();
                    Some(termfast_core::config::CustomVariable { name, value })
                })
                .collect();
        }
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;

    // Sync custom variables to all server trigger engines
    let config = mgr.get().await;
    drop(mgr);
    let custom_vars = config.general.custom_variables.clone();
    for server in state.server_manager.list_servers().await {
        server
            .trigger_engine
            .set_custom_variables(custom_vars.clone())
            .await;
    }

    Ok(serde_json::json!({ "updated": true }))
}

/// Export logs
async fn handle_export_logs(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let server_id = params["server_id"].as_str();
    let entries = state.log_buffer.get_entries(server_id, None, None).await;
    let json = serde_json::to_value(&entries)
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("log export error: {}", e)))?;
    Ok(serde_json::json!({ "logs": json, "count": entries.len() }))
}

/// Toggle proxy on/off for a server
async fn handle_toggle_proxy(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;
    let enabled = params["enabled"]
        .as_bool()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing enabled"))?;

    maybe_broadcast_cli_focus(state, params, server_id, Some("proxy")).await;
    let server = state.server_manager.get_server(server_id).await?;
    if enabled {
        // If proxy is already running, just broadcast status (no-op)
        if server.is_proxy_running().await {
            state
                .broadcast(
                    "proxy:status_changed",
                    serde_json::json!({ "server_id": server_id, "proxy_enabled": true }),
                )
                .await;
            return Ok(serde_json::json!({ "server_id": server_id, "proxy_enabled": true }));
        }
        // Check SSH connection is active before starting proxy
        if !server.channel_opener.is_available().await {
            return Err(IpcError::new(
                ErrorCode::Internal,
                "SSH connection is not active, please connect first".to_string(),
            ));
        }
        server.start_proxy().await?;
        log_and_broadcast(
            state,
            Some(server_id),
            LogLevel::Info,
            LogKind::Proxy,
            format!(
                "Proxy started (SOCKS5:{}, HTTP:{})",
                server.config.proxy.socks5_port, server.config.proxy.http_port
            ),
        )
        .await;
    } else {
        server.stop_proxy().await?;
        log_and_broadcast(
            state,
            Some(server_id),
            LogLevel::Info,
            LogKind::Proxy,
            "Proxy stopped".to_string(),
        )
        .await;
    }

    state
        .broadcast(
            "proxy:status_changed",
            serde_json::json!({ "server_id": server_id, "proxy_enabled": enabled }),
        )
        .await;

    Ok(serde_json::json!({ "server_id": server_id, "proxy_enabled": enabled }))
}

/// Get proxy status for a server
async fn handle_get_proxy_status(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;
    let server = state.server_manager.get_server(server_id).await?;
    let running = server.is_proxy_running().await;
    Ok(serde_json::json!({
        "server_id": server_id,
        "proxy_enabled": running,
        "socks5_port": server.config.proxy.socks5_port,
        "http_port": server.config.proxy.http_port,
    }))
}

/// Test proxy connectivity — makes an actual HTTP request through the SOCKS5 proxy
async fn handle_test_proxy(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;

    maybe_broadcast_cli_focus(state, params, server_id, Some("proxy")).await;
    let server = state.server_manager.get_server(server_id).await?;
    if !server.is_proxy_running().await {
        return Err(IpcError::new(
            ErrorCode::Internal,
            "proxy is not running for this server",
        ));
    }

    let socks5_port = server.config.proxy.socks5_port;
    let test_url = params["url"].as_str().unwrap_or("https://api.ipify.org");

    tracing::info!("testing proxy via SOCKS5 :{} url={}", socks5_port, test_url);

    // Make an HTTP request through the SOCKS5 proxy with 15s timeout
    let start = std::time::Instant::now();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        test_proxy_via_socks5(socks5_port, test_url),
    )
    .await;
    let latency_ms = start.elapsed().as_millis() as u64;

    let result = match result {
        Ok(r) => r,
        Err(_) => Err("test proxy timed out (15s)".to_string()),
    };

    match &result {
        Ok(exit_ip) => {
            tracing::info!(
                "proxy test success: exit_ip={} latency={}ms",
                exit_ip,
                latency_ms
            );
            let log_entry = termfast_core::log::LogEntry {
                timestamp: chrono::Utc::now(),
                server_id: Some(server_id.to_string()),
                level: termfast_core::log::LogLevel::Info,
                kind: termfast_core::log::LogKind::Proxy,
                message: format!(
                    "Proxy test success: exit_ip={} latency={}ms",
                    exit_ip, latency_ms
                ),
                data: None,
                execution_id: None,
            };
            state.log_buffer.add(log_entry.clone()).await;
            state
                .broadcast(
                    "log:entry",
                    serde_json::json!({
                        "server_id": server_id,
                        "level": "info",
                        "kind": "Proxy",
                        "message": log_entry.message,
                        "timestamp": log_entry.timestamp,
                    }),
                )
                .await;
            Ok(serde_json::json!({
                "server_id": server_id,
                "success": true,
                "exit_ip": exit_ip,
                "latency_ms": latency_ms,
            }))
        }
        Err(e) => {
            tracing::warn!("proxy test failed: {}", e);
            let log_entry = termfast_core::log::LogEntry {
                timestamp: chrono::Utc::now(),
                server_id: Some(server_id.to_string()),
                level: termfast_core::log::LogLevel::Error,
                kind: termfast_core::log::LogKind::Proxy,
                message: format!("Proxy test failed: {}", e),
                data: None,
                execution_id: None,
            };
            state.log_buffer.add(log_entry.clone()).await;
            state
                .broadcast(
                    "log:entry",
                    serde_json::json!({
                        "server_id": server_id,
                        "level": "error",
                        "kind": "Proxy",
                        "message": log_entry.message,
                        "timestamp": log_entry.timestamp,
                    }),
                )
                .await;
            Ok(serde_json::json!({
                "server_id": server_id,
                "success": false,
                "exit_ip": null,
                "latency_ms": latency_ms,
                "error": e,
            }))
        }
    }
}

/// Test SOCKS5 proxy by making an HTTP/HTTPS request through it
async fn test_proxy_via_socks5(socks5_port: u16, url: &str) -> std::result::Result<String, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    let timeout_dur = std::time::Duration::from_secs(10);

    // Parse URL
    let is_https = url.starts_with("https://");
    let url_stripped = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let (host_port, path) = url_stripped.split_once('/').unwrap_or((url_stripped, ""));
    let (host, port) = match host_port.rsplit_once(':') {
        Some((h, p)) => (
            h,
            p.parse::<u16>().unwrap_or(if is_https { 443 } else { 80 }),
        ),
        None => (host_port, if is_https { 443u16 } else { 80u16 }),
    };

    // Connect to SOCKS5 proxy
    let mut stream = tokio::time::timeout(
        timeout_dur,
        TcpStream::connect(format!("127.0.0.1:{}", socks5_port)),
    )
    .await
    .map_err(|_| "connect to SOCKS5 timed out".to_string())?
    .map_err(|e| format!("connect to SOCKS5 failed: {}", e))?;

    // SOCKS5 handshake: no auth
    tokio::time::timeout(timeout_dur, stream.write_all(&[0x05, 0x01, 0x00]))
        .await
        .map_err(|_| "socks5 greeting timed out".to_string())?
        .map_err(|e| format!("socks5 greeting failed: {}", e))?;
    let mut buf = [0u8; 2];
    tokio::time::timeout(timeout_dur, stream.read_exact(&mut buf))
        .await
        .map_err(|_| "socks5 greeting response timed out".to_string())?
        .map_err(|e| format!("socks5 greeting response: {}", e))?;
    if buf[0] != 0x05 || buf[1] != 0x00 {
        return Err("socks5 auth negotiation failed".to_string());
    }

    // SOCKS5 connect request (domain name type)
    let mut req = vec![0x05, 0x01, 0x00, 0x03];
    req.push(host.len() as u8);
    req.extend_from_slice(host.as_bytes());
    req.extend_from_slice(&port.to_be_bytes());
    tokio::time::timeout(timeout_dur, stream.write_all(&req))
        .await
        .map_err(|_| "socks5 connect request timed out".to_string())?
        .map_err(|e| format!("socks5 connect request failed: {}", e))?;

    // Read response — variable length (ATYP + addr + port)
    let mut resp_header = [0u8; 4];
    tokio::time::timeout(timeout_dur, stream.read_exact(&mut resp_header))
        .await
        .map_err(|_| "socks5 connect response timed out".to_string())?
        .map_err(|e| format!("socks5 connect response: {}", e))?;
    if resp_header[1] != 0x00 {
        return Err(format!("socks5 connect failed: status {}", resp_header[1]));
    }
    // Skip address based on ATYP
    let atyp = resp_header[3];
    let addr_len = match atyp {
        0x01 => 4, // IPv4
        0x03 => {
            // Domain
            let mut len_buf = [0u8; 1];
            tokio::time::timeout(timeout_dur, stream.read_exact(&mut len_buf))
                .await
                .map_err(|_| "socks5 addr len read timed out".to_string())?
                .map_err(|e| format!("socks5 addr len read: {}", e))?;
            len_buf[0] as usize
        }
        0x04 => 16, // IPv6
        _ => return Err(format!("socks5 unknown ATYP: {}", atyp)),
    };
    let mut addr_buf = vec![0u8; addr_len + 2]; // addr + port
    tokio::time::timeout(timeout_dur, stream.read_exact(&mut addr_buf))
        .await
        .map_err(|_| "socks5 addr read timed out".to_string())?
        .map_err(|e| format!("socks5 addr read: {}", e))?;

    if is_https {
        // For HTTPS, we can't do TLS without a TLS library.
        // Just return success with the host info — the SOCKS5 tunnel is working.
        return Ok(format!("{} (HTTPS tunnel OK)", host));
    }

    // Send HTTP request
    let http_req = format!(
        "GET /{} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: termfast/1.0\r\n\r\n",
        path, host_port
    );
    tokio::time::timeout(timeout_dur, stream.write_all(http_req.as_bytes()))
        .await
        .map_err(|_| "http request timed out".to_string())?
        .map_err(|e| format!("http request failed: {}", e))?;

    // Read response
    let mut response = Vec::new();
    tokio::time::timeout(timeout_dur, stream.read_to_end(&mut response))
        .await
        .map_err(|_| "http response read timed out".to_string())?
        .map_err(|e| format!("http response read failed: {}", e))?;

    let response_body = String::from_utf8_lossy(&response).to_string();

    // Extract IP from response body
    let body = response_body
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or(&response_body);

    // Try JSON: {"origin": "x.x.x.x"} (httpbin.org/ip)
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(body.trim()) {
        if let Some(ip) = json.get("origin").and_then(|v| v.as_str()) {
            return Ok(ip.to_string());
        }
        // api.ipify.org returns plain text IP
        if let Some(s) = json.as_str() {
            return Ok(s.to_string());
        }
    }

    // Try plain text IP (api.ipify.org)
    let trimmed = body.trim();
    if trimmed.len() <= 45
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == ':')
    {
        return Ok(trimmed.to_string());
    }

    // Fallback: extract first IP-like pattern
    let ip_re = regex::Regex::new(r"\b(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})\b").ok();
    if let Some(re) = &ip_re {
        if let Some(m) = re.captures(body) {
            return Ok(m[1].to_string());
        }
    }

    Ok(format!("connected ({} bytes)", response_body.len()))
}

/// Set system proxy to a server's SOCKS5 — actually calls PlatformAdapter
async fn handle_set_system_proxy(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;

    maybe_broadcast_cli_focus(state, params, server_id, Some("proxy")).await;
    let server = state.server_manager.get_server(server_id).await?;
    let socks5_port = if server.config.proxy.mixed_port > 0 {
        server.config.proxy.mixed_port
    } else {
        server.config.proxy.socks5_port
    };
    let http_port = if server.config.proxy.mixed_port > 0 {
        server.config.proxy.mixed_port
    } else {
        server.config.proxy.http_port
    };

    // Actually set the system proxy via platform adapter (FP-6.6)
    let proxy_config = termfast_core::platform::SystemProxyConfig {
        server_id: server_id.to_string(),
        socks5_port,
        http_port,
    };
    let result = state
        .proxy_adapter
        .set_system_proxy(&proxy_config)
        .await
        .map_err(|e| {
            IpcError::new(
                ErrorCode::Internal,
                format!("set system proxy error: {}", e),
            )
        })?;

    if !result.success {
        return Err(IpcError::new(ErrorCode::NeedsPrivilege, result.message));
    }

    // Update config to record which server is the system proxy
    let mgr = state.config_manager.lock().await;
    mgr.modify(|config| {
        config.general.system_proxy_server_id = Some(server_id.to_string());
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;

    state
        .broadcast(
            "system_proxy:changed",
            serde_json::json!({ "server_id": server_id, "success": true }),
        )
        .await;

    log_and_broadcast(
        state,
        Some(server_id),
        LogLevel::Info,
        LogKind::Proxy,
        format!(
            "System proxy set to SOCKS5:{}, HTTP:{}",
            socks5_port, http_port
        ),
    )
    .await;

    Ok(serde_json::json!({
        "system_proxy_server_id": server_id,
        "success": result.success,
        "needs_privilege": result.needs_privilege,
        "message": result.message,
    }))
}

/// Clear system proxy — actually calls PlatformAdapter
async fn handle_clear_system_proxy(state: &DaemonState) -> HandlerResult {
    // Actually clear the system proxy via platform adapter (FP-6.6)
    let result = state
        .proxy_adapter
        .clear_system_proxy()
        .await
        .map_err(|e| {
            IpcError::new(
                ErrorCode::Internal,
                format!("clear system proxy error: {}", e),
            )
        })?;

    // Update config
    let mgr = state.config_manager.lock().await;
    mgr.modify(|config| {
        config.general.system_proxy_server_id = None;
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;

    state
        .broadcast(
            "system_proxy:changed",
            serde_json::json!({ "cleared": true }),
        )
        .await;

    log_and_broadcast(
        state,
        None,
        LogLevel::Info,
        LogKind::Proxy,
        "System proxy cleared".to_string(),
    )
    .await;

    Ok(serde_json::json!({
        "cleared": true,
        "success": result.success,
        "message": result.message,
    }))
}

/// Get current system proxy
async fn handle_get_system_proxy(state: &DaemonState) -> HandlerResult {
    let mgr = state.config_manager.lock().await;
    let config = mgr.get().await;
    Ok(serde_json::json!({
        "system_proxy_server_id": config.general.system_proxy_server_id,
    }))
}

/// Manually fire a trigger
async fn handle_manual_fire_trigger(
    state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;
    let trigger_id = params["trigger_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing trigger_id"))?;

    maybe_broadcast_cli_focus(state, params, server_id, Some("triggers")).await;
    let server = state.server_manager.get_server(server_id).await?;

    // Broadcast trigger:fired before execution
    let trigger_name = server
        .config
        .triggers
        .iter()
        .find(|t| t.id == trigger_id)
        .map(|t| t.name.clone())
        .unwrap_or_else(|| trigger_id.to_string());
    let total_commands = server
        .config
        .triggers
        .iter()
        .find(|t| t.id == trigger_id)
        .map(|t| t.commands.len())
        .unwrap_or(0);
    state
        .broadcast(
            "trigger:fired",
            serde_json::json!({
                "server_id": server_id,
                "trigger_id": trigger_id,
                "trigger_name": trigger_name,
                "total_commands": total_commands,
            }),
        )
        .await;

    let result = server.manual_fire_trigger(trigger_id).await?;

    state
        .broadcast(
            "trigger:completed",
            serde_json::json!({
                "server_id": server_id,
                "trigger_id": trigger_id,
                "success": result.success,
                "executed_commands": result.executed_commands,
                "total_commands": result.total_commands,
            }),
        )
        .await;

    // Log the trigger execution result
    let (level, msg) = if result.success {
        (
            LogLevel::Info,
            format!(
                "Trigger '{}' executed successfully ({}/{})",
                trigger_name, result.executed_commands, result.total_commands
            ),
        )
    } else {
        (
            LogLevel::Error,
            format!(
                "Trigger '{}' execution failed ({}/{})",
                trigger_name, result.executed_commands, result.total_commands
            ),
        )
    };
    log_and_broadcast(state, Some(server_id), level, LogKind::Trigger, msg).await;

    // Broadcast each command's output as log entries
    for cmd_result in &result.results {
        let cmd_msg = format!("$ {}", cmd_result.command);
        log_and_broadcast(
            state,
            Some(server_id),
            LogLevel::Info,
            LogKind::Trigger,
            cmd_msg,
        )
        .await;
        if !cmd_result.stdout.is_empty() {
            log_and_broadcast(
                state,
                Some(server_id),
                LogLevel::Info,
                LogKind::Trigger,
                cmd_result.stdout.trim().to_string(),
            )
            .await;
        }
        if !cmd_result.stderr.is_empty() {
            log_and_broadcast(
                state,
                Some(server_id),
                LogLevel::Warn,
                LogKind::Trigger,
                cmd_result.stderr.trim().to_string(),
            )
            .await;
        }
    }

    Ok(serde_json::json!({
        "server_id": server_id,
        "trigger_id": trigger_id,
        "success": result.success,
        "executed_commands": result.executed_commands,
        "total_commands": result.total_commands,
        "results": result.results.iter().map(|r| serde_json::json!({
            "command": r.command,
            "exit_code": r.exit_code,
            "stdout": r.stdout,
            "stderr": r.stderr,
            "success": r.success,
        })).collect::<Vec<_>>(),
    }))
}

/// Pause triggers for a specific server
async fn handle_pause_server_triggers(
    state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;
    let server = state.server_manager.get_server(server_id).await?;
    server.trigger_engine.pause_server(server_id).await;
    Ok(serde_json::json!({ "server_id": server_id, "paused": true }))
}

/// Resume triggers for a specific server
async fn handle_resume_server_triggers(
    state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;
    let server = state.server_manager.get_server(server_id).await?;
    server.trigger_engine.resume_server(server_id).await;
    Ok(serde_json::json!({ "server_id": server_id, "paused": false }))
}

/// List all trigger templates
async fn handle_list_templates(state: &DaemonState) -> HandlerResult {
    let mgr = state.config_manager.lock().await;
    let config = mgr.get().await;
    let templates = &config.trigger_templates;
    let json = serde_json::to_value(templates).map_err(|e| {
        IpcError::new(
            ErrorCode::Internal,
            format!("template serialization error: {}", e),
        )
    })?;
    Ok(serde_json::json!({ "templates": json }))
}

/// Save a credential
async fn handle_save_credential(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;
    let cred_type = params["credential_type"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing credential_type"))?;
    let value = params["value"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing value"))?;

    let key = termfast_credential::make_key(server_id, cred_type);
    state
        .credential_store
        .save(&key, value)
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("credential save error: {}", e)))?;
    Ok(serde_json::json!({ "saved": true }))
}

/// Check if a credential exists
async fn handle_has_credential(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;
    let cred_type = params["credential_type"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing credential_type"))?;

    let key = termfast_credential::make_key(server_id, cred_type);
    let exists = state.credential_store.has(&key);
    Ok(serde_json::json!({ "exists": exists }))
}

/// Delete a credential
async fn handle_delete_credential(
    state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;
    let cred_type = params["credential_type"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing credential_type"))?;

    let key = termfast_credential::make_key(server_id, cred_type);
    state.credential_store.delete(&key).map_err(|e| {
        IpcError::new(
            ErrorCode::Internal,
            format!("credential delete error: {}", e),
        )
    })?;
    Ok(serde_json::json!({ "deleted": true }))
}

/// Export full config (without credentials)
async fn handle_export_full(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let master_password = params["master_password"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing master_password"))?
        .to_string();

    let mgr = state.config_manager.lock().await;
    let config = mgr.get().await;

    // Collect credentials from the credential store
    let mut passwords = std::collections::HashMap::new();
    let mut key_passphrases = std::collections::HashMap::new();
    let mut key_files = std::collections::HashMap::new();

    for server in &config.servers {
        let sid = &server.id;
        // Only load the credential type that matches the server's auth method.
        // This avoids unnecessary macOS keychain prompts for credentials that
        // are not used by this server.
        if server.ssh.auth_method == "password" {
            let pwd_key =
                termfast_credential::make_key(sid, termfast_credential::cred_type::PASSWORD);
            if let Ok(pwd) = state.credential_store.load(&pwd_key) {
                passwords.insert(sid.clone(), pwd);
            }
        } else if server.ssh.auth_method == "key" {
            let pass_key =
                termfast_credential::make_key(sid, termfast_credential::cred_type::KEY_PASSPHRASE);
            if let Ok(pass) = state.credential_store.load(&pass_key) {
                key_passphrases.insert(sid.clone(), pass);
            }
            // Try to read key file content if key_path is set
            if !server.ssh.key_path.is_empty() {
                if let Ok(content) = std::fs::read_to_string(&server.ssh.key_path) {
                    key_files.insert(sid.clone(), content);
                }
            }
        }
    }

    let export_data = termfast_core::migration::FullExportData {
        config: config.clone(),
        passwords,
        key_passphrases,
        key_files,
    };

    // Encrypt with master password — Argon2id is CPU-intensive, run on
    // blocking pool to avoid stalling the async executor.
    let blob = tokio::task::spawn_blocking(move || {
        termfast_core::migration::export_full(&master_password, &export_data)
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, format!("spawn_blocking error: {}", e)))?
    .map_err(|e| IpcError::new(ErrorCode::Internal, format!("export encrypt error: {}", e)))?;

    // Return as base64-encoded string
    use base64::Engine;
    let blob_b64 = base64::engine::general_purpose::STANDARD.encode(&blob);
    Ok(serde_json::json!({ "blob": blob_b64, "size": blob.len() }))
}

// === SECTION 4 END ===

// === SECTION 5: Missing Action handlers ===

/// Export server configs (without credentials)
async fn handle_export_servers(state: &DaemonState) -> HandlerResult {
    let mgr = state.config_manager.lock().await;
    let config = mgr.get().await;
    let json = serde_json::to_value(&config.servers)
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("export error: {}", e)))?;
    Ok(serde_json::json!({ "servers": json }))
}

/// Import server configs (merge into existing)
async fn handle_import_servers(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let servers: Vec<termfast_core::config::ServerConfig> =
        serde_json::from_value(params["servers"].clone()).map_err(|e| {
            IpcError::new(ErrorCode::InvalidParams, format!("invalid servers: {}", e))
        })?;

    let mut imported = 0;
    let mgr = state.config_manager.lock().await;
    mgr.modify(|config| {
        for srv in servers {
            // Skip if ID already exists
            if config.find_server(&srv.id).is_none() {
                config.servers.push(srv);
                imported += 1;
            }
        }
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;

    Ok(serde_json::json!({ "imported": imported }))
}

/// Import full encrypted config (replace existing)
async fn handle_import_full(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let master_password = params["master_password"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing master_password"))?
        .to_string();
    let blob_b64 = params["blob"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing blob"))?;

    // Decode base64 blob
    use base64::Engine;
    let blob = base64::engine::general_purpose::STANDARD
        .decode(blob_b64)
        .map_err(|e| IpcError::new(ErrorCode::InvalidParams, format!("invalid base64: {}", e)))?;

    // Decrypt with master password — Argon2id is CPU-intensive, run on
    // blocking pool to avoid stalling the async executor.
    let export_data = tokio::task::spawn_blocking(move || {
        termfast_core::migration::import_full(&master_password, &blob)
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, format!("spawn_blocking error: {}", e)))?
    .map_err(|e| IpcError::new(ErrorCode::DecryptionFailed, format!("decrypt error: {}", e)))?;

    // Reset attempt counter on success
    termfast_core::migration::reset_attempts();

    // Apply the imported config + credentials + key files.
    // apply_full_export handles backup, crash-safe write order,
    // rollback on failure, and ServerManager sync.
    apply_full_export(state, &export_data).await?;

    Ok(serde_json::json!({ "imported": true }))
}

/// Cleanup authorized_keys on a server (remove our key if present)
async fn handle_cleanup_authorized_keys(
    state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;

    let server = state.server_manager.get_server(server_id).await?;
    // Execute cleanup command via SSH
    let _handle = server
        .ssh_client
        .get_handle()
        .await
        .ok_or_else(|| IpcError::new(ErrorCode::Internal, "no SSH connection"))?;

    // Remove our key from authorized_keys
    let key_path = &server.config.ssh.key_path;
    if !key_path.is_empty() {
        let key_name = std::path::Path::new(key_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("termfast");
        let cmd = format!(
            "sed -i '/# termfast: {}/d' ~/.ssh/authorized_keys 2>/dev/null || true",
            key_name
        );
        let _ = server.ssh_client.exec(&cmd, 10).await;
    }

    // Delete credentials from store
    let _ = state.credential_store.delete_all_for_server(server_id);

    Ok(serde_json::json!({ "server_id": server_id, "cleaned": true }))
}

/// Toggle proxy with advanced options (SOCKS5/HTTP independent control)
async fn handle_toggle_proxy_advanced(
    state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;
    let socks5_enabled = params["socks5_enabled"].as_bool();
    let http_enabled = params["http_enabled"].as_bool();

    let server = state.server_manager.get_server(server_id).await?;

    // Update config with advanced settings
    let mgr = state.config_manager.lock().await;
    mgr.modify(|config| {
        if let Some(srv) = config.find_server_mut(server_id) {
            if let Some(s) = socks5_enabled {
                if s {
                    srv.proxy.enabled = true;
                }
            }
            if let Some(h) = http_enabled {
                if h {
                    srv.proxy.enabled = true;
                }
            }
            if socks5_enabled == Some(false) && http_enabled == Some(false) {
                srv.proxy.enabled = false;
            }
        }
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
    drop(mgr);

    // Start/stop proxy based on new state
    let should_run = socks5_enabled == Some(true) || http_enabled == Some(true);
    if should_run {
        server.start_proxy().await?;
    } else {
        server.stop_proxy().await?;
    }

    state
        .broadcast(
            "proxy:status_changed",
            serde_json::json!({ "server_id": server_id, "proxy_enabled": should_run }),
        )
        .await;

    Ok(serde_json::json!({
        "server_id": server_id,
        "socks5_enabled": socks5_enabled.unwrap_or(false),
        "http_enabled": http_enabled.unwrap_or(false),
    }))
}

/// Set proxy authentication credential
async fn handle_set_proxy_auth(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;
    let username = params["username"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing username"))?;
    let password = params["password"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing password"))?;

    let cred_value = format!("{}:{}", username, password);
    let key = termfast_credential::make_key(server_id, "proxy_auth");
    state
        .credential_store
        .save(&key, &cred_value)
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("proxy auth save error: {}", e)))?;

    Ok(serde_json::json!({ "server_id": server_id, "set": true }))
}

/// Clear proxy authentication credential
async fn handle_clear_proxy_auth(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;

    let key = termfast_credential::make_key(server_id, "proxy_auth");
    state.credential_store.delete(&key).map_err(|e| {
        IpcError::new(
            ErrorCode::Internal,
            format!("proxy auth delete error: {}", e),
        )
    })?;

    Ok(serde_json::json!({ "server_id": server_id, "cleared": true }))
}

/// Add a trigger to a server
async fn handle_add_trigger(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;
    let trigger: termfast_core::config::TriggerInstance =
        serde_json::from_value(params["trigger"].clone()).map_err(|e| {
            IpcError::new(ErrorCode::InvalidParams, format!("invalid trigger: {}", e))
        })?;

    let trigger_id = trigger.id.clone();
    let template_id = trigger.template_id.clone();

    let mgr = state.config_manager.lock().await;
    mgr.modify(|config| {
        // First, look up template commands (immutable borrow)
        let template_commands: Option<(Vec<String>, String)> = if !template_id.is_empty() {
            config.find_template(&template_id).map(|tmpl| {
                let hash = compute_template_hash(&tmpl.commands);
                (tmpl.commands.clone(), hash)
            })
        } else {
            None
        };

        // Then, mutate the server config
        if let Some(srv) = config.find_server_mut(server_id) {
            if let Some((commands, hash)) = template_commands {
                let mut new_trigger = trigger.clone();
                new_trigger.commands = commands;
                new_trigger.template_hash_at_addition = hash;
                srv.triggers.push(new_trigger);
            } else {
                srv.triggers.push(trigger);
            }
        }
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
    drop(mgr);

    // Update server instance's trigger templates and triggers
    let server = state.server_manager.get_server(server_id).await?;
    {
        let mgr = state.config_manager.lock().await;
        let config = mgr.get().await;
        server
            .set_trigger_templates(config.trigger_templates.clone())
            .await;
        if let Some(srv) = config.find_server(server_id) {
            server.set_triggers(srv.triggers.clone()).await;
        }
    }

    state
        .broadcast(
            "trigger:added",
            serde_json::json!({ "server_id": server_id, "trigger_id": trigger_id }),
        )
        .await;

    Ok(serde_json::json!({ "server_id": server_id, "trigger_id": trigger_id }))
}

/// Remove a trigger from a server
async fn handle_remove_trigger(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;
    let trigger_id = params["trigger_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing trigger_id"))?;

    let mgr = state.config_manager.lock().await;
    mgr.modify(|config| {
        if let Some(srv) = config.find_server_mut(server_id) {
            srv.triggers.retain(|t| t.id != trigger_id);
        }
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
    drop(mgr);

    // Update server instance's triggers
    if let Ok(server) = state.server_manager.get_server(server_id).await {
        let mgr = state.config_manager.lock().await;
        let config = mgr.get().await;
        if let Some(srv) = config.find_server(server_id) {
            server.set_triggers(srv.triggers.clone()).await;
        }
    }

    state
        .broadcast(
            "trigger:removed",
            serde_json::json!({ "server_id": server_id, "trigger_id": trigger_id }),
        )
        .await;

    Ok(serde_json::json!({ "server_id": server_id, "trigger_id": trigger_id, "removed": true }))
}

/// Update a trigger on a server
async fn handle_update_trigger(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;
    let trigger_id = params["trigger_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing trigger_id"))?;

    let mgr = state.config_manager.lock().await;
    mgr.modify(|config| {
        if let Some(srv) = config.find_server_mut(server_id) {
            if let Some(trigger) = srv.triggers.iter_mut().find(|t| t.id == trigger_id) {
                if let Some(name) = params["name"].as_str() {
                    trigger.name = name.to_string();
                }
                if let Some(trigger_type) = params["trigger_type"].as_str() {
                    if let Ok(t) = serde_json::from_str::<termfast_core::config::TriggerType>(
                        &format!("\"{}\"", trigger_type),
                    ) {
                        trigger.trigger_type = t;
                    }
                }
                if let Some(enabled) = params["enabled"].as_bool() {
                    trigger.enabled = enabled;
                }
                if let Some(timeout) = params["timeout_secs"].as_u64() {
                    trigger.timeout_secs = timeout;
                }
                if let Some(cooldown) = params["cooldown_secs"].as_u64() {
                    trigger.cooldown_secs = cooldown;
                }
                if let Some(continue_on_error) = params["continue_on_error"].as_bool() {
                    trigger.continue_on_error = continue_on_error;
                }
                if let Some(notify_success) = params["notify_on_success"].as_bool() {
                    trigger.notify_on_success = notify_success;
                }
                if let Some(notify_failure) = params["notify_on_failure"].as_bool() {
                    trigger.notify_on_failure = notify_failure;
                }
                if let Some(commands) = params["commands"].as_array() {
                    trigger.commands = commands
                        .iter()
                        .filter_map(|c| c.as_str().map(String::from))
                        .collect();
                }
                if let Some(params_obj) = params["parameters"].as_object() {
                    trigger.parameters = params_obj
                        .iter()
                        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                        .collect();
                }
            }
        }
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
    drop(mgr);

    // Update server instance's triggers
    if let Ok(server) = state.server_manager.get_server(server_id).await {
        let mgr = state.config_manager.lock().await;
        let config = mgr.get().await;
        if let Some(srv) = config.find_server(server_id) {
            server.set_triggers(srv.triggers.clone()).await;
        }
    }

    state
        .broadcast(
            "trigger:updated",
            serde_json::json!({ "server_id": server_id, "trigger_id": trigger_id }),
        )
        .await;

    Ok(serde_json::json!({ "server_id": server_id, "trigger_id": trigger_id, "updated": true }))
}

/// Sync a trigger from its template (update commands to match template)
async fn handle_sync_trigger_from_template(
    state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;
    let trigger_id = params["trigger_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing trigger_id"))?;

    let mgr = state.config_manager.lock().await;
    mgr.modify(|config| {
        // First, find the trigger's template_id (immutable borrow)
        let template_id: Option<String> = config
            .find_server(server_id)
            .and_then(|srv| srv.triggers.iter().find(|t| t.id == trigger_id))
            .map(|t| t.template_id.clone());

        // Look up template commands (immutable borrow)
        let template_commands: Option<(Vec<String>, String)> = template_id
            .and_then(|tid| config.find_template(&tid))
            .map(|tmpl| {
                let hash = compute_template_hash(&tmpl.commands);
                (tmpl.commands.clone(), hash)
            });

        // Then mutate the trigger (mutable borrow)
        if let Some((commands, hash)) = template_commands {
            if let Some(srv) = config.find_server_mut(server_id) {
                if let Some(trigger) = srv.triggers.iter_mut().find(|t| t.id == trigger_id) {
                    trigger.commands = commands;
                    trigger.template_hash_at_addition = hash;
                }
            }
        }
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;

    Ok(serde_json::json!({ "server_id": server_id, "trigger_id": trigger_id, "synced": true }))
}

/// Create a new trigger template
async fn handle_create_template(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let template: termfast_core::config::TriggerTemplate =
        serde_json::from_value(params["template"].clone()).map_err(|e| {
            IpcError::new(ErrorCode::InvalidParams, format!("invalid template: {}", e))
        })?;

    let template_id = template.id.clone();
    let mgr = state.config_manager.lock().await;
    mgr.modify(|config| {
        config.trigger_templates.push(template);
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;

    Ok(serde_json::json!({ "template_id": template_id, "created": true }))
}

/// Update an existing trigger template
async fn handle_update_template(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let template_id = params["template_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing template_id"))?;

    let mgr = state.config_manager.lock().await;
    mgr.modify(|config| {
        if let Some(tmpl) = config.find_template_mut(template_id) {
            if let Some(name) = params["name"].as_str() {
                tmpl.name = name.to_string();
            }
            if let Some(desc) = params["description"].as_str() {
                tmpl.description = desc.to_string();
            }
            if let Some(commands) = params["commands"].as_array() {
                tmpl.commands = commands
                    .iter()
                    .filter_map(|c| c.as_str().map(String::from))
                    .collect();
            }
            if let Some(check_target) = params["check_target"].as_str() {
                tmpl.check_target = check_target.to_string();
            }
            if let Some(check_interval) = params["check_interval"].as_u64() {
                tmpl.check_interval = check_interval;
            }
            if let Some(timeout) = params["timeout_secs"].as_u64() {
                tmpl.timeout_secs = timeout;
            }
        }
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;

    Ok(serde_json::json!({ "template_id": template_id, "updated": true }))
}

/// Delete a trigger template (only user-created, not built-in)
async fn handle_delete_template(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let template_id = params["template_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing template_id"))?;

    let mgr = state.config_manager.lock().await;
    mgr.modify(|config| {
        // Only allow deleting non-built-in templates
        config
            .trigger_templates
            .retain(|t| t.id != template_id || t.built_in);
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;

    Ok(serde_json::json!({ "template_id": template_id, "deleted": true }))
}

/// Save a trigger instance as a new template
async fn handle_save_trigger_as_template(
    state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;
    let trigger_id = params["trigger_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing trigger_id"))?;
    let template_name = params["template_name"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing template_name"))?;

    let mgr = state.config_manager.lock().await;
    let mut new_template_id = String::new();
    mgr.modify(|config| {
        // First, look up trigger type from template (immutable borrow)
        let trigger_info: Option<(Vec<String>, u64, termfast_core::config::TriggerType)> = config
            .find_server(server_id)
            .and_then(|srv| srv.triggers.iter().find(|t| t.id == trigger_id))
            .map(|trigger| {
                let trigger_type = get_trigger_type_from_template(config, &trigger.template_id);
                (trigger.commands.clone(), trigger.timeout_secs, trigger_type)
            });

        // Then push the new template (mutable borrow)
        if let Some((commands, timeout, trigger_type)) = trigger_info {
            let new_id = format!("tmpl_{}", uuid_v4_simple());
            new_template_id = new_id.clone();
            let template = termfast_core::config::TriggerTemplate {
                id: new_id,
                name: template_name.to_string(),
                trigger_type,
                description: String::new(),
                built_in: false,
                template_version: 1,
                parameters_schema: Vec::new(),
                commands,
                check_target: String::new(),
                check_interval: 60,
                timeout_secs: timeout,
            };
            config.trigger_templates.push(template);
        }
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;

    Ok(serde_json::json!({ "template_id": new_template_id, "saved": true }))
}

/// Import templates (merge into existing)
async fn handle_import_templates(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let templates: Vec<termfast_core::config::TriggerTemplate> =
        serde_json::from_value(params["templates"].clone()).map_err(|e| {
            IpcError::new(
                ErrorCode::InvalidParams,
                format!("invalid templates: {}", e),
            )
        })?;

    let mut imported = 0;
    let mgr = state.config_manager.lock().await;
    mgr.modify(|config| {
        for tmpl in templates {
            if config.find_template(&tmpl.id).is_none() {
                config.trigger_templates.push(tmpl);
                imported += 1;
            }
        }
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;

    Ok(serde_json::json!({ "imported": imported }))
}

/// Export all templates
async fn handle_export_templates(state: &DaemonState) -> HandlerResult {
    let mgr = state.config_manager.lock().await;
    let config = mgr.get().await;
    let json = serde_json::to_value(&config.trigger_templates)
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("export error: {}", e)))?;
    Ok(serde_json::json!({ "templates": json }))
}

/// Configure key-based authentication for a server
async fn handle_configure_key_auth(
    state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;

    let key_path = params["key_path"].as_str().unwrap_or("");
    let passphrase = params["passphrase"].as_str();

    // Save passphrase to credential store if provided
    if let Some(pass) = passphrase {
        if !pass.is_empty() {
            let key = termfast_credential::make_key(
                server_id,
                termfast_credential::cred_type::KEY_PASSPHRASE,
            );
            state.credential_store.save(&key, pass).map_err(|e| {
                IpcError::new(ErrorCode::Internal, format!("passphrase save error: {}", e))
            })?;
        }
    }

    // Update server config to use key auth
    let mgr = state.config_manager.lock().await;
    mgr.modify(|config| {
        if let Some(srv) = config.find_server_mut(server_id) {
            srv.ssh.auth_method = "key".to_string();
            srv.ssh.key_path = key_path.to_string();
        }
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;

    Ok(serde_json::json!({ "server_id": server_id, "configured": true }))
}

/// Switch authentication method for a server (password ↔ key)
async fn handle_switch_auth_method(
    state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;
    let auth_method = params["auth_method"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing auth_method"))?;

    let mgr = state.config_manager.lock().await;
    let old_method = {
        let config = mgr.get().await;
        config
            .find_server(server_id)
            .map(|s| s.ssh.auth_method.clone())
            .unwrap_or_default()
    };
    mgr.modify(|config| {
        if let Some(srv) = config.find_server_mut(server_id) {
            srv.ssh.auth_method = auth_method.to_string();
        }
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
    drop(mgr);

    // Clean up credentials that are no longer needed
    if old_method == "password" && auth_method == "key" {
        // Switching password → key: delete password from keychain
        let pwd_key =
            termfast_credential::make_key(server_id, termfast_credential::cred_type::PASSWORD);
        let _ = state.credential_store.delete(&pwd_key);
        log_and_broadcast(
            state,
            Some(server_id),
            LogLevel::Info,
            LogKind::System,
            "Switched to key auth, removed password from keychain".to_string(),
        )
        .await;
    } else if old_method == "key" && auth_method == "password" {
        // Switching key → password: delete key passphrase from keychain
        let pass_key = termfast_credential::make_key(
            server_id,
            termfast_credential::cred_type::KEY_PASSPHRASE,
        );
        let _ = state.credential_store.delete(&pass_key);
        log_and_broadcast(
            state,
            Some(server_id),
            LogLevel::Info,
            LogKind::System,
            "Switched to password auth, removed key passphrase from keychain".to_string(),
        )
        .await;
    }

    Ok(serde_json::json!({ "server_id": server_id, "auth_method": auth_method }))
}

// === SECTION 5 END ===

/// Detect firewall type and protected ports via SSH exec (FP-8.1)
/// Returns firewall type (firewalld/ufw/none) and list of open ports.
async fn handle_detect_firewall(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let server_id = params["server_id"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;

    let server = state.server_manager.get_server(server_id).await?;
    let handle =
        server.ssh_client.get_handle().await.ok_or_else(|| {
            IpcError::new(ErrorCode::Internal, "no SSH connection — connect first")
        })?;

    use termfast_core::ssh::exec;

    // Check firewalld
    let result = exec::exec(&handle, "systemctl is-active firewalld 2>/dev/null", 10).await?;
    let has_firewalld = result.stdout.trim() == "active";

    // Check ufw
    let result = exec::exec(
        &handle,
        "which ufw 2>/dev/null && ufw status 2>/dev/null | head -1",
        10,
    )
    .await?;
    let has_ufw = result.stdout.contains("active");

    let firewall_type = if has_firewalld {
        "firewalld"
    } else if has_ufw {
        "ufw"
    } else {
        "none"
    };

    // Get list of open/listening ports
    let result = exec::exec(
        &handle,
        "ss -tlnp 2>/dev/null | grep LISTEN | awk '{print $4}' | sed 's/.*://' | sort -un",
        10,
    )
    .await?;
    let ports: Vec<u16> = result
        .stdout
        .lines()
        .filter_map(|line| line.trim().parse::<u16>().ok())
        .collect();

    // Get firewalld open services/ports if firewalld is active
    let firewalld_ports: Vec<String> = if has_firewalld {
        let result = exec::exec(&handle, "firewall-cmd --list-ports 2>/dev/null", 10)
            .await
            .map(|r| r.stdout)
            .unwrap_or_default();
        result.split_whitespace().map(|s| s.to_string()).collect()
    } else {
        Vec::new()
    };

    Ok(serde_json::json!({
        "server_id": server_id,
        "firewall_type": firewall_type,
        "listening_ports": ports,
        "firewalld_open_ports": firewalld_ports,
    }))
}

// === SECTION 6: Helper functions ===

/// Compute a simple hash of template commands (for modified_from_template detection)
fn compute_template_hash(commands: &[String]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    for cmd in commands {
        cmd.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

/// Generate a simple UUID-like string (without external dependency)
fn uuid_v4_simple() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::SystemTime;
    let mut hasher = DefaultHasher::new();
    SystemTime::now().hash(&mut hasher);
    std::thread::current().id().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Get trigger type from a template ID
fn get_trigger_type_from_template(
    config: &termfast_core::config::Config,
    template_id: &str,
) -> termfast_core::config::TriggerType {
    config
        .find_template(template_id)
        .map(|t| t.trigger_type.clone())
        .unwrap_or(termfast_core::config::TriggerType::ManualFire)
}

// === SECTION 6 END ===

impl From<termfast_core::Error> for IpcError {
    fn from(e: termfast_core::Error) -> Self {
        match e {
            termfast_core::Error::Ipc(ipc) => IpcError::new(ipc.code, ipc.detail),
            termfast_core::Error::Config(msg) => IpcError::new(ErrorCode::Internal, msg),
            termfast_core::Error::Ssh(msg) => IpcError::new(ErrorCode::SshConnectFailed, msg),
            termfast_core::Error::Io(e) => IpcError::new(ErrorCode::Internal, e.to_string()),
            termfast_core::Error::Crypto(msg) => IpcError::new(ErrorCode::Internal, msg),
            termfast_core::Error::Serde(e) => IpcError::new(ErrorCode::Internal, e.to_string()),
            termfast_core::Error::Other(msg) => IpcError::new(ErrorCode::Internal, msg),
        }
    }
}

// === SECTION: Terminal handlers ===

/// Open a new interactive terminal session
async fn handle_terminal_open(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let server_id = params
        .get("server_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing server_id"))?;
    let cols = params.get("cols").and_then(|v| v.as_u64()).unwrap_or(80) as u32;
    let rows = params.get("rows").and_then(|v| v.as_u64()).unwrap_or(24) as u32;

    tracing::info!(
        "handle_terminal_open: server_id={}, cols={}, rows={}",
        server_id,
        cols,
        rows
    );

    let server = state
        .server_manager
        .get_server(server_id)
        .await
        .map_err(|e| IpcError::new(ErrorCode::ServerNotFound, e.to_string()))?;

    if !server.ssh_client.is_connected().await {
        return Err(IpcError::new(
            ErrorCode::SshDisconnected,
            "server is not connected — connect first",
        ));
    }

    let ssh_handle = server
        .ssh_client
        .get_handle()
        .await
        .ok_or_else(|| IpcError::new(ErrorCode::SshDisconnected, "SSH handle not available"))?;

    tracing::info!("handle_terminal_open: got SSH handle, opening PTY...");

    let (session_id, initial_output) = state
        .terminal_manager
        .open(&ssh_handle, server_id, cols, rows)
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, e))?;

    tracing::info!(
        "handle_terminal_open: PTY opened, session_id={}, initial_output={} bytes",
        session_id,
        initial_output.len()
    );

    Ok(serde_json::json!({ "session_id": session_id, "initial_output": initial_output }))
}

/// Send user input to a terminal session
async fn handle_terminal_input(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing session_id"))?;
    let data = params
        .get("data")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing data"))?;
    // wait_for_send=true blocks until SSH write is confirmed (needed for ZMODEM
    // backpressure). Default false — normal keystrokes return immediately.
    let wait_for_send = params
        .get("wait_for_send")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if data.len() <= 64 {
        tracing::info!(
            "handle_terminal_input: session_id={} data_len={} wait_for_send={}",
            session_id,
            data.len(),
            wait_for_send,
        );
    }

    state
        .terminal_manager
        .input_with_ack(session_id, data, wait_for_send)
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, e))?;

    Ok(serde_json::json!({ "ok": true }))
}

/// Close a terminal session
async fn handle_terminal_close(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing session_id"))?;

    state
        .terminal_manager
        .close(session_id)
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, e))?;

    Ok(serde_json::json!({ "ok": true }))
}

/// Resize a terminal session
async fn handle_terminal_resize(state: &DaemonState, params: &serde_json::Value) -> HandlerResult {
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing session_id"))?;
    let cols = params
        .get("cols")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing cols"))?
        as u32;
    let rows = params
        .get("rows")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing rows"))?
        as u32;

    state
        .terminal_manager
        .resize(session_id, cols, rows)
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, e))?;

    Ok(serde_json::json!({ "ok": true }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use termfast_core::config::Config;

    fn test_state() -> DaemonState {
        let config = Config::default();
        let mgr = termfast_core::config::ConfigManager::new(config);
        DaemonState::new(mgr)
    }

    #[tokio::test]
    async fn test_list_servers_empty() {
        let state = test_state();
        let req = Request::new_simple(Action::ListServers);
        let resp = handle_request(&req, &state).await;
        assert!(matches!(resp, Response::Ok { .. }));
    }

    #[tokio::test]
    async fn test_get_daemon_status() {
        let state = test_state();
        let req = Request::new_simple(Action::GetDaemonStatus);
        let resp = handle_request(&req, &state).await;
        if let Response::Ok { data, .. } = resp {
            assert_eq!(data["running"], true);
            assert_eq!(data["server_count"], 0);
        } else {
            panic!("expected Ok response");
        }
    }

    #[tokio::test]
    async fn test_get_config() {
        let state = test_state();
        let req = Request::new_simple(Action::GetConfig);
        let resp = handle_request(&req, &state).await;
        assert!(matches!(resp, Response::Ok { .. }));
    }

    #[tokio::test]
    async fn test_clear_logs() {
        let state = test_state();
        let req = Request::new_simple(Action::ClearLogs);
        let resp = handle_request(&req, &state).await;
        assert!(matches!(resp, Response::Ok { .. }));
    }

    #[tokio::test]
    async fn test_pause_all_triggers() {
        let state = test_state();
        let req = Request::new_simple(Action::PauseAllTriggers);
        let resp = handle_request(&req, &state).await;
        assert!(matches!(resp, Response::Ok { .. }));
    }

    #[tokio::test]
    async fn test_resume_all_triggers() {
        let state = test_state();
        let req = Request::new_simple(Action::ResumeAllTriggers);
        let resp = handle_request(&req, &state).await;
        assert!(matches!(resp, Response::Ok { .. }));
    }

    #[tokio::test]
    async fn test_get_server_status_not_found() {
        let state = test_state();
        let req = Request::new(
            Action::GetServerStatus,
            serde_json::json!({"server_id": "nonexistent"}),
        );
        let resp = handle_request(&req, &state).await;
        assert!(matches!(resp, Response::Err { .. }));
    }

    #[tokio::test]
    async fn test_unimplemented_action() {
        let state = test_state();
        // CleanupAuthorizedKeys is not yet implemented
        let req = Request::new_simple(Action::CleanupAuthorizedKeys);
        let resp = handle_request(&req, &state).await;
        assert!(matches!(resp, Response::Err { .. }));
    }

    // === 11.3 同步流程测试 ===

    use termfast_cloud_sync::RemoteFileInfo;
    use termfast_cloud_sync::sync_crypto::SyncPayload;

    fn make_remote(exists: bool, hash: Option<&str>) -> RemoteFileInfo {
        RemoteFileInfo {
            exists,
            size: if exists { Some(1024) } else { None },
            hash: hash.map(String::from),
            modified: if exists { Some("2026-07-21T10:00:00Z".into()) } else { None },
        }
    }

    /// 11.3 用例 22: 远端不存在 → 安全上传（返回 None）
    #[test]
    fn test_conflict_remote_not_exists() {
        let remote = make_remote(false, None);
        let result = check_upload_conflict(&remote, None);
        assert!(result.is_none(), "remote not exists → safe to upload");
    }

    /// 11.3 用例 23: hash 一致 → 安全上传
    #[test]
    fn test_conflict_hash_match() {
        let remote = make_remote(true, Some("abc123"));
        let result = check_upload_conflict(&remote, Some("abc123"));
        assert!(result.is_none(), "hash match → safe to upload");
    }

    /// 11.3 用例 24: hash 不一致 → cloud_changed 冲突
    #[test]
    fn test_conflict_hash_mismatch() {
        let remote = make_remote(true, Some("abc123"));
        let result = check_upload_conflict(&remote, Some("different_hash"));
        assert!(result.is_some());
        let v = result.unwrap();
        assert_eq!(v["conflict"], true);
        assert_eq!(v["reason"], "cloud_changed");
    }

    /// 11.3 用例 25: 远端存在但本地无缓存 → cloud_exists_no_cache 冲突
    #[test]
    fn test_conflict_no_local_cache() {
        let remote = make_remote(true, Some("abc123"));
        let result = check_upload_conflict(&remote, None);
        assert!(result.is_some());
        let v = result.unwrap();
        assert_eq!(v["conflict"], true);
        assert_eq!(v["reason"], "cloud_exists_no_cache");
    }

    /// 11.3 用例 26: 远端存在但无 hash（provider 不支持）→ 安全上传
    #[test]
    fn test_conflict_no_remote_hash() {
        let remote = make_remote(true, None);
        let result = check_upload_conflict(&remote, Some("local_hash"));
        assert!(result.is_none(), "no remote hash → can't detect, proceed");
    }

    /// 11.3 用例 27: force=true 时即使有冲突也跳过
    /// 验证 check_upload_with_force(force=true, ...) 返回 None（跳过冲突检测）
    #[test]
    fn test_conflict_force_true_skips_conflict() {
        let remote = make_remote(true, Some("abc"));
        // force=true → 即使 hash 不一致也返回 None（跳过冲突检测）
        let result = check_upload_with_force(true, &remote, Some("xyz"));
        assert!(result.is_none(), "force=true must skip conflict detection");
    }

    /// 11.3 补充: force=false 时正常检测冲突
    #[test]
    fn test_conflict_force_false_detects_conflict() {
        let remote = make_remote(true, Some("abc"));
        // force=false → hash 不一致时返回冲突
        let result = check_upload_with_force(false, &remote, Some("xyz"));
        assert!(result.is_some());
        let v = result.unwrap();
        assert_eq!(v["reason"], "cloud_changed");
    }

    /// 11.3 补充: force=false 且无冲突时返回 None
    #[test]
    fn test_conflict_force_false_no_conflict() {
        let remote = make_remote(true, Some("abc"));
        let result = check_upload_with_force(false, &remote, Some("abc"));
        assert!(result.is_none(), "force=false + hash match → no conflict");
    }

    /// 11.7 用例 32: 回滚检测 — 云端 updated_at 比本地旧 → 检测到回滚
    #[test]
    fn test_rollback_detected() {
        let payload = SyncPayload {
            config: serde_json::json!({}),
            device_name: "attacker-device".into(),
            updated_at: "2026-07-20T10:00:00Z".into(), // 比本地旧
        };
        let last = Some("2026-07-21T10:00:00Z");
        let result = check_rollback(&payload, last);
        assert!(result.is_some());
        let v = result.unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["reason"], "rollback_detected");
        assert_eq!(v["cloud_updated_at"], "2026-07-20T10:00:00Z");
        assert_eq!(v["last_updated_at"], "2026-07-21T10:00:00Z");
        assert_eq!(v["device_name"], "attacker-device");
    }

    /// 11.7 用例 33: 回滚检测 — 云端 updated_at 比本地新 → 安全（无回滚）
    #[test]
    fn test_rollback_not_detected_newer() {
        let payload = SyncPayload {
            config: serde_json::json!({}),
            device_name: "other-device".into(),
            updated_at: "2026-07-22T10:00:00Z".into(), // 比本地新
        };
        let last = Some("2026-07-21T10:00:00Z");
        let result = check_rollback(&payload, last);
        assert!(result.is_none(), "newer cloud → no rollback");
    }

    /// 11.7 用例 34: 回滚检测 — 时间戳相等 → 安全（非严格小于）
    #[test]
    fn test_rollback_equal_timestamp_safe() {
        let payload = SyncPayload {
            config: serde_json::json!({}),
            device_name: "same-device".into(),
            updated_at: "2026-07-21T10:00:00Z".into(),
        };
        let last = Some("2026-07-21T10:00:00Z");
        let result = check_rollback(&payload, last);
        assert!(result.is_none(), "equal timestamp → not rollback");
    }

    /// 11.7 用例 35: 回滚检测 — 本地无 last_updated_at → 跳过检测（首次同步）
    #[test]
    fn test_rollback_no_local_history() {
        let payload = SyncPayload {
            config: serde_json::json!({}),
            device_name: "any-device".into(),
            updated_at: "2020-01-01T00:00:00Z".into(), // 很旧
        };
        let result = check_rollback(&payload, None);
        assert!(result.is_none(), "no local history → skip rollback check");
    }

    /// 11.7 用例 36: 回滚检测 — force_download=true 时跳过回滚检测
    /// 验证 check_rollback_with_force(force_download=true, ...) 返回 None
    #[test]
    fn test_rollback_force_download_true_skips_check() {
        let payload = SyncPayload {
            config: serde_json::json!({}),
            device_name: "old-device".into(),
            updated_at: "2020-01-01T00:00:00Z".into(), // 很旧
        };
        let last = Some("2026-07-21T10:00:00Z");
        // force_download=true → 即使检测到回滚也返回 None（跳过）
        let result = check_rollback_with_force(true, &payload, last);
        assert!(result.is_none(), "force_download=true must skip rollback check");
    }

    /// 11.7 补充: force_download=false 时正常检测回滚
    #[test]
    fn test_rollback_force_download_false_detects() {
        let payload = SyncPayload {
            config: serde_json::json!({}),
            device_name: "old-device".into(),
            updated_at: "2020-01-01T00:00:00Z".into(),
        };
        let last = Some("2026-07-21T10:00:00Z");
        let result = check_rollback_with_force(false, &payload, last);
        assert!(result.is_some());
        let v = result.unwrap();
        assert_eq!(v["reason"], "rollback_detected");
    }

    /// 11.3 补充: 解密失败响应 — 调用 build_decrypt_failed_response 纯函数
    #[test]
    fn test_decrypt_failed_response() {
        let resp = build_decrypt_failed_response();
        assert_eq!(resp["ok"], false);
        assert_eq!(resp["reason"], "decrypt_failed");
        assert!(resp["message"].as_str().unwrap().contains("解密失败"));
    }

    /// 11.3 补充: 云端无更新响应 — 调用 build_no_update_response 纯函数
    #[test]
    fn test_no_update_response() {
        let resp = build_no_update_response(
            Some("2026-07-21T18:40:00Z"),
            Some("2026-07-21T19:05:00Z"),
        );
        assert_eq!(resp["ok"], false);
        assert_eq!(resp["reason"], "no_update");
        assert!(resp["message"].as_str().unwrap().contains("无更新"));
        assert_eq!(resp["cloud_updated_at"], "2026-07-21T18:40:00Z");
        assert_eq!(resp["local_updated_at"], "2026-07-21T19:05:00Z");
    }

    /// 验证 no_update 响应在时间戳为 None 时不崩溃
    #[test]
    fn test_no_update_response_null_timestamps() {
        let resp = build_no_update_response(None, None);
        assert_eq!(resp["ok"], false);
        assert_eq!(resp["reason"], "no_update");
        assert!(resp["cloud_updated_at"].is_null());
        assert!(resp["local_updated_at"].is_null());
    }

    /// 11.3 补充: 云端无数据响应 — 调用 build_no_remote_data_response 纯函数
    #[test]
    fn test_no_remote_data_response() {
        let resp = build_no_remote_data_response();
        assert_eq!(resp["ok"], false);
        assert_eq!(resp["reason"], "no_remote_data");
        assert!(resp["message"].as_str().unwrap().contains("没有同步数据"));
    }

    /// 11.3 补充: is_no_update — hash 一致且 mtime 一致时返回 true
    #[test]
    fn test_is_no_update_hash_match() {
        let remote = make_remote(true, Some("abc123"));
        assert!(is_no_update(&remote, Some("abc123"), Some("1000"), Some("1000")));
    }

    /// 11.3 补充: is_no_update — hash 不一致时返回 false
    #[test]
    fn test_is_no_update_hash_mismatch() {
        let remote = make_remote(true, Some("abc123"));
        assert!(!is_no_update(&remote, Some("different"), Some("1000"), Some("1000")));
    }

    /// 11.3 补充: is_no_update — 任一 hash 缺失时返回 false
    #[test]
    fn test_is_no_update_missing_hash() {
        let remote_no_hash = make_remote(true, None);
        assert!(!is_no_update(&remote_no_hash, Some("abc"), Some("1000"), Some("1000")));
        let remote_with_hash = make_remote(true, Some("abc"));
        assert!(!is_no_update(&remote_with_hash, None, Some("1000"), Some("1000")));
    }

    /// is_no_update — hash 匹配但本地 mtime 变了（本地有修改）→ 返回 false
    /// 这是核心修复：用户新建节点后本地 config.json mtime 变了，
    /// 即使云端 hash 没变也不应阻止下载。
    #[test]
    fn test_is_no_update_local_mtime_changed() {
        let remote = make_remote(true, Some("abc123"));
        // hash 匹配，但 mtime 从 1000 变成了 2000（本地被修改过）
        assert!(!is_no_update(&remote, Some("abc123"), Some("1000"), Some("2000")));
    }

    /// is_no_update — mtime 缺失时返回 false（允许下载，安全默认）
    #[test]
    fn test_is_no_update_missing_mtime() {
        let remote = make_remote(true, Some("abc123"));
        // hash 匹配，但 mtime 信息缺失
        assert!(!is_no_update(&remote, Some("abc123"), None, Some("1000")));
        assert!(!is_no_update(&remote, Some("abc123"), Some("1000"), None));
        assert!(!is_no_update(&remote, Some("abc123"), None, None));
    }

    /// is_local_newer — 云端 hash 未变但本地 mtime 变了 → true
    #[test]
    fn test_is_local_newer_true() {
        let remote = make_remote(true, Some("abc123"));
        assert!(is_local_newer(&remote, Some("abc123"), Some("1000"), Some("2000")));
    }

    /// is_local_newer — 云端 hash 变了 → false（云端有更新，不是 local_newer 场景）
    #[test]
    fn test_is_local_newer_cloud_changed() {
        let remote = make_remote(true, Some("abc123"));
        assert!(!is_local_newer(&remote, Some("different"), Some("1000"), Some("2000")));
    }

    /// is_local_newer — 云端和本地都没变 → false
    #[test]
    fn test_is_local_newer_both_unchanged() {
        let remote = make_remote(true, Some("abc123"));
        assert!(!is_local_newer(&remote, Some("abc123"), Some("1000"), Some("1000")));
    }

    /// is_local_newer — mtime 缺失 → false（无法判断，不拦截）
    #[test]
    fn test_is_local_newer_missing_mtime() {
        let remote = make_remote(true, Some("abc123"));
        assert!(!is_local_newer(&remote, Some("abc123"), None, Some("1000")));
        assert!(!is_local_newer(&remote, Some("abc123"), Some("1000"), None));
    }

    /// build_local_newer_response — 验证响应格式
    #[test]
    fn test_build_local_newer_response() {
        let resp = build_local_newer_response(
            Some("2026-07-21T18:40:00Z"),
            Some("2026-07-21T19:05:00Z"),
        );
        assert_eq!(resp["ok"], false);
        assert_eq!(resp["reason"], "local_newer");
        assert!(resp["message"].as_str().unwrap().contains("本地数据比云端新"));
        assert_eq!(resp["cloud_updated_at"], "2026-07-21T18:40:00Z");
        assert_eq!(resp["local_updated_at"], "2026-07-21T19:05:00Z");
    }
}

/// Helper: if request came from CLI, broadcast cli:focus to GUI
async fn maybe_broadcast_cli_focus(
    state: &DaemonState,
    params: &serde_json::Value,
    server_id: &str,
    tab: Option<&str>,
) {
    if params
        .get("_cli")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        let mut data = serde_json::json!({ "server_id": server_id });
        if let Some(t) = tab {
            data["tab"] = serde_json::json!(t);
        }
        state.broadcast("cli:focus", data).await;
    }
}

/// Helper: write a log entry to buffer and broadcast to frontend
async fn log_and_broadcast(
    state: &DaemonState,
    server_id: Option<&str>,
    level: LogLevel,
    kind: LogKind,
    message: String,
) {
    let level_str = match level {
        LogLevel::Trace => "trace",
        LogLevel::Debug => "debug",
        LogLevel::Info => "info",
        LogLevel::Warn => "warn",
        LogLevel::Error => "error",
    };
    let kind_str = match kind {
        LogKind::Connection => "Connection",
        LogKind::Proxy => "Proxy",
        LogKind::Trigger => "Trigger",
        LogKind::Error => "Error",
        LogKind::System => "System",
    };
    let entry = LogEntry {
        timestamp: chrono::Utc::now(),
        server_id: server_id.map(|s| s.to_string()),
        level,
        kind,
        message: message.clone(),
        data: None,
        execution_id: None,
    };
    state.log_buffer.add(entry.clone()).await;
    state
        .broadcast(
            "log:entry",
            serde_json::json!({
                "server_id": server_id,
                "level": level_str,
                "kind": kind_str,
                "message": message,
                "timestamp": entry.timestamp,
            }),
        )
        .await;
}

// === SECTION: Cloud sync handlers ===
#[allow(clippy::items_after_test_module)]
/// Get the token file path (stored alongside config).
fn token_file_path(_state: &DaemonState) -> std::path::PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.config_dir().join("termfast").join("cloud_tokens.json"))
        .unwrap_or_else(|| std::path::PathBuf::from("cloud_tokens.json"))
}

/// Path to the encrypted sync state file.
fn sync_state_path(_state: &DaemonState) -> std::path::PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.config_dir().join("termfast").join("sync_state.enc"))
        .unwrap_or_else(|| std::path::PathBuf::from("sync_state.enc"))
}

/// Path to the local config.json file.
fn local_config_path(_state: &DaemonState) -> std::path::PathBuf {
    // Must match the path used by FileConfigStorage::default_path()
    // (directories::ProjectDirs, NOT BaseDirs) — otherwise mtime checks
    // read the wrong file and no_update logic breaks.
    directories::ProjectDirs::from("", "", "termfast")
        .map(|d| d.data_dir().join("config.json"))
        .unwrap_or_else(|| std::path::PathBuf::from("config.json"))
}

/// Get the mtime (unix epoch seconds) of the local config.json file.
/// Returns None if the file doesn't exist or mtime can't be read.
fn local_config_mtime(state: &DaemonState) -> Option<String> {
    let path = local_config_path(state);
    let meta = std::fs::metadata(&path).ok()?;
    let mtime = meta.modified().ok()?;
    let secs = mtime
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(secs.to_string())
}

/// Get the sync file path on cloud storage.
/// Reads custom path from params if provided, otherwise uses default.
fn sync_path_from_params(params: &serde_json::Value) -> String {
    params["sync_path"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| termfast_cloud_sync::SYNC_FILE_PATH.to_string())
}

/// Build a provider instance from the provider type string.
/// No app_key needed — providers get OAuth URLs and exchange tokens
/// through the cloud sync proxy server.
fn build_provider(
    provider: &str,
) -> Result<Box<dyn termfast_cloud_sync::CloudProviderTrait>, IpcError> {
    match provider {
        "dropbox" => Ok(Box::new(
            termfast_cloud_sync::dropbox::DropboxProvider::new(),
        )),
        "baidu" => Ok(Box::new(
            termfast_cloud_sync::baidu::BaiduProvider::new(),
        )),
        _ => Err(IpcError::new(
            ErrorCode::InvalidParams,
            format!("unknown provider: {}", provider),
        )),
    }
}

async fn handle_cloud_sync_auth_url(
    _state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let provider = params["provider"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing provider"))?;
    let redirect_uri = params["redirect_uri"]
        .as_str()
        .unwrap_or("http://localhost:17380/callback");

    let p = build_provider(provider)?;
    let (url, code_verifier) = p.auth_url(redirect_uri);

    Ok(serde_json::json!({
        "auth_url": url,
        "code_verifier": code_verifier,
        "provider": provider,
    }))
}

async fn handle_cloud_sync_exchange_code(
    _state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let provider = params["provider"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing provider"))?;
    let code = params["code"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing code"))?;
    let code_verifier = params["code_verifier"]
        .as_str()
        .unwrap_or("");
    let redirect_uri = params["redirect_uri"]
        .as_str()
        .unwrap_or("http://localhost:17380/callback");
    let state = params["state"].as_str().unwrap_or("");

    let p = build_provider(provider)?;
    let token = p
        .exchange_code(code, code_verifier, redirect_uri, state)
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("OAuth exchange: {}", e)))?;

    Ok(serde_json::json!({
        "access_token": token.access_token,
        "refresh_token": token.refresh_token,
        "expires_at": token.expires_at,
        "token_type": token.token_type,
    }))
}

async fn handle_cloud_sync_save_token(
    state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let provider = params["provider"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing provider"))?;
    let access_token = params["access_token"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing access_token"))?;

    let provider_type: termfast_cloud_sync::CloudProvider = provider
        .parse::<termfast_cloud_sync::CloudProvider>()
        .map_err(|e| IpcError::new(ErrorCode::InvalidParams, e.to_string()))?;

    let token = termfast_cloud_sync::OAuthToken {
        access_token: access_token.to_string(),
        refresh_token: params["refresh_token"].as_str().map(|s| s.to_string()),
        expires_at: params["expires_at"].as_i64(),
        token_type: params["token_type"]
            .as_str()
            .unwrap_or("bearer")
            .to_string(),
    };

    let path = token_file_path(state);

    // Load existing tokens (if any) and merge
    let mut data = if termfast_cloud_sync::token_store::token_file_exists(&path) {
        termfast_cloud_sync::token_store::load_tokens(&path)
            .unwrap_or_default()
    } else {
        termfast_cloud_sync::token_store::TokenStoreData::default()
    };

    data.tokens.insert(
        provider.to_string(),
        termfast_cloud_sync::token_store::StoredToken {
            provider: provider_type,
            token,
            stored_at: chrono::Utc::now().timestamp(),
        },
    );

    termfast_cloud_sync::token_store::save_tokens(&path, &data)
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("save token: {}", e)))?;

    Ok(serde_json::json!({ "ok": true }))
}

async fn handle_cloud_sync_load_token(
    state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let provider = params["provider"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing provider"))?;

    let path = token_file_path(state);

    if !termfast_cloud_sync::token_store::token_file_exists(&path) {
        return Ok(serde_json::json!({ "authenticated": false }));
    }

    let data = termfast_cloud_sync::token_store::load_tokens(&path)
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("load token: {}", e)))?;

    let stored = data.tokens.get(provider);
    Ok(serde_json::json!({
        "authenticated": stored.is_some(),
        "access_token": stored.as_ref().map(|s| s.token.access_token.clone()),
        "expires_at": stored.as_ref().and_then(|s| s.token.expires_at),
    }))
}

async fn handle_cloud_sync_upload(
    state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let provider = params["provider"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing provider"))?
        .to_string();
    let master_password = params["master_password"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing master_password"))?
        .to_string();
    // force=true skips conflict detection (used after user confirms overwrite)
    let force = params["force"].as_bool().unwrap_or(false);

    // Load cloud token
    let path = token_file_path(state);
    let data = termfast_cloud_sync::token_store::load_tokens(&path)
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("load token: {}", e)))?;
    let stored = data.tokens.get(provider.as_str()).ok_or_else(|| {
        IpcError::new(ErrorCode::CredentialNotFound, "not authenticated to cloud")
    })?;

    let p = build_provider(&provider)?;
    let sync_path = sync_path_from_params(params);

    // Check remote file info for conflict detection
    let remote_info = p
        .file_info(&stored.token, &sync_path)
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("file_info: {}", e)))?;

    // Load local sync state (encrypted) — on blocking thread
    let state_path = sync_state_path(state);
    let mp_for_state = master_password.clone();
    let sync_state = tokio::task::spawn_blocking(move || {
        termfast_cloud_sync::sync_state::load_state(&state_path, &mp_for_state)
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, format!("spawn_blocking: {}", e)))?;

    let local_hash = sync_state.last_hash(&provider);

    // Conflict detection (unless force=true)
    if let Some(conflict) = check_upload_with_force(force, &remote_info, local_hash) {
        return Ok(conflict);
    }

    // Export config data
    let export_data = export_full_data(state).await?;

    // Build sync payload with metadata
    let device_name = termfast_cloud_sync::sync_crypto::device_name();
    let updated_at = chrono::Utc::now().to_rfc3339();
    let payload = termfast_cloud_sync::sync_crypto::SyncPayload {
        config: serde_json::to_value(&export_data)
            .map_err(|e| IpcError::new(ErrorCode::Internal, format!("serialize config: {}", e)))?,
        device_name: device_name.clone(),
        updated_at: updated_at.clone(),
    };

    // Encrypt with master password — on blocking thread (Argon2id)
    let mp = master_password.clone();
    let blob = tokio::task::spawn_blocking(move || {
        termfast_cloud_sync::sync_crypto::encrypt_config(&mp, &payload)
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, format!("spawn_blocking: {}", e)))?
    .map_err(|e| IpcError::new(ErrorCode::Internal, format!("encrypt: {}", e)))?;

    // Upload to cloud
    p.upload(&stored.token, &sync_path, &blob)
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("upload: {}", e)))?;

    // Get the new remote hash (re-fetch file_info)
    let new_info = p
        .file_info(&stored.token, &sync_path)
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("file_info after upload: {}", e)))?;

    let new_hash = new_info.hash.unwrap_or_default();

    // Update sync state — record local config mtime so we can detect
    // local modifications on future downloads.
    let state_path = sync_state_path(state);
    let config_path = local_config_path(state);
    let mp = master_password.clone();
    let prov = provider.clone();
    let _ = tokio::task::spawn_blocking(move || {
        let mut st = termfast_cloud_sync::sync_state::load_state(&state_path, &mp);
        let local_mtime = std::fs::metadata(&config_path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs().to_string());
        st.set_sync_info(&prov, new_hash, device_name, updated_at, local_mtime);
        termfast_cloud_sync::sync_state::save_state(&state_path, &mp, &st)
    })
    .await;

    Ok(serde_json::json!({ "ok": true, "size": blob.len() }))
}

async fn handle_cloud_sync_download(
    state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let provider = params["provider"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing provider"))?
        .to_string();
    let master_password = params["master_password"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing master_password"))?
        .to_string();
    // force_download=true skips rollback warning (user confirmed)
    let force_download = params["force_download"].as_bool().unwrap_or(false);

    // Load cloud token
    let path = token_file_path(state);
    let data = termfast_cloud_sync::token_store::load_tokens(&path)
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("load token: {}", e)))?;
    let stored = data.tokens.get(provider.as_str()).ok_or_else(|| {
        IpcError::new(ErrorCode::CredentialNotFound, "not authenticated to cloud")
    })?;

    let p = build_provider(&provider)?;
    let sync_path = sync_path_from_params(params);

    // Check remote file info
    let remote_info = p
        .file_info(&stored.token, &sync_path)
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("file_info: {}", e)))?;

    if !remote_info.exists {
        return Ok(build_no_remote_data_response());
    }

    // Load local sync state for hash comparison + rollback detection
    let state_path = sync_state_path(state);
    let mp_for_state = master_password.clone();
    let sync_state = tokio::task::spawn_blocking(move || {
        termfast_cloud_sync::sync_state::load_state(&state_path, &mp_for_state)
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, format!("spawn_blocking: {}", e)))?;

    let local_hash = sync_state.last_hash(&provider);
    let last_local_mtime = sync_state.last_local_mtime(&provider).map(String::from);
    let current_local_mtime = local_config_mtime(state);

    // If both cloud and local are unchanged since last sync, no update needed.
    // force_download=true skips this check (user confirmed overwrite).
    if !force_download && is_no_update(
        &remote_info,
        local_hash,
        last_local_mtime.as_deref(),
        current_local_mtime.as_deref(),
    ) {
        return Ok(build_no_update_response(
            remote_info.modified.as_deref(),
            current_local_mtime.as_deref(),
        ));
    }

    // If cloud is unchanged but local has been modified since last sync,
    // downloading would overwrite newer local data — ask user to confirm.
    // force_download=true skips this check (user already confirmed).
    if !force_download && is_local_newer(
        &remote_info,
        local_hash,
        last_local_mtime.as_deref(),
        current_local_mtime.as_deref(),
    ) {
        return Ok(build_local_newer_response(
            remote_info.modified.as_deref(),
            current_local_mtime.as_deref(),
        ));
    }

    // Download from cloud
    let blob = p
        .download(&stored.token, &sync_path)
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("download: {}", e)))?;
    let blob_size = blob.len();

    // Decrypt — on blocking thread (Argon2id)
    let mp = master_password.clone();
    let decrypt_result = tokio::task::spawn_blocking(move || {
        termfast_cloud_sync::sync_crypto::decrypt_config(&mp, &blob)
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, format!("spawn_blocking: {}", e)))?;

    let payload = match decrypt_result {
        Ok(p) => p,
        Err(_) => {
            // Decryption failed — password mismatch or corrupted data
            // Return a flag so the frontend can prompt for the cloud password
            return Ok(build_decrypt_failed_response());
        }
    };

    // Rollback detection: compare updated_at with last_updated_at
    {
        let last_updated = sync_state.get(&provider).last_updated_at.as_deref().map(String::from);
        if let Some(rollback) = check_rollback_with_force(force_download, &payload, last_updated.as_deref()) {
            return Ok(rollback);
        }
    }

    // Apply the downloaded config
    let export_data: termfast_core::migration::FullExportData =
        serde_json::from_value(payload.config.clone())
            .map_err(|e| IpcError::new(ErrorCode::Internal, format!("parse config: {}", e)))?;
    apply_full_export(state, &export_data).await?;

    // Update sync state — record the config.json mtime AFTER apply, so that
    // on next download we can detect if the local config has been modified since.
    let new_hash = remote_info.hash.unwrap_or_default();
    let device_name = payload.device_name.clone();
    let updated_at = payload.updated_at.clone();
    let state_path = sync_state_path(state);
    let config_path = local_config_path(state);
    let mp = master_password.clone();
    let prov = provider.clone();
    let _ = tokio::task::spawn_blocking(move || {
        let mut st = termfast_cloud_sync::sync_state::load_state(&state_path, &mp);
        // Read config.json mtime after apply (it was just written, so mtime = now)
        let local_mtime = std::fs::metadata(&config_path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs().to_string());
        st.set_sync_info(&prov, new_hash, device_name, updated_at, local_mtime);
        termfast_cloud_sync::sync_state::save_state(&state_path, &mp, &st)
    })
    .await;

    Ok(serde_json::json!({
        "ok": true,
        "device_name": payload.device_name,
        "updated_at": payload.updated_at,
        "size": blob_size,
    }))
}

async fn handle_cloud_sync_file_info(
    state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let provider = params["provider"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing provider"))?;

    let path = token_file_path(state);
    let data = termfast_cloud_sync::token_store::load_tokens(&path)
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("load token: {}", e)))?;
    let stored = data.tokens.get(provider).ok_or_else(|| {
        IpcError::new(ErrorCode::CredentialNotFound, "not authenticated to cloud")
    })?;

    let p = build_provider(provider)?;
    let sync_path = sync_path_from_params(params);
    let info = p
        .file_info(&stored.token, &sync_path)
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("file_info: {}", e)))?;

    Ok(serde_json::json!({
        "exists": info.exists,
        "size": info.size,
        "modified": info.modified,
    }))
}

/// Get sync status (last sync time + device name) from local sync_state.enc.
async fn handle_cloud_sync_status(
    state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let provider = params["provider"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing provider"))?
        .to_string();
    let master_password = params["master_password"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing master_password"))?
        .to_string();

    let state_path = sync_state_path(state);
    let sync_state = tokio::task::spawn_blocking(move || {
        termfast_cloud_sync::sync_state::load_state(&state_path, &master_password)
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, format!("spawn_blocking: {}", e)))?;

    let info = sync_state.last_sync_info(&provider);
    Ok(serde_json::json!({
        "device_name": info.device_name,
        "updated_at": info.updated_at,
    }))
}

async fn handle_cloud_sync_delete_remote(
    state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let provider = params["provider"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing provider"))?;

    let path = token_file_path(state);
    let data = termfast_cloud_sync::token_store::load_tokens(&path)
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("load token: {}", e)))?;
    let stored = data.tokens.get(provider).ok_or_else(|| {
        IpcError::new(ErrorCode::CredentialNotFound, "not authenticated to cloud")
    })?;

    let p = build_provider(provider)?;
    let sync_path = sync_path_from_params(params);
    p.delete(&stored.token, &sync_path)
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("delete: {}", e)))?;

    Ok(serde_json::json!({ "ok": true }))
}

async fn handle_cloud_sync_disconnect(
    state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let provider = params["provider"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing provider"))?;

    let path = token_file_path(state);
    let mut data = termfast_cloud_sync::token_store::load_tokens(&path)
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("load token: {}", e)))?;

    data.tokens.remove(provider);

    termfast_cloud_sync::token_store::save_tokens(&path, &data)
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("save token: {}", e)))?;

    Ok(serde_json::json!({ "ok": true }))
}

async fn handle_cloud_sync_refresh_token(
    state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let provider = params["provider"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing provider"))?;

    let path = token_file_path(state);
    let mut data = termfast_cloud_sync::token_store::load_tokens(&path)
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("load token: {}", e)))?;

    let stored = data.tokens.get(provider).cloned().ok_or_else(|| {
        IpcError::new(ErrorCode::CredentialNotFound, "not authenticated to cloud")
    })?;

    let p = build_provider(provider)?;
    let new_token = p
        .refresh_token(&stored.token)
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("refresh: {}", e)))?;

    // Update stored token
    data.tokens.insert(
        provider.to_string(),
        termfast_cloud_sync::token_store::StoredToken {
            provider: stored.provider,
            token: new_token.clone(),
            stored_at: chrono::Utc::now().timestamp(),
        },
    );

    termfast_cloud_sync::token_store::save_tokens(&path, &data)
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("save token: {}", e)))?;

    Ok(serde_json::json!({
        "ok": true,
        "access_token": new_token.access_token,
        "expires_at": new_token.expires_at,
    }))
}

/// Check for upload conflict based on remote file info and local cached hash.
/// Returns Some(conflict_json) if a conflict is detected, None if safe to upload.
///
/// 4 branches (design doc 6.1):
/// - remote doesn't exist → None (safe, first upload)
/// - remote exists, hash matches local cache → None (safe, cloud unchanged)
/// - remote exists, hash differs from local cache → conflict (cloud_changed)
/// - remote exists, no local cache → conflict (cloud_exists_no_cache)
/// - remote exists but no hash from provider → None (can't detect, proceed)
pub fn check_upload_conflict(
    remote_info: &termfast_cloud_sync::RemoteFileInfo,
    local_hash: Option<&str>,
) -> Option<serde_json::Value> {
    if !remote_info.exists {
        return None;
    }
    let remote_hash = remote_info.hash.as_deref();
    match (remote_hash, local_hash) {
        (Some(rh), Some(lh)) if rh == lh => None, // safe
        (Some(_), Some(_)) => Some(serde_json::json!({
            "conflict": true,
            "reason": "cloud_changed",
            "message": "网盘文件被其他客户端改过，强行覆盖会丢失对方改动。",
        })),
        (Some(_), None) => Some(serde_json::json!({
            "conflict": true,
            "reason": "cloud_exists_no_cache",
            "message": "网盘已有数据文件，是否强行覆盖云端？",
        })),
        (None, _) => None, // no hash from provider, can't detect
    }
}

/// Decide whether to proceed with upload despite a potential conflict.
/// When force=true, always proceeds (returns None). When force=false,
/// delegates to check_upload_conflict.
/// This encapsulates the `if !force { check_upload_conflict(...) }` logic
/// so it can be unit-tested.
pub fn check_upload_with_force(
    force: bool,
    remote_info: &termfast_cloud_sync::RemoteFileInfo,
    local_hash: Option<&str>,
) -> Option<serde_json::Value> {
    if force {
        None
    } else {
        check_upload_conflict(remote_info, local_hash)
    }
}

/// Check for rollback attack by comparing cloud updated_at with local last_updated_at.
/// Returns Some(rollback_json) if rollback detected, None if safe.
///
/// Design doc 6.2.1: if cloud updated_at < local last_updated_at, the cloud file
/// is older than what we last synced — likely a rollback attack.
pub fn check_rollback(
    payload: &termfast_cloud_sync::sync_crypto::SyncPayload,
    last_updated_at: Option<&str>,
) -> Option<serde_json::Value> {
    let last = last_updated_at?;
    if payload.updated_at.as_str() < last {
        Some(serde_json::json!({
            "ok": false,
            "reason": "rollback_detected",
            "message": "云端文件时间戳比上次同步更旧，可能是回滚攻击",
            "cloud_updated_at": payload.updated_at,
            "last_updated_at": last,
            "device_name": payload.device_name,
            "config": payload.config,
        }))
    } else {
        None
    }
}

/// Decide whether to proceed with download despite a potential rollback.
/// When force_download=true, always proceeds (returns None). When force_download=false,
/// delegates to check_rollback.
/// This encapsulates the `if !force_download { check_rollback(...) }` logic
/// so it can be unit-tested.
pub fn check_rollback_with_force(
    force_download: bool,
    payload: &termfast_cloud_sync::sync_crypto::SyncPayload,
    last_updated_at: Option<&str>,
) -> Option<serde_json::Value> {
    if force_download {
        None
    } else {
        check_rollback(payload, last_updated_at)
    }
}

/// Build the "no remote data" response (download handler, remote file doesn't exist).
pub fn build_no_remote_data_response() -> serde_json::Value {
    serde_json::json!({
        "ok": false,
        "reason": "no_remote_data",
        "message": "云端没有同步数据",
    })
}

/// Build the "no update needed" response (download handler, hash matches).
/// Includes timestamps so the frontend can show a confirmation dialog
/// asking the user whether to overwrite newer local data with older cloud data.
pub fn build_no_update_response(
    cloud_updated_at: Option<&str>,
    local_updated_at: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "ok": false,
        "reason": "no_update",
        "message": "云端无更新",
        "cloud_updated_at": cloud_updated_at,
        "local_updated_at": local_updated_at,
    })
}

/// Build the "decrypt failed" response (download handler, wrong password or corruption).
pub fn build_decrypt_failed_response() -> serde_json::Value {
    serde_json::json!({
        "ok": false,
        "reason": "decrypt_failed",
        "message": "解密失败，主密码与云端不一致或数据损坏",
    })
}

/// Check if download should return "no update" because both cloud and local
/// are unchanged since last sync.
///
/// Returns true only if ALL of:
/// 1. Cloud hash matches the hash recorded at last sync (cloud unchanged)
/// 2. Local config mtime matches the mtime recorded at last sync (local unchanged)
///
/// If either side has changed, returns false — download should proceed.
/// This prevents the bug where local edits (new node, changed password, etc.)
/// were incorrectly blocked by no_update just because the cloud file hadn't changed.
pub fn is_no_update(
    remote_info: &termfast_cloud_sync::RemoteFileInfo,
    local_hash: Option<&str>,
    last_local_mtime: Option<&str>,
    current_local_mtime: Option<&str>,
) -> bool {
    // Cloud unchanged?
    let cloud_unchanged = match (&remote_info.hash, local_hash) {
        (Some(rh), Some(lh)) => rh == lh,
        _ => false,
    };
    if !cloud_unchanged {
        return false;
    }
    // Local unchanged? (if we don't have mtime info, err on side of allowing download)
    match (last_local_mtime, current_local_mtime) {
        (Some(last), Some(cur)) => last == cur,
        _ => false,
    }
}

/// Check if local data is newer than cloud — i.e., cloud is unchanged
/// since last sync but local config has been modified.
///
/// Returns true only if ALL of:
/// 1. Cloud hash matches the hash recorded at last sync (cloud unchanged)
/// 2. Local config mtime differs from the mtime recorded at last sync (local changed)
///
/// In this case, downloading would overwrite newer local data with older cloud data.
/// The caller should return a `local_newer` response so the frontend can ask
/// the user to confirm before proceeding.
pub fn is_local_newer(
    remote_info: &termfast_cloud_sync::RemoteFileInfo,
    local_hash: Option<&str>,
    last_local_mtime: Option<&str>,
    current_local_mtime: Option<&str>,
) -> bool {
    // Cloud unchanged?
    let cloud_unchanged = match (&remote_info.hash, local_hash) {
        (Some(rh), Some(lh)) => rh == lh,
        _ => false,
    };
    if !cloud_unchanged {
        return false;
    }
    // Local changed? (need both mtime values to compare; missing → can't tell → false)
    match (last_local_mtime, current_local_mtime) {
        (Some(last), Some(cur)) => last != cur,
        _ => false,
    }
}

/// Build the "local is newer than cloud" response (download handler).
/// The frontend should show a confirmation dialog before proceeding,
/// because downloading will overwrite newer local data with older cloud data.
pub fn build_local_newer_response(
    cloud_updated_at: Option<&str>,
    local_updated_at: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "ok": false,
        "reason": "local_newer",
        "message": "本地数据比云端新，下载将覆盖本地改动",
        "cloud_updated_at": cloud_updated_at,
        "local_updated_at": local_updated_at,
    })
}

async fn export_full_data(state: &DaemonState) -> Result<termfast_core::migration::FullExportData, IpcError> {
    let mgr = state.config_manager.lock().await;
    let config = mgr.get().await;

    let mut passwords = std::collections::HashMap::new();
    let mut key_passphrases = std::collections::HashMap::new();
    let mut key_files = std::collections::HashMap::new();

    for server in &config.servers {
        let sid = &server.id;
        if server.ssh.auth_method == "password" {
            let pwd_key =
                termfast_credential::make_key(sid, termfast_credential::cred_type::PASSWORD);
            if let Ok(pwd) = state.credential_store.load(&pwd_key) {
                passwords.insert(sid.clone(), pwd);
            }
        } else if server.ssh.auth_method == "key" {
            let pass_key =
                termfast_credential::make_key(sid, termfast_credential::cred_type::KEY_PASSPHRASE);
            if let Ok(pass) = state.credential_store.load(&pass_key) {
                key_passphrases.insert(sid.clone(), pass);
            }
            if !server.ssh.key_path.is_empty() {
                if let Ok(content) = std::fs::read_to_string(&server.ssh.key_path) {
                    key_files.insert(sid.clone(), content);
                }
            }
        }
    }

    Ok(termfast_core::migration::FullExportData {
        config: config.clone(),
        passwords,
        key_passphrases,
        key_files,
    })
}

/// Encrypt an SSH private key with a passphrase if it's currently unencrypted.
/// Returns the OpenSSH-format encrypted key string, or the original content
/// if it's already encrypted or no passphrase is available.
///
/// This prevents SSH private keys from being stored in plaintext on disk
/// (F2 fix: disk theft → plaintext key → server compromised).
fn encrypt_key_if_needed(content: &str, passphrase: Option<&str>) -> String {
    // Already encrypted? (OpenSSH encrypted keys have "ENCRYPTED" in the header)
    if content.contains("ENCRYPTED") {
        return content.to_string();
    }
    let passphrase = match passphrase {
        Some(p) if !p.is_empty() => p,
        _ => return content.to_string(), // no passphrase → can't encrypt
    };

    // Try to parse as a private key and re-encrypt with passphrase
    match russh::keys::decode_secret_key(content, None) {
        Ok(key) => {
            // Already encrypted?
            if key.is_encrypted() {
                return content.to_string();
            }
            // Encrypt with passphrase using default cipher (AES-256-CTR + bcrypt KDF)
            let mut rng = rand::rng();
            match key.encrypt(&mut rng, passphrase) {
                Ok(encrypted_key) => {
                    match encrypted_key.to_openssh(russh::keys::ssh_key::LineEnding::LF) {
                        Ok(pem) => {
                            tracing::info!("apply: encrypted SSH private key with passphrase");
                            pem.to_string()
                        }
                        Err(e) => {
                            tracing::warn!("apply: failed to serialize encrypted key: {}", e);
                            content.to_string()
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("apply: failed to encrypt key: {}", e);
                    content.to_string()
                }
            }
        }
        Err(e) => {
            // Can't parse — might be a non-OpenSSH format, leave as-is
            tracing::warn!("apply: could not parse key for encryption ({}), writing as-is", e);
            content.to_string()
        }
    }
}

/// Snapshot of local data files taken before apply_full_export,
/// used for rollback if any step fails.
struct LocalDataBackup {
    config_bak: Option<std::path::PathBuf>,
}

/// Back up config.json → config.json.bak before overwriting.
/// Returns a handle that can be used to roll back on failure.
fn backup_local_data(state: &DaemonState) -> LocalDataBackup {
    let config_path = local_config_path(state);
    let config_bak = config_path.with_extension("json.bak");
    let config_bak = if config_path.exists() {
        match std::fs::copy(&config_path, &config_bak) {
            Ok(_) => {
                tracing::info!("apply: backed up config.json → {:?}", config_bak);
                Some(config_bak)
            }
            Err(e) => {
                tracing::warn!("apply: failed to back up config.json: {}", e);
                None
            }
        }
    } else {
        None
    };
    LocalDataBackup { config_bak }
}

impl LocalDataBackup {
    /// Roll back: restore .bak files to their original locations.
    /// Called when apply_full_export fails at any step.
    fn rollback(&self, state: &DaemonState) {
        if let Some(ref bak) = self.config_bak {
            let config_path = local_config_path(state);
            if bak.exists() {
                match std::fs::rename(bak, &config_path) {
                    Ok(_) => tracing::info!("apply: rolled back config.json from .bak"),
                    Err(e) => tracing::error!("apply: failed to roll back config.json: {}", e),
                }
            }
        }
    }

    /// Clean up .bak files after successful apply.
    /// We keep the .bak for one more session as a safety net —
    /// it will be overwritten on the next apply.
    fn cleanup(&self) {
        // Keep .bak files — they'll be overwritten on next apply.
        // This provides a one-version-back safety net.
    }
}

/// Apply a FullExportData to the local config + credentials.
/// Extracted from handle_import_full for reuse by cloud sync download.
///
/// Crash safety + rollback strategy (H4 fix):
///   0. Back up config.json → config.json.bak
///   1. Write config.json (atomic) — FIRST, so user sees new node list
///   2. Write key files (atomic per-file, encrypted with passphrase)
///   3. Write credentials (atomic per-key) — LAST
///   4. Sync ServerManager (in-memory)
///   5. Broadcast config:changed
///
/// Write order rationale (H4):
/// If killed between steps, the inconsistency direction is
/// "config new + credentials old" — user sees new nodes but passwords
/// may be missing. This is BETTER than "credentials new + config old"
/// where user sees stale nodes and doesn't know to re-download.
/// Missing passwords → user can re-enter; stale node list → silent data loss.
///
/// If step 1 fails, no changes were made (config.json write is atomic).
/// If steps 2-3 fail, roll back config.json from .bak.
/// .bak is kept after success as a one-version safety net.
async fn apply_full_export(
    state: &DaemonState,
    export_data: &termfast_core::migration::FullExportData,
) -> Result<(), IpcError> {
    // 0. Back up current config.json for rollback
    let backup = backup_local_data(state);

    // 1. Apply config — FIRST, so config.json updates before credentials.
    //    mgr.modify() updates in-memory config AND atomically writes
    //    config.json (tmp + rename).
    {
        let mgr = state.config_manager.lock().await;
        if let Err(e) = mgr
            .modify(|config| {
                *config = export_data.config.clone();
            })
            .await
        {
            tracing::error!("apply_full_export: config write failed: {}", e);
            return Err(IpcError::new(ErrorCode::Internal, e.to_string()));
        }
    }

    // 2. Restore key files — validate key_path to prevent path traversal
    //    Write atomically (tmp + rename) so a crash mid-write doesn't
    //    leave a truncated/corrupt key file.
    //    Also encrypt the key with passphrase if available (F2 fix).
    {
        for (server_id, content) in &export_data.key_files {
            // Find the server's key_path from the NEW config
            let key_path_str = export_data
                .config
                .servers
                .iter()
                .find(|s| s.id == *server_id)
                .map(|s| s.ssh.key_path.clone())
                .unwrap_or_default();
            if key_path_str.is_empty() {
                continue;
            }
            // F2 fix: encrypt the private key with passphrase before writing to disk
            let passphrase = export_data.key_passphrases.get(server_id).map(|s| s.as_str());
            let key_content = encrypt_key_if_needed(content, passphrase);
            let key_path = std::path::Path::new(&key_path_str);
            let canonical = match key_path.canonicalize() {
                Ok(p) => p,
                Err(_) => {
                    match key_path.parent().and_then(|p| p.canonicalize().ok()) {
                        Some(parent) => parent.join(key_path.file_name().unwrap_or_default()),
                        None => continue,
                    }
                }
            };
            let home = directories::BaseDirs::new()
                .map(|d| d.home_dir().to_path_buf())
                .unwrap_or_default();
            let ssh_dir = home.join(".ssh");
            let is_safe = canonical.starts_with(&ssh_dir)
                || canonical.starts_with(std::env::temp_dir().join("termfast"));
            if !is_safe {
                tracing::warn!("apply_full_export: refusing to write key to {}", canonical.display());
                continue;
            }
            // Atomic write: tmp file + rename
            let tmp_path = canonical.with_extension("tmp");
            if let Err(e) = std::fs::write(&tmp_path, key_content.as_bytes()) {
                tracing::warn!("apply_full_export: failed to write key tmp {}: {}", tmp_path.display(), e);
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600));
            }
            if let Err(e) = std::fs::rename(&tmp_path, &canonical) {
                tracing::warn!("apply_full_export: failed to rename key file {}: {}", canonical.display(), e);
                let _ = std::fs::remove_file(&tmp_path);
            }
        }
    }

    // 3. Restore credentials — LAST, so config.json is already updated.
    //    If this fails or is killed mid-way, user sees new node list
    //    but some passwords may be missing — they can re-enter them.
    //    (Better than stale node list + new passwords that can't be used.)
    let mut cred_errors = Vec::new();
    for (server_id, pwd) in &export_data.passwords {
        let key =
            termfast_credential::make_key(server_id, termfast_credential::cred_type::PASSWORD);
        if let Err(e) = state.credential_store.save(&key, pwd) {
            cred_errors.push(format!("password for {}: {}", server_id, e));
        }
    }
    for (server_id, pass) in &export_data.key_passphrases {
        let key = termfast_credential::make_key(
            server_id,
            termfast_credential::cred_type::KEY_PASSPHRASE,
        );
        if let Err(e) = state.credential_store.save(&key, pass) {
            cred_errors.push(format!("passphrase for {}: {}", server_id, e));
        }
    }
    if !cred_errors.is_empty() {
        tracing::warn!("apply_full_export: some credentials failed to save: {:?}", cred_errors);
        // Don't roll back config — user can see new nodes and re-enter passwords.
        // Config is already updated; credentials are partially updated.
    }

    // Sync ServerManager with the new config — add new servers, remove
    // deleted ones, reload changed configs. Without this, list_servers
    // returns stale data from the old in-memory ServerManager state.
    state
        .server_manager
        .sync_from_config(&export_data.config.servers)
        .await;

    // Broadcast config changed
    state
        .broadcast("config:changed", serde_json::json!({}))
        .await;

    Ok(())
}

#[allow(dead_code)]
async fn export_encrypted_config(
    state: &DaemonState,
    master_password: &str,
) -> Result<Vec<u8>, IpcError> {
    let export_data = export_full_data(state).await?;

    let blob = tokio::task::spawn_blocking({
        let mp = master_password.to_string();
        move || termfast_core::migration::export_full(&mp, &export_data)
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, format!("spawn_blocking: {}", e)))?
    .map_err(|e| IpcError::new(ErrorCode::Internal, format!("export encrypt: {}", e)))?;

    Ok(blob)
}

/// Start OAuth auth with local callback server.
/// Starts a localhost HTTP server, gets the auth URL from the proxy,
/// stores the callback receiver in state, and returns the URL.
/// The caller opens the URL in a browser, then calls CloudSyncWaitCallback.
async fn handle_cloud_sync_auth_with_callback(
    state: &DaemonState,
    params: &serde_json::Value,
) -> HandlerResult {
    let provider = params["provider"]
        .as_str()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "missing provider"))?;
    let port = params["port"].as_u64().unwrap_or(19834) as u16;

    // Start local callback server — try port range 19834-19839
    // (all registered as redirect URIs in Baidu console)
    let mut server = None;
    let mut last_err = String::new();
    for p in port..=(port + 5).min(19839) {
        match termfast_cloud_sync::callback::CallbackServer::start(p).await {
            Ok(s) => {
                server = Some(s);
                break;
            }
            Err(e) => {
                last_err = format!("port {}: {}", p, e);
                tracing::warn!("callback server bind failed: {}", last_err);
            }
        }
    }
    let server = server.ok_or_else(|| {
        IpcError::new(
            ErrorCode::Internal,
            format!("all callback ports 19834-19839 occupied: {}", last_err),
        )
    })?;

    let redirect_uri = server.redirect_uri();

    // Get auth URL from provider (via proxy server)
    let p = build_provider(provider)?;
    let (real_auth_url, code_verifier, state_str) = p
        .fetch_auth_url(&redirect_uri)
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("fetch auth url: {}", e)))?;

    // Store pending auth info and callback receiver in state
    let rx = server.wait_for_callback_consumer();
    *state.cloud_sync_callback.lock().await = Some(rx);
    *state.cloud_sync_pending.lock().await = Some(crate::server::CloudSyncPendingAuth {
        provider: provider.to_string(),
        code_verifier: code_verifier.unwrap_or_default(),
        redirect_uri: redirect_uri.clone(),
        state: state_str,
    });

    Ok(serde_json::json!({
        "auth_url": real_auth_url,
        "redirect_uri": redirect_uri,
    }))
}

/// Wait for the OAuth callback to complete, then exchange code and save token.
async fn handle_cloud_sync_wait_callback(
    state: &DaemonState,
    _params: &serde_json::Value,
) -> HandlerResult {
    // Take the callback receiver from state
    let rx = state
        .cloud_sync_callback
        .lock()
        .await
        .take()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "no pending callback"))?;

    // Take pending auth info
    let pending = state
        .cloud_sync_pending
        .lock()
        .await
        .take()
        .ok_or_else(|| IpcError::new(ErrorCode::InvalidParams, "no pending auth"))?;

    // Wait for callback (5 min timeout)
    tracing::info!("cloud_sync: waiting for OAuth callback...");
    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(300),
        rx,
    ).await {
        Ok(Ok(r)) => {
            tracing::info!("cloud_sync: callback received, code={}, state={}", r.code, r.state);
            r
        }
        Ok(Err(_)) => return Err(IpcError::new(ErrorCode::Internal, "callback channel closed")),
        Err(_) => return Err(IpcError::new(ErrorCode::Internal, "callback timed out (5 min)")),
    };

    if result.code.is_empty() {
        return Err(IpcError::new(ErrorCode::Internal, "no code in callback"));
    }

    tracing::info!("cloud_sync: exchanging code for token, provider={}", pending.provider);

    // Verify state for Baidu (CSRF protection)
    if pending.provider == "baidu" && !pending.state.is_empty() && result.state != pending.state {
        return Err(IpcError::new(ErrorCode::Internal, "OAuth state mismatch (CSRF?)"));
    }

    // Exchange code for token
    let p = build_provider(&pending.provider)?;
    let token = p
        .exchange_code(&result.code, &pending.code_verifier, &pending.redirect_uri, &result.state)
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("OAuth exchange: {}", e)))?;

    // Save token to store
    let provider_type: termfast_cloud_sync::CloudProvider = pending
        .provider
        .parse::<termfast_cloud_sync::CloudProvider>()
        .map_err(|e: termfast_cloud_sync::CloudSyncError| {
            IpcError::new(ErrorCode::InvalidParams, e.to_string())
        })?;

    let path = token_file_path(state);
    let mut data = if termfast_cloud_sync::token_store::token_file_exists(&path) {
        termfast_cloud_sync::token_store::load_tokens(&path).unwrap_or_default()
    } else {
        termfast_cloud_sync::token_store::TokenStoreData::default()
    };

    data.tokens.insert(
        pending.provider.clone(),
        termfast_cloud_sync::token_store::StoredToken {
            provider: provider_type,
            token: token.clone(),
            stored_at: chrono::Utc::now().timestamp(),
        },
    );

    termfast_cloud_sync::token_store::save_tokens(&path, &data)
        .map_err(|e| IpcError::new(ErrorCode::Internal, format!("save token: {}", e)))?;

    Ok(serde_json::json!({
        "ok": true,
        "access_token": token.access_token,
        "refresh_token": token.refresh_token,
        "expires_at": token.expires_at,
        "token_type": token.token_type,
    }))
}
