//! Daemon embed — FP-6.2
//!
//! Embeds the daemon inside the Tauri process for GUI mode.
//! Starts the daemon socket server and provides IPC bridge to frontend.
//! Loads config from file (FileConfigStorage) and uses keychain for credentials.
//! Starts NetworkMonitor for offline detection (FP-6.9).

use std::sync::Arc;
use termfast_core::config::migration::load_config_with_migration;
use termfast_core::config::{Config, ConfigManager, FileConfigStorage};
use termfast_core::platform::{SetProxyResult, SystemProxyAdapter, SystemProxyConfig};
use termfast_credential::KeychainCredentialStore;
use termfast_daemon::{DaemonServer, DaemonState};

/// Embedded daemon handle
pub struct EmbeddedDaemon {
    pub server: DaemonServer,
    _network_monitor_task: Option<tokio::task::JoinHandle<()>>,
}

impl EmbeddedDaemon {
    /// Start the embedded daemon.
    /// Loads config from the platform default path (FileConfigStorage).
    /// Uses KeychainCredentialStore for credential persistence.
    /// Starts NetworkMonitor for offline/online detection (FP-6.9).
    pub async fn start() -> anyhow::Result<Self> {
        let storage = match FileConfigStorage::with_default_path() {
            Ok(s) => {
                tracing::info!("config path: {}", s.path().display());
                s
            }
            Err(e) => {
                tracing::warn!("failed to determine config path, using default: {}", e);
                FileConfigStorage::new("config.json")
            }
        };

        // Use load_config_with_migration instead of storage.load() so that a
        // corrupt config file is backed up (config.json.corrupt.<ts>) before
        // falling back to defaults. The previous storage.load().unwrap_or_default()
        // path silently returned an empty Config (servers: []) on any parse error,
        // and a subsequent save would overwrite the user's real config with the
        // empty one — permanently destroying all configured servers.
        let config = match load_config_with_migration(storage.path()) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(
                    "failed to load config from {}: {} — starting with empty config \
                     (existing file was backed up if it was corrupt)",
                    storage.path().display(),
                    e
                );
                Config::default()
            }
        };

        Self::start_with_config_and_storage(config, storage).await
    }

    /// Start with a specific config (uses FileConfigStorage for persistence)
    pub async fn start_with_config(config: Config) -> anyhow::Result<Self> {
        let storage = FileConfigStorage::with_default_path()
            .unwrap_or_else(|_| FileConfigStorage::new("config.json"));
        Self::start_with_config_and_storage(config, storage).await
    }

    /// Start with a specific config and storage (ensures read/write path consistency)
    pub async fn start_with_config_and_storage(
        config: Config,
        storage: FileConfigStorage,
    ) -> anyhow::Result<Self> {
        // Load servers from config into server_manager before starting daemon
        let servers_from_config = config.servers.clone();
        let mgr = ConfigManager::with_storage(config, Arc::new(storage));
        let cred_store = Arc::new(KeychainCredentialStore::new());
        let proxy_adapter: Arc<dyn SystemProxyAdapter> = Arc::new(DesktopProxyAdapter);
        let state = DaemonState::with_adapter(mgr, cred_store, proxy_adapter);

        // Populate server_manager with servers from the config file
        for srv_config in servers_from_config {
            if let Err(e) = state.server_manager.add_server(srv_config).await {
                tracing::warn!("failed to load server from config: {}", e);
            }
        }

        let server = DaemonServer::start(state).await?;
        tracing::info!(
            "embedded daemon started on {}",
            server.socket_path().display()
        );

        // Set runtime_state on all existing servers and load persisted IPs (FP-1.3b)
        {
            let state = server.state();
            // Load runtime state from disk
            if let Err(e) = state.runtime_state.load().await {
                tracing::warn!("failed to load runtime_state: {}", e);
            }
            // Set runtime_state on all existing server instances
            let servers = state.server_manager.list_servers().await;
            for s in &servers {
                s.set_runtime_state(state.runtime_state.clone()).await;
            }
        }

        // Start network monitor for offline detection (FP-6.9)
        let monitor = Arc::new(termfast_desktop::network::NetworkMonitor::new());
        let state_clone = server.state().clone();
        let monitor_task = monitor.start_monitoring(5, move |new_state, servers_to_reconnect| {
            let state = state_clone.clone();
            tokio::spawn(async move {
                match new_state {
                    termfast_desktop::network::NetworkState::Offline => {
                        tracing::warn!("network offline — pausing reconnection");
                        let servers = state.server_manager.list_servers().await;
                        let mut connected = Vec::new();
                        for s in &servers {
                            if s.is_connected().await {
                                connected.push(s.id().to_string());
                            }
                        }
                        // Broadcast offline event with connected server list
                        state
                            .broadcast(
                                "network:offline",
                                serde_json::json!({
                                    "connected_servers": connected,
                                }),
                            )
                            .await;
                    }
                    termfast_desktop::network::NetworkState::Online => {
                        tracing::info!(
                            "network online — {} servers should reconnect",
                            servers_to_reconnect.len()
                        );
                        // Broadcast online event — frontend/ServerInstance will handle reconnection
                        state
                            .broadcast(
                                "network:online",
                                serde_json::json!({
                                    "servers_to_reconnect": servers_to_reconnect,
                                }),
                            )
                            .await;
                    }
                }
            });
        });

        Ok(Self {
            server,
            _network_monitor_task: Some(monitor_task),
        })
    }

    pub fn socket_path(&self) -> &std::path::Path {
        self.server.socket_path()
    }

    pub async fn shutdown(&self) {
        self.server.shutdown().await;
    }
}

impl Drop for EmbeddedDaemon {
    fn drop(&mut self) {
        if let Some(task) = &self._network_monitor_task {
            task.abort();
        }
    }
}

// === SECTION 1 END ===

/// Desktop proxy adapter — bridges core's SystemProxyAdapter to desktop's PlatformAdapter
struct DesktopProxyAdapter;

#[async_trait::async_trait]
impl SystemProxyAdapter for DesktopProxyAdapter {
    async fn set_system_proxy(&self, config: &SystemProxyConfig) -> anyhow::Result<SetProxyResult> {
        let adapter = termfast_desktop::platform::get_platform_adapter();
        let desktop_config = termfast_desktop::platform::SystemProxyConfig {
            server_id: config.server_id.clone(),
            socks5_port: config.socks5_port,
            http_port: config.http_port,
        };
        let result = adapter.set_system_proxy(&desktop_config).await?;
        Ok(SetProxyResult {
            needs_privilege: result.needs_privilege,
            success: result.success,
            message: result.message,
        })
    }

    async fn clear_system_proxy(&self) -> anyhow::Result<SetProxyResult> {
        let adapter = termfast_desktop::platform::get_platform_adapter();
        let result = adapter.clear_system_proxy().await?;
        Ok(SetProxyResult {
            needs_privilege: result.needs_privilege,
            success: result.success,
            message: result.message,
        })
    }

    async fn get_system_proxy(&self) -> anyhow::Result<Option<SystemProxyConfig>> {
        let adapter = termfast_desktop::platform::get_platform_adapter();
        let result = adapter.get_system_proxy().await?;
        Ok(result.map(|c| SystemProxyConfig {
            server_id: c.server_id,
            socks5_port: c.socks5_port,
            http_port: c.http_port,
        }))
    }
}
