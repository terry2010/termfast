//! AES-256-GCM encryption for remote terminal frames.
//!
//! Implements the HKDF session key derivation + counter-based nonce scheme
//! from the design doc:
//!
//! - Pairing key K (ECDH-derived, 32 bytes) is persistent across reconnects.
//! - Each connection exchanges 32-byte randoms via HELLO (encrypted with K).
//! - session_key = HKDF-SHA256(ikm=K, salt=client_random||server_random, info="termfast-session-v1")
//! - HELLO frames use K; all subsequent frames use session_key.
//! - nonce = [direction:1][counter:8][padding:3=0], per-direction monotonic counter.
//! - direction: 0=desktop→mobile, 1=mobile→desktop.
//!
//! This prevents nonce reuse across reconnects (session_key changes each connect)
//! and avoids the catastrophic consequences of AES-256-GCM nonce reuse.

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use ring::hkdf;

/// HKDF info string for session key derivation (matches design doc).
const HKDF_INFO: &[u8] = b"termfast-session-v1";

/// Nonce length for AES-256-GCM (12 bytes).
const NONCE_LEN: usize = 12;

/// Salt length for HKDF = client_random(32) + server_random(32) = 64 bytes.
const SALT_LEN: usize = 64;

/// Direction byte in nonce: 0 = desktop → mobile, 1 = mobile → desktop.
pub const DIR_DESKTOP_TO_MOBILE: u8 = 0;
pub const DIR_MOBILE_TO_DESKTOP: u8 = 1;

/// Encode a nonce from direction + counter.
/// Format: [direction:1][counter:8][padding:3=0]
fn encode_nonce(direction: u8, counter: u64) -> [u8; NONCE_LEN] {
    let mut nonce = [0u8; NONCE_LEN];
    nonce[0] = direction;
    nonce[1..9].copy_from_slice(&counter.to_be_bytes());
    // nonce[9..12] = 0 (padding, already zero)
    nonce
}

/// Key length for HKDF output (32 bytes = AES-256 key).
struct KeyLen32;

impl hkdf::KeyType for KeyLen32 {
    fn len(&self) -> usize {
        32
    }
}

/// Derive session key via HKDF-SHA256.
///
/// ikm = pairing key K (32 bytes)
/// salt = client_random(32) || server_random(32) = 64 bytes
/// info = "termfast-session-v1"
/// output = 32 bytes (AES-256 key)
fn derive_session_key(
    pairing_key: &[u8; 32],
    client_random: &[u8; 32],
    server_random: &[u8; 32],
) -> [u8; 32] {
    let mut salt = [0u8; SALT_LEN];
    salt[..32].copy_from_slice(client_random);
    salt[32..].copy_from_slice(server_random);

    let salt = hkdf::Salt::new(hkdf::HKDF_SHA256, &salt);
    let prk = salt.extract(pairing_key);
    let okm = prk
        .expand(&[HKDF_INFO], KeyLen32)
        .expect("HKDF expand should not fail for 32-byte output");

    let mut session_key = [0u8; 32];
    okm.fill(&mut session_key)
        .expect("fill 32 bytes from HKDF output");
    session_key
}

// === SECTION 1 END ===

/// A cipher for one direction of a tunnel connection.
///
/// Uses counter-based nonces (monotonic u64) with a direction byte to ensure
/// nonce uniqueness within a session. The session_key is derived via HKDF
/// from the pairing key + HELLO randoms, so each connection has a different key.
///
/// Thread safety: NOT thread-safe. Each tunnel direction (send/receive) has
/// its own FrameCipher instance. The counter is not atomic — it's only accessed
/// from the single task that sends/receives frames for that direction.
pub struct FrameCipher {
    cipher: Aes256Gcm,
    direction: u8,
    counter: u64,
}

impl FrameCipher {
    /// Create a cipher from a 32-byte key + direction byte.
    /// Counter starts at 0.
    pub fn new(key: &[u8; 32], direction: u8) -> Self {
        let cipher = Aes256Gcm::new(key.into());
        Self {
            cipher,
            direction,
            counter: 0,
        }
    }

    /// Create a cipher from the session key + direction.
    /// Used after HELLO exchange completes and session_key is derived.
    pub fn from_session_key(
        pairing_key: &[u8; 32],
        client_random: &[u8; 32],
        server_random: &[u8; 32],
        direction: u8,
    ) -> Self {
        let session_key = derive_session_key(pairing_key, client_random, server_random);
        Self::new(&session_key, direction)
    }

    /// Create a cipher from the pairing key K directly (for HELLO frames).
    /// Direction is set but counter starts at 0 — HELLO is the first frame.
    pub fn from_pairing_key(pairing_key: &[u8; 32], direction: u8) -> Self {
        Self::new(pairing_key, direction)
    }

    /// Encrypt plaintext frame, returns ciphertext (includes nonce prepended).
    ///
    /// Wire format: [nonce:12][ciphertext:N][tag:16] (AES-256-GCM tag is appended by aes-gcm crate)
    /// The receiver extracts the first 12 bytes as nonce, rest as ciphertext+tag.
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        let nonce = encode_nonce(self.direction, self.counter);
        let nonce_ref = Nonce::from_slice(&nonce);
        let ciphertext = self
            .cipher
            .encrypt(nonce_ref, plaintext)
            .map_err(|e| format!("encrypt error: {}", e))?;
        // Prepend nonce so receiver can decrypt
        let mut output = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        output.extend_from_slice(&nonce);
        output.extend_from_slice(&ciphertext);
        // Security: check counter overflow to prevent GCM nonce reuse.
        // 2^64 frames is practically unreachable, but defense-in-depth.
        self.counter = self.counter.checked_add(1)
            .ok_or_else(|| "frame crypto counter overflow (nonce reuse risk)".to_string())?;
        Ok(output)
    }

    /// Decrypt ciphertext (nonce prepended format from `encrypt`).
    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        if data.len() < NONCE_LEN {
            return Err("ciphertext too short (missing nonce)".to_string());
        }
        let (nonce_bytes, ciphertext) = data.split_at(NONCE_LEN);
        let nonce = Nonce::from_slice(nonce_bytes);
        self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| format!("decrypt error: {}", e))
    }

    /// Get current counter value (for debugging / testing).
    pub fn counter(&self) -> u64 {
        self.counter
    }
}

// === SECTION 2 END ===

/// Generate a 32-byte random for HELLO exchange.
/// Uses ring's OsRng for cryptographic randomness.
pub fn generate_random_32() -> [u8; 32] {
    use aes_gcm::aead::rand_core::RngCore;
    let mut buf = [0u8; 32];
    OsRng.fill_bytes(&mut buf);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = [0x42u8; 32];
        let mut cipher = FrameCipher::new(&key, DIR_DESKTOP_TO_MOBILE);
        let plaintext = b"Hello, remote terminal!";
        let ciphertext = cipher.encrypt(plaintext).unwrap();
        assert_ne!(&ciphertext[NONCE_LEN..], plaintext);
        let decrypted = cipher.decrypt(&ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_decrypt_wrong_key() {
        let mut cipher1 = FrameCipher::new(&[0x01u8; 32], DIR_DESKTOP_TO_MOBILE);
        let cipher2 = FrameCipher::new(&[0x02u8; 32], DIR_DESKTOP_TO_MOBILE);
        let ciphertext = cipher1.encrypt(b"secret").unwrap();
        assert!(cipher2.decrypt(&ciphertext).is_err());
    }

    #[test]
    fn test_decrypt_too_short() {
        let cipher = FrameCipher::new(&[0x01u8; 32], DIR_DESKTOP_TO_MOBILE);
        assert!(cipher.decrypt(b"short").is_err());
        assert!(cipher.decrypt(&[0u8; NONCE_LEN]).is_err()); // nonce only, no ciphertext
    }

    #[test]
    fn test_encrypt_large_data() {
        let mut cipher = FrameCipher::new(&[0xABu8; 32], DIR_DESKTOP_TO_MOBILE);
        let plaintext = vec![0x55u8; 65536];
        let ciphertext = cipher.encrypt(&plaintext).unwrap();
        let decrypted = cipher.decrypt(&ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_counter_increments() {
        let mut cipher = FrameCipher::new(&[0x42u8; 32], DIR_DESKTOP_TO_MOBILE);
        assert_eq!(cipher.counter(), 0);
        cipher.encrypt(b"first").unwrap();
        assert_eq!(cipher.counter(), 1);
        cipher.encrypt(b"second").unwrap();
        assert_eq!(cipher.counter(), 2);
    }

    #[test]
    fn test_nonce_uniqueness_across_directions() {
        // Same counter, different direction → different nonce
        let nonce_d2m_0 = encode_nonce(DIR_DESKTOP_TO_MOBILE, 0);
        let nonce_m2d_0 = encode_nonce(DIR_MOBILE_TO_DESKTOP, 0);
        assert_ne!(nonce_d2m_0, nonce_m2d_0);
        assert_eq!(nonce_d2m_0[0], 0);
        assert_eq!(nonce_m2d_0[0], 1);
    }

    #[test]
    fn test_nonce_uniqueness_across_counters() {
        let nonce_0 = encode_nonce(DIR_DESKTOP_TO_MOBILE, 0);
        let nonce_1 = encode_nonce(DIR_DESKTOP_TO_MOBILE, 1);
        assert_ne!(nonce_0, nonce_1);
        // Counter is in bytes 1-8 (big-endian)
        let ctr_0 = u64::from_be_bytes([nonce_0[1], nonce_0[2], nonce_0[3], nonce_0[4], nonce_0[5], nonce_0[6], nonce_0[7], nonce_0[8]]);
        let ctr_1 = u64::from_be_bytes([nonce_1[1], nonce_1[2], nonce_1[3], nonce_1[4], nonce_1[5], nonce_1[6], nonce_1[7], nonce_1[8]]);
        assert_eq!(ctr_0, 0);
        assert_eq!(ctr_1, 1);
    }

    #[test]
    fn test_hkdf_session_key_deterministic() {
        let pairing_key = [0x42u8; 32];
        let client_random = [0xAAu8; 32];
        let server_random = [0xBBu8; 32];
        let key1 = derive_session_key(&pairing_key, &client_random, &server_random);
        let key2 = derive_session_key(&pairing_key, &client_random, &server_random);
        assert_eq!(key1, key2, "same inputs should produce same session key");
    }

    #[test]
    fn test_hkdf_different_randoms_different_keys() {
        let pairing_key = [0x42u8; 32];
        let client_random_1 = [0xAAu8; 32];
        let server_random_1 = [0xBBu8; 32];
        let client_random_2 = [0xCCu8; 32];
        let server_random_2 = [0xDDu8; 32];

        let key1 = derive_session_key(&pairing_key, &client_random_1, &server_random_1);
        let key2 = derive_session_key(&pairing_key, &client_random_2, &server_random_2);
        assert_ne!(key1, key2, "different randoms should produce different session keys");
    }

    #[test]
    fn test_hkdf_different_pairing_keys_different_keys() {
        let pairing_key_1 = [0x42u8; 32];
        let pairing_key_2 = [0x43u8; 32];
        let client_random = [0xAAu8; 32];
        let server_random = [0xBBu8; 32];

        let key1 = derive_session_key(&pairing_key_1, &client_random, &server_random);
        let key2 = derive_session_key(&pairing_key_2, &client_random, &server_random);
        assert_ne!(key1, key2, "different pairing keys should produce different session keys");
    }

    #[test]
    fn test_session_key_cipher_roundtrip() {
        let pairing_key = [0x42u8; 32];
        let client_random = generate_random_32();
        let server_random = generate_random_32();

        // Desktop sends (direction 0) → mobile receives (direction 0)
        let mut desktop_send = FrameCipher::from_session_key(
            &pairing_key, &client_random, &server_random, DIR_DESKTOP_TO_MOBILE,
        );
        let mobile_recv = FrameCipher::from_session_key(
            &pairing_key, &client_random, &server_random, DIR_DESKTOP_TO_MOBILE,
        );

        let plaintext = b"session encrypted data";
        let ciphertext = desktop_send.encrypt(plaintext).unwrap();
        let decrypted = mobile_recv.decrypt(&ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_bidirectional_session_keys() {
        // Desktop send (dir=0) and mobile send (dir=1) use same session_key
        // but different direction bytes → nonces don't overlap
        let pairing_key = [0x42u8; 32];
        let client_random = [0xAAu8; 32];
        let server_random = [0xBBu8; 32];

        let mut desktop_send = FrameCipher::from_session_key(
            &pairing_key, &client_random, &server_random, DIR_DESKTOP_TO_MOBILE,
        );
        let mut mobile_send = FrameCipher::from_session_key(
            &pairing_key, &client_random, &server_random, DIR_MOBILE_TO_DESKTOP,
        );

        // Both encrypt at counter 0 — nonces should differ (direction byte)
        let ct1 = desktop_send.encrypt(b"desktop msg").unwrap();
        let ct2 = mobile_send.encrypt(b"mobile msg").unwrap();
        assert_ne!(&ct1[..NONCE_LEN], &ct2[..NONCE_LEN], "nonces must differ by direction");
    }

    #[test]
    fn test_hello_key_vs_session_key() {
        // HELLO uses pairing key K; subsequent frames use session_key
        let pairing_key = [0x42u8; 32];
        let client_random = [0xAAu8; 32];
        let server_random = [0xBBu8; 32];

        // HELLO cipher (uses K directly)
        let mut hello_cipher = FrameCipher::from_pairing_key(&pairing_key, DIR_DESKTOP_TO_MOBILE);
        let hello_ct = hello_cipher.encrypt(b"HELLO payload").unwrap();

        // HELLO receiver uses K to decrypt
        let hello_recv = FrameCipher::from_pairing_key(&pairing_key, DIR_DESKTOP_TO_MOBILE);
        let hello_pt = hello_recv.decrypt(&hello_ct).unwrap();
        assert_eq!(hello_pt, b"HELLO payload");

        // Session cipher (uses derived key) — should NOT decrypt HELLO ciphertext
        let session_recv = FrameCipher::from_session_key(
            &pairing_key, &client_random, &server_random, DIR_DESKTOP_TO_MOBILE,
        );
        assert!(session_recv.decrypt(&hello_ct).is_err(), "session key should not decrypt HELLO");
    }

    #[test]
    fn test_reconnect_different_session_keys() {
        // Simulate two connections with different randoms → different session keys
        let pairing_key = [0x42u8; 32];

        let cr1 = generate_random_32();
        let sr1 = generate_random_32();
        let cr2 = generate_random_32();
        let sr2 = generate_random_32();

        let mut send1 = FrameCipher::from_session_key(&pairing_key, &cr1, &sr1, DIR_DESKTOP_TO_MOBILE);
        let mut send2 = FrameCipher::from_session_key(&pairing_key, &cr2, &sr2, DIR_DESKTOP_TO_MOBILE);

        let ct1 = send1.encrypt(b"conn1 data").unwrap();
        let ct2 = send2.encrypt(b"conn2 data").unwrap();

        // Receiver of conn1 cannot decrypt conn2's data (different session key)
        let recv1 = FrameCipher::from_session_key(&pairing_key, &cr1, &sr1, DIR_DESKTOP_TO_MOBILE);
        let recv2 = FrameCipher::from_session_key(&pairing_key, &cr2, &sr2, DIR_DESKTOP_TO_MOBILE);

        assert_eq!(recv1.decrypt(&ct1).unwrap(), b"conn1 data");
        assert!(recv1.decrypt(&ct2).is_err(), "conn1 key should not decrypt conn2 data");
        assert_eq!(recv2.decrypt(&ct2).unwrap(), b"conn2 data");
        assert!(recv2.decrypt(&ct1).is_err(), "conn2 key should not decrypt conn1 data");
    }

    #[test]
    fn test_generate_random_32_unique() {
        let r1 = generate_random_32();
        let r2 = generate_random_32();
        // Extremely unlikely to collide (2^256 space)
        assert_ne!(r1, r2, "random generation should produce unique values");
    }
}
