// src-tauri/src/ecdh_key_store.rs — X25519 ECDH key pair for tunnel key agreement
//
// Each desktop holds an X25519 key pair. The private key never leaves the device.
// The public key is shared via QR code and stored in the backend (non-secret).
// Two desktops each compute the same shared_secret using ECDH:
//   shared = ECDH(my_private_key, peer_public_key)
//
// Storage mirrors device_key_store.rs: AES-256-GCM encrypted file (low-security fallback).

#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use rand_core::OsRng;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};

/// X25519 ECDH key pair: public key (32 bytes) + static secret (32 bytes).
pub struct EcdhKeyPair {
    pub public_key: [u8; 32],
    secret: StaticSecret,
}

impl EcdhKeyPair {
    /// Returns the public key as base64-encoded 32 bytes.
    pub fn public_key_base64(&self) -> String {
        STANDARD.encode(self.public_key)
    }

    /// Compute the shared secret with a peer's public key.
    /// Returns 32-byte shared secret.
    pub fn diffie_hellman(&self, peer_public_key: &[u8; 32]) -> [u8; 32] {
        let peer = PublicKey::from(*peer_public_key);
        *self.secret.diffie_hellman(&peer).as_bytes()
    }
}

/// On-disk JSON structure for encrypted ECDH key (same pattern as device_key_store).
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct EcdhKeyFile {
    nonce_b64: String,
    ciphertext_b64: String,
    public_key_b64: String,
    salt_b64: String,
}

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

fn ecdh_key_path() -> PathBuf {
    data_dir().join("ecdh_key.json")
}

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

fn decrypt_private_key(ciphertext: &[u8], nonce: &[u8], salt: &[u8]) -> Result<Vec<u8>, String> {
    let key = derive_machine_key(salt);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| format!("create cipher: {}", e))?;
    let nonce = Nonce::from_slice(nonce);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("decrypt: {}", e))
}

/// Get or create the ECDH key pair.
pub fn get_or_create_key() -> Result<EcdhKeyPair, String> {
    let path = ecdh_key_path();
    get_or_create_key_at(&path)
}

/// Get or create key at a specific path (for testing).
pub fn get_or_create_key_at(path: &std::path::Path) -> Result<EcdhKeyPair, String> {
    // Try to load existing key
    if let Ok(content) = fs::read_to_string(path) {
        if let Ok(f) = serde_json::from_str::<EcdhKeyFile>(&content) {
            if let (Ok(nonce), Ok(ciphertext), Ok(salt), Ok(pub_bytes)) = (
                STANDARD.decode(&f.nonce_b64),
                STANDARD.decode(&f.ciphertext_b64),
                STANDARD.decode(&f.salt_b64),
                STANDARD.decode(&f.public_key_b64),
            ) {
                if let Ok(priv_bytes) = decrypt_private_key(&ciphertext, &nonce, &salt) {
                    if priv_bytes.len() == 32 {
                        let mut priv_arr = [0u8; 32];
                        priv_arr.copy_from_slice(&priv_bytes);
                        let secret = StaticSecret::from(priv_arr);
                        let public = PublicKey::from(&secret);
                        if pub_bytes.len() == 32 {
                            let mut pub_arr = [0u8; 32];
                            pub_arr.copy_from_slice(&pub_bytes);
                            return Ok(EcdhKeyPair {
                                public_key: pub_arr,
                                secret,
                            });
                        }
                        // Fallback: recompute public from private
                        return Ok(EcdhKeyPair {
                            public_key: public.to_bytes(),
                            secret,
                        });
                    }
                }
            }
        }
    }

    // Generate new key pair
    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);

    let mut salt = [0u8; 16];
    rand_core::RngCore::fill_bytes(&mut OsRng, &mut salt);

    let priv_bytes = secret.to_bytes();
    let (nonce, ciphertext) = encrypt_private_key(&priv_bytes, &salt)?;

    let file = EcdhKeyFile {
        nonce_b64: STANDARD.encode(&nonce),
        ciphertext_b64: STANDARD.encode(&ciphertext),
        public_key_b64: STANDARD.encode(public.to_bytes()),
        salt_b64: STANDARD.encode(salt),
    };

    if let Ok(json) = serde_json::to_string(&file) {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Err(e) = fs::write(path, json) {
            return Err(format!("failed to persist ECDH key: {}", e));
        }
    }

    Ok(EcdhKeyPair {
        public_key: public.to_bytes(),
        secret,
    })
}

/// Get the ECDH public key as base64 (for QR code generation).
pub fn get_public_key_base64() -> Result<String, String> {
    let key = get_or_create_key()?;
    Ok(key.public_key_base64())
}

/// Compute shared secret with a peer's public key (base64-encoded).
/// Returns 32-byte shared secret as hex string (for use as pairing_key).
pub fn compute_shared_secret_hex(peer_public_key_b64: &str) -> Result<String, String> {
    compute_shared_secret_hex_at(&ecdh_key_path(), peer_public_key_b64)
}

/// Compute shared secret at a specific key path (for testing).
pub fn compute_shared_secret_hex_at(path: &std::path::Path, peer_public_key_b64: &str) -> Result<String, String> {
    let key = get_or_create_key_at(path)?;
    let peer_bytes = STANDARD
        .decode(peer_public_key_b64)
        .map_err(|e| format!("decode peer public key: {}", e))?;
    if peer_bytes.len() != 32 {
        return Err(format!("peer public key must be 32 bytes, got {}", peer_bytes.len()));
    }
    let mut peer_arr = [0u8; 32];
    peer_arr.copy_from_slice(&peer_bytes);
    let shared = key.diffie_hellman(&peer_arr);
    Ok(hex::encode(shared))
}

/// Compute shared secret with a peer's public key (raw bytes).
/// Returns 32-byte shared secret.
pub fn compute_shared_secret(peer_public_key: &[u8; 32]) -> Result<[u8; 32], String> {
    let key = get_or_create_key()?;
    Ok(key.diffie_hellman(peer_public_key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypair_generation() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let key = get_or_create_key_at(tmp.path()).unwrap();
        assert_eq!(key.public_key.len(), 32);
    }

    #[test]
    fn test_keypair_persists_across_calls() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let key1 = get_or_create_key_at(tmp.path()).unwrap();
        let key2 = get_or_create_key_at(tmp.path()).unwrap();
        assert_eq!(key1.public_key, key2.public_key, "public key should persist");
    }

    #[test]
    fn test_ecdh_roundtrip() {
        // Two parties each generate key pairs and compute the same shared secret
        let tmp_a = tempfile::NamedTempFile::new().unwrap();
        let tmp_b = tempfile::NamedTempFile::new().unwrap();
        let alice = get_or_create_key_at(tmp_a.path()).unwrap();
        let bob = get_or_create_key_at(tmp_b.path()).unwrap();

        let shared_a = alice.diffie_hellman(&bob.public_key);
        let shared_b = bob.diffie_hellman(&alice.public_key);

        assert_eq!(shared_a, shared_b, "ECDH shared secrets must match");
    }

    #[test]
    fn test_compute_shared_secret_hex() {
        let tmp_a = tempfile::NamedTempFile::new().unwrap();
        let tmp_b = tempfile::NamedTempFile::new().unwrap();
        let alice = get_or_create_key_at(tmp_a.path()).unwrap();
        let bob = get_or_create_key_at(tmp_b.path()).unwrap();

        let alice_pub_b64 = alice.public_key_base64();
        let bob_pub_b64 = bob.public_key_base64();
        drop(alice);
        drop(bob);

        // Alice computes with Bob's public key (via compute_shared_secret_hex_at)
        let shared_a_hex = compute_shared_secret_hex_at(tmp_a.path(), &bob_pub_b64).unwrap();
        // Bob computes with Alice's public key
        let shared_b_hex = compute_shared_secret_hex_at(tmp_b.path(), &alice_pub_b64).unwrap();

        assert_eq!(shared_a_hex, shared_b_hex, "shared secret hex must match");
        assert_eq!(shared_a_hex.len(), 64, "hex should be 64 chars (32 bytes)");
    }

    #[test]
    fn test_compute_shared_secret_hex_invalid_base64() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let _ = get_or_create_key_at(tmp.path()).unwrap();
        let result = compute_shared_secret_hex_at(tmp.path(), "!!!not-valid-base64!!!");
        assert!(result.is_err(), "invalid base64 should return error");
        assert!(result.unwrap_err().contains("decode"), "error should mention decode failure");
    }

    #[test]
    fn test_compute_shared_secret_hex_wrong_length() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let _ = get_or_create_key_at(tmp.path()).unwrap();
        // 16 bytes instead of 32
        let short_key = STANDARD.encode([0u8; 16]);
        let result = compute_shared_secret_hex_at(tmp.path(), &short_key);
        assert!(result.is_err(), "wrong-length peer key should return error");
        assert!(result.unwrap_err().contains("32 bytes"), "error should mention expected length");
    }

    #[test]
    fn test_private_key_is_encrypted_on_disk() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let key = get_or_create_key_at(tmp.path()).unwrap();
        let priv_bytes = key.secret.to_bytes();
        let priv_hex = hex::encode(priv_bytes);

        let file_content = fs::read_to_string(tmp.path()).unwrap();
        // Private key must NOT appear in plaintext in the file
        assert!(
            !file_content.contains(&priv_hex),
            "private key hex must not appear in file (must be encrypted)"
        );
        // File should contain ciphertext_b64 (encrypted), not raw private key
        assert!(
            file_content.contains("ciphertext_b64"),
            "file should contain encrypted ciphertext field"
        );
        // Public key SHOULD appear (it's not secret)
        let pub_b64 = key.public_key_base64();
        assert!(
            file_content.contains(&pub_b64),
            "public key should appear in file (it's not secret)"
        );
    }

    #[test]
    fn test_corrupted_file_regenerates() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let _ = get_or_create_key_at(tmp.path()).unwrap();
        // Corrupt the file
        std::fs::write(tmp.path(), "garbage").unwrap();
        let key = get_or_create_key_at(tmp.path()).unwrap();
        assert_eq!(key.public_key.len(), 32);
    }

    #[test]
    fn test_different_keypairs_different_keys() {
        let tmp_a = tempfile::NamedTempFile::new().unwrap();
        let tmp_b = tempfile::NamedTempFile::new().unwrap();
        let key_a = get_or_create_key_at(tmp_a.path()).unwrap();
        let key_b = get_or_create_key_at(tmp_b.path()).unwrap();
        assert_ne!(key_a.public_key, key_b.public_key, "different keypairs should have different public keys");
    }
}
// === SECTION 1 END ===
