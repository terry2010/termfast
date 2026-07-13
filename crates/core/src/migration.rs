//! Encrypted export/import — FP-1.6
//!
//! Full export: all server configs + templates + credentials + auto-generated key files.
//! Encrypted with AES-256-GCM, key derived from master password via Argon2id.
//! Blob format: [magic(4B)][salt(16B)][nonce(12B)][ciphertext]

use crate::error::{Error, ErrorCode, IpcError, Result};
use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// Magic bytes for VPS Guard encrypted blob format
const MAGIC: &[u8; 4] = b"VPG1";

/// Salt length for Argon2id
const SALT_LEN: usize = 16;

/// Nonce length for AES-256-GCM
const NONCE_LEN: usize = 12;

/// Lockout threshold (3 wrong passwords)
const MAX_ATTEMPTS: u32 = 3;

/// Lockout duration (5 minutes)
const LOCKOUT_DURATION: Duration = Duration::from_secs(300);

/// Minimum master password length
const MIN_PASSWORD_LEN: usize = 12;

/// Full export data structure — all config + credentials + key files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullExportData {
    pub config: crate::config::Config,
    /// Map of server_id → password (for password auth)
    #[serde(default)]
    pub passwords: HashMap<String, String>,
    /// Map of server_id → key passphrase (for key auth)
    #[serde(default)]
    pub key_passphrases: HashMap<String, String>,
    /// Map of server_id → auto-generated private key file content
    #[serde(default)]
    pub key_files: HashMap<String, String>,
}

/// Encrypted blob structure
#[derive(Debug)]
pub struct EncryptedBlob {
    pub data: Vec<u8>,
}

/// Validate master password strength (§10.1)
/// ≥12 characters, must contain letters + numbers
pub fn validate_master_password(password: &str) -> Result<()> {
    if password.len() < MIN_PASSWORD_LEN {
        return Err(Error::Ipc(IpcError::new(
            ErrorCode::DecryptionFailed,
            format!("password must be at least {} characters", MIN_PASSWORD_LEN),
        )));
    }

    let has_letter = password.chars().any(|c| c.is_alphabetic());
    let has_digit = password.chars().any(|c| c.is_ascii_digit());

    if !has_letter || !has_digit {
        return Err(Error::Ipc(IpcError::new(
            ErrorCode::DecryptionFailed,
            "password must contain both letters and numbers",
        )));
    }

    Ok(())
}

/// Derive AES-256 key from master password using Argon2id
fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; 32]> {
    let params = Params::new(64 * 1024, 3, 4, Some(32))
        .map_err(|e| Error::Crypto(format!("argon2 params error: {}", e)))?;

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| Error::Crypto(format!("argon2 derive error: {}", e)))?;

    Ok(key)
}

/// Encrypt full export data with master password
pub fn export_full(master_password: &str, data: &FullExportData) -> Result<Vec<u8>> {
    validate_master_password(master_password)?;

    // Generate salt and nonce
    let rng = ring::rand::SystemRandom::new();
    let mut salt = [0u8; SALT_LEN];
    let mut nonce_bytes = [0u8; NONCE_LEN];
    ring::rand::SecureRandom::fill(&rng, &mut salt)
        .map_err(|e| Error::Crypto(format!("rng salt error: {}", e)))?;
    ring::rand::SecureRandom::fill(&rng, &mut nonce_bytes)
        .map_err(|e| Error::Crypto(format!("rng nonce error: {}", e)))?;

    // Derive key
    let key = derive_key(master_password, &salt)?;

    // Serialize data
    let plaintext = serde_json::to_vec(data)?;

    // Encrypt
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| Error::Crypto(format!("aes init error: {}", e)))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_ref())
        .map_err(|e| Error::Crypto(format!("aes encrypt error: {}", e)))?;

    // Build blob: [magic][salt][nonce][ciphertext]
    let mut blob = Vec::with_capacity(4 + SALT_LEN + NONCE_LEN + ciphertext.len());
    blob.extend_from_slice(MAGIC);
    blob.extend_from_slice(&salt);
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);

    Ok(blob)
}

/// Import lockout state (thread-safe)
static IMPORT_ATTEMPTS: AtomicU32 = AtomicU32::new(0);
static LOCKOUT_UNTIL: std::sync::OnceLock<std::sync::Mutex<Option<Instant>>> =
    std::sync::OnceLock::new();

fn get_lockout() -> &'static std::sync::Mutex<Option<Instant>> {
    LOCKOUT_UNTIL.get_or_init(|| std::sync::Mutex::new(None))
}

/// Check if currently locked out
pub fn is_locked_out() -> bool {
    let lockout = get_lockout().lock().unwrap();
    if let Some(until) = *lockout {
        Instant::now() < until
    } else {
        false
    }
}

/// Reset import attempt counter (call on successful import)
pub fn reset_attempts() {
    IMPORT_ATTEMPTS.store(0, Ordering::SeqCst);
    let mut lockout = get_lockout().lock().unwrap();
    *lockout = None;
}

/// Decrypt blob with master password
pub fn import_full(master_password: &str, blob: &[u8]) -> Result<FullExportData> {
    // Check lockout
    if is_locked_out() {
        let lockout = get_lockout().lock().unwrap();
        if let Some(until) = *lockout {
            let remaining = until.duration_since(Instant::now());
            return Err(Error::Ipc(IpcError::new(
                ErrorCode::DecryptionFailed,
                format!(
                    "too many wrong attempts, locked out for {} more seconds",
                    remaining.as_secs()
                ),
            )));
        }
    }

    // Parse blob
    if blob.len() < 4 + SALT_LEN + NONCE_LEN {
        return Err(Error::Ipc(IpcError::new(
            ErrorCode::DecryptionFailed,
            "blob too short",
        )));
    }

    let magic = &blob[..4];
    if magic != MAGIC {
        return Err(Error::Ipc(IpcError::new(
            ErrorCode::DecryptionFailed,
            "invalid magic bytes",
        )));
    }

    let salt = &blob[4..4 + SALT_LEN];
    let nonce_bytes = &blob[4 + SALT_LEN..4 + SALT_LEN + NONCE_LEN];
    let ciphertext = &blob[4 + SALT_LEN + NONCE_LEN..];

    // Derive key
    let key = derive_key(master_password, salt)?;

    // Decrypt
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| Error::Crypto(format!("aes init error: {}", e)))?;
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = match cipher.decrypt(nonce, ciphertext) {
        Ok(pt) => pt,
        Err(_) => {
            // Wrong password — increment attempt counter
            let attempts = IMPORT_ATTEMPTS.fetch_add(1, Ordering::SeqCst) + 1;
            if attempts >= MAX_ATTEMPTS {
                let mut lockout = get_lockout().lock().unwrap();
                *lockout = Some(Instant::now() + LOCKOUT_DURATION);
                IMPORT_ATTEMPTS.store(0, Ordering::SeqCst);
            }
            return Err(Error::Ipc(IpcError::new(
                ErrorCode::DecryptionFailed,
                "wrong master password or corrupted file",
            )));
        }
    };

    // Reset attempts on success
    reset_attempts();

    // Deserialize
    let data: FullExportData = serde_json::from_slice(&plaintext)?;

    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_data() -> FullExportData {
        FullExportData {
            config: crate::config::Config::default(),
            passwords: HashMap::new(),
            key_passphrases: HashMap::new(),
            key_files: HashMap::new(),
        }
    }

    #[test]
    fn test_export_import_round_trip() {
        let password = "valid_password_123";
        let data = test_data();
        let blob = export_full(password, &data).unwrap();
        let imported = import_full(password, &blob).unwrap();
        assert_eq!(imported.config.version, data.config.version);
    }

    #[test]
    fn test_wrong_password_fails() {
        let password = "valid_password_123";
        let data = test_data();
        let blob = export_full(password, &data).unwrap();
        let result = import_full("wrong_password_456", &blob);
        assert!(result.is_err());
    }

    #[test]
    fn test_password_too_short() {
        let result = validate_master_password("short");
        assert!(result.is_err());
    }

    #[test]
    fn test_password_no_digits() {
        let result = validate_master_password("onlylettershere");
        assert!(result.is_err());
    }

    #[test]
    fn test_password_no_letters() {
        let result = validate_master_password("123456789012");
        assert!(result.is_err());
    }

    #[test]
    fn test_password_valid() {
        let result = validate_master_password("valid_password_123");
        assert!(result.is_ok());
    }

    #[test]
    fn test_blob_magic_bytes() {
        let password = "valid_password_123";
        let data = test_data();
        let blob = export_full(password, &data).unwrap();
        assert_eq!(&blob[..4], MAGIC);
    }

    #[test]
    fn test_corrupt_blob_fails() {
        let result = import_full("some_password_123", b"VPG1bad");
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_magic_fails() {
        let mut blob = vec![0u8; 100];
        blob[..4].copy_from_slice(b"XXXX");
        let result = import_full("some_password_123", &blob);
        assert!(result.is_err());
    }
}
