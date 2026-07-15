//! Server instance — FP-5.1
//!
//! Single server runtime state: SSH connection, proxy, triggers, IP detector.

use crate::config::ServerConfig;
use crate::config::TriggerInstance;
use crate::config::TriggerTemplate;
use crate::config::TriggerType;
use crate::error::{Error, Result};
use crate::proxy::{ChannelManager, HttpProxyServer, MixedProxyServer, Socks5Server};
use crate::ssh::auth::AuthMethod;
use crate::ssh::channel_opener::SshChannelOpener;
use crate::ssh::client::{ConnectionState, SshClientConfig, SshClientHandle};
use crate::ssh::exec;
use crate::trigger::engine::{TriggerEngine, TriggerEvent};
use crate::trigger::ipcheck::IpChangeDetector;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

/// Runtime status of a server (for UI display)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerStatus {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    AuthFailed,
    Error,
    /// Network is offline — reconnection paused, doesn't consume max_attempts
    Offline,
}

impl From<ConnectionState> for ServerStatus {
    fn from(state: ConnectionState) -> Self {
        match state {
            ConnectionState::Disconnected => ServerStatus::Disconnected,
            ConnectionState::Connecting => ServerStatus::Connecting,
            ConnectionState::Connected => ServerStatus::Connected,
            ConnectionState::Reconnecting { .. } => ServerStatus::Reconnecting,
            ConnectionState::AuthFailed => ServerStatus::AuthFailed,
        }
    }
}

/// A single server instance with all runtime state
pub struct ServerInstance {
    pub config: ServerConfig,
    pub ssh_client: Arc<SshClientHandle>,
    pub trigger_engine: Arc<TriggerEngine>,
    pub channel_opener: Arc<SshChannelOpener>,
    pub status: Mutex<ServerStatus>,
    pub current_ip: Arc<Mutex<Option<String>>>,
    /// Trigger templates (shared, for type lookup during fire_event)
    pub trigger_templates: Arc<Mutex<Vec<TriggerTemplate>>>,
    /// Runtime triggers (kept in sync with config, so fire_on_connect sees latest)
    pub triggers: Arc<Mutex<Vec<TriggerInstance>>>,
    proxy_tasks: Mutex<Vec<JoinHandle<()>>>,
    ip_check_task: Mutex<Option<JoinHandle<()>>>,
    /// Health check tasks for OnProcessDead/OnPortClosed triggers (FP-4.4)
    health_check_tasks: Mutex<Vec<JoinHandle<()>>>,
    /// Connection monitor task — watches SSH connection and auto-reconnects
    connection_monitor_task: Mutex<Option<JoinHandle<()>>>,
    /// Whether proxy was running before disconnect (for auto-reconnect proxy restart)
    proxy_was_running: Mutex<bool>,
    proxy_running: Mutex<bool>,
    /// Channel manager for proxy (set when proxy starts, cleared when stops)
    channel_manager: Mutex<Option<Arc<ChannelManager>>>,
    /// Runtime state manager for IP persistence (FP-1.3b)
    runtime_state: Mutex<Option<Arc<crate::config::RuntimeStateManager>>>,
    /// Optional callback to broadcast trigger results to frontend
    trigger_result_callback: Mutex<
        Option<
            Arc<
                dyn Fn(TriggerEvent, &[crate::trigger::engine::TriggerExecutionResult])
                    + Send
                    + Sync,
            >,
        >,
    >,
    /// Optional callback to broadcast status changes to frontend
    status_change_callback: Mutex<Option<Arc<dyn Fn(&ServerStatus) + Send + Sync>>>,
    /// Client IP detected after SSH connection (§5.2)
    client_ip: Mutex<Option<String>>,
    /// Last auth method used (for auto-reconnect)
    last_auth: Mutex<Option<AuthMethod>>,
}

// === SECTION 1 END ===

impl ServerInstance {
    /// Create a new server instance from config
    pub fn new(config: ServerConfig) -> Self {
        let ssh_config = SshClientConfig {
            host: config.ssh.host.clone(),
            port: config.ssh.port,
            user: config.ssh.user.clone(),
            heartbeat_interval: config.reconnect.heartbeat_interval,
            max_attempts: config.reconnect.max_attempts,
            initial_backoff_secs: config.reconnect.initial_backoff_secs,
            max_backoff_secs: config.reconnect.max_backoff_secs,
            skip_hostkey_verify: config.ssh.skip_hostkey_verify,
            hostkey_mismatch_callback: None,
            socket_protector: None,
        };

        Self {
            ssh_client: Arc::new(SshClientHandle::new(ssh_config)),
            trigger_engine: Arc::new(TriggerEngine::new()),
            channel_opener: Arc::new(SshChannelOpener::empty()),
            triggers: Arc::new(Mutex::new(config.triggers.clone())),
            config,
            status: Mutex::new(ServerStatus::Disconnected),
            current_ip: Arc::new(Mutex::new(None)),
            trigger_templates: Arc::new(Mutex::new(Vec::new())),
            proxy_tasks: Mutex::new(Vec::new()),
            ip_check_task: Mutex::new(None),
            health_check_tasks: Mutex::new(Vec::new()),
            connection_monitor_task: Mutex::new(None),
            proxy_was_running: Mutex::new(false),
            proxy_running: Mutex::new(false),
            channel_manager: Mutex::new(None),
            runtime_state: Mutex::new(None),
            trigger_result_callback: Mutex::new(None),
            status_change_callback: Mutex::new(None),
            client_ip: Mutex::new(None),
            last_auth: Mutex::new(None),
        }
    }

    pub fn id(&self) -> &str {
        &self.config.id
    }

    pub fn name(&self) -> &str {
        &self.config.name
    }

    pub async fn status(&self) -> ServerStatus {
        let status = self.status.lock().await.clone();
        if status == ServerStatus::Connected && !self.ssh_client.is_connected().await {
            return ServerStatus::Disconnected;
        }
        status
    }

    pub async fn current_ip(&self) -> Option<String> {
        self.current_ip.lock().await.clone()
    }

    pub async fn client_ip(&self) -> Option<String> {
        self.client_ip.lock().await.clone()
    }

    pub async fn set_client_ip(&self, ip: Option<String>) {
        *self.client_ip.lock().await = ip;
    }

    /// Get the SSH auth banner received during the most recent connection
    /// (RFC4252 §5.4). This is the welcome message sent by the server during
    /// authentication, before the shell starts.
    pub async fn auth_banner(&self) -> Option<String> {
        self.ssh_client.auth_banner().await
    }

    /// Set the runtime state manager for IP persistence (FP-1.3b)
    pub async fn set_runtime_state(&self, rs: Arc<crate::config::RuntimeStateManager>) {
        *self.runtime_state.lock().await = Some(rs);
    }

    /// Set the hostkey mismatch callback (§17.2: triple notification)
    pub async fn set_hostkey_mismatch_callback(
        &self,
        cb: Arc<dyn Fn(String, String) + Send + Sync>,
    ) {
        self.ssh_client.set_hostkey_mismatch_callback(cb).await;
    }

    /// Update trigger templates (called when config changes)
    pub async fn set_trigger_templates(&self, templates: Vec<TriggerTemplate>) {
        *self.trigger_templates.lock().await = templates;
    }

    /// Update runtime triggers (call after add/remove/update trigger)
    pub async fn set_triggers(&self, triggers: Vec<TriggerInstance>) {
        *self.triggers.lock().await = triggers;
    }

    /// Set callback for trigger execution results (to broadcast to frontend)
    pub async fn set_trigger_result_callback(
        &self,
        cb: Arc<
            dyn Fn(TriggerEvent, &[crate::trigger::engine::TriggerExecutionResult]) + Send + Sync,
        >,
    ) {
        *self.trigger_result_callback.lock().await = Some(cb);
    }

    /// Set callback for status changes (to broadcast to frontend)
    pub async fn set_status_change_callback(&self, cb: Arc<dyn Fn(&ServerStatus) + Send + Sync>) {
        *self.status_change_callback.lock().await = Some(cb);
    }

    /// Broadcast status change to frontend if callback is set
    async fn broadcast_status(&self, status: &ServerStatus) {
        let cb = self.status_change_callback.lock().await;
        if let Some(ref f) = *cb {
            f(status);
        }
    }

    /// Connect to the server with the given auth method.
    /// After connecting, starts proxy (if enabled) and IP detection.
    pub async fn connect(&self, auth: &AuthMethod) -> Result<()> {
        *self.status.lock().await = ServerStatus::Connecting;

        match self.ssh_client.connect(auth).await {
            Ok(()) => {
                let handle = self.ssh_client.get_handle().await;
                if let Some(h) = handle {
                    self.channel_opener.set_handle(h.clone()).await;
                    // Detect client IP from SSH connection (§5.2)
                    match crate::ssh::exec::detect_client_ip(&*h).await {
                        Ok(ip) => {
                            tracing::debug!("detected client ip {} for {}", ip, self.config.name);
                            *self.client_ip.lock().await = Some(ip);
                        }
                        Err(e) => {
                            tracing::warn!(
                                "failed to detect client ip for {}: {}",
                                self.config.name,
                                e
                            );
                        }
                    }
                }
                *self.status.lock().await = ServerStatus::Connected;
                // Save auth for auto-reconnect
                *self.last_auth.lock().await = Some(auth.clone());

                // Proxy is NOT auto-started on connect — it's controlled independently
                // by the user via the proxy toggle button.
                if self.config.ip_check.enabled {
                    self.start_ip_detection().await;
                }
                self.start_health_checks().await;
                self.fire_on_connect_triggers().await;
                Ok(())
            }
            Err(e) => {
                let status = if matches!(e, Error::Ipc(ref ipc) if ipc.code == crate::error::ErrorCode::AuthFailed)
                {
                    ServerStatus::AuthFailed
                } else {
                    ServerStatus::Error
                };
                *self.status.lock().await = status;
                Err(e)
            }
        }
    }

    /// Connect with reconnection logic (exponential backoff)
    pub async fn connect_with_reconnect(&self, auth: &AuthMethod) -> Result<()> {
        self.ssh_client.connect_with_reconnect(auth).await?;
        let handle = self.ssh_client.get_handle().await;
        if let Some(h) = handle {
            self.channel_opener.set_handle(h.clone()).await;
        }
        *self.status.lock().await = ServerStatus::Connected;

        if self.config.proxy.enabled {
            let _ = self.start_proxy().await;
        }
        if self.config.ip_check.enabled {
            self.start_ip_detection().await;
        }
        self.start_health_checks().await;
        self.fire_on_reconnect_triggers().await;
        Ok(())
    }

    /// Disconnect from the server
    pub async fn disconnect(&self) -> Result<()> {
        // Stop connection monitor first so it doesn't trigger auto-reconnect
        self.stop_connection_monitor().await;
        let _ = self.stop_proxy().await;
        if let Some(task) = self.ip_check_task.lock().await.take() {
            task.abort();
        }
        // Abort all health check tasks
        let mut health_tasks = self.health_check_tasks.lock().await;
        for task in health_tasks.drain(..) {
            task.abort();
        }
        self.channel_opener.clear_handle().await;
        self.ssh_client.disconnect().await?;
        *self.status.lock().await = ServerStatus::Disconnected;
        Ok(())
    }

    /// Start a background task that monitors the SSH connection and auto-reconnects
    /// if it drops and auto_reconnect is enabled in config.
    pub async fn start_connection_monitor(self: &Arc<Self>) {
        self.stop_connection_monitor().await;

        let instance = Arc::clone(self);
        let check_interval =
            std::time::Duration::from_secs(instance.config.reconnect.heartbeat_interval.max(5));

        let handle = tokio::spawn(async move {
            loop {
                tokio::time::sleep(check_interval).await;

                // Check if auto_reconnect is enabled
                if !instance.config.reconnect.auto_reconnect {
                    continue;
                }

                // Check if still connected
                if instance.ssh_client.is_connected().await {
                    continue;
                }

                // Connection dropped — check if this was a user-initiated disconnect
                let current_status = instance.status.lock().await.clone();
                if current_status == ServerStatus::Disconnected {
                    tracing::info!(
                        "connection monitor: {} was explicitly disconnected, stopping",
                        instance.config.name
                    );
                    break;
                }

                // Connection dropped unexpectedly — start auto-reconnect
                tracing::warn!(
                    "connection monitor: {} SSH connection lost, auto-reconnecting",
                    instance.config.name
                );
                *instance.status.lock().await = ServerStatus::Reconnecting;
                instance.broadcast_status(&ServerStatus::Reconnecting).await;

                let auth = instance.last_auth.lock().await.clone();
                if auth.is_none() {
                    tracing::error!(
                        "connection monitor: no saved auth for {}, cannot reconnect",
                        instance.config.name
                    );
                    *instance.status.lock().await = ServerStatus::Error;
                    instance.broadcast_status(&ServerStatus::Error).await;
                    break;
                }
                let auth = auth.unwrap();

                // Segmented backoff strategy:
                //   0-10s:    every 1s
                //   10-60s:   every 3s
                //   60s-5m:   every 10s
                //   5m-15m:   every 30s
                //   15m+:     every 60s
                // Stops when elapsed >= reconnect_timeout_secs (0 = unlimited)
                let reconnect_timeout = instance.config.reconnect.reconnect_timeout_secs;
                let mut elapsed_secs: u64 = 0;
                let mut reconnected = false;
                let mut attempt: u32 = 0;

                loop {
                    // Check timeout (0 = unlimited)
                    if reconnect_timeout > 0 && elapsed_secs >= reconnect_timeout {
                        break;
                    }

                    let backoff = if elapsed_secs < 10 {
                        1
                    } else if elapsed_secs < 60 {
                        3
                    } else if elapsed_secs < 300 {
                        10
                    } else if elapsed_secs < 900 {
                        30
                    } else {
                        60
                    };

                    attempt += 1;
                    tracing::info!(
                        "connection monitor: {} reconnect attempt {} after {}s (next in {}s)",
                        instance.config.name,
                        attempt,
                        elapsed_secs,
                        backoff
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
                    elapsed_secs += backoff;

                    match instance.ssh_client.connect(&auth).await {
                        Ok(()) => {
                            tracing::info!(
                                "connection monitor: {} reconnected on attempt {} after {}s",
                                instance.config.name,
                                attempt,
                                elapsed_secs
                            );
                            // Restore channel opener handle
                            if let Some(h) = instance.ssh_client.get_handle().await {
                                instance.channel_opener.set_handle(h).await;
                            }
                            *instance.status.lock().await = ServerStatus::Connected;
                            instance.broadcast_status(&ServerStatus::Connected).await;

                            // Restart proxy if it was running before disconnect
                            if *instance.proxy_was_running.lock().await {
                                tracing::info!(
                                    "connection monitor: restarting proxy for {}",
                                    instance.config.name
                                );
                                if let Err(e) = instance.start_proxy().await {
                                    tracing::warn!(
                                        "connection monitor: failed to restart proxy for {}: {}",
                                        instance.config.name,
                                        e
                                    );
                                }
                            }

                            // Restart IP detection and health checks
                            if instance.config.ip_check.enabled {
                                instance.start_ip_detection().await;
                            }
                            instance.start_health_checks().await;
                            instance.fire_on_reconnect_triggers().await;

                            reconnected = true;
                            break;
                        }
                        Err(e) => {
                            tracing::warn!(
                                "connection monitor: {} reconnect attempt {} failed: {}",
                                instance.config.name,
                                attempt,
                                e
                            );
                        }
                    }
                }

                if !reconnected {
                    tracing::error!(
                        "connection monitor: {} failed to reconnect after {} attempts ({}s, timeout={})",
                        instance.config.name, attempt, elapsed_secs, if reconnect_timeout == 0 { "unlimited".to_string() } else { reconnect_timeout.to_string() }
                    );
                    *instance.status.lock().await = ServerStatus::Error;
                    instance.broadcast_status(&ServerStatus::Error).await;
                    break;
                }

                // Continue monitoring after successful reconnect
            }
        });

        *self.connection_monitor_task.lock().await = Some(handle);
    }

    /// Stop the connection monitor task
    async fn stop_connection_monitor(&self) {
        if let Some(task) = self.connection_monitor_task.lock().await.take() {
            task.abort();
        }
    }

    /// Track proxy running state for auto-reconnect (called from start_proxy/stop_proxy)
    async fn track_proxy_state(&self, running: bool) {
        *self.proxy_was_running.lock().await = running;
    }

    /// Start health checks for OnProcessDead/OnPortClosed triggers (FP-4.4)
    async fn start_health_checks(&self) {
        // Find triggers that need health checks
        let templates = self.trigger_templates.lock().await;
        let mut health_configs = Vec::new();

        for trigger in &self.config.triggers {
            if !trigger.enabled {
                continue;
            }
            // Look up template to get trigger type
            let template = templates.iter().find(|t| t.id == trigger.template_id);
            if let Some(tmpl) = template {
                use crate::config::TriggerType;
                match tmpl.trigger_type {
                    TriggerType::OnProcessDead => {
                        // Extract process name from parameters or template
                        let process_name = trigger
                            .parameters
                            .get("ProcessName")
                            .or_else(|| trigger.parameters.get("process"))
                            .cloned()
                            .unwrap_or_default();
                        if !process_name.is_empty() {
                            health_configs.push(crate::trigger::health::HealthCheckConfig {
                                check_type: crate::trigger::health::HealthCheckType::Process(
                                    process_name,
                                ),
                                interval_secs: 30,
                            });
                        }
                    }
                    TriggerType::OnPortClosed => {
                        let port_str = trigger
                            .parameters
                            .get("ProtectedPort")
                            .or_else(|| trigger.parameters.get("port"))
                            .cloned()
                            .unwrap_or_default();
                        if let Ok(port) = port_str.parse::<u16>() {
                            health_configs.push(crate::trigger::health::HealthCheckConfig {
                                check_type: crate::trigger::health::HealthCheckType::Port(port),
                                interval_secs: 30,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        drop(templates);

        if health_configs.is_empty() {
            return;
        }

        let ssh = self.ssh_client.clone();
        let templates = self.trigger_templates.lock().await.clone();
        let mut tasks = self.health_check_tasks.lock().await;
        for config in health_configs {
            let task = crate::trigger::health::HealthChecker::start_periodic_check(
                ssh.clone(),
                self.trigger_engine.clone(),
                self.config.id.clone(),
                self.config.name.clone(),
                config,
                self.config.triggers.clone(),
                templates.clone(),
            );
            tasks.push(task);
        }
        if !tasks.is_empty() {
            tracing::info!(
                "started {} health check tasks for {}",
                tasks.len(),
                self.config.name
            );
        }
    }

    /// Start SOCKS5 + HTTP proxy servers
    pub async fn start_proxy(&self) -> Result<()> {
        let mut running = self.proxy_running.lock().await;
        if *running {
            return Ok(());
        }

        let channel_manager = Arc::new(ChannelManager::new(
            self.channel_opener.clone(),
            self.config.proxy.max_channels,
            self.config.proxy.channel_idle_timeout,
        ));

        // Store channel manager reference for active_channel_count queries
        *self.channel_manager.lock().await = Some(channel_manager.clone());

        let mut tasks = Vec::new();

        // If mixed_port is set, start a single mixed server instead of separate SOCKS5/HTTP
        if self.config.proxy.mixed_port > 0 {
            let mixed = MixedProxyServer::new(self.config.proxy.mixed_port, channel_manager);
            let mixed_port = self.config.proxy.mixed_port;
            let mixed_task = tokio::spawn(async move {
                if let Err(e) = mixed.start().await {
                    tracing::error!("Mixed proxy on port {} exited: {}", mixed_port, e);
                }
            });
            tasks.push(mixed_task);
            tracing::info!(
                "proxy started for {} (Mixed:{})",
                self.config.name,
                self.config.proxy.mixed_port
            );
        } else {
            let socks5 = Socks5Server::new(self.config.proxy.socks5_port, channel_manager.clone());
            let socks5_port = self.config.proxy.socks5_port;
            let socks5_task = tokio::spawn(async move {
                if let Err(e) = socks5.start().await {
                    tracing::error!("SOCKS5 server on port {} exited: {}", socks5_port, e);
                }
            });
            tasks.push(socks5_task);

            let http = HttpProxyServer::new(self.config.proxy.http_port, channel_manager);
            let http_port = self.config.proxy.http_port;
            let http_task = tokio::spawn(async move {
                if let Err(e) = http.start().await {
                    tracing::error!("HTTP proxy on port {} exited: {}", http_port, e);
                }
            });
            tasks.push(http_task);
            tracing::info!(
                "proxy started for {} (SOCKS5:{}, HTTP:{})",
                self.config.name,
                self.config.proxy.socks5_port,
                self.config.proxy.http_port
            );
        }

        *self.proxy_tasks.lock().await = tasks;
        *running = true;
        self.track_proxy_state(true).await;
        Ok(())
    }

    /// Stop proxy servers
    pub async fn stop_proxy(&self) -> Result<()> {
        let mut running = self.proxy_running.lock().await;
        if !*running {
            return Ok(());
        }
        let tasks = std::mem::take(&mut *self.proxy_tasks.lock().await);
        for task in tasks {
            task.abort();
        }
        // Clear channel manager reference
        *self.channel_manager.lock().await = None;
        *running = false;
        self.track_proxy_state(false).await;
        tracing::info!("proxy stopped for {}", self.config.name);
        Ok(())
    }

    /// Check if proxy is running
    pub async fn is_proxy_running(&self) -> bool {
        *self.proxy_running.lock().await
    }

    /// Check if SSH connection is established
    pub async fn is_connected(&self) -> bool {
        self.ssh_client.is_connected().await
    }

    /// Get the number of active proxy channels (for graceful shutdown drain)
    /// Returns 0 if proxy is not running. After stop_proxy, all channels are terminated.
    pub async fn active_channel_count(&self) -> u32 {
        if !self.is_proxy_running().await {
            return 0;
        }
        // Query the actual channel manager for real active count
        let mgr = self.channel_manager.lock().await;
        if let Some(mgr) = mgr.as_ref() {
            mgr.active_channel_count()
        } else {
            0
        }
    }

    /// Get the number of active client connections
    pub async fn active_clients(&self) -> u32 {
        if !self.is_proxy_running().await {
            return 0;
        }
        let mgr = self.channel_manager.lock().await;
        if let Some(mgr) = mgr.as_ref() {
            mgr.active_clients()
        } else {
            0
        }
    }

    /// Get total bytes received from clients (upload)
    pub async fn bytes_in(&self) -> u64 {
        if !self.is_proxy_running().await {
            return 0;
        }
        let mgr = self.channel_manager.lock().await;
        if let Some(mgr) = mgr.as_ref() {
            mgr.bytes_in()
        } else {
            0
        }
    }

    /// Get total bytes sent to clients (download)
    pub async fn bytes_out(&self) -> u64 {
        if !self.is_proxy_running().await {
            return 0;
        }
        let mgr = self.channel_manager.lock().await;
        if let Some(mgr) = mgr.as_ref() {
            mgr.bytes_out()
        } else {
            0
        }
    }

    /// Start periodic IP change detection
    async fn start_ip_detection(&self) {
        let ssh_client = self.ssh_client.clone();
        let current_ip = self.current_ip.clone();
        let interval = self.config.ip_check.interval_secs;
        let server_id = self.config.id.clone();
        let server_name = self.config.name.clone();
        let trigger_engine = self.trigger_engine.clone();
        let trigger_templates = self.trigger_templates.clone();
        let server_triggers = self.triggers.lock().await.clone();
        let runtime_state = self.runtime_state.lock().await.clone();

        // Load initial IP from RuntimeStateManager (FP-1.3b) or fall back to config
        let initial_ip = if let Some(ref rs) = runtime_state {
            rs.get_last_known_ip(&server_id).await
        } else {
            None
        };
        let initial_ip = initial_ip.or_else(|| self.config.last_known_ip.clone());

        let task = tokio::spawn(async move {
            let detector = IpChangeDetector::new(interval);
            detector.set_last_ip(initial_ip).await;

            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;

                let handle = ssh_client.get_handle().await;
                let detected = match handle {
                    Some(h) => exec::detect_client_ip(&h).await,
                    None => Err(crate::error::Error::Other("no SSH handle".into())),
                };

                match detected {
                    Ok(new_ip) => {
                        *current_ip.lock().await = Some(new_ip.clone());
                        let old_ip = detector.last_ip().await;
                        let changed = old_ip.as_ref() != Some(&new_ip);
                        if changed {
                            tracing::info!(
                                "IP changed for {}: {:?} -> {}",
                                server_name,
                                old_ip,
                                new_ip
                            );
                            detector.set_last_ip(Some(new_ip.clone())).await;
                            // Persist to RuntimeStateManager (FP-1.3b)
                            if let Some(ref rs) = runtime_state {
                                if let Err(e) = rs.set_last_known_ip(&server_id, &new_ip).await {
                                    tracing::warn!("failed to persist last_known_ip: {}", e);
                                }
                            }
                            let event = TriggerEvent {
                                server_id: server_id.clone(),
                                server_name: server_name.clone(),
                                trigger_type: TriggerType::OnIpChange,
                                new_ip: Some(new_ip),
                                old_ip,
                            };
                            let templates = trigger_templates.lock().await.clone();
                            let _ = trigger_engine
                                .fire_event(&ssh_client, &server_triggers, &templates, &event)
                                .await;
                        }
                    }
                    Err(e) => {
                        tracing::debug!("IP check failed for {}: {}", server_name, e);
                    }
                }
            }
        });

        *self.ip_check_task.lock().await = Some(task);
    }

    async fn fire_on_connect_triggers(&self) {
        let event = TriggerEvent {
            server_id: self.config.id.clone(),
            server_name: self.config.name.clone(),
            trigger_type: TriggerType::OnConnect,
            new_ip: None,
            old_ip: None,
        };
        let templates = self.trigger_templates.lock().await.clone();
        let triggers = self.triggers.lock().await.clone();
        tracing::info!(
            "fire_on_connect_triggers: {} triggers in instance",
            triggers.len()
        );
        for t in &triggers {
            tracing::info!(
                "  trigger: {} type={:?} enabled={}",
                t.name,
                t.trigger_type,
                t.enabled
            );
        }
        let results = self
            .trigger_engine
            .fire_event(&self.ssh_client, &triggers, &templates, &event)
            .await;
        if let Ok(ref results) = results {
            for r in results {
                tracing::info!(
                    "OnConnect trigger '{}' fired: success={}, {}/{}",
                    r.trigger_name,
                    r.success,
                    r.executed_commands,
                    r.total_commands
                );
            }
            let cb = self.trigger_result_callback.lock().await;
            if let Some(ref cb) = *cb {
                cb(event, results);
            }
        }
    }

    async fn fire_on_reconnect_triggers(&self) {
        let event = TriggerEvent {
            server_id: self.config.id.clone(),
            server_name: self.config.name.clone(),
            trigger_type: TriggerType::OnReconnect,
            new_ip: None,
            old_ip: None,
        };
        let templates = self.trigger_templates.lock().await.clone();
        let triggers = self.triggers.lock().await.clone();
        let results = self
            .trigger_engine
            .fire_event(&self.ssh_client, &triggers, &templates, &event)
            .await;
        if let Ok(ref results) = results {
            for r in results {
                tracing::info!(
                    "OnReconnect trigger '{}' fired: success={}, {}/{}",
                    r.trigger_name,
                    r.success,
                    r.executed_commands,
                    r.total_commands
                );
            }
            let cb = self.trigger_result_callback.lock().await;
            if let Some(ref cb) = *cb {
                cb(event, results);
            }
        }
    }

    /// Manually fire a specific trigger by ID
    pub async fn manual_fire_trigger(
        &self,
        trigger_id: &str,
    ) -> Result<crate::trigger::engine::TriggerExecutionResult> {
        let triggers = self.triggers.lock().await.clone();
        self.trigger_engine
            .manual_fire(
                &self.config.id,
                &self.config.name,
                trigger_id,
                &self.ssh_client,
                &triggers,
            )
            .await
    }
}

// === SECTION 2 END ===

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;

    fn test_config() -> ServerConfig {
        ServerConfig {
            id: "srv_test".into(),
            name: "Test".into(),
            ssh: SshConfig {
                host: "1.2.3.4".into(),
                port: 22,
                user: "root".into(),
                auth_method: "key".into(),
                key_path: "~/.ssh/test_key".into(),
                key_auto_generated: false,
                connection_mode: "single".into(),
                skip_hostkey_verify: false,
            },
            proxy: ProxyConfig {
                enabled: false,
                socks5_port: 1080,
                mixed_port: 0,
                http_port: 8080,
                max_channels: 64,
                channel_idle_timeout: 300,
            },
            reconnect: ReconnectConfig::default(),
            ip_check: IpCheckConfig {
                enabled: false,
                interval_secs: 300,
            },
            last_known_ip: None,
            triggers: Vec::new(),
            suppress_firewall_badge: false,
        }
    }

    #[tokio::test]
    async fn test_server_instance_creation() {
        let instance = ServerInstance::new(test_config());
        assert_eq!(instance.id(), "srv_test");
        assert_eq!(instance.name(), "Test");
        assert_eq!(instance.status().await, ServerStatus::Disconnected);
    }

    #[tokio::test]
    async fn test_server_status_from_connection_state() {
        assert_eq!(
            ServerStatus::from(ConnectionState::Connected),
            ServerStatus::Connected
        );
        assert_eq!(
            ServerStatus::from(ConnectionState::Disconnected),
            ServerStatus::Disconnected
        );
        assert_eq!(
            ServerStatus::from(ConnectionState::AuthFailed),
            ServerStatus::AuthFailed
        );
    }

    #[tokio::test]
    async fn test_proxy_not_running_initially() {
        let instance = ServerInstance::new(test_config());
        assert!(!instance.is_proxy_running().await);
    }

    #[tokio::test]
    async fn test_disconnect_when_not_connected() {
        let instance = ServerInstance::new(test_config());
        let result = instance.disconnect().await;
        assert!(result.is_ok());
        assert_eq!(instance.status().await, ServerStatus::Disconnected);
    }
}
