//! Token store — persists OAuth tokens encrypted with a passphrase.
//!
//! Tokens are encrypted with AES-256-GCM using a key derived from a
//! passphrase via Argon2id. This reuses the same crypto primitives as
//! the credential store to avoid pulling extra dependencies.
//!
//! For simplicity, tokens are stored in a single JSON file at a known
//! path. The file is encrypted as a whole, not individual fields.

use crate::{CloudProvider, OAuthToken};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// On-disk format: a map of provider → token, encrypted as one blob.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenStoreData {
    pub tokens: HashMap<String, StoredToken>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredToken {
    pub provider: CloudProvider,
    pub token: OAuthToken,
    /// When this token was stored (unix timestamp seconds)
    pub stored_at: i64,
}

/// File header magic + version
const MAGIC: &[u8] = b"TFTK";
const VERSION: u8 = 1;

/// Encrypt and save tokens to disk.
///
/// `passphrase` is used to derive an AES-256 key via Argon2id.
/// The file is written with 0600 permissions on Unix.
pub fn save_tokens(
    path: &std::path::Path,
    passphrase: &str,
    data: &TokenStoreData,
) -> Result<(), crate::CloudSyncError> {
    let plaintext = serde_json::to_vec(data)
        .map_err(|e| crate::CloudSyncError::Api(format!("serialize error: {}", e)))?;

    let ciphertext = encrypt_blob(passphrase, &plaintext)?;

    // Write atomically: write to temp file, then rename
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &ciphertext)
        .map_err(|e| crate::CloudSyncError::Io(e.to_string()))?;

    // Set permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| crate::CloudSyncError::Io(e.to_string()))?;
    }

    std::fs::rename(&tmp, path)
        .map_err(|e| crate::CloudSyncError::Io(e.to_string()))?;

    Ok(())
}

/// Load and decrypt tokens from disk.
pub fn load_tokens(
    path: &std::path::Path,
    passphrase: &str,
) -> Result<TokenStoreData, crate::CloudSyncError> {
    let ciphertext = std::fs::read(path)
        .map_err(|e| crate::CloudSyncError::Io(e.to_string()))?;

    let plaintext = decrypt_blob(passphrase, &ciphertext)?;

    serde_json::from_slice(&plaintext)
        .map_err(|e| crate::CloudSyncError::Api(format!("deserialize error: {}", e)))
}

/// Check if a token file exists.
pub fn token_file_exists(path: &std::path::Path) -> bool {
    path.exists()
}

/// Encrypt a plaintext blob with AES-256-GCM.
/// Format: MAGIC(4) + VERSION(1) + SALT(16) + NONCE(12) + CIPHERTEXT
fn encrypt_blob(passphrase: &str, plaintext: &[u8]) -> Result<Vec<u8>, crate::CloudSyncError> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    use argon2::Argon2;
    use rand::Rng;

    let mut salt = [0u8; 16];
    let mut nonce_bytes = [0u8; 12];
    rand::rng().fill_bytes(&mut salt);
    rand::rng().fill_bytes(&mut nonce_bytes);

    // Derive key with Argon2id (same params as credential store: 32MiB/3iter/1lane)
    let params = argon2::Params::new(32768, 3, 1, Some(32))
        .map_err(|e| crate::CloudSyncError::Config(format!("argon2 params: {}", e)))?;
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

    let mut key_bytes = [0u8; 32];
    argon2
        .hash_password_into(passphrase.as_bytes(), &salt, &mut key_bytes)
        .map_err(|e| crate::CloudSyncError::Config(format!("argon2 derive: {}", e)))?;

    let cipher = Aes256Gcm::new_from_slice(&key_bytes)
        .map_err(|e| crate::CloudSyncError::Config(format!("aes init: {}", e)))?;

    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| crate::CloudSyncError::Config(format!("aes encrypt: {}", e)))?;

    // Zeroize key material
    zeroize_key(&mut key_bytes);

    let mut output = Vec::with_capacity(4 + 1 + 16 + 12 + ciphertext.len());
    output.extend_from_slice(MAGIC);
    output.push(VERSION);
    output.extend_from_slice(&salt);
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);

    Ok(output)
}

/// Decrypt a blob produced by encrypt_blob.
fn decrypt_blob(passphrase: &str, ciphertext: &[u8]) -> Result<Vec<u8>, crate::CloudSyncError> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    use argon2::Argon2;

    if ciphertext.len() < 4 + 1 + 16 + 12 {
        return Err(crate::CloudSyncError::Config("file too short".into()));
    }

    if &ciphertext[..4] != MAGIC {
        return Err(crate::CloudSyncError::Config("bad magic".into()));
    }

    let version = ciphertext[4];
    if version != VERSION {
        return Err(crate::CloudSyncError::Config(format!(
            "unsupported version: {}",
            version
        )));
    }

    let salt = &ciphertext[5..21];
    let nonce_bytes = &ciphertext[21..33];
    let ct = &ciphertext[33..];

    let params = argon2::Params::new(32768, 3, 1, Some(32))
        .map_err(|e| crate::CloudSyncError::Config(format!("argon2 params: {}", e)))?;
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

    let mut key_bytes = [0u8; 32];
    argon2
        .hash_password_into(passphrase.as_bytes(), salt, &mut key_bytes)
        .map_err(|e| crate::CloudSyncError::Config(format!("argon2 derive: {}", e)))?;

    let cipher = Aes256Gcm::new_from_slice(&key_bytes)
        .map_err(|e| crate::CloudSyncError::Config(format!("aes init: {}", e)))?;

    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ct)
        .map_err(|_| crate::CloudSyncError::Config("decrypt failed (wrong password?)".into()))?;

    // Zeroize key material
    zeroize_key(&mut key_bytes);

    Ok(plaintext)
}

/// Zeroize a key buffer by overwriting with zeros.
fn zeroize_key(key: &mut [u8]) {
    for byte in key.iter_mut() {
        // Use volatile write to prevent compiler optimization
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(name)
    }

    #[test]
    fn test_save_load_roundtrip() {
        let tmp = tmp_path("test_token_roundtrip.enc");
        let _ = std::fs::remove_file(&tmp);

        let mut data = TokenStoreData::default();
        data.tokens.insert(
            "dropbox".to_string(),
            StoredToken {
                provider: CloudProvider::Dropbox,
                token: OAuthToken {
                    access_token: "test_access_123".into(),
                    refresh_token: Some("test_refresh_456".into()),
                    expires_at: Some(9999999999),
                    token_type: "bearer".into(),
                },
                stored_at: 1234567890,
            },
        );

        save_tokens(&tmp, "my_passphrase", &data).unwrap();
        let loaded = load_tokens(&tmp, "my_passphrase").unwrap();

        assert_eq!(loaded.tokens.len(), 1);
        let stored = loaded.tokens.get("dropbox").unwrap();
        assert_eq!(stored.token.access_token, "test_access_123");
        assert_eq!(stored.token.refresh_token.as_deref(), Some("test_refresh_456"));
        assert_eq!(stored.token.expires_at, Some(9999999999));

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_wrong_passphrase_fails() {
        let tmp = tmp_path("test_wrong_pass.enc");
        let _ = std::fs::remove_file(&tmp);

        let data = TokenStoreData::default();
        save_tokens(&tmp, "correct_pass", &data).unwrap();

        let result = load_tokens(&tmp, "wrong_pass");
        assert!(result.is_err());

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_multi_provider() {
        let tmp = tmp_path("test_multi_provider.enc");
        let _ = std::fs::remove_file(&tmp);

        let mut data = TokenStoreData::default();
        data.tokens.insert(
            "dropbox".to_string(),
            StoredToken {
                provider: CloudProvider::Dropbox,
                token: OAuthToken {
                    access_token: "dropbox_token".into(),
                    refresh_token: None,
                    expires_at: None,
                    token_type: "bearer".into(),
                },
                stored_at: 100,
            },
        );
        data.tokens.insert(
            "baidu".to_string(),
            StoredToken {
                provider: CloudProvider::Baidu,
                token: OAuthToken {
                    access_token: "baidu_token".into(),
                    refresh_token: None,
                    expires_at: None,
                    token_type: "bearer".into(),
                },
                stored_at: 200,
            },
        );

        save_tokens(&tmp, "pass", &data).unwrap();
        let loaded = load_tokens(&tmp, "pass").unwrap();

        assert_eq!(loaded.tokens.len(), 2);
        assert_eq!(
            loaded.tokens.get("dropbox").unwrap().token.access_token,
            "dropbox_token"
        );
        assert_eq!(
            loaded.tokens.get("baidu").unwrap().token.access_token,
            "baidu_token"
        );

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_bad_magic_fails() {
        let result = decrypt_blob("pass", b"XXXXbad_data_too_short");
        assert!(result.is_err());
    }
}

// === SECTION token_store END ===
