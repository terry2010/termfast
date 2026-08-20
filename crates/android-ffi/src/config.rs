//! Android ConfigStorage implementation.
//!
//! On Android, config and runtime state are persisted under the app private
//! directory passed in from Kotlin (`context.getFilesDir()`).
//!
//! ## SQLCipher migration (§4.2)
//!
//! The unified `SqlCipherStorage` is initialized as a singleton in
//! `init_sqlcipher_storage()`. All config, credentials, and runtime state
//! share the same DB file (`termfast.db`).

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use termfast_core::config::{
    open_or_recover, ConfigManager, OpenResult, RuntimeStateManager, SqlCipherConfigStorage,
    SqlCipherStorage, DEFAULT_DEK,
};

/// Global SqlCipherStorage singleton for Android.
pub(crate) static SQLCIPHER_STORAGE: OnceLock<Arc<SqlCipherStorage>> = OnceLock::new();

/// Initialize the SqlCipherStorage singleton for the given data directory.
/// Must be called once after `nativeSetDataDir`.
/// Uses DEFAULT_DEK — if the user has set a master password, this will fail
/// and the Kotlin layer should call `nativeCredentialUnlockWithKey` first.
pub fn init_sqlcipher_storage(data_dir: &str) -> Result<(), String> {
    let db_path = PathBuf::from(data_dir).join("termfast.db");
    let storage = if !db_path.exists() {
        SqlCipherStorage::create_new(&db_path, &DEFAULT_DEK)
            .map_err(|e| format!("failed to create DB: {}", e))?
    } else {
        match open_or_recover(&db_path, &DEFAULT_DEK) {
            Ok(conn) => SqlCipherStorage::from_conn(conn, db_path)
                .map_err(|e| format!("failed to init schema: {}", e))?,
            Err(OpenResult::WrongKey) | Err(OpenResult::Corrupt) => {
                // DB exists but key doesn't match — user has set a master password.
                // Kotlin layer should call unlock_with_key() first, then init again.
                return Err("NEED_UNLOCK".to_string());
            }
            Err(e) => return Err(format!("failed to open DB: {}", e)),
        }
    };
    let _ = SQLCIPHER_STORAGE.set(Arc::new(storage));
    Ok(())
}

/// Initialize SqlCipherStorage with a specific DEK (after user unlock).
pub fn init_sqlcipher_storage_with_key(data_dir: &str, dek: &[u8; 32]) -> Result<(), String> {
    let db_path = PathBuf::from(data_dir).join("termfast.db");
    let conn = open_or_recover(&db_path, dek)
        .map_err(|e| format!("failed to open DB with key: {}", e))?;
    let storage = SqlCipherStorage::from_conn(conn, db_path)
        .map_err(|e| format!("failed to init schema: {}", e))?;
    let _ = SQLCIPHER_STORAGE.set(Arc::new(storage));
    Ok(())
}

/// Get the singleton SqlCipherStorage. Panics if not initialized.
pub fn sqlcipher_storage() -> &'static Arc<SqlCipherStorage> {
    SQLCIPHER_STORAGE.get().expect("SqlCipherStorage not initialized")
}

/// Check if SqlCipherStorage has been initialized.
pub fn is_sqlcipher_initialized() -> bool {
    SQLCIPHER_STORAGE.get().is_some()
}

/// Legacy: config manager from config.json (used during migration period).
pub fn config_manager_for_dir(dir: PathBuf) -> anyhow::Result<ConfigManager> {
    let config_path = dir.join("config.json");
    let cm = ConfigManager::load(&config_path)?;
    Ok(cm)
}

/// Create a ConfigManager backed by SqlCipherConfigStorage.
pub fn config_manager_from_sqlcipher() -> anyhow::Result<ConfigManager> {
    let storage = sqlcipher_storage().clone();
    let config_storage = Arc::new(SqlCipherConfigStorage::new(storage));
    let config = termfast_core::config::ConfigStorage::load(&*config_storage).unwrap_or_default();
    Ok(ConfigManager::with_storage(config, config_storage))
}

/// Create a RuntimeStateManager backed by the singleton SqlCipherStorage.
pub fn runtime_state_manager() -> RuntimeStateManager {
    RuntimeStateManager::new(sqlcipher_storage().clone())
}

/// Legacy: runtime state manager from a specific directory.
pub fn runtime_state_manager_for_dir(dir: PathBuf) -> RuntimeStateManager {
    let db_path = dir.join("termfast.db");
    let storage = if db_path.exists() {
        SqlCipherStorage::open(&db_path, &DEFAULT_DEK).ok()
    } else {
        SqlCipherStorage::create_new(&db_path, &DEFAULT_DEK).ok()
    };
    match storage {
        Some(s) => RuntimeStateManager::new(Arc::new(s)),
        None => {
            tracing::warn!("failed to open runtime DB, using temp");
            let temp_path = dir.join("runtime_tmp.db");
            let s = SqlCipherStorage::create_new(&temp_path, &DEFAULT_DEK)
                .expect("failed to create temp runtime DB");
            RuntimeStateManager::new(Arc::new(s))
        }
    }
}
