//! Cloud sync encryption — AES-256-GCM with Argon2id key derivation.
//!
//! Supports two format versions:
//!
//! **v1** (legacy):
//! ```text
//! magic(4) + version=1(1) + salt(16) + nonce(12) + ciphertext
//! ```
//! Direct KEK from password+salt encrypts data. Argon2 params hardcoded.
//!
//! **v2** (envelope encryption):
//! ```text
//! magic(4) + version=2(1) + argon2_params(10) + salt(16) + nonce_kek(12) +
//! wrapped_dek(48) + nonce_data(12) + ciphertext
//! ```
//! KEK from password+salt wraps DEK. DEK encrypts data.
//! Argon2 params stored in header (upgradable). No hw_id binding (cross-device).
//!
//! **Migration**: v1 files are still decryptable. New uploads use v2.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use argon2::Argon2;
use rand::rng;
use rand::Rng;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;
use zeroize::Zeroizing;

use termfast_credential::envelope;

/// Magic for sync config data file: "TermFast Sync Config".
pub const MAGIC_CONFIG: &[u8; 4] = b"TFSC";
/// Magic for sync state file: "TermFast Sync State".
pub const MAGIC_STATE: &[u8; 4] = b"TFSS";

/// v1 format version (legacy).
const FORMAT_VERSION_V1: u8 = 1;
/// v2 format version (envelope encryption).
const FORMAT_VERSION_V2: u8 = 2;
/// Argon2id salt length in bytes.
const SALT_LEN: usize = 16;
/// AES-GCM nonce length in bytes.
const NONCE_LEN: usize = 12;
/// v1 header size: magic(4) + version(1) + salt(16) + nonce(12).
pub const HEADER_LEN: usize = 4 + 1 + SALT_LEN + NONCE_LEN;
/// v1 Argon2id parameters (must match for v1 compat).
const ARGON2_M_COST: u32 = 32768; // 32 MiB
const ARGON2_T_COST: u32 = 3;
const ARGON2_P_COST: u32 = 1;
/// Derived key length (AES-256).
const KEY_LEN: usize = 32;
/// Max password byte length after NFKC normalization.
const MAX_PASSWORD_LEN: usize = 1024;

/// Compute a hash of the master password for comparison purposes.
/// Uses NFKC normalization (same as encryption) + SHA-256.
/// This hash is stored locally to detect password changes across
/// upload/download operations. It is NOT used for encryption.
pub fn password_hash(password: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let normalized: String = password.nfkc().collect();
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    let result = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

/// Save the password hash to a file (plaintext, 32 bytes).
pub fn save_password_hash(path: &std::path::Path, hash: &[u8; 32]) {
    let _ = std::fs::write(path, hash);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
}

/// Load the password hash from a file. Returns None if file doesn't exist.
pub fn load_password_hash(path: &std::path::Path) -> Option<[u8; 32]> {
    let data = std::fs::read(path).ok()?;
    if data.len() != 32 {
        return None;
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&data);
    Some(out)
}

/// Wrapper JSON stored inside the encrypted config file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPayload {
    /// The full export data (config + credentials) as a JSON value.
    pub config: serde_json::Value,
    /// Device name that uploaded this config.
    pub device_name: String,
    /// Upload timestamp (RFC 3339 / ISO 8601 UTC).
    pub updated_at: String,
}

/// Errors from sync crypto operations.
#[derive(Debug, thiserror::Error)]
pub enum SyncCryptoError {
    #[error("password too long after normalization: {0} bytes (max {1})")]
    PasswordTooLong(usize, usize),
    #[error("encrypted data too short: {0} bytes (need at least {1})")]
    TooShort(usize, usize),
    #[error("invalid magic bytes")]
    InvalidMagic,
    #[error("unsupported format version: {0}")]
    UnsupportedVersion(u8),
    #[error("decryption failed: wrong password or corrupted data")]
    DecryptFailed,
    #[error("crypto error: {0}")]
    Crypto(String),
    #[error("json error: {0}")]
    Json(String),
}

impl From<serde_json::Error> for SyncCryptoError {
    fn from(e: serde_json::Error) -> Self {
        SyncCryptoError::Json(e.to_string())
    }
}

/// Normalize password with NFKC and validate length.
/// Returns a Zeroizing<String> so the normalized password is cleared
/// from memory when dropped.
fn normalize_password(password: &str) -> Result<Zeroizing<String>, SyncCryptoError> {
    let normalized: String = password.nfkc().collect();
    let byte_len = normalized.len();
    if byte_len > MAX_PASSWORD_LEN {
        return Err(SyncCryptoError::PasswordTooLong(byte_len, MAX_PASSWORD_LEN));
    }
    Ok(Zeroizing::new(normalized))
}

/// Derive a 32-byte key from password + salt using Argon2id.
fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; KEY_LEN], SyncCryptoError> {
    let argon2 = Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        argon2::Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, Some(KEY_LEN))
            .map_err(|e| SyncCryptoError::Crypto(format!("invalid argon2 params: {}", e)))?,
    );
    let mut out = [0u8; KEY_LEN];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut out)
        .map_err(|e| SyncCryptoError::Crypto(format!("argon2 key derivation failed: {}", e)))?;
    Ok(out)
}

/// Encrypt plaintext with the given magic and password.
/// Always writes v2 (envelope) format. No hw_id binding (cloud sync is cross-device).
fn encrypt_blob(magic: &[u8; 4], password: &str, plaintext: &[u8]) -> Result<Vec<u8>, SyncCryptoError> {
    let mut salt = [0u8; SALT_LEN];
    rng().fill_bytes(&mut salt);
    let params = envelope::Argon2Params::default_for_platform();
    // No hw_id binding for cloud sync (salt_extra = empty)
    envelope::encrypt(magic, password, &salt, &[], params, plaintext)
        .map_err(|e| match e {
            envelope::EnvelopeError::PasswordTooLong(n, max) => SyncCryptoError::PasswordTooLong(n, max),
            envelope::EnvelopeError::Crypto(msg) => SyncCryptoError::Crypto(msg),
            _ => SyncCryptoError::Crypto(format!("envelope encrypt failed: {}", e)),
        })
}

/// Decrypt a blob produced by `encrypt_blob` (v2) or a legacy v1 blob.
/// Returns the plaintext on success, or a unified error on failure
/// (wrong password / corrupted data — not distinguished to avoid
/// side-channel leakage).
fn decrypt_blob(magic: &[u8; 4], password: &str, blob: &[u8]) -> Result<Vec<u8>, SyncCryptoError> {
    if blob.len() < 5 {
        return Err(SyncCryptoError::TooShort(blob.len(), 5));
    }
    if &blob[..4] != magic {
        return Err(SyncCryptoError::InvalidMagic);
    }
    let version = blob[4];
    match version {
        FORMAT_VERSION_V1 => decrypt_blob_v1(magic, password, blob),
        FORMAT_VERSION_V2 => {
            // v2: use envelope decrypt (no hw_id binding)
            envelope::decrypt(magic, password, &[], blob)
                .map_err(|e| match e {
                    envelope::EnvelopeError::DecryptFailed => SyncCryptoError::DecryptFailed,
                    envelope::EnvelopeError::TooShort(n, min) => SyncCryptoError::TooShort(n, min),
                    _ => SyncCryptoError::Crypto(format!("envelope decrypt failed: {}", e)),
                })
        }
        v => Err(SyncCryptoError::UnsupportedVersion(v)),
    }
}

/// Decrypt a legacy v1 blob (direct KEK, hardcoded Argon2 params).
fn decrypt_blob_v1(magic: &[u8; 4], password: &str, blob: &[u8]) -> Result<Vec<u8>, SyncCryptoError> {
    let normalized = normalize_password(password)?;

    if blob.len() < HEADER_LEN {
        return Err(SyncCryptoError::TooShort(blob.len(), HEADER_LEN));
    }
    let salt = &blob[5..5 + SALT_LEN];
    let nonce_bytes = &blob[5 + SALT_LEN..5 + SALT_LEN + NONCE_LEN];
    let ciphertext = &blob[HEADER_LEN..];

    let key = derive_key(&normalized, salt)?;

    // Reconstruct AAD from the parsed header.
    let mut aad = Vec::with_capacity(HEADER_LEN);
    aad.extend_from_slice(magic);
    aad.push(FORMAT_VERSION_V1);
    aad.extend_from_slice(salt);
    aad.extend_from_slice(nonce_bytes);

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, Payload { msg: ciphertext, aad: &aad })
        .map_err(|_| SyncCryptoError::DecryptFailed)
}

/// Encrypt a sync config payload with the user's master password.
/// The result is the `config.enc` file content to upload to cloud.
///
/// **Must be called on a `spawn_blocking` thread** — Argon2id is
/// CPU-intensive (32 MiB memory, 3 iterations) and will block.
pub fn encrypt_config(password: &str, payload: &SyncPayload) -> Result<Vec<u8>, SyncCryptoError> {
    let plaintext = serde_json::to_vec(payload)?;
    encrypt_blob(MAGIC_CONFIG, password, &plaintext)
}

/// Decrypt a sync config file downloaded from cloud.
/// Returns the parsed payload (config + device_name + updated_at).
///
/// **Must be called on a `spawn_blocking` thread**.
pub fn decrypt_config(password: &str, blob: &[u8]) -> Result<SyncPayload, SyncCryptoError> {
    let plaintext = decrypt_blob(MAGIC_CONFIG, password, blob)?;
    let payload: SyncPayload = serde_json::from_slice(&plaintext)?;
    Ok(payload)
}

/// Encrypt arbitrary plaintext data with the given magic and password.
/// Used by `sync_state` for the TFSS state file.
pub fn encrypt_with_magic(magic: &[u8; 4], password: &str, plaintext: &[u8]) -> Result<Vec<u8>, SyncCryptoError> {
    encrypt_blob(magic, password, plaintext)
}

/// Decrypt arbitrary data encrypted with the given magic and password.
/// Used by `sync_state` for the TFSS state file.
pub fn decrypt_with_magic(magic: &[u8; 4], password: &str, blob: &[u8]) -> Result<Vec<u8>, SyncCryptoError> {
    decrypt_blob(magic, password, blob)
}

/// Get the device name for the current platform.
/// Truncated to 16 UTF-8 characters (not in the middle of a multi-byte char).
pub fn device_name() -> String {
    let host = hostname();
    truncate_utf8(&host, 16)
}

/// Get the hostname / device name (platform-specific).
fn hostname() -> String {
    #[cfg(target_os = "android")]
    {
        // On Android, hostname() returns localhost — use the ANDROID_HOSTNAME
        // env var if set, otherwise fall back to "Android".
        std::env::var("ANDROID_HOSTNAME")
            .unwrap_or_else(|_| "Android".to_string())
    }
    #[cfg(target_os = "macos")]
    {
        // macOS: HOSTNAME env var is not exported by default in zsh,
        // and /etc/hostname doesn't exist. Use scutil to get the user-facing
        // computer name, falling back to LocalHostName, then hostname command.
        if let Ok(name) = std::process::Command::new("scutil")
            .args(["--get", "ComputerName"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .map(|s| if s.is_empty() { "unknown".to_string() } else { s })
        {
            if name != "unknown" {
                return name;
            }
        }
        // Fall back to LocalHostName (Bonjour name)
        if let Ok(name) = std::process::Command::new("scutil")
            .args(["--get", "LocalHostName"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        {
            if !name.is_empty() {
                return name;
            }
        }
        // Last resort: hostname command
        if let Ok(name) = std::process::Command::new("hostname")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        {
            if !name.is_empty() {
                return name;
            }
        }
        "unknown".to_string()
    }
    #[cfg(all(not(target_os = "android"), not(target_os = "macos")))]
    {
        std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("COMPUTERNAME"))
            .unwrap_or_else(|_| {
                // Try reading /etc/hostname as a last resort on Unix
                #[cfg(unix)]
                {
                    std::fs::read_to_string("/etc/hostname")
                        .ok()
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|| "unknown".to_string())
                }
                #[cfg(not(unix))]
                {
                    "unknown".to_string()
                }
            })
    }
}

/// Truncate a string to at most `max_chars` UTF-8 characters,
/// without splitting a multi-byte character.
fn truncate_utf8(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    s.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_payload() -> SyncPayload {
        SyncPayload {
            config: serde_json::json!({"servers": [{"id": "s1", "name": "test"}]}),
            device_name: "test-device".to_string(),
            updated_at: "2026-07-21T10:00:00Z".to_string(),
        }
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let pw = "testPassword123";
        let payload = test_payload();
        let blob = encrypt_config(pw, &payload).unwrap();
        let decrypted = decrypt_config(pw, &blob).unwrap();
        assert_eq!(decrypted.device_name, payload.device_name);
        assert_eq!(decrypted.updated_at, payload.updated_at);
        assert_eq!(decrypted.config, payload.config);
    }

    #[test]
    fn test_wrong_password_fails() {
        let payload = test_payload();
        let blob = encrypt_config("correctPassword123", &payload).unwrap();
        let result = decrypt_config("wrongPassword123", &blob);
        assert!(matches!(result, Err(SyncCryptoError::DecryptFailed)));
    }

    #[test]
    fn test_tamper_ciphertext_fails() {
        let payload = test_payload();
        let mut blob = encrypt_config("pw1234567890ab", &payload).unwrap();
        blob[HEADER_LEN] ^= 0xff; // flip a bit in ciphertext
        let result = decrypt_config("pw1234567890ab", &blob);
        assert!(matches!(result, Err(SyncCryptoError::DecryptFailed)));
    }

    #[test]
    fn test_tamper_salt_fails() {
        let payload = test_payload();
        let mut blob = encrypt_config("pw1234567890ab", &payload).unwrap();
        blob[5] ^= 0xff; // flip a bit in salt
        let result = decrypt_config("pw1234567890ab", &blob);
        assert!(result.is_err());
    }

    #[test]
    fn test_tamper_nonce_fails() {
        let payload = test_payload();
        let mut blob = encrypt_config("pw1234567890ab", &payload).unwrap();
        blob[5 + SALT_LEN] ^= 0xff; // flip a bit in nonce
        let result = decrypt_config("pw1234567890ab", &blob);
        assert!(result.is_err());
    }

    #[test]
    fn test_tamper_magic_rejected() {
        let payload = test_payload();
        let mut blob = encrypt_config("pw1234567890ab", &payload).unwrap();
        blob[0] = b'X';
        let result = decrypt_config("pw1234567890ab", &blob);
        assert!(matches!(result, Err(SyncCryptoError::InvalidMagic)));
    }

    #[test]
    fn test_tamper_version_fails() {
        let payload = test_payload();
        let mut blob = encrypt_config("pw1234567890ab", &payload).unwrap();
        blob[4] = 99; // change to unsupported version
        let result = decrypt_config("pw1234567890ab", &blob);
        assert!(matches!(result, Err(SyncCryptoError::UnsupportedVersion(99))));
    }

    #[test]
    fn test_nonce_randomness() {
        let payload = test_payload();
        let pw = "pw1234567890ab";
        let blob1 = encrypt_config(pw, &payload).unwrap();
        let blob2 = encrypt_config(pw, &payload).unwrap();
        // Nonces should differ (random generation)
        let nonce1 = &blob1[5 + SALT_LEN..HEADER_LEN];
        let nonce2 = &blob2[5 + SALT_LEN..HEADER_LEN];
        assert_ne!(nonce1, nonce2, "nonces must differ (random generation)");
        // Full blobs should differ
        assert_ne!(blob1, blob2);
    }

    #[test]
    fn test_nfkc_normalization() {
        // é can be represented as:
        //   NFC:  U+00E9 (single codepoint)
        //   NFD:  U+0065 + U+0301 (e + combining acute accent)
        let nfc_pw = "pass\u{00E9}word12345";
        let nfd_pw = "pass\u{0065}\u{0301}word12345";
        let payload = test_payload();
        // Encrypt with NFC form, decrypt with NFD form → should succeed
        // because both normalize to the same NFKC form.
        let blob = encrypt_config(nfc_pw, &payload).unwrap();
        let result = decrypt_config(nfd_pw, &blob);
        assert!(result.is_ok(), "NFC encrypt + NFD decrypt should succeed with NFKC normalization");
    }

    #[test]
    fn test_password_too_long() {
        let long_pw = "a".repeat(1025);
        let payload = test_payload();
        let result = encrypt_config(&long_pw, &payload);
        assert!(matches!(result, Err(SyncCryptoError::PasswordTooLong(_, _))));
    }

    #[test]
    fn test_password_max_length_boundary() {
        // Exactly 1024 bytes after NFKC (all ASCII 'a')
        let pw = "a".repeat(1024);
        let payload = test_payload();
        let blob = encrypt_config(&pw, &payload).unwrap();
        let result = decrypt_config(&pw, &blob);
        assert!(result.is_ok());
    }

    #[test]
    fn test_metadata_roundtrip() {
        let payload = SyncPayload {
            config: serde_json::json!({"test": "value"}),
            device_name: "Terry-iPhone-15".to_string(),
            updated_at: "2026-07-21T10:00:00Z".to_string(),
        };
        let blob = encrypt_config("testPassword123", &payload).unwrap();
        let decrypted = decrypt_config("testPassword123", &blob).unwrap();
        assert_eq!(decrypted.device_name, "Terry-iPhone-15");
        assert_eq!(decrypted.updated_at, "2026-07-21T10:00:00Z");
    }

    #[test]
    fn test_metadata_not_exposed_in_ciphertext() {
        let payload = SyncPayload {
            config: serde_json::json!({}),
            device_name: "SECRET_DEVICE_NAME_XYZ".to_string(),
            updated_at: "2026-07-21T10:00:00Z".to_string(),
        };
        let blob = encrypt_config("testPassword123", &payload).unwrap();
        // The device_name string should NOT appear in the encrypted blob
        let blob_str = String::from_utf8_lossy(&blob);
        assert!(!blob_str.contains("SECRET_DEVICE_NAME_XYZ"), "device_name must not be visible in ciphertext");
    }

    #[test]
    fn test_truncate_utf8() {
        assert_eq!(truncate_utf8("hello", 10), "hello");
        assert_eq!(truncate_utf8("hello", 3), "hel");
        // Multi-byte: 中文字符
        assert_eq!(truncate_utf8("中文字符测试", 3), "中文字");
        // Don't split in the middle of a multi-byte char (shouldn't happen with chars())
        assert_eq!(truncate_utf8("ab中", 2), "ab");
    }

    /// Create a v1 blob for backward compat testing.
    fn encrypt_blob_v1(magic: &[u8; 4], password: &str, plaintext: &[u8]) -> Vec<u8> {
        let normalized = normalize_password(password).unwrap();
        let mut salt = [0u8; SALT_LEN];
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rng().fill_bytes(&mut salt);
        rng().fill_bytes(&mut nonce_bytes);
        let key = derive_key(&normalized, &salt).unwrap();
        let mut aad = Vec::with_capacity(HEADER_LEN);
        aad.extend_from_slice(magic);
        aad.push(FORMAT_VERSION_V1);
        aad.extend_from_slice(&salt);
        aad.extend_from_slice(&nonce_bytes);
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ct = cipher.encrypt(nonce, Payload { msg: plaintext, aad: &aad }).unwrap();
        let mut blob = Vec::with_capacity(HEADER_LEN + ct.len());
        blob.extend_from_slice(magic);
        blob.push(FORMAT_VERSION_V1);
        blob.extend_from_slice(&salt);
        blob.extend_from_slice(&nonce_bytes);
        blob.extend_from_slice(&ct);
        blob
    }

    #[test]
    fn test_v1_backward_compat() {
        // v1 encrypted blob should be decryptable with current code
        let payload = test_payload();
        let plaintext = serde_json::to_vec(&payload).unwrap();
        let v1_blob = encrypt_blob_v1(MAGIC_CONFIG, "testPassword123", &plaintext);
        // Decrypt v1 blob
        let decrypted = decrypt_config("testPassword123", &v1_blob).unwrap();
        assert_eq!(decrypted.device_name, payload.device_name);
        assert_eq!(decrypted.config, payload.config);
    }

    #[test]
    fn test_v2_encrypt_produces_v2_format() {
        let payload = test_payload();
        let blob = encrypt_config("pw", &payload).unwrap();
        // Version byte should be 2
        assert_eq!(blob[4], FORMAT_VERSION_V2);
    }

    #[test]
    fn test_v2_cross_platform_params() {
        // Simulate cross-device: encrypt with mobile params (Android),
        // decrypt on desktop (params read from blob header).
        // This verifies that params are stored in the header and decrypt
        // uses the stored params, not platform defaults.
        let payload = test_payload();
        let plaintext = serde_json::to_vec(&payload).unwrap();

        // Encrypt with mobile params (as Android would)
        let mut salt = [0u8; SALT_LEN];
        rng().fill_bytes(&mut salt);
        let mobile_params = envelope::Argon2Params::mobile();
        let blob = envelope::encrypt(MAGIC_CONFIG, "testPw123", &salt, &[], mobile_params, &plaintext).unwrap();

        // Decrypt with decrypt_config (which reads params from blob header)
        let decrypted = decrypt_config("testPw123", &blob).unwrap();
        assert_eq!(decrypted.device_name, payload.device_name);
        assert_eq!(decrypted.config, payload.config);
    }
}
