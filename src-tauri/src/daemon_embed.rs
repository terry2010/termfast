//! Daemon embed — FP-6.2
//!
//! Embeds the daemon inside the Tauri process for GUI mode.
//! Starts the daemon socket server and provides IPC bridge to frontend.
//! Loads config from SQLCipher DB and uses keychain for credentials.
//! Starts NetworkMonitor for offline detection (FP-6.9).
//!
//! ## SQLCipher migration (§4.2)
//!
//! The `start_with_sqlcipher()` method implements the DEK resolution flow:
//! 1. DB file doesn't exist → create with DEFAULT_DEK
//! 2. Try DEFAULT_DEK → success (no master password set)
//! 3. Try keychain cached DEK → success (returning user)
//! 4. All fail → return `NeedUnlock` (frontend shows CredentialGate)

use std::sync::Arc;
use termfast_core::config::{
    open_or_recover, ConfigManager, OpenResult, SqlCipherConfigStorage,
    SqlCipherStorage, DEFAULT_DEK,
};
use termfast_core::platform::{SetProxyResult, SystemProxyAdapter, SystemProxyConfig};
use termfast_credential::{CredentialStore, SqlCipherCredentialStore};
use termfast_daemon::{DaemonServer, DaemonState};

/// Error indicating the DB needs a master password to unlock.
/// The frontend should show the CredentialGate UI.
#[derive(Debug)]
pub struct NeedUnlock;

impl std::fmt::Display for NeedUnlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "database needs master password to unlock")
    }
}

impl std::error::Error for NeedUnlock {}

/// Embedded daemon handle
pub struct EmbeddedDaemon {
    pub server: DaemonServer,
    _network_monitor_task: Option<tokio::task::JoinHandle<()>>,
    /// Credential store from SQLCipher path (None for legacy file-based path).
    pub cred_store: Option<Arc<SqlCipherCredentialStore>>,
}

impl EmbeddedDaemon {
    // === SQLCipher startup flow (§4.2) ===

    /// Start with SQLCipher as the unified storage backend.
    ///
    /// DEK resolution flow:
    /// 1. DB file doesn't exist → create with DEFAULT_DEK
    /// 2. Try DEFAULT_DEK → success (no master password set)
    /// 3. Try keychain cached DEK → success (returning user)
    /// 4. All fail → return `NeedUnlock` error (frontend shows CredentialGate)
    pub async fn start_with_sqlcipher(
        db_path: std::path::PathBuf,
    ) -> anyhow::Result<Self> {
        Self::resolve_dek_and_start(db_path, None).await
    }

    /// Start with SQLCipher using a pre-resolved DEK (e.g., from CredentialGate unlock).
    /// Used after the user enters their master password.
    pub async fn start_with_sqlcipher_and_dek(
        db_path: std::path::PathBuf,
        dek: [u8; 32],
    ) -> anyhow::Result<Self> {
        Self::resolve_dek_and_start(db_path, Some(dek)).await
    }

    /// Internal: resolve DEK and start daemon with SqlCipherStorage.
    async fn resolve_dek_and_start(
        db_path: std::path::PathBuf,
        explicit_dek: Option<[u8; 32]>,
    ) -> anyhow::Result<Self> {
        // If an explicit DEK is provided (user just entered password), use it directly
        if let Some(dek) = explicit_dek {
            let conn = open_or_recover(&db_path, &dek)
                .map_err(|e| anyhow::anyhow!("failed to open DB with provided DEK: {}", e))?;
            let storage = Arc::new(
                SqlCipherStorage::from_conn(conn, db_path)
                    .map_err(|e| anyhow::anyhow!("failed to init schema: {}", e))?,
            );
            return Self::build_and_start_with_storage(storage, Some(dek)).await;
        }

        // 1. DB file doesn't exist → create with DEFAULT_DEK
        if !db_path.exists() {
            tracing::info!("DB file doesn't exist, creating with default DEK: {}", db_path.display());
            let storage = SqlCipherStorage::create_new(&db_path, &DEFAULT_DEK)?;
            return Self::build_and_start_with_storage(Arc::new(storage), None).await;
        }

        // 2. Try DEFAULT_DEK (no master password set)
        match open_or_recover(&db_path, &DEFAULT_DEK) {
            Ok(conn) => {
                tracing::info!("DB opened with default DEK (no master password)");
                let storage = Arc::new(
                    SqlCipherStorage::from_conn(conn, db_path)
                        .map_err(|e| anyhow::anyhow!("failed to init schema: {}", e))?,
                );
                return Self::build_and_start_with_storage(storage, None).await;
            }
            Err(OpenResult::WrongKey) | Err(OpenResult::Corrupt)
                if !db_path.with_extension("db.bak").exists() =>
            {
                // Default DEK failed and no backup → user set master password
                tracing::info!("default DEK failed, user has master password");
            }
            Err(e) => {
                return Err(anyhow::anyhow!("failed to open DB: {}", e));
            }
        }

        // 3. Try keychain cached DEK
        if let Some(cached_dek) = load_cached_dek_from_keychain() {
            let dek_bytes: [u8; 32] = cached_dek.as_bytes().try_into().unwrap_or([0u8; 32]);
            match open_or_recover(&db_path, &dek_bytes) {
                Ok(conn) => {
                    tracing::info!("DB opened with keychain cached DEK");
                    let storage = Arc::new(
                        SqlCipherStorage::from_conn(conn, db_path)
                            .map_err(|e| anyhow::anyhow!("failed to init schema: {}", e))?,
                    );
                    return Self::build_and_start_with_storage(storage, Some(dek_bytes)).await;
                }
                Err(OpenResult::WrongKey) => {
                    tracing::warn!("keychain cached DEK is stale, clearing");
                    clear_cached_dek_from_keychain();
                }
                Err(e) => return Err(anyhow::anyhow!("failed to open DB with cached DEK: {}", e)),
            }
        }

        // 4. All DEK resolution attempts failed → need user to enter password
        tracing::info!("no working DEK found, need user unlock");
        Err(anyhow::Error::new(NeedUnlock))
    }

    /// Build daemon state from a unified SqlCipherStorage and start the daemon.
    async fn build_and_start_with_storage(
        storage: Arc<SqlCipherStorage>,
        explicit_dek: Option<[u8; 32]>,
    ) -> anyhow::Result<Self> {
        // Set global storage singleton for PC-specific stores
        crate::storage_singleton::set_storage(storage.clone());

        // Load config from DB
        let config_storage = Arc::new(SqlCipherConfigStorage::new(storage.clone()));
        let config = termfast_core::config::ConfigStorage::load(&*config_storage).unwrap_or_default();

        // Construct ConfigManager with SqlCipherConfigStorage
        let servers_from_config = config.servers.clone();
        let mgr = ConfigManager::with_storage(config, config_storage);

        // Construct SqlCipherCredentialStore (starts unlocked since DB is already open)
        let sql_cred_store = match explicit_dek {
            Some(dek) => Arc::new(SqlCipherCredentialStore::new_with_dek(storage.clone(), dek)),
            None => Arc::new(SqlCipherCredentialStore::new(storage.clone())),
        };
        // Set global credential store singleton for IPC commands
        crate::storage_singleton::set_cred_store(sql_cred_store.clone());
        let cred_store: Arc<dyn CredentialStore> = sql_cred_store.clone();

        // Construct proxy adapter
        let proxy_adapter: Arc<dyn SystemProxyAdapter> = Arc::new(DesktopProxyAdapter);

        // Create daemon state — RuntimeStateManager is created inside
        // DaemonState::with_adapter using a temp DB. We need to override it
        // with our shared storage.
        let state = DaemonState::with_adapter(mgr, cred_store, proxy_adapter);

        // Override runtime_state with our shared SqlCipherStorage
        let runtime_state = Arc::new(termfast_core::config::RuntimeStateManager::new(storage));
        let state = state.with_runtime_state(runtime_state);

        // Populate server_manager with servers from config
        for srv_config in servers_from_config {
            if let Err(e) = state.server_manager.add_server(srv_config).await {
                tracing::warn!("failed to load server from config: {}", e);
            }
        }

        let server = DaemonServer::start(state).await?;
        tracing::info!(
            "embedded daemon (SQLCipher) started on {}",
            server.socket_path().display()
        );

        // Set runtime_state on all existing servers
        {
            let state = server.state();
            let servers = state.server_manager.list_servers().await;
            for s in &servers {
                s.set_runtime_state(state.runtime_state.clone()).await;
            }
        }

        // Start network monitor
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
                        state
                            .broadcast("network:offline", serde_json::json!({ "connected_servers": connected }))
                            .await;
                    }
                    termfast_desktop::network::NetworkState::Online => {
                        tracing::info!("network online — {} servers should reconnect", servers_to_reconnect.len());
                        state
                            .broadcast("network:online", serde_json::json!({ "servers_to_reconnect": servers_to_reconnect }))
                            .await;
                    }
                }
            });
        });

        Ok(Self {
            server,
            _network_monitor_task: Some(monitor_task),
            cred_store: Some(sql_cred_store),
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

// === SECTION 2 END ===

// === Keychain DEK helpers (§4.2) ===

/// Keychain service/entry for caching the DEK (separate from credential master key).
const DEK_KEYCHAIN_SERVICE: &str = "termfast";
const DEK_KEYCHAIN_ENTRY: &str = "sqlcipher_dek";

/// Try to load cached DEK from OS keychain.
/// Returns None on non-desktop platforms or if no cached key exists.
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn load_cached_dek_from_keychain() -> Option<termfast_credential::DerivedKey> {
    use termfast_credential::keychain::KeychainCredentialStore;
    // Reuse the keychain credential store to read the DEK
    let store = KeychainCredentialStore::new();
    store.load(&format!("{}::{}", DEK_KEYCHAIN_SERVICE, DEK_KEYCHAIN_ENTRY)).ok()
        .and_then(|hex_str| {
            // DEK is stored as hex string in keychain
            let bytes = hex::decode(&hex_str).ok()?;
            if bytes.len() == 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                Some(termfast_credential::DerivedKey::from_bytes(&arr))
            } else {
                None
            }
        })
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn load_cached_dek_from_keychain() -> Option<termfast_credential::DerivedKey> {
    None
}

/// Clear cached DEK from OS keychain (when it's stale).
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn clear_cached_dek_from_keychain() {
    use termfast_credential::keychain::KeychainCredentialStore;
    let store = KeychainCredentialStore::new();
    let _ = store.delete(&format!("{}::{}", DEK_KEYCHAIN_SERVICE, DEK_KEYCHAIN_ENTRY));
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn clear_cached_dek_from_keychain() {}

/// Save DEK to OS keychain for future startup caching.
#[allow(dead_code)]
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub fn save_dek_to_keychain(dek: &[u8; 32]) -> anyhow::Result<()> {
    use termfast_credential::keychain::KeychainCredentialStore;
    let store = KeychainCredentialStore::new();
    let hex_str = hex::encode(dek);
    store.save(&format!("{}::{}", DEK_KEYCHAIN_SERVICE, DEK_KEYCHAIN_ENTRY), &hex_str)
        .map_err(|e| anyhow::anyhow!("failed to save DEK to keychain: {}", e))
}

#[allow(dead_code)]
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn save_dek_to_keychain(_dek: &[u8; 32]) -> anyhow::Result<()> {
    Ok(())
}

/// Determine the DB path in the platform data directory.
/// Uses the same directory that config.json used to live in
/// (directories::ProjectDirs data_dir), but no longer depends
/// on config.json existing.
pub fn sqlcipher_db_path() -> std::path::PathBuf {
    match directories::ProjectDirs::from("", "", "termfast") {
        Some(d) => d.data_dir().join("termfast.db"),
        None => std::path::PathBuf::from("termfast.db"),
    }
}
