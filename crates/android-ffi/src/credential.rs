//! Android credential store — SQLCipher-backed persistent singleton.
//!
//! Uses `SqlCipherCredentialStore` backed by the shared `SqlCipherStorage`
//! singleton (initialized in `config::init_sqlcipher_storage()`).
//! The store is initialized once after the DB is opened and shared across
//! all JNI calls via a static `OnceLock`.
//!
//! Lifecycle:
//! 1. `init_sqlcipher_storage(data_dir)` — opens DB with DEFAULT_DEK
//! 2. `init_credential_store()` — wraps the storage in SqlCipherCredentialStore
//! 3. If user has a master password, Kotlin calls `nativeCredentialUnlockWithKey`
//!    → `init_sqlcipher_storage_with_key(data_dir, dek)` → `init_credential_store()`

use std::sync::OnceLock;
use termfast_credential::SqlCipherCredentialStore;

static CREDENTIAL_STORE: OnceLock<SqlCipherCredentialStore> = OnceLock::new();

/// Initialize the credential store from the shared SqlCipherStorage singleton.
/// Must be called after `init_sqlcipher_storage()` or `init_sqlcipher_storage_with_key()`.
pub fn init_credential_store() {
    let storage = crate::config::sqlcipher_storage().clone();
    let _ = CREDENTIAL_STORE.set(SqlCipherCredentialStore::new(storage));
}

/// Get the singleton credential store. Falls back to creating a default
/// store if `init_credential_store` was never called (should not happen in production).
pub fn android_credential_store() -> &'static SqlCipherCredentialStore {
    CREDENTIAL_STORE.get_or_init(|| {
        // Fallback: should not happen in production. Create from storage singleton.
        let storage = crate::config::sqlcipher_storage().clone();
        SqlCipherCredentialStore::new(storage)
    })
}
