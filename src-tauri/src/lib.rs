//! TermFast Tauri App — main entry point
//!
//! Embeds the daemon and provides IPC bridge to the React frontend.
//! All IPC commands forward to the daemon handler (FP-6.2) to ensure
//! events are broadcast to both CLI and GUI clients.

mod daemon_embed;
mod credential_manager;
mod pairing;
mod pairing_store;
mod tunnel_manager;

use credential_manager::{credential_file_path, CredentialState};
use daemon_embed::EmbeddedDaemon;
use std::sync::Arc;
use tauri::{Emitter, Manager};

/// Shared embedded daemon — None until async startup completes
pub struct AppState {
    pub daemon: tokio::sync::Mutex<Option<Arc<EmbeddedDaemon>>>,
    /// Flag: true when user requested quit (tray menu / Cmd+Q).
    /// CloseRequested checks this to decide hide-vs-exit.
    pub is_quitting: std::sync::atomic::AtomicBool,
    /// Terminal output channels — key = session_id, value = Channel for raw bytes.
    /// Set by ipc_terminal_open, consumed by the binary event forwarder.
    pub terminal_channels: std::sync::Mutex<std::collections::HashMap<String, tauri::ipc::Channel<tauri::ipc::InvokeResponseBody>>>,
    /// Desktop tunnel manager — manages WebSocket tunnels to relay for paired phones.
    /// Initialized after daemon starts (needs TerminalManager from DaemonState).
    pub tunnel_manager: tokio::sync::Mutex<Option<Arc<tunnel_manager::DesktopTunnelManager>>>,
    /// Remote client manager — manages RemoteClient connections for desktop-to-desktop
    /// pairings where this desktop is the client (Desktop A).
    pub remote_client_manager: tokio::sync::Mutex<Option<Arc<termfast_daemon::remote_client::RemoteClientManager>>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize tracing to log file (persisted for debugging, cross-platform)
    // Use platform-appropriate log directory:
    //   macOS: ~/Library/Logs/com.termfast.app/
    //   Windows: %APPDATA%\com.termfast.app\logs\
    //   Linux: ~/.local/share/com.termfast.app/logs/
    #[cfg(target_os = "macos")]
    let log_dir = {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        std::path::PathBuf::from(home).join("Library/Logs/com.termfast.app")
    };
    #[cfg(target_os = "windows")]
    let log_dir = {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        std::path::PathBuf::from(appdata).join("com.termfast.app").join("logs")
    };
    #[cfg(target_os = "linux")]
    let log_dir = {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        std::path::PathBuf::from(home).join(".local/share/com.termfast.app/logs")
    };
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("termfast-app.log");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect("cannot open log file");
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("termfast_app=debug".parse().unwrap())
                .add_directive("termfast_daemon=debug".parse().unwrap())
                .add_directive("termfast_core=debug".parse().unwrap())
                .add_directive("keychain=debug".parse().unwrap()),
        )
        .with_writer(file)
        .with_ansi(false)
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
            // All platforms: clicking the close button hides the window
            // instead of exiting. The app stays alive in the system tray /
            // Dock. The user quits via Cmd+Q, tray menu "Quit", or the
            // app menu. This matches macOS HIG and standard tray-app
            // behavior on Windows.
            match event {
                tauri::WindowEvent::DragDrop(drag_drop) => {
                    use tauri::DragDropEvent;
                    match drag_drop {
                        DragDropEvent::Enter { paths, .. } => {
                            let paths: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
                            let _ = window.emit("file-drag-enter", &paths);
                        }
                        DragDropEvent::Over { .. } => {}
                        DragDropEvent::Leave => {
                            let _ = window.emit("file-drag-leave", ());
                        }
                        DragDropEvent::Drop { paths, .. } => {
                            let paths: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
                            let _ = window.emit("file-drop", &paths);
                        }
                        _ => {}
                    }
                }
                tauri::WindowEvent::CloseRequested { api, .. } => {
                let state = window.app_handle().try_state::<AppState>();
                let quitting = state
                    .map(|s| s.is_quitting.load(std::sync::atomic::Ordering::SeqCst))
                    .unwrap_or(false);
                if quitting {
                    // User requested quit (tray/Cmd+Q) — allow close, do graceful shutdown
                    let app_handle = window.app_handle().clone();
                    tauri::async_runtime::spawn(async move {
                        if let Some(state) = app_handle.try_state::<AppState>() {
                            let guard = state.daemon.lock().await;
                            if let Some(ref daemon) = *guard {
                                daemon.server.shutdown().await;
                            }
                        }
                        tracing::info!("graceful shutdown complete, exiting");
                        app_handle.exit(0);
                    });
                } else {
                    // Window close button — hide instead of exit
                    api.prevent_close();
                    let app_handle = window.app_handle().clone();
                    tauri::async_runtime::spawn(async move {
                        if let Some(win) = app_handle.get_webview_window("main") {
                            let _ = win.hide();
                        }
                    });
                }
                }
                _ => {}
            }
        })
        .setup(|app| {
            // Open DevTools in dev mode for debugging
            #[cfg(debug_assertions)]
            if let Some(window) = app.get_webview_window("main") {
                window.open_devtools();
            }

            // macOS: set up app menu with custom Quit that sets is_quitting flag
            // so CloseRequested allows the window to close (instead of hiding).
            #[cfg(target_os = "macos")]
            {
                use tauri::menu::{Menu, MenuItem, Submenu, PredefinedMenuItem, MenuEvent, AboutMetadata};
                let app_handle = app.handle();
                let quit_item = MenuItem::with_id(app_handle, "quit", "Quit TermFast", true, Some("CmdOrCtrl+Q"))?;
                let about_item = PredefinedMenuItem::about(app_handle, None, Some(AboutMetadata::default()))?;
                let app_menu = Submenu::with_items(app_handle, "TermFast", true, &[
                    &about_item,
                    &quit_item,
                ])?;
                let edit_menu = Submenu::with_items(app_handle, "Edit", true, &[
                    &PredefinedMenuItem::undo(app_handle, None)?,
                    &PredefinedMenuItem::redo(app_handle, None)?,
                    &PredefinedMenuItem::separator(app_handle)?,
                    &PredefinedMenuItem::cut(app_handle, None)?,
                    &PredefinedMenuItem::copy(app_handle, None)?,
                    &PredefinedMenuItem::paste(app_handle, None)?,
                    &PredefinedMenuItem::select_all(app_handle, None)?,
                ])?;
                let menu = Menu::with_items(app_handle, &[&app_menu, &edit_menu])?;
                app.set_menu(menu)?;

                let app_handle2 = app.handle().clone();
                app_handle2.on_menu_event(move |_handle, event: MenuEvent| {
                    if event.id() == "quit" {
                        tracing::info!("Cmd+Q quit from app menu");
                        if let Some(state) = _handle.try_state::<AppState>() {
                            state.is_quitting.store(true, std::sync::atomic::Ordering::SeqCst);
                        }
                        if let Some(win) = _handle.get_webview_window("main") {
                            let _ = win.close();
                        }
                    }
                });
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

            // Pre-manage AppState with daemon=None so IPC commands that
            // reference AppState don't fail with "state not managed" if the
            // frontend calls them before the daemon finishes starting.
            // forward_to_daemon already retries until daemon is Some.
            let initial_state = AppState {
                daemon: tokio::sync::Mutex::new(None),
                is_quitting: std::sync::atomic::AtomicBool::new(false),
                terminal_channels: std::sync::Mutex::new(std::collections::HashMap::new()),
                tunnel_manager: tokio::sync::Mutex::new(None),
                remote_client_manager: tokio::sync::Mutex::new(None),
            };
            app.manage(initial_state);

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

                        // Set up binary event forwarder: terminal:output raw bytes → Channel
                        let handle_for_bin = handle.clone();
                        daemon.server.state().set_binary_event_forwarder(Box::new(
                            move |session_id: &str, data: &[u8], _is_stderr: bool| {
                                use tauri::ipc::InvokeResponseBody;
                                use tauri::Manager;
                                if let Some(app_state) = handle_for_bin.try_state::<AppState>() {
                                    if let Ok(channels) = app_state.terminal_channels.lock() {
                                        if let Some(channel) = channels.get(session_id) {
                                            let _ = channel.send(InvokeResponseBody::Raw(data.to_vec()));
                                        }
                                    }
                                }
                            },
                        ));

                        // Update the pre-managed AppState with the started daemon.
                        use tauri::Manager;
                        if let Some(app_state) = handle.try_state::<AppState>() {
                            *app_state.daemon.lock().await = Some(Arc::new(daemon));
                        }

                        // Initialize RemoteClientManager for desktop-to-desktop pairings
                        let rcm = Arc::new(termfast_daemon::remote_client::RemoteClientManager::new());
                        if let Some(app_state) = handle.try_state::<AppState>() {
                            *app_state.remote_client_manager.lock().await = Some(rcm.clone());
                        }

                        // Set desktop_pair_callback on RemoteServer — called when a
                        // DESKTOP_PAIR frame arrives from the phone via an existing tunnel.
                        // The callback stores the pairing and starts a tunnel (server) or
                        // RemoteClient (client) depending on the role.
                        let handle_for_cb = handle.clone();
                        let rcm_for_cb = rcm.clone();
                        let tm_for_cb = {
                            let app_state = handle.state::<AppState>();
                            let tm = app_state.tunnel_manager.lock().await.clone();
                            tm
                        };
                        if let Some(tm) = tm_for_cb {
                            tm.remote_server().set_desktop_pair_callback(Box::new(
                                move |msg: termfast_daemon::remote_server::DesktopPairMessage| {
                                    let pairing_id = msg.pairing_id.clone();
                                    let pairing_key_hex = msg.pairing_key_hex.clone();
                                    let relay_url = msg.relay_url.clone();
                                    let peer_name = msg.peer_name.clone();
                                    let role = msg.role.clone();
                                    let pairing_jwt = msg.pairing_jwt.clone();
                                    let handle = handle_for_cb.clone();
                                    let rcm = rcm_for_cb.clone();
                                    Box::pin(async move {
                                        // Decode pairing key
                                        let pairing_key = decode_hex_32(&pairing_key_hex)
                                            .map_err(|e| format!("decode key: {}", e))?;

                                        // Store pairing persistently
                                        pairing_store::save(pairing_store::StoredPairing {
                                            pairing_id: pairing_id.clone(),
                                            pairing_key_hex: pairing_key_hex.clone(),
                                            relay_url: relay_url.clone(),
                                            jwt: pairing_jwt.clone(),
                                            pairing_type: "desktop".to_string(),
                                            peer_name: peer_name.clone(),
                                            peer_role: role.clone(),
                                        });

                                        if role == "server" {
                                            // Desktop B: start a tunnel (register as server)
                                            // B uses its own user JWT — get from existing tunnel manager
                                            // The user JWT is stored in the existing mobile pairing's store
                                            let user_jwt = get_user_jwt(&handle).await
                                                .ok_or_else(|| "no user JWT available".to_string())?;
                                            let tm = {
                                                let app_state = handle.state::<AppState>();
                                                let tm = app_state.tunnel_manager.lock().await.clone();
                                                tm
                                            };
                                            if let Some(tm) = tm {
                                                tm.start_tunnel(
                                                    pairing_id.clone(),
                                                    pairing_key,
                                                    relay_url.clone(),
                                                    user_jwt,
                                                ).await?;
                                            }
                                        } else if role == "client" {
                                            // Desktop A: start a RemoteClient (connect as client)
                                            let app_handle = handle.clone();
                                            let config = termfast_daemon::remote_client::RemoteClientConfig {
                                                relay_url: relay_url.clone(),
                                                pairing_jwt: pairing_jwt.clone(),
                                                pairing_id: pairing_id.clone(),
                                                pairing_key,
                                            };
                                            rcm.start_client(
                                                config,
                                                move |pid, frame_type, terminal_id, payload| {
                                                    use tauri::Emitter;
                                                    use base64::Engine;
                                                    let data_b64 = base64::engine::general_purpose::STANDARD.encode(payload);
                                                    let _ = app_handle.emit("remote_client_frame", serde_json::json!({
                                                        "pairing_id": pid,
                                                        "frame_type": frame_type,
                                                        "terminal_id": terminal_id,
                                                        "data": data_b64,
                                                    }));
                                                },
                                                move |pid, connected| {
                                                    use tauri::Emitter;
                                                    let _ = handle.emit("remote_client_state", serde_json::json!({
                                                        "pairing_id": pid,
                                                        "connected": connected,
                                                    }));
                                                },
                                            ).await?;
                                        } else {
                                            return Err(format!("unknown role: {}", role));
                                        }
                                        Ok(())
                                    })
                                },
                            ));
                        }

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
            ipc_list_local_triggers,
            ipc_save_local_trigger,
            ipc_remove_local_trigger,
            ipc_manual_fire_local_trigger,
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
            ipc_tmux_list_sessions,
            ipc_tmux_new_session,
            ipc_tmux_attach_session,
            ipc_tmux_kill_session,
            // Pairing
            ipc_pairing_register,
            ipc_pairing_login,
            ipc_pairing_initiate,
            ipc_generate_pairing_key,
            ipc_pairing_status,
            ipc_pairing_revoke,
            ipc_pairing_upload_config,
            ipc_pairing_list_devices,
            ipc_push_send,
            ipc_set_trigger_overrides,
            ipc_get_trigger_overrides,
            // Remote terminal tunnels (FP-4a-3/4)
            ipc_tunnel_start,
            ipc_tunnel_stop,
            ipc_tunnel_stop_all,
            ipc_restore_tunnels,
            // Quit app from tray menu (forces exit even if minimize_to_tray is on)
            ipc_quit_app,
            // Developer options
            ipc_toggle_devtools,
            // Cloud sync
            ipc_cloud_sync_auth_url,
            ipc_cloud_sync_exchange_code,
            ipc_cloud_sync_save_token,
            ipc_cloud_sync_load_token,
            ipc_cloud_sync_upload,
            ipc_cloud_sync_download,
            ipc_cloud_sync_file_info,
            ipc_cloud_sync_delete_remote,
            ipc_cloud_sync_disconnect,
            ipc_cloud_sync_refresh_token,
            ipc_cloud_sync_auth_with_callback,
            ipc_cloud_sync_wait_callback,
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
            ipc_get_system_locale,
            ipc_get_local_info,
            // Port forwarding (PF-6)
            ipc_list_port_forwards,
            ipc_add_port_forward,
            ipc_update_port_forward,
            ipc_delete_port_forward,
            ipc_start_port_forward,
            ipc_stop_port_forward,
            ipc_get_port_forward_status,
            // Remote client (desktop-to-desktop)
            ipc_remote_client_connect,
            ipc_remote_client_disconnect,
            ipc_remote_client_list_terminals,
            ipc_remote_client_subscribe,
            ipc_remote_client_send_input,
            ipc_remote_client_send_resize,
            ipc_remote_client_unsubscribe,
            ipc_list_desktop_pairings,
            ipc_initiate_desktop_pairing,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, _event| {
            // macOS: when user clicks Dock icon while window is hidden,
            // show the window again (matches standard macOS behavior).
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = _event {
                if let Some(window) = _app_handle.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        });
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
        // Clone the Arc<DaemonState> and release the lock BEFORE calling
        // handle_request.  Holding the lock during handle_request serializes
        // all IPC calls — a slow request (e.g. terminal input waiting for SSH
        // write ack) blocks every subsequent request, causing the UI to hang.
        let daemon_state = {
            let guard = state.daemon.lock().await;
            guard.as_ref().map(|d| d.server.state().clone())
        };
        if let Some(ds) = daemon_state {
            let req = termfast_daemon::proto::Request::new(action, params);
            let resp = termfast_daemon::handler::handle_request(&req, &ds).await;
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
#[allow(clippy::too_many_arguments)]
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
    dev_terminal_log: Option<bool>,
    dev_devtools: Option<bool>,
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
    if let Some(v) = dev_terminal_log {
        params["dev_terminal_log"] = serde_json::json!(v);
    }
    if let Some(v) = dev_devtools {
        params["dev_devtools"] = serde_json::json!(v);
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
#[allow(clippy::too_many_arguments)]
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

/// Toggle DevTools for the main window.
/// Called from the Developer Options settings section.
#[tauri::command]
fn ipc_toggle_devtools(app_handle: tauri::AppHandle, open: bool) -> Result<(), String> {
    // open_devtools/close_devtools only available in debug builds.
    #[cfg(debug_assertions)]
    if let Some(window) = app_handle.get_webview_window("main") {
        if open {
            window.open_devtools();
        } else {
            window.close_devtools();
        }
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = (app_handle, open);
        tracing::warn!("DevTools toggle is only available in debug builds");
    }
    Ok(())
}

/// Quit the app gracefully (from tray menu "Quit").
/// Performs daemon shutdown then exits, bypassing minimize_to_tray.
#[tauri::command]
async fn ipc_quit_app(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    tracing::info!("quit requested from tray menu");
    // Set quitting flag so CloseRequested handler allows the window to close
    state.is_quitting.store(true, std::sync::atomic::Ordering::SeqCst);
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

// === Local trigger IPC commands ===

#[tauri::command]
async fn ipc_list_local_triggers(
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    // 复用 GetConfig，在 Tauri 侧取 local_triggers（与 ipc_list_triggers 模式一致）
    let config = forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::GetConfig,
        serde_json::json!({}),
    )
    .await?;
    Ok(config["local_triggers"].clone())
}

#[tauri::command]
async fn ipc_save_local_trigger(
    state: tauri::State<'_, AppState>,
    trigger: serde_json::Value,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::SaveLocalTrigger,
        serde_json::json!({ "trigger": trigger }),
    )
    .await
}

#[tauri::command]
async fn ipc_remove_local_trigger(
    state: tauri::State<'_, AppState>,
    trigger_id: String,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::RemoveLocalTrigger,
        serde_json::json!({ "trigger_id": trigger_id }),
    )
    .await
}

#[tauri::command]
async fn ipc_manual_fire_local_trigger(
    state: tauri::State<'_, AppState>,
    trigger_id: String,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::ManualFireLocalTrigger,
        serde_json::json!({ "trigger_id": trigger_id }),
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
    master_password: Option<String>,
    blob: String,
) -> Result<serde_json::Value, String> {
    let master_password = master_password
        .map(zeroize::Zeroizing::new)
        .or_else(crate::credential_manager::cached_master_password)
        .ok_or_else(|| "master password not available — unlock credential store first".to_string())?;
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::ImportFull,
        serde_json::json!({ "master_password": master_password.as_str(), "blob": blob }),
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
        .icon_as_template(true)
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
#[allow(clippy::too_many_arguments)]
async fn ipc_terminal_open(
    state: tauri::State<'_, AppState>,
    server_id: String,
    cols: Option<u64>,
    rows: Option<u64>,
    on_output: tauri::ipc::Channel<tauri::ipc::InvokeResponseBody>,
    backend: Option<String>,
    shell: Option<String>,
    name: Option<String>,
    trigger_overrides: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "server_id": server_id,
        "cols": cols.unwrap_or(80),
        "rows": rows.unwrap_or(24),
        "backend": backend.as_deref().unwrap_or("ssh"),
        "shell": shell,
        "name": name,
        "trigger_overrides": trigger_overrides,
    });
    let result = forward_to_daemon(&state, termfast_daemon::proto::Action::TerminalOpen, params).await?;
    // Register the channel for this session so the binary event forwarder can send raw bytes
    if let Some(session_id) = result.get("session_id").and_then(|v| v.as_str()) {
        state.terminal_channels.lock().unwrap().insert(session_id.to_string(), on_output);
    }
    Ok(result)
}

#[tauri::command]
async fn ipc_terminal_input(
    state: tauri::State<'_, AppState>,
    session_id: String,
    data: Vec<u8>,
    wait_for_send: Option<bool>,
) -> Result<serde_json::Value, String> {
    // Encode raw bytes as base64 string — 33% overhead vs 200-300% for JSON
    // number array. The daemon handler decodes base64 back to Vec<u8>.
    use base64::Engine;
    let data_b64 = base64::engine::general_purpose::STANDARD.encode(&data);
    let params = serde_json::json!({
        "session_id": session_id,
        "data": data_b64,
        "wait_for_send": wait_for_send.unwrap_or(false),
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
    // Remove the channel from the registry
    state.terminal_channels.lock().unwrap().remove(&session_id);
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

#[tauri::command]
async fn ipc_tmux_list_sessions(
    state: tauri::State<'_, AppState>,
    server_id: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "server_id": server_id });
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::TmuxListSessions,
        params,
    )
    .await
}

#[tauri::command]
async fn ipc_tmux_new_session(
    state: tauri::State<'_, AppState>,
    server_id: String,
    description: Option<String>,
    cols: Option<u64>,
    rows: Option<u64>,
    on_output: tauri::ipc::Channel<tauri::ipc::InvokeResponseBody>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "server_id": server_id,
        "description": description.as_deref().unwrap_or(""),
        "cols": cols.unwrap_or(80),
        "rows": rows.unwrap_or(24),
    });
    let result = forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::TmuxNewSession,
        params,
    )
    .await?;
    if let Some(session_id) = result.get("session_id").and_then(|v| v.as_str()) {
        state
            .terminal_channels
            .lock()
            .unwrap()
            .insert(session_id.to_string(), on_output);
    }
    Ok(result)
}

#[tauri::command]
async fn ipc_tmux_attach_session(
    state: tauri::State<'_, AppState>,
    server_id: String,
    tmux_session_name: String,
    cols: Option<u64>,
    rows: Option<u64>,
    on_output: tauri::ipc::Channel<tauri::ipc::InvokeResponseBody>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "server_id": server_id,
        "tmux_session_name": tmux_session_name,
        "cols": cols.unwrap_or(80),
        "rows": rows.unwrap_or(24),
    });
    let result = forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::TmuxAttachSession,
        params,
    )
    .await?;
    if let Some(session_id) = result.get("session_id").and_then(|v| v.as_str()) {
        state
            .terminal_channels
            .lock()
            .unwrap()
            .insert(session_id.to_string(), on_output);
    }
    Ok(result)
}

#[tauri::command]
async fn ipc_tmux_kill_session(
    state: tauri::State<'_, AppState>,
    server_id: String,
    tmux_session_name: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "server_id": server_id,
        "tmux_session_name": tmux_session_name,
    });
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::TmuxKillSession,
        params,
    )
    .await
}

#[tauri::command]
async fn ipc_set_trigger_overrides(
    state: tauri::State<'_, AppState>,
    session_id: String,
    overrides: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "session_id": session_id,
        "overrides": overrides,
    });
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::SetTriggerOverrides,
        params,
    )
    .await
}

#[tauri::command]
async fn ipc_get_trigger_overrides(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "session_id": session_id,
    });
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::GetTriggerOverrides,
        params,
    )
    .await
}

// === Cloud sync IPC ===

#[tauri::command]
async fn ipc_cloud_sync_auth_url(
    state: tauri::State<'_, AppState>,
    provider: String,
    redirect_uri: Option<String>,
) -> Result<serde_json::Value, String> {
    let mut params = serde_json::json!({ "provider": provider });
    if let Some(uri) = redirect_uri {
        params["redirect_uri"] = serde_json::json!(uri);
    }
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::CloudSyncGetAuthUrl,
        params,
    )
    .await
}

#[tauri::command]
async fn ipc_cloud_sync_exchange_code(
    state: tauri::State<'_, AppState>,
    provider: String,
    code: String,
    code_verifier: String,
    redirect_uri: Option<String>,
) -> Result<serde_json::Value, String> {
    let mut params = serde_json::json!({
        "provider": provider,
        "code": code,
        "code_verifier": code_verifier,
    });
    if let Some(uri) = redirect_uri {
        params["redirect_uri"] = serde_json::json!(uri);
    }
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::CloudSyncExchangeCode,
        params,
    )
    .await
}

#[tauri::command]
async fn ipc_cloud_sync_save_token(
    state: tauri::State<'_, AppState>,
    provider: String,
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<i64>,
    token_type: Option<String>,
) -> Result<serde_json::Value, String> {
    let mut params = serde_json::json!({
        "provider": provider,
        "access_token": access_token,
    });
    if let Some(rt) = refresh_token {
        params["refresh_token"] = serde_json::json!(rt);
    }
    if let Some(ea) = expires_at {
        params["expires_at"] = serde_json::json!(ea);
    }
    if let Some(tt) = token_type {
        params["token_type"] = serde_json::json!(tt);
    }
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::CloudSyncSaveToken,
        params,
    )
    .await
}

#[tauri::command]
async fn ipc_cloud_sync_load_token(
    state: tauri::State<'_, AppState>,
    provider: String,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::CloudSyncLoadToken,
        serde_json::json!({ "provider": provider }),
    )
    .await
}

#[tauri::command]
async fn ipc_cloud_sync_upload(
    state: tauri::State<'_, AppState>,
    cred_state: tauri::State<'_, CredentialState>,
    provider: String,
    master_password: Option<String>,
    sync_path: Option<String>,
    force: Option<bool>,
) -> Result<serde_json::Value, String> {
    // Block upload if credential store is in pending mode (no master password
    // set). Uploading without a local master password doesn't make sense.
    if cred_state.store.is_pending() {
        return Ok(serde_json::json!({
            "ok": false,
            "reason": "not_initialized",
            "message": "请先设置主密码后再上传到云端",
        }));
    }
    // Use provided password, or fall back to cached master password from
    // credential store unlock (so user doesn't need to type it again).
    let master_password = master_password
        .map(zeroize::Zeroizing::new)
        .or_else(crate::credential_manager::cached_master_password)
        .ok_or_else(|| "master password not available — unlock credential store first".to_string())?;
    // Verify the password can unlock the local credential store.
    let cred_path = credential_manager::credential_file_path();
    if cred_path.exists() {
        if let Err(e) = cred_state.store.unlock(master_password.as_str()) {
            tracing::warn!("upload pre-check: unlock failed: {:?}", e);
            return Ok(serde_json::json!({
                "ok": false,
                "reason": "wrong_password",
                "message": "输入的主密码与本地主密码不一致，请先修改主密码后再上传",
            }));
        }
    }
    let mut params = serde_json::json!({
        "provider": provider,
        "master_password": master_password.as_str(),
    });
    if let Some(sp) = sync_path {
        params["sync_path"] = serde_json::json!(sp);
    }
    if let Some(f) = force {
        params["force"] = serde_json::json!(f);
    }
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::CloudSyncUpload,
        params,
    )
    .await
}

#[tauri::command]
async fn ipc_cloud_sync_download(
    state: tauri::State<'_, AppState>,
    cred_state: tauri::State<'_, CredentialState>,
    provider: String,
    master_password: Option<String>,
    sync_path: Option<String>,
    force_download: Option<bool>,
) -> Result<serde_json::Value, String> {
    // Resolve the master password: use provided or cached.
    let master_password = master_password
        .map(zeroize::Zeroizing::new)
        .or_else(crate::credential_manager::cached_master_password)
        .ok_or_else(|| "master password not available — unlock credential store first".to_string())?;
    // Verify the password can unlock the local credential store.
    // If the encrypted file exists, the password must match it.
    // If the file doesn't exist (no master password set), require the
    // user to set a master password first — downloading without a local
    // master password means the synced credentials can't be protected.
    let cred_path = credential_manager::credential_file_path();
    let cred_file_exists = cred_path.exists();
    tracing::info!("download pre-check: cred_file={}, path={}", cred_file_exists, cred_path.display());
    if !cred_file_exists {
        return Ok(serde_json::json!({
            "ok": false,
            "reason": "not_initialized",
            "message": "请先设置主密码后再从云端下载",
        }));
    }
    match cred_state.store.unlock(master_password.as_str()) {
        Ok(_) => tracing::info!("download pre-check: unlock OK"),
        Err(e) => {
            tracing::warn!("download pre-check: unlock failed: {:?}", e);
            return Ok(serde_json::json!({
                "ok": false,
                "reason": "wrong_password",
                "message": "输入的主密码与本地主密码不一致，请先修改主密码后再下载",
            }));
        }
    }
    let mut params = serde_json::json!({
        "provider": provider,
        "master_password": master_password.as_str(),
    });
    if let Some(sp) = sync_path {
        params["sync_path"] = serde_json::json!(sp);
    }
    if let Some(fd) = force_download {
        params["force_download"] = serde_json::json!(fd);
    }
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::CloudSyncDownload,
        params,
    )
    .await
}

#[tauri::command]
async fn ipc_cloud_sync_file_info(
    state: tauri::State<'_, AppState>,
    provider: String,
    sync_path: Option<String>,
) -> Result<serde_json::Value, String> {
    let mut params = serde_json::json!({ "provider": provider });
    if let Some(sp) = sync_path {
        params["sync_path"] = serde_json::json!(sp);
    }
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::CloudSyncGetFileInfo,
        params,
    )
    .await
}

#[tauri::command]
async fn ipc_cloud_sync_delete_remote(
    state: tauri::State<'_, AppState>,
    provider: String,
    sync_path: Option<String>,
) -> Result<serde_json::Value, String> {
    let mut params = serde_json::json!({ "provider": provider });
    if let Some(sp) = sync_path {
        params["sync_path"] = serde_json::json!(sp);
    }
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::CloudSyncDeleteRemote,
        params,
    )
    .await
}

#[tauri::command]
async fn ipc_cloud_sync_disconnect(
    state: tauri::State<'_, AppState>,
    provider: String,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::CloudSyncDisconnect,
        serde_json::json!({ "provider": provider }),
    )
    .await
}

#[tauri::command]
async fn ipc_cloud_sync_refresh_token(
    state: tauri::State<'_, AppState>,
    provider: String,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::CloudSyncRefreshToken,
        serde_json::json!({ "provider": provider }),
    )
    .await
}

#[tauri::command]
async fn ipc_cloud_sync_auth_with_callback(
    state: tauri::State<'_, AppState>,
    provider: String,
    port: Option<u16>,
) -> Result<serde_json::Value, String> {
    let mut params = serde_json::json!({ "provider": provider });
    if let Some(p) = port {
        params["port"] = serde_json::json!(p);
    }
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::CloudSyncAuthWithCallback,
        params,
    )
    .await
}

#[tauri::command]
async fn ipc_cloud_sync_wait_callback(
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::CloudSyncWaitCallback,
        serde_json::json!({}),
    )
    .await
}

/// Create a tray icon image — loaded from icons/tray-icon.png (embedded at compile time).
/// Uses icon_as_template(true) so macOS auto-inverts on light/dark menus.
fn create_tray_icon(_color: termfast_desktop::tray::TrayIconColor) -> tauri::image::Image<'static> {
    tauri::image::Image::from_bytes(include_bytes!("../icons/tray-icon.png"))
        .unwrap_or_else(|e| {
            tracing::error!("failed to load tray-icon.png: {}", e);
            // Fallback: simple 32x32 white square
            let size = 32u32;
            let rgba = vec![255u8; (size * size * 4) as usize];
            tauri::image::Image::new_owned(rgba, size, size)
        })
}

/// Get the system locale for language detection.
/// Returns BCP-47 tag like "zh-CN", "zh-TW", "en-US", "ja-JP" etc.
#[tauri::command]
fn ipc_get_system_locale() -> String {
    // sys-locale crate: returns the user's preferred locale as a BCP-47 tag.
    // On macOS: reads NSLocale preferred languages.
    // On Windows: reads GetUserDefaultLocaleName / GetUserPreferredUILanguages.
    // On Linux: reads LANG/LC_ALL env.
    sys_locale::get_locales().next().unwrap_or_else(|| "en-US".to_string())
}

/// Returns local terminal info: default shell, OS details, hostname, username.
/// Used by the "My Computer" overview to display real system data.
#[tauri::command]
fn ipc_get_local_info() -> serde_json::Value {
    let default_shell = termfast_core::local::shell::detect_default_shell();
    // Extract shell name from path (e.g. "/bin/zsh" → "zsh")
    let shell_name = std::path::Path::new(&default_shell)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&default_shell)
        .to_string();

    // Detailed OS info via os_info crate
    let os = os_info::get();
    let os_version = os.version().to_string();
    let os_arch = std::env::consts::ARCH;

    // User + hostname (whoami 2.x returns Result)
    let username = whoami::username().unwrap_or_else(|_| "unknown".to_string());
    let hostname = whoami::hostname().unwrap_or_else(|_| "unknown".to_string());
    let real_name = whoami::realname().unwrap_or_else(|_| username.clone());

    // OS display name (friendly)
    let os_name = match os.os_type() {
        os_info::Type::Macos => "macOS".to_string(),
        os_info::Type::Windows => "Windows".to_string(),
        os_info::Type::Linux => "Linux".to_string(),
        other => format!("{:?}", other),
    };

    let available_shells = termfast_core::local::shell::list_available_shells();

    serde_json::json!({
        "default_shell": default_shell,
        "shell_name": shell_name,
        "os_name": os_name,
        "os_version": os_version,
        "os_arch": os_arch,
        "hostname": hostname,
        "username": username,
        "real_name": real_name,
        "available_shells": available_shells,
    })
}

// === SECTION: Port forwarding IPC commands (PF-6) ===

#[tauri::command]
async fn ipc_list_port_forwards(
    state: tauri::State<'_, AppState>,
    server_id: String,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::ListPortForwards,
        serde_json::json!({ "server_id": server_id }),
    )
    .await
}

#[tauri::command]
async fn ipc_add_port_forward(
    state: tauri::State<'_, AppState>,
    server_id: String,
    rule: serde_json::Value,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::AddPortForward,
        serde_json::json!({ "server_id": server_id, "rule": rule }),
    )
    .await
}

#[tauri::command]
async fn ipc_update_port_forward(
    state: tauri::State<'_, AppState>,
    server_id: String,
    rule_id: String,
    rule: serde_json::Value,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::UpdatePortForward,
        serde_json::json!({ "server_id": server_id, "rule_id": rule_id, "rule": rule }),
    )
    .await
}

#[tauri::command]
async fn ipc_delete_port_forward(
    state: tauri::State<'_, AppState>,
    server_id: String,
    rule_id: String,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::DeletePortForward,
        serde_json::json!({ "server_id": server_id, "rule_id": rule_id }),
    )
    .await
}

#[tauri::command]
async fn ipc_start_port_forward(
    state: tauri::State<'_, AppState>,
    server_id: String,
    rule_id: String,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::StartPortForward,
        serde_json::json!({ "server_id": server_id, "rule_id": rule_id }),
    )
    .await
}

#[tauri::command]
async fn ipc_stop_port_forward(
    state: tauri::State<'_, AppState>,
    server_id: String,
    rule_id: String,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::StopPortForward,
        serde_json::json!({ "server_id": server_id, "rule_id": rule_id }),
    )
    .await
}

#[tauri::command]
async fn ipc_get_port_forward_status(
    state: tauri::State<'_, AppState>,
    server_id: String,
    rule_id: String,
) -> Result<serde_json::Value, String> {
    forward_to_daemon(
        &state,
        termfast_daemon::proto::Action::GetPortForwardStatus,
        serde_json::json!({ "server_id": server_id, "rule_id": rule_id }),
    )
    .await
}

// === SECTION: Port forwarding IPC commands END ===

// === SECTION: Pairing IPC commands ===

#[tauri::command]
async fn ipc_pairing_register(email: String, password: String) -> Result<serde_json::Value, String> {
    pairing::auth_register(&email, &password).await
}

#[tauri::command]
async fn ipc_pairing_login(email: String, password: String) -> Result<serde_json::Value, String> {
    pairing::auth_login(&email, &password).await
}

#[tauri::command]
async fn ipc_pairing_initiate(token: String, desktop_device_id: String, desktop_name: String) -> Result<serde_json::Value, String> {
    pairing::pair_initiate(&token, &desktop_device_id, &desktop_name).await
}

/// Generate a random 32-byte pairing key (hex-encoded) for tunnel crypto.
/// Called by frontend after pair_initiate, to embed in QR code.
#[tauri::command]
async fn ipc_generate_pairing_key() -> Result<String, String> {
    let mut key = [0u8; 32];
    getrandom::fill(&mut key).map_err(|e| e.to_string())?;
    Ok(hex::encode(key))
}

#[tauri::command]
async fn ipc_pairing_status(token: String, pairing_id: String) -> Result<serde_json::Value, String> {
    pairing::pair_status(&token, &pairing_id).await
}

#[tauri::command]
async fn ipc_pairing_revoke(
    app: tauri::AppHandle,
    token: String,
    pairing_id: String,
) -> Result<serde_json::Value, String> {
    // 1. Revoke on backend (removes pairing from DB)
    let result = pairing::pair_revoke(&token, &pairing_id).await;

    // 2. Stop the tunnel for this pairing_id (if active)
    let state = app.state::<AppState>();
    let tm_guard = state.tunnel_manager.lock().await;
    if let Some(tm) = tm_guard.as_ref() {
        tm.stop_tunnel(&pairing_id).await;
        // 3. Revoke pairing in RemoteServer (removes auth_key + remote_subscribers)
        tm.remote_server().revoke_pairing(&pairing_id).await;
    }
    drop(tm_guard);

    // 4. Remove persisted pairing so it won't be restored on next startup
    pairing_store::remove(&pairing_id);

    result
}

#[tauri::command]
async fn ipc_pairing_upload_config(pairing_jwt: String, ciphertext: String, nonce: String) -> Result<serde_json::Value, String> {
    pairing::sync_upload_config(&pairing_jwt, &ciphertext, &nonce).await
}

#[tauri::command]
async fn ipc_pairing_list_devices(token: String, desktop_device_id: String) -> Result<serde_json::Value, String> {
    pairing::list_devices(&token, &desktop_device_id).await
}

#[tauri::command]
async fn ipc_push_send(
    token: String,
    pairing_id: String,
    event_type: String,
    title: String,
    body: String,
    terminal_id: Option<String>,
) -> Result<serde_json::Value, String> {
    pairing::send_push(
        &token,
        &pairing_id,
        &event_type,
        &title,
        &body,
        terminal_id.as_deref(),
    )
    .await
}

// === SECTION: Pairing IPC commands END ===

// === SECTION: Remote terminal tunnel IPC commands (FP-4a-3/4) ===

/// Upload a local file to cloud storage for FILE_REQUEST (local terminal file transfer).
/// Reads the file, encrypts with master password, uploads to cloud, returns FileUploadResult.
async fn upload_file_to_cloud(
    file_path: String,
    file_upload_config: Arc<tokio::sync::Mutex<Option<termfast_daemon::server::FileUploadConfig>>>,
    config_manager: Arc<tokio::sync::Mutex<termfast_core::config::ConfigManager>>,
) -> Result<termfast_daemon::remote_server::FileUploadResult, String> {
    // Get upload config
    let upload_cfg = {
        let guard = file_upload_config.lock().await;
        guard.as_ref().ok_or_else(|| "cloud sync not configured — please sync config first".to_string())?.clone()
    };

    // Read file
    let file_data = tokio::fs::read(&file_path).await
        .map_err(|e| format!("read file failed: {}", e))?;
    let file_size = file_data.len() as u64;

    // Get file name from path
    let file_name = std::path::Path::new(&file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();

    // Compute SHA-256
    let sha256 = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&file_data);
        let hash = hasher.finalize();
        hex::encode(hash)
    };

    // Detect MIME type (simple extension-based)
    let mime_type = detect_mime_type(&file_name);

    // Encrypt with master password (on blocking thread — Argon2id)
    let mp = upload_cfg.master_password.clone();
    let encrypted = tokio::task::spawn_blocking(move || {
        // Use a dedicated magic for file uploads: "TFFI" (TermFast File)
        let magic = *b"TFFI";
        termfast_cloud_sync::sync_crypto::encrypt_with_magic(&magic, &mp, &file_data)
    })
    .await
    .map_err(|e| format!("spawn_blocking: {}", e))?
    .map_err(|e| format!("encrypt: {}", e))?;

    // Build provider (need proxy settings from config)
    let (proxy_mode_str, proxy_url) = {
        let mgr = config_manager.lock().await;
        let config = mgr.get().await;
        (
            config.general.http_proxy_mode.clone(),
            config.general.http_proxy_url.clone(),
        )
    };
    let proxy_mode = termfast_cloud_sync::proxy::ProxyMode::from_config(&proxy_mode_str, &proxy_url);

    let provider: Box<dyn termfast_cloud_sync::CloudProviderTrait> = match upload_cfg.provider.as_str() {
        "dropbox" => Box::new(
            termfast_cloud_sync::dropbox::DropboxProvider::with_proxy_mode(proxy_mode),
        ),
        "baidu" => Box::new(
            termfast_cloud_sync::baidu::BaiduProvider::with_proxy_mode(proxy_mode),
        ),
        _ => return Err(format!("unknown provider: {}", upload_cfg.provider)),
    };

    // Generate cloud path: /TermFast/files/{uuid}.enc
    let cloud_path = format!("/TermFast/files/{}.enc", uuid::Uuid::new_v4());

    // Upload
    provider
        .upload(&upload_cfg.token, &cloud_path, &encrypted)
        .await
        .map_err(|e| format!("upload: {}", e))?;

    tracing::info!(
        "FILE_REQUEST: uploaded {} ({} bytes) to {}",
        file_name, file_size, cloud_path
    );

    Ok(termfast_daemon::remote_server::FileUploadResult {
        cloud_path,
        file_name,
        size: file_size,
        sha256,
        mime_type,
    })
}

/// Simple MIME type detection based on file extension.
fn detect_mime_type(file_name: &str) -> String {
    let ext = std::path::Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "txt" | "log" => "text/plain",
        "json" => "application/json",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "xml" => "application/xml",
        "zip" => "application/zip",
        "tar" => "application/x-tar",
        "gz" => "application/gzip",
        "mp4" => "video/mp4",
        "mp3" => "audio/mpeg",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// Start a WebSocket tunnel to the relay for a paired phone.
///
/// Called by the frontend after pairing completes (or on app startup for
/// existing pairings). The tunnel connects to the relay, registers as
/// desktop, and bridges encrypted frame I/O between the phone (via relay)
/// and the desktop's RemoteServer (which shares local terminals).
///
/// # Arguments
///
/// * `pairing_id` - Pairing ID for this phone
/// * `pairing_key_hex` - 32-byte pairing key K as hex string (64 chars)
/// * `relay_url` - Relay WebSocket URL (e.g. "wss://termfast.xisj.com/tunnel")
/// * `jwt` - Desktop user JWT (for relay authentication)
#[tauri::command]
async fn ipc_tunnel_start(
    app: tauri::AppHandle,
    pairing_id: String,
    pairing_key_hex: String,
    relay_url: String,
    jwt: String,
) -> Result<(), String> {
    // Decode pairing key from hex
    let pairing_key = decode_hex_32(&pairing_key_hex)?;

    // Get or create the DesktopTunnelManager
    let state = app.state::<AppState>();
    let mut tm_guard = state.tunnel_manager.lock().await;
    if tm_guard.is_none() {
        // Initialize: get TerminalManager + ConfigManager from daemon
        let daemon_guard = state.daemon.lock().await;
        if let Some(daemon) = daemon_guard.as_ref() {
            let terminal_manager = daemon.server.state().terminal_manager.clone();
            let config_manager = daemon.server.state().config_manager.clone();
            let file_upload_config = daemon.server.state().file_upload_config.clone();
            let tm = Arc::new(tunnel_manager::DesktopTunnelManager::new(
                terminal_manager,
                config_manager.clone(),
            ));
            // Register file upload callback for FILE_REQUEST (local terminal file transfer)
            let config_mgr_cb = config_manager.clone();
            tm.remote_server().set_file_upload_callback(Box::new(move |file_path: String| {
                let file_upload_config = file_upload_config.clone();
                let config_mgr = config_mgr_cb.clone();
                Box::pin(async move {
                    upload_file_to_cloud(file_path, file_upload_config, config_mgr).await
                })
            }));
            *tm_guard = Some(tm);
            tracing::info!("DesktopTunnelManager initialized");
        } else {
            return Err("daemon not started".to_string());
        }
    }
    let tm = tm_guard.as_ref().unwrap().clone();
    drop(tm_guard);

    tm.start_tunnel(pairing_id.clone(), pairing_key, relay_url.clone(), jwt.clone()).await?;

    // Persist pairing so tunnel can be restored after restart
    pairing_store::save(pairing_store::StoredPairing {
        pairing_id,
        pairing_key_hex,
        relay_url,
        jwt,
        pairing_type: "mobile".to_string(),
        peer_name: String::new(),
        peer_role: String::new(),
    });
    Ok(())
}

/// Stop the WebSocket tunnel for a specific pairing_id.
#[tauri::command]
async fn ipc_tunnel_stop(
    app: tauri::AppHandle,
    pairing_id: String,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let tm_guard = state.tunnel_manager.lock().await;
    if let Some(tm) = tm_guard.as_ref() {
        tm.stop_tunnel(&pairing_id).await;
    }
    drop(tm_guard);
    // Remove persisted pairing so it won't be restored on next startup
    pairing_store::remove(&pairing_id);
    Ok(())
}

/// Stop all tunnels WITHOUT removing persisted pairing records.
/// Used on logout — tunnels are restored when user logs back in.
#[tauri::command]
async fn ipc_tunnel_stop_all(app: tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let tm_guard = state.tunnel_manager.lock().await;
    if let Some(tm) = tm_guard.as_ref() {
        tm.stop_all().await;
    }
    Ok(())
}

/// Restore all persisted tunnels on app startup.
///
/// Called by the frontend after loading the saved JWT. Loads all pairings
/// from pairings.json and starts a tunnel for each. Pairings whose backend
/// record has been revoked will fail at relay register time and be cleaned
/// up by the tunnel client's reconnect logic.
///
/// Returns the number of tunnels attempted.
#[tauri::command]
async fn ipc_restore_tunnels(
    app: tauri::AppHandle,
    jwt: String,
) -> Result<usize, String> {
    let stored = pairing_store::load();
    let count = stored.len();
    tracing::info!("ipc_restore_tunnels: {} persisted pairing(s)", count);
    for p in stored {
        let pairing_id = p.pairing_id.clone();
        // Skip desktop pairings — they are managed by RemoteClientManager,
        // not the tunnel manager. Desktop pairings where this desktop is the
        // server (role=server) do use the tunnel manager, but with the user JWT.
        if p.pairing_type == "desktop" && p.peer_role == "client" {
            tracing::info!("ipc_restore_tunnels: skipping desktop client pairing {}", pairing_id);
            continue;
        }
        // Reuse ipc_tunnel_start logic by calling start_tunnel directly.
        // We must initialize the tunnel manager the same way ipc_tunnel_start does.
        let pairing_key_hex = p.pairing_key_hex.clone();
        let relay_url = p.relay_url.clone();
        // For desktop server pairings, use the stored jwt; for mobile, use the passed jwt
        let jwt_clone = if p.pairing_type == "desktop" {
            p.jwt.clone()
        } else {
            jwt.clone()
        };
        let app_clone = app.clone();
        // Delegate to ipc_tunnel_start to keep init logic in one place.
        if let Err(e) = ipc_tunnel_start(
            app_clone,
            pairing_id.clone(),
            pairing_key_hex.clone(),
            relay_url.clone(),
            jwt_clone,
        ).await {
            tracing::warn!("ipc_restore_tunnels: failed to restore pairing {}: {}", pairing_id, e);
            // Remove invalid pairing from store so we don't keep retrying
            pairing_store::remove(&pairing_id);
        }
    }
    Ok(count)
}

/// Decode a 64-char hex string into a 32-byte array.
fn decode_hex_32(hex: &str) -> Result<[u8; 32], String> {
    if hex.len() != 64 {
        return Err(format!("pairing key hex must be 64 chars, got {}", hex.len()));
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|e| format!("invalid hex at byte {}: {}", i, e))?;
    }
    Ok(out)
}

// === SECTION: Remote terminal tunnel IPC commands END ===

/// Get the user JWT from the first stored mobile pairing (for Desktop B to
/// start a tunnel for desktop pairing). The user JWT is the same across all
/// mobile pairings for the same user.
async fn get_user_jwt(handle: &tauri::AppHandle) -> Option<String> {
    let pairings = pairing_store::load();
    // Find a mobile pairing with a jwt stored
    for p in &pairings {
        if p.pairing_type == "mobile" && !p.jwt.is_empty() {
            return Some(p.jwt.clone());
        }
    }
    // Fallback: check desktop pairings with jwt
    for p in &pairings {
        if !p.jwt.is_empty() {
            return Some(p.jwt.clone());
        }
    }
    let _ = handle;
    None
}

/// Start a RemoteClient connection for a desktop-to-desktop pairing.
/// Called by the frontend when the user clicks "Connect" on a remote desktop.
#[tauri::command]
async fn ipc_remote_client_connect(
    app: tauri::AppHandle,
    pairing_id: String,
    pairing_key_hex: String,
    pairing_jwt: String,
    relay_url: String,
) -> Result<(), String> {
    let pairing_key = decode_hex_32(&pairing_key_hex)?;
    let state = app.state::<AppState>();
    let rcm_guard = state.remote_client_manager.lock().await;
    let rcm = rcm_guard.as_ref().ok_or("remote client manager not initialized")?.clone();
    drop(rcm_guard);

    let app_handle = app.clone();
    rcm.start_client(
        termfast_daemon::remote_client::RemoteClientConfig {
            relay_url,
            pairing_jwt,
            pairing_id: pairing_id.clone(),
            pairing_key,
        },
        move |pid, frame_type, terminal_id, payload| {
            use tauri::Emitter;
            use base64::Engine;
            let data_b64 = base64::engine::general_purpose::STANDARD.encode(payload);
            let _ = app_handle.emit("remote_client_frame", serde_json::json!({
                "pairing_id": pid,
                "frame_type": frame_type,
                "terminal_id": terminal_id,
                "data": data_b64,
            }));
        },
        move |pid, connected| {
            use tauri::Emitter;
            let _ = app.emit("remote_client_state", serde_json::json!({
                "pairing_id": pid,
                "connected": connected,
            }));
        },
    ).await
}

/// Disconnect a RemoteClient.
#[tauri::command]
async fn ipc_remote_client_disconnect(
    app: tauri::AppHandle,
    pairing_id: String,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let rcm_guard = state.remote_client_manager.lock().await;
    if let Some(rcm) = rcm_guard.as_ref() {
        rcm.stop_client(&pairing_id).await;
    }
    Ok(())
}

/// Send a LIST_REQUEST to the remote desktop.
#[tauri::command]
async fn ipc_remote_client_list_terminals(
    app: tauri::AppHandle,
    pairing_id: String,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let rcm_guard = state.remote_client_manager.lock().await;
    let rcm = rcm_guard.as_ref().ok_or("remote client manager not initialized")?;
    rcm.send_frame(&pairing_id, termfast_daemon::remote_client::OutboundFrame::List).await
}

/// Subscribe to a terminal on the remote desktop.
#[tauri::command]
async fn ipc_remote_client_subscribe(
    app: tauri::AppHandle,
    pairing_id: String,
    terminal_id: u32,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let rcm_guard = state.remote_client_manager.lock().await;
    let rcm = rcm_guard.as_ref().ok_or("remote client manager not initialized")?;
    rcm.send_frame(&pairing_id, termfast_daemon::remote_client::OutboundFrame::Subscribe(terminal_id)).await
}

/// Send input to a remote terminal.
#[tauri::command]
async fn ipc_remote_client_send_input(
    app: tauri::AppHandle,
    pairing_id: String,
    terminal_id: u32,
    data: Vec<u8>,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let rcm_guard = state.remote_client_manager.lock().await;
    let rcm = rcm_guard.as_ref().ok_or("remote client manager not initialized")?;
    rcm.send_frame(&pairing_id, termfast_daemon::remote_client::OutboundFrame::Input(terminal_id, data)).await
}

/// Send resize to a remote terminal.
#[tauri::command]
async fn ipc_remote_client_send_resize(
    app: tauri::AppHandle,
    pairing_id: String,
    terminal_id: u32,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let rcm_guard = state.remote_client_manager.lock().await;
    let rcm = rcm_guard.as_ref().ok_or("remote client manager not initialized")?;
    rcm.send_frame(&pairing_id, termfast_daemon::remote_client::OutboundFrame::Resize(terminal_id, cols, rows)).await
}

/// Unsubscribe from a terminal on the remote desktop.
#[tauri::command]
async fn ipc_remote_client_unsubscribe(
    app: tauri::AppHandle,
    pairing_id: String,
    terminal_id: u32,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let rcm_guard = state.remote_client_manager.lock().await;
    let rcm = rcm_guard.as_ref().ok_or("remote client manager not initialized")?;
    rcm.send_frame(&pairing_id, termfast_daemon::remote_client::OutboundFrame::Unsubscribe(terminal_id)).await
}

/// List desktop-to-desktop pairings from the local store.
/// Returns pairings where pairing_type = "desktop".
#[tauri::command]
async fn ipc_list_desktop_pairings(
    _app: tauri::AppHandle,
) -> Result<serde_json::Value, String> {
    let all = pairing_store::load();
    let desktop_pairings: Vec<_> = all.into_iter()
        .filter(|p| p.pairing_type == "desktop")
        .collect();
    Ok(serde_json::json!({ "pairings": desktop_pairings }))
}

/// Initiate a desktop-to-desktop pairing from the desktop (not phone).
/// This calls the backend API and then sends DESKTOP_PAIR frames to both
/// desktops via existing tunnels. Currently, the phone initiates this,
/// but this IPC command allows the desktop to do it too.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn ipc_initiate_desktop_pairing(
    _app: tauri::AppHandle,
    token: String,
    server_user_id: u64,
    server_device_id: String,
    server_name: String,
    client_user_id: u64,
    client_device_id: String,
    client_name: String,
) -> Result<serde_json::Value, String> {
    pairing::pair_initiate_desktop(
        &token,
        server_user_id,
        &server_device_id,
        &server_name,
        client_user_id,
        &client_device_id,
        &client_name,
    ).await
}

// === SECTION: Remote client IPC commands END ===
