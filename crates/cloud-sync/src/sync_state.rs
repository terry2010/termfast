//! Encrypted local sync state — records the last-known cloud hash
//! and sync metadata per provider.
//!
//! Stored as `sync_state.enc` with format `[TFSS][version][salt][nonce][ciphertext]`.
//! Encrypted with the same Argon2id + AES-256-GCM scheme as the config file,
//! using magic `TFSS` to distinguish from config (`TFSC`).
//!
//! **Why encrypted**: a plaintext `sync_state.json` would allow an attacker
//! with brief device access to tamper with `last_hash`, bypassing conflict
//! detection and enabling rollback attacks. GCM authentication prevents this.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::sync_crypto::{decrypt_with_magic, encrypt_with_magic, MAGIC_STATE};

/// Per-provider sync state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderState {
    /// Last-known cloud hash of the config file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_hash: Option<String>,
    /// Device name from the last sync.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_device_name: Option<String>,
    /// Timestamp of the last sync (RFC 3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_updated_at: Option<String>,
    /// mtime (unix epoch seconds, as string) of the local config.json file
    /// at the time of last sync. Used to detect local modifications since
    /// last sync — if this differs from the current config.json mtime,
    /// the local data has changed and download should not be blocked
    /// by no_update even if the cloud hash is unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_local_mtime: Option<String>,
}

/// The full sync state, mapping provider name → state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncState {
    #[serde(default)]
    pub baidu: ProviderState,
    #[serde(default)]
    pub dropbox: ProviderState,
}

/// Info returned to the frontend for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastSyncInfo {
    pub device_name: Option<String>,
    pub updated_at: Option<String>,
}

impl SyncState {
    /// Get the provider state by provider string ("baidu" / "dropbox").
    pub fn get(&self, provider: &str) -> &ProviderState {
        match provider {
            "baidu" => &self.baidu,
            "dropbox" => &self.dropbox,
            _ => &self.dropbox, // fallback
        }
    }

    /// Get a mutable reference to the provider state.
    pub fn get_mut(&mut self, provider: &str) -> &mut ProviderState {
        match provider {
            "baidu" => &mut self.baidu,
            "dropbox" => &mut self.dropbox,
            _ => &mut self.dropbox,
        }
    }

    /// Get the last-known hash for a provider.
    pub fn last_hash(&self, provider: &str) -> Option<&str> {
        self.get(provider).last_hash.as_deref()
    }

    /// Get the last-recorded local config mtime for a provider.
    pub fn last_local_mtime(&self, provider: &str) -> Option<&str> {
        self.get(provider).last_local_mtime.as_deref()
    }

    /// Record sync info (hash + metadata) for a provider.
    pub fn set_sync_info(
        &mut self,
        provider: &str,
        hash: String,
        device_name: String,
        updated_at: String,
        local_mtime: Option<String>,
    ) {
        let ps = self.get_mut(provider);
        ps.last_hash = Some(hash);
        ps.last_device_name = Some(device_name);
        ps.last_updated_at = Some(updated_at);
        ps.last_local_mtime = local_mtime;
    }

    /// Get the last sync info (device_name + updated_at) for display.
    pub fn last_sync_info(&self, provider: &str) -> LastSyncInfo {
        let ps = self.get(provider);
        LastSyncInfo {
            device_name: ps.last_device_name.clone(),
            updated_at: ps.last_updated_at.clone(),
        }
    }

    /// Serialize to JSON bytes.
    fn to_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Deserialize from JSON bytes.
    fn from_json(data: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(data)
    }
}

/// Load sync state from the encrypted file.
/// Returns an empty state if the file doesn't exist or decryption fails
/// (e.g. wrong password after a password change). This is intentional —
/// sync_state is a cache, not a security boundary, so failure degrades
/// gracefully to "first sync" behavior.
///
/// **Must be called on a `spawn_blocking` thread** (Argon2id).
pub fn load_state(path: &Path, password: &str) -> SyncState {
    let blob = match std::fs::read(path) {
        Ok(data) => data,
        Err(_) => return SyncState::default(),
    };
    match decrypt_with_magic(MAGIC_STATE, password, &blob) {
        Ok(plaintext) => SyncState::from_json(&plaintext).unwrap_or_default(),
        Err(e) => {
            tracing::warn!("sync_state decrypt failed, using empty state: {}", e);
            SyncState::default()
        }
    }
}

/// Save sync state to the encrypted file.
/// Writes atomically (temp file + rename) with 0600 permissions on Unix.
///
/// **Must be called on a `spawn_blocking` thread** (Argon2id).
pub fn save_state(path: &Path, password: &str, state: &SyncState) -> Result<(), SyncStateError> {
    let plaintext = state.to_json()?;
    let blob = encrypt_with_magic(MAGIC_STATE, password, &plaintext)
        .map_err(|e| SyncStateError::Encrypt(e.to_string()))?;

    write_atomic(path, &blob)?;
    Ok(())
}

/// Write data to a file atomically (write to temp, then rename).
/// Sets 0600 permissions on Unix.
fn write_atomic(path: &Path, data: &[u8]) -> Result<(), SyncStateError> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data).map_err(|e| SyncStateError::Io(e.to_string()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }

    std::fs::rename(&tmp, path).map_err(|e| SyncStateError::Io(e.to_string()))?;
    Ok(())
}

/// Errors from sync state operations.
#[derive(Debug, thiserror::Error)]
pub enum SyncStateError {
    #[error("IO error: {0}")]
    Io(String),
    #[error("encrypt error: {0}")]
    Encrypt(String),
    #[error("JSON error: {0}")]
    Json(String),
}

impl From<serde_json::Error> for SyncStateError {
    fn from(e: serde_json::Error) -> Self {
        SyncStateError::Json(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> SyncState {
        let mut s = SyncState::default();
        s.set_sync_info(
            "dropbox",
            "abc123hash".to_string(),
            "Terry-MacBook".to_string(),
            "2026-07-21T10:00:00Z".to_string(),
            Some("1784718000".to_string()),
        );
        s
    }

    #[test]
    fn test_state_encrypt_decrypt_roundtrip() {
        let dir = std::env::temp_dir().join("sync_state_test_roundtrip");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sync_state.enc");

        let state = test_state();
        save_state(&path, "testPassword123", &state).unwrap();
        let loaded = load_state(&path, "testPassword123");

        assert_eq!(loaded.last_hash("dropbox"), Some("abc123hash"));
        let info = loaded.last_sync_info("dropbox");
        assert_eq!(info.device_name.as_deref(), Some("Terry-MacBook"));
        assert_eq!(info.updated_at.as_deref(), Some("2026-07-21T10:00:00Z"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_wrong_password_returns_empty_state() {
        let dir = std::env::temp_dir().join("sync_state_test_wrongpw");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sync_state.enc");

        let state = test_state();
        save_state(&path, "correctPassword123", &state).unwrap();
        // Wrong password → should return empty state, not error
        let loaded = load_state(&path, "wrongPassword123");
        assert_eq!(loaded.last_hash("dropbox"), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_tampered_state_returns_empty() {
        let dir = std::env::temp_dir().join("sync_state_test_tamper");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sync_state.enc");

        let state = test_state();
        save_state(&path, "testPassword123", &state).unwrap();

        // Tamper with the file
        let mut data = std::fs::read(&path).unwrap();
        if data.len() > 40 {
            data[40] ^= 0xff;
        }
        std::fs::write(&path, &data).unwrap();

        let loaded = load_state(&path, "testPassword123");
        assert_eq!(loaded.last_hash("dropbox"), None, "tampered state should return empty");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_no_file_returns_empty_state() {
        let dir = std::env::temp_dir().join("sync_state_test_nofile");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("nonexistent.enc");

        let loaded = load_state(&path, "testPassword123");
        assert_eq!(loaded.last_hash("dropbox"), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_set_and_read_sync_info() {
        let mut state = SyncState::default();
        state.set_sync_info(
            "baidu",
            "md5hash123".to_string(),
            "Terry-iPhone".to_string(),
            "2026-07-20T18:30:00Z".to_string(),
            Some("1784717000".to_string()),
        );
        assert_eq!(state.last_hash("baidu"), Some("md5hash123"));
        let info = state.last_sync_info("baidu");
        assert_eq!(info.device_name.as_deref(), Some("Terry-iPhone"));
        assert_eq!(info.updated_at.as_deref(), Some("2026-07-20T18:30:00Z"));
    }

    #[test]
    fn test_password_change_returns_empty() {
        let dir = std::env::temp_dir().join("sync_state_test_pwchange");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sync_state.enc");

        let state = test_state();
        // Save with old password
        save_state(&path, "oldPassword123", &state).unwrap();
        // Load with new password → should return empty (can't decrypt)
        let loaded = load_state(&path, "newPassword123");
        assert_eq!(loaded.last_hash("dropbox"), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 向后兼容: 反序列化不含 last_local_mtime 字段的旧格式 JSON
    /// 验证旧 sync_state 文件能正常加载，last_local_mtime 默认为 None
    #[test]
    fn test_backward_compat_old_json_without_local_mtime() {
        let old_json = r#"{
            "baidu": {
                "last_hash": "md5hash123",
                "last_device_name": "Terry-iPhone",
                "last_updated_at": "2026-07-20T18:30:00Z"
            },
            "dropbox": {
                "last_hash": "abc123hash",
                "last_device_name": "Terry-MacBook",
                "last_updated_at": "2026-07-21T10:00:00Z"
            }
        }"#;
        let state: SyncState = serde_json::from_str(old_json).unwrap();
        // 旧字段正常加载
        assert_eq!(state.last_hash("baidu"), Some("md5hash123"));
        assert_eq!(state.last_local_mtime("baidu"), None);
        assert_eq!(state.last_hash("dropbox"), Some("abc123hash"));
        assert_eq!(state.last_local_mtime("dropbox"), None);
        // 其他字段也正常
        let info = state.last_sync_info("baidu");
        assert_eq!(info.device_name.as_deref(), Some("Terry-iPhone"));
        assert_eq!(info.updated_at.as_deref(), Some("2026-07-20T18:30:00Z"));
    }
}
