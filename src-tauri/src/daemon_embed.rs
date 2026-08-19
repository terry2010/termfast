//! Daemon embed — FP-6.2
//!
//! Embeds the daemon inside the Tauri process for GUI mode.
//! Starts the daemon socket server and provides IPC bridge to frontend.
//! Loads config from file (FileConfigStorage) and uses keychain for credentials.
//! Starts NetworkMonitor for offline detection (FP-6.9).
//!
//! ## SQLCipher migration (§4.2)
//!
//! The new `start_with_sqlcipher()` method implements the DEK resolution flow:
//! 1. DB file doesn't exist → create with DEFAULT_DEK
//! 2. Try DEFAULT_DEK → success (no master password set)
//! 3. Try keychain cached DEK → success (returning user)
//! 4. All fail → return `NeedUnlock` (frontend shows CredentialGate)

use std::sync::Arc;
use termfast_core::config::migration::load_config_with_migration_fallback;
use termfast_core::config::{
    open_or_recover, Config, ConfigManager, FileConfigStorage, OpenResult, SqlCipherConfigStorage,
    SqlCipherStorage, DEFAULT_DEK,
};
use termfast_core::platform::{SetProxyResult, SystemProxyAdapter, SystemProxyConfig};
use termfast_credential::{CredentialStore, EncryptedFileCredentialStore, SqlCipherCredentialStore};
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

        // Use load_config_with_migration_fallback so that we know whether
        // the config was loaded from a valid file or fell back to defaults
        // (corrupt JSON, missing file, migration failure). The is_fallback
        // flag is passed to ConfigManager to prevent an empty config from
        // overwriting the backed-up original until the user explicitly
        // adds data.
        let (config, is_fallback) = load_config_with_migration_fallback(storage.path());
        if is_fallback {
            tracing::warn!(
                "config loaded as fallback (empty or corrupt) from {} — \
                 corrupt_load flag set, will not overwrite file until user adds data",
                storage.path().display()
            );
        }

        Self::start_with_config_and_storage_fallback(config, storage, is_fallback).await
    }

    /// Start with a specific config (uses FileConfigStorage for persistence)
    pub async fn start_with_config(config: Config) -> anyhow::Result<Self> {
        let storage = FileConfigStorage::with_default_path()
            .unwrap_or_else(|_| FileConfigStorage::new("config.json"));
        Self::start_with_config_and_storage(config, storage).await
    }

    /// Start with a shared encrypted credential store (used by Tauri GUI).
    /// The store starts locked; the frontend unlocks it via IPC before any
    /// credential access. Until then, credential load/save returns errors
    /// which the daemon handles gracefully (e.g. auto-connect is skipped).
    pub async fn start_with_credential_store(
        cred_store: Arc<EncryptedFileCredentialStore>,
    ) -> anyhow::Result<Self> {
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
        let (config, is_fallback) = load_config_with_migration_fallback(storage.path());
        if is_fallback {
            tracing::warn!(
                "config loaded as fallback (empty or corrupt) from {} — \
                 corrupt_load flag set",
                storage.path().display()
            );
        }
        Self::start_with_config_storage_credential_and_fallback(config, storage, cred_store, is_fallback).await
    }

    /// Start with a specific config and storage (ensures read/write path consistency)
    pub async fn start_with_config_and_storage(
        config: Config,
        storage: FileConfigStorage,
    ) -> anyhow::Result<Self> {
        Self::start_with_config_and_storage_fallback(config, storage, false).await
    }

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

    /// Start with a specific config, storage, and corrupt_load flag.
    /// When is_fallback=true, the ConfigManager will refuse to save an empty
    /// config over the (backed-up) original until the user explicitly adds data.
    pub async fn start_with_config_and_storage_fallback(
        config: Config,
        storage: FileConfigStorage,
        is_fallback: bool,
    ) -> anyhow::Result<Self> {
        // Load servers from config into server_manager before starting daemon
        let servers_from_config = config.servers.clone();
        let mgr = ConfigManager::with_storage_and_corrupt(
            config,
            Arc::new(storage),
            is_fallback,
        );
        let cred_store: Arc<dyn CredentialStore> =
            Arc::new(termfast_credential::KeychainCredentialStore::new());
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
            // Runtime state is now backed by SQLCipher — no explicit load needed.
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
            cred_store: None,
        })
    }

    /// Start with a specific config, storage, and a pre-created encrypted
    /// credential store (shared with Tauri IPC for unlock/migration).
    pub async fn start_with_config_storage_and_credential_store(
        config: Config,
        storage: FileConfigStorage,
        cred_store: Arc<EncryptedFileCredentialStore>,
    ) -> anyhow::Result<Self> {
        Self::start_with_config_storage_credential_and_fallback(config, storage, cred_store, false).await
    }

    /// Start with config, storage, credential store, and corrupt_load flag.
    pub async fn start_with_config_storage_credential_and_fallback(
        config: Config,
        storage: FileConfigStorage,
        cred_store: Arc<EncryptedFileCredentialStore>,
        is_fallback: bool,
    ) -> anyhow::Result<Self> {
        let servers_from_config = config.servers.clone();
        let mgr = ConfigManager::with_storage_and_corrupt(
            config,
            Arc::new(storage),
            is_fallback,
        );
        let cred_store_dyn: Arc<dyn CredentialStore> = cred_store;
        let proxy_adapter: Arc<dyn SystemProxyAdapter> = Arc::new(DesktopProxyAdapter);
        let state = DaemonState::with_adapter(mgr, cred_store_dyn, proxy_adapter);

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

        {
            let state = server.state();
            // Runtime state is now backed by SQLCipher — no explicit load needed.
            let servers = state.server_manager.list_servers().await;
            for s in &servers {
                s.set_runtime_state(state.runtime_state.clone()).await;
            }
        }

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
                            .broadcast(
                                "network:offline",
                                serde_json::json!({ "connected_servers": connected }),
                            )
                            .await;
                    }
                    termfast_desktop::network::NetworkState::Online => {
                        tracing::info!(
                            "network online — {} servers should reconnect",
                            servers_to_reconnect.len()
                        );
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
            cred_store: None,
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

/// Determine the DB path (next to config.json / credentials.enc).
pub fn sqlcipher_db_path() -> std::path::PathBuf {
    match termfast_core::config::FileConfigStorage::with_default_path() {
        Ok(s) => s
            .path()
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("termfast.db"),
        Err(_) => std::path::PathBuf::from("termfast.db"),
    }
}
