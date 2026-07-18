//! Android credential store — encrypted file-based persistent singleton.
//!
//! Uses `EncryptedFileCredentialStore` backed by an AES-256-GCM encrypted
//! file in the app's private data directory. The store is initialized once
//! with `init_credential_store` and shared across all JNI calls via a static
//! `OnceLock`. The store starts locked; the Android UI must call
//! `nativeCredentialUnlock` / `nativeCredentialInitialize` before any
//! credential access.

use std::path::PathBuf;
use std::sync::OnceLock;
use termfast_credential::EncryptedFileCredentialStore;

static CREDENTIAL_STORE: OnceLock<EncryptedFileCredentialStore> = OnceLock::new();

/// Initialize the credential store with the app's data directory.
/// Must be called once after `nativeSetDataDir`.
pub fn init_credential_store(data_dir: &str) {
    let path = PathBuf::from(data_dir).join("credentials.enc");
    let _ = CREDENTIAL_STORE.set(EncryptedFileCredentialStore::open(path));
}

/// Get the singleton credential store. Falls back to a temporary encrypted
/// store if `init_credential_store` was never called (e.g. in tests).
pub fn android_credential_store() -> &'static EncryptedFileCredentialStore {
    CREDENTIAL_STORE.get_or_init(|| {
        // Fallback: use a temp directory (should not happen in production)
        let path = std::env::temp_dir().join("termfast_credentials_fallback.enc");
        EncryptedFileCredentialStore::open(path)
    })
}
