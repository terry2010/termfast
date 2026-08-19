//! Global SqlCipherStorage singleton for the Tauri desktop app.
//!
//! Set during daemon startup, accessed by ecdh_key_store, device_key_store,
//! and pairing_store for PC-specific data storage.

use std::sync::{Arc, OnceLock};
use termfast_core::config::SqlCipherStorage;
use termfast_credential::SqlCipherCredentialStore;

static STORAGE: OnceLock<Arc<SqlCipherStorage>> = OnceLock::new();
static CRED_STORE: OnceLock<Arc<SqlCipherCredentialStore>> = OnceLock::new();
/// Guard to prevent concurrent daemon startup (race between initial spawn
/// and ipc_try_cached_unlock). Once set, subsequent startup attempts are no-ops.
static DAEMON_STARTED: OnceLock<()> = OnceLock::new();

/// Set the global SqlCipherStorage. Called once during daemon startup.
pub fn set_storage(storage: Arc<SqlCipherStorage>) {
    let _ = STORAGE.set(storage);
}

/// Get the global SqlCipherStorage. Returns None if not yet initialized.
pub fn get_storage() -> Option<&'static Arc<SqlCipherStorage>> {
    STORAGE.get()
}

/// Check if the storage singleton has been initialized.
#[allow(dead_code)]
pub fn is_initialized() -> bool {
    STORAGE.get().is_some()
}

/// Set the global SqlCipherCredentialStore. Called once during daemon startup.
pub fn set_cred_store(store: Arc<SqlCipherCredentialStore>) {
    let _ = CRED_STORE.set(store);
}

/// Get the global SqlCipherCredentialStore. Returns None if not yet initialized
/// (e.g., NeedUnlock case where the DB couldn't be opened).
pub fn get_cred_store() -> Option<&'static Arc<SqlCipherCredentialStore>> {
    CRED_STORE.get()
}

/// Mark daemon as started. Returns true if this is the first caller (caller
/// should proceed with setup_daemon_after_start), false if daemon was already
/// started (caller should skip — race prevention).
pub fn try_mark_daemon_started() -> bool {
    DAEMON_STARTED.set(()).is_ok()
}

/// Check if daemon has been started.
#[allow(dead_code)]
pub fn is_daemon_started() -> bool {
    DAEMON_STARTED.get().is_some()
}
