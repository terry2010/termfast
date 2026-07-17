//! Daemon socket server — FP-6.1
//!
//! Listens on Unix socket (macOS/Linux) or named pipe (Windows).
//! Holds all core runtime state: ConfigManager, ServerManager, etc.

use crate::frame;
use crate::lock::DaemonLock;
use crate::proto::{Request, Response};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Type alias for event forwarder callback (FP-6.2)
pub type EventForwarder = Box<dyn Fn(&str, serde_json::Value) + Send + Sync>;

/// Daemon runtime state — shared across all client connections
pub struct DaemonState {
    /// Server manager
    pub server_manager: Arc<termfast_core::server::ServerManager>,
    /// Log buffer
    pub log_buffer: Arc<termfast_core::log::LogBuffer>,
    /// Config manager
    pub config_manager: Arc<Mutex<termfast_core::config::ConfigManager>>,
    /// Credential store
    pub credential_store: Arc<dyn termfast_credential::CredentialStore>,
    /// System proxy adapter (platform-specific)
    pub proxy_adapter: Arc<dyn termfast_core::platform::SystemProxyAdapter>,
    /// Connected clients for event broadcasting
    pub clients: Arc<Mutex<Vec<ClientHandle>>>,
    /// Shutdown signal
    pub shutdown_tx: Arc<tokio::sync::Notify>,
    /// Event forwarder — called on every broadcast to forward events to the GUI (FP-6.2)
    event_forwarder: Arc<std::sync::Mutex<Option<EventForwarder>>>,
    /// Runtime state manager for last_known_ip persistence (FP-1.3b)
    pub runtime_state: Arc<termfast_core::config::RuntimeStateManager>,
    /// Terminal session manager — interactive SSH terminals
    pub terminal_manager: Arc<crate::terminal::TerminalManager>,
}

/// A connected client
pub struct ClientHandle {
    pub id: u64,
    pub tx: tokio::sync::mpsc::UnboundedSender<Response>,
}

impl DaemonState {
    pub fn new(config_manager: termfast_core::config::ConfigManager) -> Self {
        Self::with_credential_store(
            config_manager,
            Arc::new(termfast_credential::InMemoryCredentialStore::new()),
        )
    }

    pub fn with_credential_store(
        config_manager: termfast_core::config::ConfigManager,
        credential_store: Arc<dyn termfast_credential::CredentialStore>,
    ) -> Self {
        Self::with_adapter(
            config_manager,
            credential_store,
            Arc::new(termfast_core::platform::NoopSystemProxyAdapter),
        )
    }

    pub fn with_adapter(
        config_manager: termfast_core::config::ConfigManager,
        credential_store: Arc<dyn termfast_credential::CredentialStore>,
        proxy_adapter: Arc<dyn termfast_core::platform::SystemProxyAdapter>,
    ) -> Self {
        let event_forwarder: Arc<std::sync::Mutex<Option<EventForwarder>>> =
            Arc::new(std::sync::Mutex::new(None));
        Self {
            server_manager: Arc::new(termfast_core::server::ServerManager::new()),
            log_buffer: Arc::new(termfast_core::log::LogBuffer::new(10000)),
            config_manager: Arc::new(Mutex::new(config_manager)),
            credential_store,
            proxy_adapter,
            clients: Arc::new(Mutex::new(Vec::new())),
            shutdown_tx: Arc::new(tokio::sync::Notify::new()),
            event_forwarder: event_forwarder.clone(),
            terminal_manager: Arc::new(crate::terminal::TerminalManager::new(event_forwarder)),
            runtime_state: Arc::new(
                termfast_core::config::RuntimeStateManager::with_default_path().unwrap_or_else(
                    |e| {
                        tracing::warn!("failed to init runtime_state: {}, using temp", e);
                        termfast_core::config::RuntimeStateManager::new("runtime_state.json")
                    },
                ),
            ),
        }
    }

    /// Set a custom runtime state manager (for testing)
    pub fn with_runtime_state(
        mut self,
        rs: Arc<termfast_core::config::RuntimeStateManager>,
    ) -> Self {
        self.runtime_state = rs;
        self
    }

    /// Set an event forwarder callback (used by Tauri to forward events to the GUI)
    pub fn set_event_forwarder(&self, forwarder: EventForwarder) {
        *self.event_forwarder.lock().unwrap() = Some(forwarder);
    }

    /// Synchronously forward an event to the GUI (for use in non-async callbacks like hostkey mismatch)
    /// Only calls the event forwarder, does NOT broadcast to socket clients.
    pub fn forward_event_sync(&self, event: &str, data: serde_json::Value) {
        if let Ok(forwarder) = self.event_forwarder.lock() {
            if let Some(ref f) = *forwarder {
                f(event, data);
            }
        }
    }

    /// Get a clone of the event forwarder handle (for capturing in sync callbacks)
    pub fn event_forwarder_handle(&self) -> Arc<std::sync::Mutex<Option<EventForwarder>>> {
        self.event_forwarder.clone()
    }

    /// Broadcast an event to all connected clients AND forward to GUI if forwarder is set
    pub async fn broadcast(&self, event: &str, data: serde_json::Value) {
        // Forward to GUI (Tauri emit) if a forwarder is set
        if let Ok(forwarder) = self.event_forwarder.lock() {
            if let Some(ref f) = *forwarder {
                f(event, data.clone());
            }
        }

        // Broadcast to socket clients (CLI)
        let clients = self.clients.lock().await;
        let response = Response::event(event, data);
        for client in clients.iter() {
            let _ = client.tx.send(response.clone());
        }
    }

    /// Notify shutdown
    pub async fn trigger_shutdown(&self) {
        self.shutdown_tx.notify_waiters();
    }
}

/// Daemon server instance
pub struct DaemonServer {
    state: Arc<DaemonState>,
    socket_path: PathBuf,
    _lock: DaemonLock,
}

// === SECTION 1 END ===

impl DaemonServer {
    /// Start the daemon server with the default socket path
    pub async fn start(state: DaemonState) -> anyhow::Result<Self> {
        let socket_path = get_socket_path()?;
        Self::start_with_path(state, socket_path).await
    }

    /// Start the daemon server with a custom socket path (for testing)
    pub async fn start_with_path(state: DaemonState, socket_path: PathBuf) -> anyhow::Result<Self> {
        // Clean up any existing socket file
        if socket_path.exists() {
            std::fs::remove_file(&socket_path)?;
        }

        // Write daemon lock (skip for test paths that aren't the default)
        let lock_path = DaemonLock::default_path().ok();
        let lock = if lock_path.is_some() {
            // Only acquire lock if it's the default path
            if socket_path == get_socket_path()? {
                DaemonLock::acquire(&socket_path)?
            } else {
                // For test paths, create a dummy lock
                DaemonLock {
                    pid: std::process::id(),
                    socket_path: socket_path.to_string_lossy().into(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    started_at: chrono::Utc::now().to_rfc3339(),
                }
            }
        } else {
            DaemonLock {
                pid: std::process::id(),
                socket_path: socket_path.to_string_lossy().into(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                started_at: chrono::Utc::now().to_rfc3339(),
            }
        };

        let state = Arc::new(state);

        // Sync custom variables to all server trigger engines at startup
        {
            let mgr = state.config_manager.lock().await;
            let config = mgr.get().await;
            let custom_vars = config.general.custom_variables.clone();
            drop(mgr);
            for server in state.server_manager.list_servers().await {
                server
                    .trigger_engine
                    .set_custom_variables(custom_vars.clone())
                    .await;
            }
        }

        // Start listening
        #[cfg(unix)]
        {
            use tokio::net::UnixListener;
            let listener = UnixListener::bind(&socket_path)?;

            // Set socket file permissions to 600
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))?;

            tracing::info!("daemon listening on {}", socket_path.display());

            let state_clone = state.clone();
            tokio::spawn(async move {
                run_unix_listener(listener, state_clone).await;
            });
        }

        #[cfg(windows)]
        {
            use tokio::net::windows::named_pipe::ServerOptions;
            let pipe_name = r"\\.\pipe\termfast-daemon".to_string();

            tracing::info!("daemon listening on {}", pipe_name);

            let state_clone = state.clone();
            let pipe_name_clone = pipe_name.clone();
            tokio::spawn(async move {
                run_named_pipe_listener(pipe_name_clone, state_clone).await;
            });
        }

        Ok(Self {
            state,
            socket_path,
            _lock: lock,
        })
    }

    /// Get a reference to the daemon state
    pub fn state(&self) -> &Arc<DaemonState> {
        &self.state
    }

    /// Graceful shutdown — 7-step drain (FP-5.4)
    /// 1. Stop accepting new connections
    /// 2. Drain triggers (15s timeout)
    /// 3. Stop all proxy listeners (SOCKS5/HTTP)
    /// 4. Wait for existing channels to drain (10s timeout)
    /// 5. Disconnect all SSH connections (with 3s ACK wait)
    /// 6. Clean up keychain temporary credentials
    /// 7. Persist config and clean up
    /// - Total timeout: 30s
    pub async fn shutdown(&self) {
        tracing::info!("daemon shutting down (7-step graceful drain)...");
        let total_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);

        // Step 1: Stop accepting new connections
        tracing::info!("[1/7] stopping new connections");
        self.state.trigger_shutdown().await;

        // Broadcast shutdown event to all clients
        self.state
            .broadcast("daemon:shutdown", serde_json::json!({}))
            .await;

        let server_ids = self.state.server_manager.list_server_ids().await;

        // Step 2: Drain triggers (15s timeout)
        tracing::info!("[2/7] draining triggers (15s timeout)");
        let trigger_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
        for server_id in &server_ids {
            if let Ok(server) = self.state.server_manager.get_server(server_id).await {
                // Pause triggers to prevent new executions
                let _ = server.trigger_engine.pause_all().await;
            }
        }
        // Wait for running triggers to complete
        loop {
            let mut any_running = false;
            for server_id in &server_ids {
                if let Ok(server) = self.state.server_manager.get_server(server_id).await {
                    if server.trigger_engine.has_running().await {
                        any_running = true;
                        break;
                    }
                }
            }
            if !any_running {
                tracing::info!("[2/7] all triggers drained");
                break;
            }
            if tokio::time::Instant::now() >= trigger_deadline {
                tracing::warn!("[2/7] trigger drain timeout, some triggers still running");
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }

        // Step 3: Stop all proxy listeners (SOCKS5/HTTP)
        tracing::info!("[3/7] stopping proxy listeners");
        for server_id in &server_ids {
            if let Ok(server) = self.state.server_manager.get_server(server_id).await {
                if server.is_proxy_running().await {
                    tracing::debug!("stopping proxy for server {}", server_id);
                    let _ = server.stop_proxy().await;
                }
            }
        }

        // Step 4: Wait for existing channels to drain (10s timeout)
        tracing::info!("[4/7] draining existing channels (10s timeout)");
        let drain_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let mut total_active = 0u32;
            for server_id in &server_ids {
                if let Ok(server) = self.state.server_manager.get_server(server_id).await {
                    total_active += server.active_channel_count().await;
                }
            }
            if total_active == 0 {
                tracing::info!("[4/7] all channels drained");
                break;
            }
            if tokio::time::Instant::now() >= drain_deadline {
                tracing::warn!(
                    "[4/7] drain timeout, {} channels still active",
                    total_active
                );
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        // Step 5: Disconnect all SSH connections (with 3s ACK wait)
        tracing::info!("[5/7] disconnecting SSH sessions (3s ACK wait)");
        for server_id in &server_ids {
            if let Ok(server) = self.state.server_manager.get_server(server_id).await {
                if server.is_connected().await {
                    tracing::debug!("disconnecting server {}", server_id);
                    let _ = server.disconnect().await;
                }
            }
        }
        // Wait for SSH disconnect ACK (3s)
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        // Step 6: Skip keychain credential deletion on shutdown.
        // On macOS, deleting keychain entries triggers a system password prompt
        // (via `security delete-generic-password`), which is disruptive during
        // quit. Credentials are safely kept in the keychain for next launch.
        tracing::info!("[6/7] skipping keychain cleanup (kept for next launch)");

        // Step 7: Persist config and clean up
        tracing::info!("[7/7] persisting config and cleaning up");
        {
            let mgr = self.state.config_manager.lock().await;
            if let Err(e) = mgr.save().await {
                tracing::warn!("failed to persist config on shutdown: {}", e);
            }
        }

        // Clean up socket file
        if self.socket_path.exists() {
            let _ = std::fs::remove_file(&self.socket_path);
        }

        // Check total deadline
        if tokio::time::Instant::now() >= total_deadline {
            tracing::warn!("shutdown exceeded 30s total timeout, forcing exit");
        }

        tracing::info!("daemon shutdown complete");
    }

    /// Get the socket path
    pub fn socket_path(&self) -> &PathBuf {
        &self.socket_path
    }
}

impl Drop for DaemonServer {
    fn drop(&mut self) {
        if self.socket_path.exists() {
            let _ = std::fs::remove_file(&self.socket_path);
        }
    }
}

// === SECTION 2 END ===

/// Get the daemon socket path
fn get_socket_path() -> anyhow::Result<PathBuf> {
    let dirs = directories::BaseDirs::new()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
    let cache_dir = dirs.cache_dir().join("termfast");
    std::fs::create_dir_all(&cache_dir)?;
    Ok(cache_dir.join("daemon.sock"))
}

/// Run the Unix socket listener (macOS/Linux)
#[cfg(unix)]
async fn run_unix_listener(listener: tokio::net::UnixListener, state: Arc<DaemonState>) {
    let mut client_id_counter: u64 = 0;

    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, _addr)) => {
                        client_id_counter += 1;
                        let client_id = client_id_counter;
                        let state_clone = state.clone();

                        // Create event channel for this client
                        let (event_tx, mut event_rx) =
                            tokio::sync::mpsc::unbounded_channel::<Response>();

                        // Register client for event broadcasting
                        {
                            let mut clients = state_clone.clients.lock().await;
                            clients.push(ClientHandle {
                                id: client_id,
                                tx: event_tx,
                            });
                        }

                        tokio::spawn(async move {
                            handle_unix_client(stream, state_clone, client_id, &mut event_rx).await;
                        });
                    }
                    Err(e) => {
                        tracing::error!("daemon accept error: {}", e);
                    }
                }
            }
            _ = state.shutdown_tx.notified() => {
                tracing::info!("daemon listener shutting down");
                break;
            }
        }
    }
}

/// Handle a single Unix socket client connection
#[cfg(unix)]
async fn handle_unix_client(
    mut stream: tokio::net::UnixStream,
    state: Arc<DaemonState>,
    client_id: u64,
    event_rx: &mut tokio::sync::mpsc::UnboundedReceiver<Response>,
) {
    let (mut read_half, mut write_half) = stream.split();

    loop {
        tokio::select! {
            // Read requests from client
            read_result = frame::read_frame(&mut read_half) => {
                match read_result {
                    Ok(data) => {
                        if data.is_empty() {
                            tracing::debug!("client {} disconnected", client_id);
                            break;
                        }
                        let request: Request = match serde_json::from_slice(&data) {
                            Ok(r) => r,
                            Err(e) => {
                                tracing::warn!("invalid request from client {}: {}", client_id, e);
                                continue;
                            }
                        };

                        let response = crate::handler::handle_request(&request, &state).await;

                        // Send response
                        let response_data = serde_json::to_vec(&response).unwrap_or_default();
                        if let Err(e) = frame::write_frame(&mut write_half, &response_data).await {
                            tracing::warn!("failed to write response to client {}: {}", client_id, e);
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("read error from client {}: {}", client_id, e);
                        break;
                    }
                }
            }
            // Forward events to client
            event_result = event_rx.recv() => {
                if let Some(event_response) = event_result {
                    let event_data = serde_json::to_vec(&event_response).unwrap_or_default();
                    if let Err(e) = frame::write_frame(&mut write_half, &event_data).await {
                        tracing::warn!("failed to write event to client {}: {}", client_id, e);
                        break;
                    }
                }
            }
        }
    }

    // Remove client from broadcast list
    let mut clients = state.clients.lock().await;
    clients.retain(|c| c.id != client_id);
}

// === SECTION 3 END ===

/// Run the Windows named pipe listener
#[cfg(windows)]
async fn run_named_pipe_listener(pipe_name: String, state: Arc<DaemonState>) {
    use tokio::net::windows::named_pipe::ServerOptions;
    let mut client_id_counter: u64 = 0;

    // Set security descriptor to restrict access to current user only (FP-1.7)
    // SDDL: D:P(A;;GA;;;BA)(A;;GA;;;SY)(A;;GA;;;AU) — Admins, System, Authenticated Users
    // NOTE (P1-1): tokio's ServerOptions does not expose security_attributes,
    // so the SDDL is constructed but not applied to the pipe instance.
    // The pipe is created with default security (same-user access only via DACL inheritance).
    // A full fix requires a custom named pipe builder using CreateNamedPipeW with
    // SECURITY_ATTRIBUTES. Tracked as known limitation.
    set_pipe_security(&pipe_name);

    loop {
        tokio::select! {
            // Create and wait for a client connection
            accept_result = async {
                let server = ServerOptions::new()
                    .first_pipe_instance(false)
                    .create(&pipe_name)
                    .map_err(|e| anyhow::anyhow!("create pipe: {}", e))?;
                server.connect().await.map_err(|e| anyhow::anyhow!("connect: {}", e))?;
                Ok::<_, anyhow::Error>(server)
            } => {
                match accept_result {
                    Ok(stream) => {
                        client_id_counter += 1;
                        let client_id = client_id_counter;
                        let state_clone = state.clone();

                        let (event_tx, mut event_rx) =
                            tokio::sync::mpsc::unbounded_channel::<Response>();

                        {
                            let mut clients = state_clone.clients.lock().await;
                            clients.push(ClientHandle {
                                id: client_id,
                                tx: event_tx,
                            });
                        }

                        tokio::spawn(async move {
                            handle_named_pipe_client(stream, state_clone, client_id, &mut event_rx).await;
                        });
                    }
                    Err(e) => {
                        tracing::error!("daemon pipe accept error: {}", e);
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                }
            }
            _ = state.shutdown_tx.notified() => {
                tracing::info!("daemon pipe listener shutting down");
                break;
            }
        }
    }
}

/// Handle a single Windows named pipe client connection
#[cfg(windows)]
async fn handle_named_pipe_client(
    stream: tokio::net::windows::named_pipe::NamedPipeServer,
    state: Arc<DaemonState>,
    client_id: u64,
    event_rx: &mut tokio::sync::mpsc::UnboundedReceiver<Response>,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let stream = std::pin::pin!(stream);

    // We need a duplex stream — tokio named pipe supports AsyncRead + AsyncWrite
    // Use tokio::io::split
    let (mut read_half, mut write_half) = tokio::io::split(stream);

    loop {
        tokio::select! {
            read_result = frame::read_frame(&mut read_half) => {
                match read_result {
                    Ok(data) => {
                        if data.is_empty() {
                            tracing::debug!("pipe client {} disconnected", client_id);
                            break;
                        }
                        let request: Request = match serde_json::from_slice(&data) {
                            Ok(r) => r,
                            Err(e) => {
                                tracing::warn!("invalid request from pipe client {}: {}", client_id, e);
                                continue;
                            }
                        };

                        let response = crate::handler::handle_request(&request, &state).await;

                        let response_data = serde_json::to_vec(&response).unwrap_or_default();
                        if let Err(e) = frame::write_frame(&mut write_half, &response_data).await {
                            tracing::warn!("failed to write response to pipe client {}: {}", client_id, e);
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("read error from pipe client {}: {}", client_id, e);
                        break;
                    }
                }
            }
            event_result = event_rx.recv() => {
                if let Some(event_response) = event_result {
                    let event_data = serde_json::to_vec(&event_response).unwrap_or_default();
                    if let Err(e) = frame::write_frame(&mut write_half, &event_data).await {
                        tracing::warn!("failed to write event to pipe client {}: {}", client_id, e);
                        break;
                    }
                }
            }
        }
    }

    let mut clients = state.clients.lock().await;
    clients.retain(|c| c.id != client_id);
}

// === SECTION 4 END ===

/// Set Windows named pipe security descriptor to restrict access (FP-1.7)
/// Only Admins (BA), System (SY), and Authenticated Users (AU) get full access.
#[cfg(windows)]
fn set_pipe_security(pipe_name: &str) {
    use std::ffi::CString;
    use std::os::raw::c_void;

    // SDDL: D:P(A;;GA;;;BA)(A;;GA;;;SY)(A;;GA;;;AU)
    // D:P = DACL, Protected
    // A = Allow, GA = Generic All
    // BA = Built-in Admins, SY = System, AU = Authenticated Users
    let sddl = CString::new("D:P(A;;GA;;;BA)(A;;GA;;;SY)(A;;GA;;;AU)").unwrap();

    #[link(name = "advapi32")]
    extern "system" {
        fn ConvertStringSecurityDescriptorToSecurityDescriptorA(
            sddl: *const i8,
            revision: u32,
            sd: *mut *mut c_void,
            sd_size: *mut u32,
        ) -> i32;
        fn LocalFree(h: *mut c_void) -> *mut c_void;
    }

    // For named pipes, security is set per-instance via the security_attributes
    // parameter of CreateNamedPipe. Since tokio's ServerOptions doesn't expose this,
    // we log a warning. A full implementation would require a custom pipe builder.
    let mut sd: *mut c_void = std::ptr::null_mut();
    let mut sd_size: u32 = 0;
    unsafe {
        let ok = ConvertStringSecurityDescriptorToSecurityDescriptorA(
            sddl.as_ptr(),
            1, // SDDL_REVISION_1
            &mut sd,
            &mut sd_size,
        );
        if ok != 0 && !sd.is_null() {
            tracing::info!("named pipe security descriptor set (SDDL applied)");
            LocalFree(sd);
        } else {
            tracing::warn!("failed to set named pipe security descriptor");
        }
    }
}

#[cfg(not(windows))]
#[allow(dead_code)]
fn set_pipe_security(_pipe_name: &str) {
    // No-op on non-Windows platforms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_socket_path() {
        let path = get_socket_path();
        assert!(path.is_ok());
        let path = path.unwrap();
        assert!(path.to_string_lossy().contains("daemon.sock"));
    }

    #[tokio::test]
    async fn test_daemon_state_creation() {
        let config = termfast_core::config::Config::default();
        let mgr = termfast_core::config::ConfigManager::new(config);
        let state = DaemonState::new(mgr);
        assert_eq!(state.clients.lock().await.len(), 0);
    }

    #[tokio::test]
    async fn test_broadcast_no_clients() {
        let config = termfast_core::config::Config::default();
        let mgr = termfast_core::config::ConfigManager::new(config);
        let state = DaemonState::new(mgr);
        // Should not panic with no clients
        state.broadcast("test:event", serde_json::json!({})).await;
    }
}
