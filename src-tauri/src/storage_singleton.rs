//! Global SqlCipherStorage singleton for the Tauri desktop app.
//!
//! Set during daemon startup, accessed by ecdh_key_store, device_key_store,
//! and pairing_store for PC-specific data storage.

use std::sync::{Arc, OnceLock};
use termfast_core::config::SqlCipherStorage;

static STORAGE: OnceLock<Arc<SqlCipherStorage>> = OnceLock::new();

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
