//! TermFast Tauri App — main entry point
//!
//! Embeds the daemon and provides IPC bridge to the React frontend.
//! All IPC commands forward to the daemon handler (FP-6.2) to ensure
//! events are broadcast to both CLI and GUI clients.

mod daemon_embed;
mod credential_manager;

use credential_manager::{credential_file_path, CredentialState};
use daemon_embed::EmbeddedDaemon;
use std::sync::Arc;
use tauri::Manager;

/// Shared embedded daemon — None until async startup completes
pub struct AppState {
    pub daemon: tokio::sync::Mutex<Option<Arc<EmbeddedDaemon>>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize tracing to stderr (visible in `npm start` terminal)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("termfast_app=info".parse().unwrap())
                .add_directive("termfast_daemon=info".parse().unwrap())
                .add_directive("termfast_core=info".parse().unwrap())
                .add_directive("keychain=debug".parse().unwrap()),
        )
        .with_writer(std::io::stderr)
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["com.termfast.app"]),
        ))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_network::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .on_window_event(|window, event| {
            // macOS HIG: clicking the red close button only closes the window,
            // the app stays alive in the menu bar / Dock. The user quits via
            // Cmd+Q or the tray menu "Quit".  On non-macOS platforms the
            // close button exits the app (unless minimize_to_tray is on).
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app_handle = window.app_handle().clone();
                api.prevent_close();

                tauri::async_runtime::spawn(async move {
                    #[cfg(target_os = "macos")]
                    {
                        // macOS: always hide window, never exit on close
                        if let Some(win) = app_handle.get_webview_window("main") {
                            let _ = win.hide();
                        }
                        return;
                    }

                    #[cfg(not(target_os = "macos"))]
                    {
                        // Non-macOS: check minimize_to_tray setting
                        let minimize_to_tray = if let Some(state) =
                            app_handle.try_state::<AppState>()
                        {
                            let guard = state.daemon.lock().await;
                            if let Some(ref daemon) = *guard {
                                let mgr = daemon.server.state().config_manager.lock().await;
                                let config = mgr.get().await;
                                config.general.minimize_to_tray
                            } else {
                                false
                            }
                        } else {
                            false
                        };

                        if minimize_to_tray {
                            if let Some(win) = app_handle.get_webview_window("main") {
                                let _ = win.hide();
                            }
                            return;
                        }

                        // Graceful shutdown
                        if let Some(state) = app_handle.try_state::<AppState>() {
                            tracing::info!("window close: starting graceful shutdown");
                            let guard = state.daemon.lock().await;
                            if let Some(ref daemon) = *guard {
                                daemon.server.shutdown().await;
                            }
                            tracing::info!("graceful shutdown complete, exiting");
                        }
                        app_handle.exit(0);
                    }
                });
            }
        })
        .setup(|app| {
            // Open DevTools in dev mode for debugging
            #[cfg(debug_assertions)]
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.open_devtools();
            }

            // Apply window vibrancy effect (FP-6.10)
            if let Some(window) = app.get_webview_window("main") {
                let adapter = termfast_desktop::platform::get_platform_adapter();
                if let Err(e) = adapter.apply_window_effect(&window) {
                    tracing::warn!("failed to apply window effect: {}", e);
                }
            }

            // Setup system tray icon (FP-6.4, FP-6.5)
            setup_tray(app)?;

            // Create the encrypted credential store early so both the daemon
            // and IPC commands can share it. The store starts locked; the
            // frontend will call ipc_try_cached_unlock / ipc_unlock_credentials
            // before any credential access.
            let cred_path = credential_file_path();
            tracing::info!("credential file path: {}", cred_path.display());
            let cred_store = Arc::new(
                termfast_credential::EncryptedFileCredentialStore::open(cred_path),
            );
            app.manage(CredentialState {
                store: cred_store.clone(),
            });

            // Start embedded daemon in background, passing the shared credential store.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                match EmbeddedDaemon::start_with_credential_store(cred_store).await {
                    Ok(daemon) => {
                        // Set up event forwarder: daemon events → Tauri emit to frontend (FP-6.2)
                        let handle_for_forwarder = handle.clone();
                        daemon.server.state().set_event_forwarder(Box::new(
                            move |event: &str, data: serde_json::Value| {
                                use tauri::Emitter;
                                if let Err(e) = handle_for_forwarder.emit(event, data) {
                                    tracing::warn!("failed to emit event {}: {}", event, e);
                                }
                            },
                        ));

                        let state = AppState {
                            daemon: tokio::sync::Mutex::new(Some(Arc::new(daemon))),
                        };
                        handle.manage(state);
                        tracing::info!("Tauri app state initialized with event forwarding");
                    }
                    Err(e) => {
                        tracing::error!("failed to start embedded daemon: {}", e);
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc_get_config,
            ipc_update_general_config,
            ipc_list_servers,
            ipc_connect_server,
            ipc_accept_host_key,
            ipc_disconnect_server,
            ipc_add_server,
            ipc_remove_server,
            ipc_reorder_servers,
            ipc_update_server,
            ipc_toggle_proxy,
            ipc_get_proxy_status,
            ipc_get_logs,
            ipc_clear_logs,
            ipc_pause_all_triggers,
            ipc_resume_all_triggers,
            ipc_manual_fire_trigger,
            ipc_list_templates,
            ipc_create_template,
            ipc_update_template,
            ipc_delete_template,
            ipc_export_templates,
            ipc_import_templates,
            ipc_save_credential,
            ipc_get_daemon_status,
            ipc_shutdown,
            // Trigger management (FP-6.1)
            ipc_list_triggers,
            ipc_add_trigger,
            ipc_update_trigger,
            ipc_remove_trigger,
            ipc_add_trigger_from_template,
            // System proxy (FP-6.6)
            ipc_set_system_proxy,
            ipc_clear_system_proxy,
            ipc_test_proxy,
            // Auth (FP-6.1)
            ipc_switch_auth_method,
            ipc_generate_ssh_key,
            // Onboarding helpers (FP-8.1)
            ipc_check_port_reachable,
            ipc_detect_firewall,
            ipc_test_connection,
            // Network status (FP-6.9)
            ipc_get_network_status,
            // Export/Import (FP-1.6)
            ipc_export_full,
            ipc_import_full,
            // Autostart (FP-6.5 / M1 fix)
            ipc_set_autostart,
            ipc_get_autostart,
            ipc_send_notification,
            // Terminal — interactive SSH shell sessions
            ipc_terminal_open,
            ipc_terminal_input,
            ipc_terminal_close,
            ipc_terminal_resize,
            // Quit app from tray menu (forces exit even if minimize_to_tray is on)
            ipc_quit_app,
            // Credential encryption management
            credential_manager::ipc_credential_status,
            credential_manager::ipc_initialize_credentials,
            credential_manager::ipc_unlock_credentials,
            credential_manager::ipc_try_cached_unlock,
            credential_manager::ipc_lock_credentials,
            credential_manager::ipc_migrate_credentials,
            credential_manager::ipc_change_credential_password,
            credential_manager::ipc_reset_credentials,
            credential_manager::ipc_export_credentials,
            credential_manager::ipc_import_credentials,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// === SECTION 1 END ===

/// Helper: forward a request to the daemon handler and return the result.
/// All IPC commands go through this to ensure events are broadcast (FP-6.2).
async fn forward_to_daemon(
    state: &tauri::State<'_, AppState>,
    action: termfast_daemon::proto::Action,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    // Daemon may not be ready yet (async startup) — retry briefly
    for _attempt in 0..20 {
        let guard = state.daemon.lock().await;
        if let Some(ref daemon) = *guard {
            let req = termfast_daemon::proto::Request::new(action, params);
            let resp = termfast_daemon::handler::handle_request(&req, daemon.server.state()).await;
            match resp {
                termfast_daemon::proto::Response::Ok { data, .. } => return Ok(data),
                termfast_daemon::proto::Response::Err { error, .. } => {
                    // Serialize as JSON object {code, detail} so the frontend
                    // can parse the ErrorCode and render a localized message.
                    return Err(serde_json::to_string(&error)
                        .unwrap_or_else(|_| format!("{:?}: {}", error.code, error.detail)));
                }
                termfast_daemon::proto::Response::Event { .. } => {
                    return Err("unexpected event response".to_string());
                }
            }
        }
        drop(guard);
        // Daemon not ready yet, wait and retry
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Err("daemon not ready after 2s".to_string())
}

// === SECTION 2 END ===

#[tauri::command]
async fn ipc_get_config(state: tauri::State<'_, AppState>) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::GetConfig,
        serde_json::json!({}),
    )
    .await
}

#[tauri::command]
async fn ipc_update_general_config(
    state: tauri::State<'_, AppState>,
    theme: Option<String>,
    language: Option<String>,
    auto_start: Option<bool>,
    minimize_to_tray: Option<bool>,
    log_level: Option<String>,
    log_to_file: Option<bool>,
    log_max_days: Option<u32>,
    log_max_size_mb: Option<u32>,
    custom_variables: Option<Vec<serde_json::Value>>,
) -> Result<serde_json::Value, String> {
    let mut params = serde_json::json!({});
    if let Some(v) = theme {
        params["theme"] = serde_json::json!(v);
    }
    if let Some(v) = language {
        params["language"] = serde_json::json!(v);
    }
    if let Some(v) = auto_start {
        params["auto_start"] = serde_json::json!(v);
    }
    if let Some(v) = minimize_to_tray {
        params["minimize_to_tray"] = serde_json::json!(v);
    }
    if let Some(v) = log_level {
        params["log_level"] = serde_json::json!(v);
    }
    if let Some(v) = log_to_file {
        params["log_to_file"] = serde_json::json!(v);
    }
    if let Some(v) = log_max_days {
        params["log_max_days"] = serde_json::json!(v);
    }
    if let Some(v) = log_max_size_mb {
        params["log_max_size_mb"] = serde_json::json!(v);
    }
    if let Some(v) = custom_variables {
        params["custom_variables"] = serde_json::json!(v);
    }
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::UpdateGeneralConfig,
        params,
    )
    .await
}

#[tauri::command]
async fn ipc_list_servers(state: tauri::State<'_, AppState>) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::ListServers,
        serde_json::json!({}),
    )
    .await
}

#[tauri::command]
async fn ipc_connect_server(
    state: tauri::State<'_, AppState>,
    server_id: String,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::ConnectServer,
        serde_json::json!({ "server_id": server_id }),
    )
    .await
}

#[tauri::command]
async fn ipc_accept_host_key(
    state: tauri::State<'_, AppState>,
    server_id: String,
    fingerprint: String,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::AcceptHostKey,
        serde_json::json!({ "server_id": server_id, "fingerprint": fingerprint }),
    )
    .await
}

#[tauri::command]
async fn ipc_disconnect_server(
    state: tauri::State<'_, AppState>,
    server_id: String,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::DisconnectServer,
        serde_json::json!({ "server_id": server_id }),
    )
    .await
}

#[tauri::command]
async fn ipc_add_server(
    state: tauri::State<'_, AppState>,
    config: serde_json::Value,
) -> Result<String, String> {
    let result =
        forward_to_daemon(&state, termfast_daemon::proto::Action::AddServer, config).await?;
    result["server_id"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "missing server_id in response".to_string())
}

#[tauri::command]
async fn ipc_remove_server(
    state: tauri::State<'_, AppState>,
    server_id: String,
) -> Result<(), String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::RemoveServer,
        serde_json::json!({ "server_id": server_id }),
    )
    .await?;
    Ok(())
}

#[tauri::command]
async fn ipc_reorder_servers(
    state: tauri::State<'_, AppState>,
    server_ids: Vec<String>,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::ReorderServers,
        serde_json::json!({ "server_ids": server_ids }),
    )
    .await
}

#[tauri::command]
async fn ipc_update_server(
    state: tauri::State<'_, AppState>,
    server_id: String,
    name: Option<String>,
    socks5_port: Option<u16>,
    http_port: Option<u16>,
    mixed_port: Option<u16>,
    ssh: Option<serde_json::Value>,
    auto_reconnect: Option<bool>,
    reconnect_timeout_secs: Option<u64>,
) -> Result<serde_json::Value, String> {
    let mut params = serde_json::json!({ "server_id": server_id });
    if let Some(n) = name {
        params["name"] = serde_json::json!(n);
    }
    if let Some(p) = socks5_port {
        params["socks5_port"] = serde_json::json!(p);
    }
    if let Some(p) = http_port {
        params["http_port"] = serde_json::json!(p);
    }
    if let Some(p) = mixed_port {
        params["mixed_port"] = serde_json::json!(p);
    }
    if let Some(s) = ssh {
        params["ssh"] = s;
    }
    if let Some(v) = auto_reconnect {
        params["auto_reconnect"] = serde_json::json!(v);
    }
    if let Some(v) = reconnect_timeout_secs {
        params["reconnect_timeout_secs"] = serde_json::json!(v);
    }
    forward_to_daemon(&state, termfast_daemon::proto::Action::UpdateServer, params).await
}

#[tauri::command]
async fn ipc_toggle_proxy(
    state: tauri::State<'_, AppState>,
    server_id: String,
    enabled: bool,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::ToggleProxy,
        serde_json::json!({ "server_id": server_id, "enabled": enabled }),
    )
    .await
}

#[tauri::command]
async fn ipc_get_proxy_status(
    state: tauri::State<'_, AppState>,
    server_id: String,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::GetProxyStatus,
        serde_json::json!({ "server_id": server_id }),
    )
    .await
}

#[tauri::command]
async fn ipc_get_logs(
    state: tauri::State<'_, AppState>,
    server_id: Option<String>,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::GetLogs,
        serde_json::json!({ "server_id": server_id, "limit": limit }),
    )
    .await
}

#[tauri::command]
async fn ipc_clear_logs(state: tauri::State<'_, AppState>) -> Result<(), String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::ClearLogs,
        serde_json::json!({}),
    )
    .await?;
    Ok(())
}

#[tauri::command]
async fn ipc_pause_all_triggers(state: tauri::State<'_, AppState>) -> Result<(), String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::PauseAllTriggers,
        serde_json::json!({}),
    )
    .await?;
    Ok(())
}

#[tauri::command]
async fn ipc_resume_all_triggers(state: tauri::State<'_, AppState>) -> Result<(), String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::ResumeAllTriggers,
        serde_json::json!({}),
    )
    .await?;
    Ok(())
}

#[tauri::command]
async fn ipc_manual_fire_trigger(
    state: tauri::State<'_, AppState>,
    server_id: String,
    trigger_id: String,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::ManualFireTrigger,
        serde_json::json!({ "server_id": server_id, "trigger_id": trigger_id }),
    )
    .await
}

#[tauri::command]
async fn ipc_list_templates(
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::ListTemplates,
        serde_json::json!({}),
    )
    .await
}

#[tauri::command]
async fn ipc_create_template(
    state: tauri::State<'_, AppState>,
    template: serde_json::Value,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::CreateTemplate,
        template,
    )
    .await
}

#[tauri::command]
async fn ipc_update_template(
    state: tauri::State<'_, AppState>,
    template_id: String,
    template: serde_json::Value,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::UpdateTemplate,
        serde_json::json!({ "template_id": template_id, "template": template }),
    )
    .await
}

#[tauri::command]
async fn ipc_delete_template(
    state: tauri::State<'_, AppState>,
    template_id: String,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::DeleteTemplate,
        serde_json::json!({ "template_id": template_id }),
    )
    .await
}

#[tauri::command]
async fn ipc_export_templates(
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::ExportTemplates,
        serde_json::json!({}),
    )
    .await
}

#[tauri::command]
async fn ipc_import_templates(
    state: tauri::State<'_, AppState>,
    templates: serde_json::Value,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::ImportTemplates,
        serde_json::json!({ "templates": templates }),
    )
    .await
}

#[tauri::command]
async fn ipc_save_credential(
    state: tauri::State<'_, AppState>,
    server_id: String,
    credential_type: String,
    value: String,
) -> Result<(), String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::SaveCredential,
        serde_json::json!({ "server_id": server_id, "credential_type": credential_type, "value": value }),
    ).await?;
    Ok(())
}

#[tauri::command]
async fn ipc_get_daemon_status(
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::GetDaemonStatus,
        serde_json::json!({}),
    )
    .await
}

#[tauri::command]
async fn ipc_shutdown(state: tauri::State<'_, AppState>) -> Result<(), String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::Shutdown,
        serde_json::json!({}),
    )
    .await?;
    Ok(())
}

// === SECTION 3 END ===

/// Quit the app gracefully (from tray menu "Quit").
/// Performs daemon shutdown then exits, bypassing minimize_to_tray.
#[tauri::command]
async fn ipc_quit_app(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    tracing::info!("quit requested from tray menu");
    // Graceful daemon shutdown
    let guard = state.daemon.lock().await;
    if let Some(ref daemon) = *guard {
        daemon.server.shutdown().await;
    }
    drop(guard);
    tracing::info!("graceful shutdown complete, exiting");
    app_handle.exit(0);
    Ok(())
}

// === Trigger management IPC (FP-6.1) ===

#[tauri::command]
async fn ipc_list_triggers(
    state: tauri::State<'_, AppState>,
    server_id: String,
) -> Result<serde_json::Value, String> {
    // Read triggers from server config via handler
    let config = forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::GetConfig,
        serde_json::json!({}),
    )
    .await?;
    let servers = config["servers"].as_array().ok_or("invalid config")?;
    let server = servers
        .iter()
        .find(|s| s["id"] == server_id)
        .ok_or("server not found")?;
    Ok(server["triggers"].clone())
}

#[tauri::command]
async fn ipc_add_trigger(
    state: tauri::State<'_, AppState>,
    server_id: String,
    trigger: serde_json::Value,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::AddTrigger,
        serde_json::json!({ "server_id": server_id, "trigger": trigger }),
    )
    .await
}

#[tauri::command]
async fn ipc_update_trigger(
    state: tauri::State<'_, AppState>,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::UpdateTrigger,
        params,
    )
    .await
}

#[tauri::command]
async fn ipc_remove_trigger(
    state: tauri::State<'_, AppState>,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::RemoveTrigger,
        params,
    )
    .await
}

#[tauri::command]
async fn ipc_add_trigger_from_template(
    state: tauri::State<'_, AppState>,
    server_id: String,
    template_id: String,
) -> Result<serde_json::Value, String> {
    // Find template, create trigger instance, add to server config
    let config = forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::GetConfig,
        serde_json::json!({}),
    )
    .await?;
    let templates = config["trigger_templates"]
        .as_array()
        .ok_or("invalid config")?;
    let template = templates
        .iter()
        .find(|t| t["id"] == template_id)
        .ok_or_else(|| format!("template {} not found", template_id))?;
    let trigger = serde_json::json!({
        "id": format!("trig_{}", chrono::Utc::now().timestamp_millis()),
        "template_id": template["id"],
        "name": template["name"],
        "enabled": true,
        "parameters": {},
        "commands": template["commands"],
        "timeout_secs": template["timeout_secs"],
        "cooldown_secs": 0,
        "continue_on_error": false,
        "notify_on_success": false,
        "notify_on_failure": true,
        "last_fired_at": null,
        "template_hash_at_addition": template["template_hash"],
    });
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::AddTrigger,
        serde_json::json!({ "server_id": server_id, "trigger": trigger }),
    )
    .await
}

// === System proxy IPC (FP-6.6) ===

#[tauri::command]
async fn ipc_set_system_proxy(
    state: tauri::State<'_, AppState>,
    server_id: String,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::SetSystemProxy,
        serde_json::json!({ "server_id": server_id }),
    )
    .await
}

#[tauri::command]
async fn ipc_clear_system_proxy(
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::ClearSystemProxy,
        serde_json::json!({}),
    )
    .await
}

#[tauri::command]
async fn ipc_test_proxy(
    state: tauri::State<'_, AppState>,
    server_id: String,
    url: Option<String>,
) -> Result<serde_json::Value, String> {
    let mut params = serde_json::json!({ "server_id": server_id });
    if let Some(u) = url {
        params["url"] = serde_json::json!(u);
    }
    forward_to_daemon(&state, termfast_daemon::proto::Action::TestProxy, params).await
}

// === Auth IPC (FP-6.1) ===

#[tauri::command]
async fn ipc_switch_auth_method(
    state: tauri::State<'_, AppState>,
    server_id: String,
    auth_method: String,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::SwitchAuthMethod,
        serde_json::json!({ "server_id": server_id, "auth_method": auth_method }),
    )
    .await
}

#[tauri::command]
async fn ipc_generate_ssh_key(
    state: tauri::State<'_, AppState>,
    _key_type: String,
    comment: String,
) -> Result<serde_json::Value, String> {
    use termfast_core::ssh::auth;
    let safe_id = comment.replace(['@', '.', ':', '/'], "_");
    let (key_path, _pub_key, passphrase) =
        auth::generate_keypair(&safe_id).map_err(|e| e.to_string())?;
    let cred_key =
        termfast_credential::make_key(&safe_id, termfast_credential::cred_type::KEY_PASSPHRASE);
    let guard = state.daemon.lock().await;
    if let Some(ref daemon) = *guard {
        let _ = daemon
            .server
            .state()
            .credential_store
            .save(&cred_key, &passphrase);
    }
    Ok(serde_json::json!({ "key_path": key_path.to_string_lossy() }))
}

// === Onboarding helpers (FP-8.1) ===

#[tauri::command]
async fn ipc_test_connection(
    _state: tauri::State<'_, AppState>,
    host: String,
    port: u16,
    username: String,
    auth_method: String,
    password: Option<String>,
    key_path: Option<String>,
) -> Result<serde_json::Value, String> {
    use termfast_core::ssh::auth::AuthMethod;
    use termfast_core::ssh::client::{SshClientConfig, SshClientHandle};

    let config = SshClientConfig {
        host: host.clone(),
        port,
        user: username.clone(),
        heartbeat_interval: 15,
        max_attempts: 1,
        initial_backoff_secs: 1,
        max_backoff_secs: 5,
        skip_hostkey_verify: true,
        known_host_key: None,
        hostkey_mismatch_callback: None,
        socket_protector: None,
    };

    let client = SshClientHandle::new(config);

    let auth = if auth_method == "key" {
        let kp = key_path.unwrap_or_default();
        if kp.is_empty() {
            return Err("key_path is required for key auth".to_string());
        }
        AuthMethod::Key {
            key_path: kp,
            passphrase: None,
        }
    } else {
        let pw = password.unwrap_or_default();
        if pw.is_empty() {
            return Err("password is required for password auth".to_string());
        }
        AuthMethod::Password { password: zeroize::Zeroizing::new(pw) }
    };

    match client.connect(&auth).await {
        Ok(()) => {
            // Disconnect cleanly
            let _ = client.disconnect().await;
            Ok(serde_json::json!({ "success": true, "message": "Connection successful" }))
        }
        Err(e) => {
            let msg = e.to_string();
            Ok(serde_json::json!({ "success": false, "message": msg }))
        }
    }
}

#[tauri::command]
async fn ipc_check_port_reachable(
    _state: tauri::State<'_, AppState>,
    host: String,
    port: u16,
) -> Result<serde_json::Value, String> {
    use tokio::net::TcpStream;
    use tokio::time::Duration;
    let start = std::time::Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        TcpStream::connect((host.as_str(), port)),
    )
    .await;
    match result {
        Ok(Ok(_stream)) => {
            Ok(serde_json::json!({ "reachable": true, "latency_ms": start.elapsed().as_millis() }))
        }
        _ => {
            Ok(serde_json::json!({ "reachable": false, "latency_ms": start.elapsed().as_millis() }))
        }
    }
}

/// Detect firewall type and protected ports via SSH exec (FP-8.1)
#[tauri::command]
async fn ipc_detect_firewall(
    state: tauri::State<'_, AppState>,
    server_id: String,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::DetectFirewall,
        serde_json::json!({ "server_id": server_id }),
    )
    .await
}

// === Network status IPC (FP-6.9) ===

#[tauri::command]
async fn ipc_get_network_status(_app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    use tauri_plugin_network::network::utils::get_non_empty_interfaces;
    let interface_count = get_non_empty_interfaces().map(|v| v.len()).unwrap_or(0);
    let can_reach_internet = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        tokio::net::TcpStream::connect(("1.1.1.1", 53)),
    )
    .await
    .is_ok();
    Ok(serde_json::json!({ "online": can_reach_internet, "interface_count": interface_count }))
}

#[tauri::command]
async fn ipc_send_notification(
    app_handle: tauri::AppHandle,
    title: String,
    body: String,
) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;
    app_handle
        .notification()
        .builder()
        .title(&title)
        .body(&body)
        .show()
        .map_err(|e| e.to_string())?;
    Ok(())
}

// === Export/Import IPC (FP-1.6) ===

#[tauri::command]
async fn ipc_export_full(
    state: tauri::State<'_, AppState>,
    master_password: String,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::ExportFull,
        serde_json::json!({ "master_password": master_password }),
    )
    .await
}

#[tauri::command]
async fn ipc_import_full(
    state: tauri::State<'_, AppState>,
    master_password: String,
    blob: String,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::ImportFull,
        serde_json::json!({ "master_password": master_password, "blob": blob }),
    )
    .await
}

// === Autostart IPC (FP-6.5 / M1 fix) ===

#[tauri::command]
async fn ipc_set_autostart(app_handle: tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app_handle.autolaunch();
    if enabled {
        manager.enable().map_err(|e| e.to_string())?;
    } else {
        manager.disable().map_err(|e| e.to_string())?;
    }
    Ok(enabled)
}

#[tauri::command]
async fn ipc_get_autostart(app_handle: tauri::AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app_handle.autolaunch();
    manager.is_enabled().map_err(|e| e.to_string())
}

// === Tray icon setup (FP-6.4, FP-6.5) ===

/// Setup system tray icon (menu is built dynamically from the frontend)
fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    // Create tray icon — the menu is managed from the frontend via
    // @tauri-apps/api/tray and @tauri-apps/api/menu so it can use i18n
    // and dynamic server lists with submenus.
    let icon = create_tray_icon(termfast_desktop::tray::TrayIconColor::Gray);
    let _tray = TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .tooltip("TermFast")
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?;

    tracing::info!("system tray icon created");
    Ok(())
}

// === SECTION: Terminal IPC commands ===

#[tauri::command]
async fn ipc_terminal_open(
    state: tauri::State<'_, AppState>,
    server_id: String,
    cols: Option<u64>,
    rows: Option<u64>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "server_id": server_id,
        "cols": cols.unwrap_or(80),
        "rows": rows.unwrap_or(24),
    });
    forward_to_daemon(&state, termfast_daemon::proto::Action::TerminalOpen, params).await
}

#[tauri::command]
async fn ipc_terminal_input(
    state: tauri::State<'_, AppState>,
    session_id: String,
    data: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "session_id": session_id,
        "data": data,
    });
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::TerminalInput,
        params,
    )
    .await
}

#[tauri::command]
async fn ipc_terminal_close(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "session_id": session_id,
    });
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::TerminalClose,
        params,
    )
    .await
}

#[tauri::command]
async fn ipc_terminal_resize(
    state: tauri::State<'_, AppState>,
    session_id: String,
    cols: u64,
    rows: u64,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "session_id": session_id,
        "cols": cols,
        "rows": rows,
    });
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::TerminalResize,
        params,
    )
    .await
}

/// Create a tray icon image based on color
fn create_tray_icon(color: termfast_desktop::tray::TrayIconColor) -> tauri::image::Image<'static> {
    // Generate a simple 22x22 colored circle as PNG
    let (r, g, b) = match color {
        termfast_desktop::tray::TrayIconColor::Green => (34, 197, 94),
        termfast_desktop::tray::TrayIconColor::Yellow => (234, 179, 8),
        termfast_desktop::tray::TrayIconColor::Red => (239, 68, 68),
        termfast_desktop::tray::TrayIconColor::Gray => (107, 114, 128),
    };

    // Create a simple 22x22 RGBA image with a filled circle
    let size = 22u32;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    let center = (size / 2) as i32;
    let radius = (size / 2 - 2) as i32;
    for y in 0..size as i32 {
        for x in 0..size as i32 {
            let dx = x - center;
            let dy = y - center;
            if dx * dx + dy * dy <= radius * radius {
                rgba.extend_from_slice(&[r, g, b, 255]);
            } else {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    tauri::image::Image::new_owned(rgba, size, size)
}
