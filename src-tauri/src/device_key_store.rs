// src-tauri/src/device_key_store.rs — ECDSA P-256 key pair generation + OS secure storage
//
// D3: ECDSA P-256 密钥对生成与存储
// 首次启动生成密钥对，私钥存 OS 安全存储（按 §5.5 矩阵降级），
// 标记 non-exportable，检测并记录安全等级。
// 发起 mobile 配对时（Initiate）携带公钥 + 安全等级。
//
// 降级链：检测最强可用存储 → 逐级降级 → 记录安全等级
//   高: TPM / Secure Enclave（硬件隔离）
//   中: CNG Software KSP（LSASS 进程隔离）
//   低: Keychain / libsecret / 加密文件（App 进程内）
//
// 本次实现：
//   - 所有平台: 加密文件降级（AES-GCM + 机器绑定密钥）→ 安全等级"低"
//   macOS Keychain / Secure Enclave / Windows CNG / Linux TPM 在后续迭代补齐

#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;
use p256::ecdsa::{SigningKey, VerifyingKey, signature::Signer};
use p256::pkcs8::EncodePublicKey;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use rand_core::OsRng;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use sha2::{Digest, Sha256};

/// Security level of the key storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SecurityLevel {
    High,   // TPM / Secure Enclave
    Medium, // CNG Software KSP (LSASS)
    Low,    // Keychain / libsecret / encrypted file
}

impl SecurityLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            SecurityLevel::High => "high",
            SecurityLevel::Medium => "medium",
            SecurityLevel::Low => "low",
        }
    }
}

/// Device key pair: public key (DER-encoded, for sending to backend) + signing key (for signing).
pub struct DeviceKeyPair {
    pub public_key_der: Vec<u8>,
    pub(crate) signing_key: SigningKey,
    pub security_level: SecurityLevel,
}

impl DeviceKeyPair {
    /// Returns the public key as base64-encoded DER string (for API calls).
    pub fn public_key_base64(&self) -> String {
        STANDARD.encode(&self.public_key_der)
    }

    /// Sign a payload with the device private key.
    /// Returns base64-encoded DER signature.
    pub fn sign(&self, payload: &[u8]) -> String {
        let sig: p256::ecdsa::Signature = self.signing_key.sign(payload);
        let sig_der = sig.to_der();
        STANDARD.encode(sig_der.as_bytes())
    }
}

/// Returns the platform-appropriate data directory for termfast.
/// Matches the path used by `pairing_store::data_dir`.
fn data_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join("Library/Application Support/termfast")
    }
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(appdata).join("termfast")
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            PathBuf::from(xdg).join("termfast")
        } else {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".local/share/termfast")
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        PathBuf::from(".")
    }
}

fn device_key_path() -> PathBuf {
    data_dir().join("device_key.json")
}

/// On-disk JSON structure for encrypted device key (low-security fallback).
/// Private key is encrypted with AES-256-GCM using a machine-bound key.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct DeviceKeyFile {
    /// AES-GCM nonce (12 bytes, base64).
    nonce_b64: String,
    /// Encrypted private key ciphertext (base64).
    ciphertext_b64: String,
    /// Base64-encoded DER public key (not encrypted — public key is not secret).
    public_key_b64: String,
    /// Security level string.
    security_level: String,
    /// Salt used for machine-bound key derivation (base64, 16 bytes).
    salt_b64: String,
}

/// Derive a machine-bound AES-256 key from hostname + username + salt.
/// This is a low-security approach (key can be recomputed by any process on the machine),
/// but it prevents casual file-copy attacks across machines.
fn derive_machine_key(salt: &[u8]) -> [u8; 32] {
    let hostname = whoami::hostname().unwrap_or_else(|_| "unknown".to_string());
    let username = whoami::username().unwrap_or_else(|_| "unknown".to_string());
    let mut hasher = Sha256::new();
    hasher.update(hostname.as_bytes());
    hasher.update(b"|");
    hasher.update(username.as_bytes());
    hasher.update(b"|");
    hasher.update(salt);
    let result = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    key
}

/// Encrypt private key bytes with AES-256-GCM.
fn encrypt_private_key(plaintext: &[u8], salt: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let key = derive_machine_key(salt);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| format!("create cipher: {}", e))?;
    let mut nonce_bytes = [0u8; 12];
    rand_core::RngCore::fill_bytes(&mut OsRng, &mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| format!("encrypt: {}", e))?;
    Ok((nonce_bytes.to_vec(), ciphertext))
}

/// Decrypt private key bytes with AES-256-GCM.
fn decrypt_private_key(ciphertext: &[u8], nonce: &[u8], salt: &[u8]) -> Result<Vec<u8>, String> {
    let key = derive_machine_key(salt);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| format!("create cipher: {}", e))?;
    let nonce = Nonce::from_slice(nonce);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("decrypt: {}", e))
}

/// Get or create the device key pair.
/// On first call, generates a new P-256 key pair and persists it.
/// On subsequent calls, loads the existing key pair.
/// Prefers SQLCipher DB if the global storage singleton is initialized.
pub fn get_or_create_key() -> Result<DeviceKeyPair, String> {
    // Try SQLCipher first
    if let Some(storage) = crate::storage_singleton::get_storage() {
        return get_or_create_key_from_storage(storage);
    }
    // Fallback to file-based storage
    let path = device_key_path();
    get_or_create_key_at(&path)
}

/// Get or create device key from SqlCipherStorage.
fn get_or_create_key_from_storage(
    storage: &std::sync::Arc<termfast_core::config::SqlCipherStorage>,
) -> Result<DeviceKeyPair, String> {
    // Try to load existing key from DB
    if let Ok(Some((pub_der, priv_bytes, level_str, _))) = storage.get_device_key() {
        if let Ok(signing_key) = SigningKey::from_slice(&priv_bytes) {
            let level = match level_str.as_str() {
                "high" => SecurityLevel::High,
                "medium" => SecurityLevel::Medium,
                _ => SecurityLevel::Low,
            };
            return Ok(DeviceKeyPair {
                public_key_der: pub_der,
                signing_key,
                security_level: level,
            });
        }
    }

    // Generate new key pair
    let signing_key = SigningKey::random(&mut OsRng);
    let verifying_key = signing_key.verifying_key();
    let public_key_der = verifying_key.to_sec1_bytes().to_vec();
    let priv_bytes = signing_key.to_bytes().to_vec();
    let level = SecurityLevel::Low;

    // Save to DB
    storage
        .upsert_device_key(&public_key_der, &priv_bytes, level.as_str())
        .map_err(|e| format!("failed to persist device key to DB: {}", e))?;

    Ok(DeviceKeyPair {
        public_key_der,
        signing_key,
        security_level: level,
    })
}

/// Get or create key at a specific path (for testing).
pub fn get_or_create_key_at(path: &std::path::Path) -> Result<DeviceKeyPair, String> {
    // Try to load existing key
    if let Ok(content) = fs::read_to_string(path) {
        if let Ok(f) = serde_json::from_str::<DeviceKeyFile>(&content) {
            // Decode fields
            if let (Ok(nonce), Ok(ciphertext), Ok(salt), Ok(pub_bytes)) = (
                STANDARD.decode(&f.nonce_b64),
                STANDARD.decode(&f.ciphertext_b64),
                STANDARD.decode(&f.salt_b64),
                STANDARD.decode(&f.public_key_b64),
            ) {
                // Decrypt private key
                if let Ok(priv_bytes) = decrypt_private_key(&ciphertext, &nonce, &salt) {
                    // Decode raw 32-byte private key
                    if let Ok(signing_key) = SigningKey::from_slice(&priv_bytes) {
                        let level = match f.security_level.as_str() {
                            "high" => SecurityLevel::High,
                            "medium" => SecurityLevel::Medium,
                            _ => SecurityLevel::Low,
                        };
                        return Ok(DeviceKeyPair {
                            public_key_der: pub_bytes,
                            signing_key,
                            security_level: level,
                        });
                    }
                }
            }
        }
    }

    // Generate new key pair
    let signing_key = SigningKey::random(&mut OsRng);
    let verifying_key: VerifyingKey = *signing_key.verifying_key();
    let public_key_der = verifying_key
        .to_public_key_der()
        .map_err(|e| format!("encode public key: {}", e))?;

    // Generate salt for machine-bound key derivation
    let mut salt = [0u8; 16];
    rand_core::RngCore::fill_bytes(&mut OsRng, &mut salt);

    // Encrypt private key
    let priv_bytes = signing_key.to_bytes();
    let (nonce, ciphertext) = encrypt_private_key(priv_bytes.as_slice(), &salt)?;

    let file = DeviceKeyFile {
        nonce_b64: STANDARD.encode(&nonce),
        ciphertext_b64: STANDARD.encode(&ciphertext),
        public_key_b64: STANDARD.encode(public_key_der.as_bytes()),
        security_level: SecurityLevel::Low.as_str().to_string(),
        salt_b64: STANDARD.encode(salt),
    };

    if let Ok(json) = serde_json::to_string(&file) {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, json);
    }

    Ok(DeviceKeyPair {
        public_key_der: public_key_der.as_bytes().to_vec(),
        signing_key,
        security_level: SecurityLevel::Low,
    })
}

/// Sign a payload with the device private key.
/// Returns DER-encoded signature, base64-encoded.
pub fn sign_payload(payload: &[u8]) -> Result<String, String> {
    let key = get_or_create_key()?;
    Ok(key.sign(payload))
}

/// Sign a payload with a specific key pair (for testing).
pub fn sign_with_key(key: &DeviceKeyPair, payload: &[u8]) -> String {
    key.sign(payload)
}

/// Regenerate the device key pair (for lost-key recovery).
/// Overwrites the existing key with a new one.
pub fn regenerate_key() -> Result<DeviceKeyPair, String> {
    let path = device_key_path();
    regenerate_key_at(&path)
}

/// Regenerate key at a specific path (for testing).
pub fn regenerate_key_at(path: &std::path::Path) -> Result<DeviceKeyPair, String> {
    // Generate new key pair
    let signing_key = SigningKey::random(&mut OsRng);
    let verifying_key: VerifyingKey = *signing_key.verifying_key();
    let public_key_der = verifying_key
        .to_public_key_der()
        .map_err(|e| format!("encode public key: {}", e))?;

    // Generate salt for machine-bound key derivation
    let mut salt = [0u8; 16];
    rand_core::RngCore::fill_bytes(&mut OsRng, &mut salt);

    // Encrypt private key
    let priv_bytes = signing_key.to_bytes();
    let (nonce, ciphertext) = encrypt_private_key(priv_bytes.as_slice(), &salt)?;

    let file = DeviceKeyFile {
        nonce_b64: STANDARD.encode(&nonce),
        ciphertext_b64: STANDARD.encode(&ciphertext),
        public_key_b64: STANDARD.encode(public_key_der.as_bytes()),
        security_level: SecurityLevel::Low.as_str().to_string(),
        salt_b64: STANDARD.encode(salt),
    };

    if let Ok(json) = serde_json::to_string(&file) {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, json);
    }

    Ok(DeviceKeyPair {
        public_key_der: public_key_der.as_bytes().to_vec(),
        signing_key,
        security_level: SecurityLevel::Low,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::signature::Verifier;

    fn temp_key_path(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "termfast_key_test_{}_{}_{}",
            test_name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        dir.join("device_key.json")
    }

    #[test]
    fn test_generate_keypair_produces_valid_p256() {
        let path = temp_key_path("generate");
        let key = get_or_create_key_at(&path).unwrap();

        // Public key should be DER-encoded (starts with SEQUENCE tag 0x30)
        assert!(!key.public_key_der.is_empty());
        assert_eq!(key.public_key_der[0], 0x30, "DER should start with SEQUENCE");

        // Security level should be Low (fallback)
        assert_eq!(key.security_level, SecurityLevel::Low);

        // Clean up
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn test_keypair_persists_across_calls() {
        let path = temp_key_path("persist");
        let key1 = get_or_create_key_at(&path).unwrap();
        let key2 = get_or_create_key_at(&path).unwrap();

        // Both should have the same public key
        assert_eq!(key1.public_key_der, key2.public_key_der,
            "second call should load same key");

        // Clean up
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn test_sign_and_verify_roundtrip() {
        let path = temp_key_path("sign_verify");
        let key = get_or_create_key_at(&path).unwrap();

        let payload = b"test payload for signing";
        let sig_b64 = sign_with_key(&key, payload);

        // Decode signature
        let sig_bytes = STANDARD.decode(&sig_b64).unwrap();
        let sig = p256::ecdsa::Signature::from_der(&sig_bytes).unwrap();

        // Decode public key from DER using p256's DecodePublicKey trait
        use p256::pkcs8::DecodePublicKey;
        let pub_key = p256::ecdsa::VerifyingKey::from_public_key_der(&key.public_key_der).unwrap();
        pub_key.verify(payload, &sig).expect("signature should verify");

        // Clean up
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn test_regenerate_creates_different_key() {
        let path = temp_key_path("regenerate");
        let key1 = get_or_create_key_at(&path).unwrap();
        let key2 = regenerate_key_at(&path).unwrap();

        // Public keys should differ
        assert_ne!(key1.public_key_der, key2.public_key_der,
            "regenerated key should be different");

        // Clean up
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn test_corrupted_file_regenerates() {
        let path = temp_key_path("corrupt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "not valid json").unwrap();

        // Should regenerate, not crash
        let key = get_or_create_key_at(&path).unwrap();
        assert!(!key.public_key_der.is_empty());

        // Clean up
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn test_private_key_is_encrypted_on_disk() {
        let path = temp_key_path("encrypted");
        let key = get_or_create_key_at(&path).unwrap();

        // Read the file content
        let content = std::fs::read_to_string(&path).unwrap();

        // The raw private key bytes (32 bytes) should NOT appear in the file
        let priv_bytes = key.signing_key.to_bytes();
        let priv_b64 = STANDARD.encode(priv_bytes);
        assert!(!content.contains(&priv_b64),
            "private key should not be stored in plaintext (base64 of raw key found in file)");

        // The file should contain ciphertext_b64 field (encrypted)
        assert!(content.contains("ciphertext_b64"), "file should have ciphertext field");
        assert!(content.contains("nonce_b64"), "file should have nonce field");
        assert!(content.contains("salt_b64"), "file should have salt field");

        // Clean up
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn test_sign_payload_returns_valid_der_signature() {
        let path = temp_key_path("sign_payload");
        let payload = b"test payload for sign_payload";

        // sign_payload uses the default path, but we test the round-trip via get_or_create_key_at
        let key = get_or_create_key_at(&path).unwrap();
        let sig_b64 = key.sign(payload);

        // Decode and verify
        let sig_bytes = STANDARD.decode(&sig_b64).unwrap();
        let sig = p256::ecdsa::Signature::from_der(&sig_bytes).unwrap();

        use p256::pkcs8::DecodePublicKey;
        let pub_key = p256::ecdsa::VerifyingKey::from_public_key_der(&key.public_key_der).unwrap();
        pub_key.verify(payload, &sig).expect("signature should verify");

        // Clean up
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let salt = [0u8; 16];
        let plaintext = b"secret private key bytes 32 bytes long!!!!"; // 42 bytes

        let (nonce, ciphertext) = encrypt_private_key(plaintext, &salt).unwrap();
        let decrypted = decrypt_private_key(&ciphertext, &nonce, &salt).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_decrypt_with_wrong_salt_fails() {
        let salt1 = [0u8; 16];
        let salt2 = [1u8; 16];
        let plaintext = b"secret key";

        let (nonce, ciphertext) = encrypt_private_key(plaintext, &salt1).unwrap();
        let result = decrypt_private_key(&ciphertext, &nonce, &salt2);

        assert!(result.is_err(), "decrypt with wrong salt should fail");
    }
}
