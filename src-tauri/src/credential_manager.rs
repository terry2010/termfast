//! Credential manager — bridges `EncryptedFileCredentialStore` to Tauri IPC.
//!
//! Responsibilities:
//! - Owns the `Arc<EncryptedFileCredentialStore>` shared with the daemon.
//! - Caches the derived key in the OS keychain (via `keyring` crate) so the
//!   user only enters the master password once per device.
//! - Exposes IPC commands for unlock / setup / migration / password change /
//!   reset / export / import / lock.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::State;
use termfast_credential::EncryptedFileCredentialStore;

/// Keychain entry name for the cached derived key.
const KEYCHAIN_SERVICE: &str = "termfast";
const KEYCHAIN_ENTRY: &str = "credential_master_key";

/// In-memory cache of the keychain value to avoid repeated Touch ID prompts.
/// On macOS, every keychain access triggers a Touch ID/password prompt.
/// We cache the result so we only hit the keychain once per process.
/// Uses Mutex (not OnceLock) to prevent race conditions when multiple
/// callers try to load the key simultaneously (e.g. React StrictMode).
/// Outer Option = whether we've queried yet; inner Option = the key value.
static CACHED_KEYCHAIN_KEY: Mutex<Option<Option<termfast_credential::DerivedKey>>> = Mutex::new(None);

/// Tauri-managed state holding the encrypted credential store.
pub struct CredentialState {
    pub store: Arc<EncryptedFileCredentialStore>,
}

/// Credential status returned to the frontend.
#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialStatus {
    /// No password set yet — credentials in memory only (pending mode).
    Pending,
    /// Legacy plaintext file detected, needs migration.
    NeedsMigration,
    /// Encrypted file exists but not yet unlocked.
    Locked,
    /// Unlocked and ready to use.
    Unlocked,
}

/// Determine the credential file path from the config storage path.
/// The credential file sits next to config.json as `credentials.enc`.
pub fn credential_file_path() -> PathBuf {
    match termfast_core::config::FileConfigStorage::with_default_path() {
        Ok(s) => s.path().parent().unwrap_or_else(|| std::path::Path::new(".")).join("credentials.enc"),
        Err(_) => PathBuf::from("credentials.enc"),
    }
}

/// Try to read the cached derived key from the OS keychain.
/// Uses an in-memory cache with Mutex to avoid repeated Touch ID prompts.
/// The Mutex prevents race conditions when multiple callers try to load
/// the key simultaneously (e.g. React StrictMode double-invoke).
fn load_cached_key() -> Option<termfast_credential::DerivedKey> {
    let mut guard = CACHED_KEYCHAIN_KEY.lock().unwrap();
    if let Some(cached) = &*guard {
        // Already queried — return cached result (no keychain access).
        return cached.clone();
    }
    // First time — actually query the keychain while holding the lock.
    let result = load_cached_key_from_keychain();
    *guard = Some(result.clone());
    result
}

/// Actually query the OS keychain (triggers Touch ID on macOS).
fn load_cached_key_from_keychain() -> Option<termfast_credential::DerivedKey> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ENTRY).ok()?;
    let encoded: String = match entry.get_password() {
        Ok(s) => s,
        Err(keyring::Error::NoEntry) => return None,
        Err(e) => {
            tracing::warn!("failed to read cached key from keychain: {}", e);
            return None;
        }
    };
    let bytes = base64_decode(&encoded)?;
    if bytes.len() != 32 {
        tracing::warn!("cached key has wrong length: {}", bytes.len());
        return None;
    }
    Some(termfast_credential::DerivedKey::from_bytes(&bytes))
}

/// Store the derived key in the OS keychain for future auto-unlock.
/// Also updates the in-memory cache so subsequent reads don't hit the keychain.
fn save_cached_key(key: &termfast_credential::DerivedKey) {
    // Update in-memory cache first.
    *CACHED_KEYCHAIN_KEY.lock().unwrap() = Some(Some(key.clone()));
    let encoded = base64_encode(key.as_bytes());
    match keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ENTRY) {
        Ok(entry) => {
            if let Err(e) = entry.set_password(&encoded) {
                tracing::warn!("failed to cache derived key in keychain: {}", e);
            }
        }
        Err(e) => {
            tracing::warn!("failed to create keychain entry for derived key: {}", e);
        }
    }
}

/// Delete the cached derived key from the OS keychain.
fn delete_cached_key() {
    // Clear in-memory cache.
    *CACHED_KEYCHAIN_KEY.lock().unwrap() = Some(None);
    if let Ok(entry) = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ENTRY) {
        let _: Result<(), _> = entry.delete_credential();
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(bytes.len() * 4 / 3 + 4);
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8) | (bytes[i + 2] as u32);
        s.push(TABLE[((n >> 18) & 63) as usize] as char);
        s.push(TABLE[((n >> 12) & 63) as usize] as char);
        s.push(TABLE[((n >> 6) & 63) as usize] as char);
        s.push(TABLE[(n & 63) as usize] as char);
        i += 3;
    }
    let rem = bytes.len() - i;
    if rem == 1 {
        let n = (bytes[i] as u32) << 16;
        s.push(TABLE[((n >> 18) & 63) as usize] as char);
        s.push(TABLE[((n >> 12) & 63) as usize] as char);
        s.push_str("==");
    } else if rem == 2 {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8);
        s.push(TABLE[((n >> 18) & 63) as usize] as char);
        s.push(TABLE[((n >> 12) & 63) as usize] as char);
        s.push(TABLE[((n >> 6) & 63) as usize] as char);
        s.push('=');
    }
    s
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut buf = Vec::with_capacity(s.len() * 3 / 4);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 4 <= bytes.len() {
        let mut vals = [0u8; 4];
        let mut pad = 0;
        for j in 0..4 {
            let b = bytes[i + j];
            if b == b'=' {
                vals[j] = 0;
                pad += 1;
            } else {
                vals[j] = TABLE.iter().position(|&c| c == b)? as u8;
            }
        }
        let n = ((vals[0] as u32) << 18)
            | ((vals[1] as u32) << 12)
            | ((vals[2] as u32) << 6)
            | (vals[3] as u32);
        buf.push((n >> 16) as u8);
        if pad < 2 {
            buf.push((n >> 8) as u8);
        }
        if pad < 1 {
            buf.push(n as u8);
        }
        i += 4;
    }
    Some(buf)
}

// === SECTION 1 END ===

#[tauri::command]
pub async fn ipc_credential_status(
    state: State<'_, CredentialState>,
) -> Result<CredentialStatus, String> {
    let store = &state.store;
    if store.is_pending() {
        Ok(CredentialStatus::Pending)
    } else if store.is_legacy_plaintext() {
        Ok(CredentialStatus::NeedsMigration)
    } else if store.is_unlocked() {
        Ok(CredentialStatus::Unlocked)
    } else {
        Ok(CredentialStatus::Locked)
    }
}

#[tauri::command]
pub async fn ipc_initialize_credentials(
    state: State<'_, CredentialState>,
    master_password: String,
) -> Result<(), String> {
    let store = state.store.clone();
    // Argon2id is CPU-intensive — run on blocking pool to avoid stalling
    // the async executor and freezing the UI.
    tauri::async_runtime::spawn_blocking(move || {
        store.initialize(&master_password).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    // Cache the derived key in OS keychain.
    if let Some(key) = state.store.derived_key() {
        save_cached_key(&key);
    }
    Ok(())
}

#[tauri::command]
pub async fn ipc_unlock_credentials(
    state: State<'_, CredentialState>,
    master_password: String,
) -> Result<(), String> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.unlock(&master_password).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    // Cache the derived key for future auto-unlock.
    if let Some(key) = state.store.derived_key() {
        save_cached_key(&key);
    }
    Ok(())
}

#[tauri::command]
pub async fn ipc_try_cached_unlock(
    state: State<'_, CredentialState>,
) -> Result<bool, String> {
    if state.store.is_unlocked() {
        return Ok(true);
    }
    if let Some(key) = load_cached_key() {
        let store = state.store.clone();
        // decrypt() reads the credential file — run on blocking pool.
        let result = tauri::async_runtime::spawn_blocking(move || {
            store.unlock_with_key(key)
        })
        .await
        .map_err(|e| e.to_string())?;
        match result {
            Ok(()) => return Ok(true),
            Err(e) => {
                tracing::warn!("cached key failed to unlock: {}", e);
                // Cached key is stale (e.g. password was changed on another
                // device and file was synced). Delete it so user is prompted.
                delete_cached_key();
            }
        }
    }
    Ok(false)
}

#[tauri::command]
pub async fn ipc_lock_credentials(state: State<'_, CredentialState>) -> Result<(), String> {
    state.store.lock();
    delete_cached_key();
    Ok(())
}

#[tauri::command]
pub async fn ipc_migrate_credentials(
    state: State<'_, CredentialState>,
    master_password: String,
) -> Result<(), String> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.migrate(&master_password).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    if let Some(key) = state.store.derived_key() {
        save_cached_key(&key);
    }
    Ok(())
}

#[tauri::command]
pub async fn ipc_change_credential_password(
    state: State<'_, CredentialState>,
    old_password: String,
    new_password: String,
) -> Result<(), String> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .change_password(&old_password, &new_password)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    // Update the cached key in OS keychain.
    if let Some(key) = state.store.derived_key() {
        save_cached_key(&key);
    }
    Ok(())
}

#[tauri::command]
pub async fn ipc_reset_credentials(state: State<'_, CredentialState>) -> Result<(), String> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.reset().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    delete_cached_key();
    Ok(())
}

#[tauri::command]
pub async fn ipc_export_credentials(
    state: State<'_, CredentialState>,
    dest_path: String,
) -> Result<(), String> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .export_to(std::path::Path::new(&dest_path))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn ipc_import_credentials(
    state: State<'_, CredentialState>,
    src_path: String,
    master_password: String,
) -> Result<(), String> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .import_from(std::path::Path::new(&src_path), &master_password)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    // Import succeeded and store is now unlocked with the new credentials.
    // Cache the new derived key for future auto-unlock.
    if let Some(key) = state.store.derived_key() {
        save_cached_key(&key);
    }
    Ok(())
}

// === SECTION 2 END ===
