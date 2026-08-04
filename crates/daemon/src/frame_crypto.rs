//! AES-256-GCM encryption for remote terminal frames.
//!
//! Used by the WebSocket tunnel to encrypt frame data between desktop and mobile.
//! The shared key is derived from the ECDH pairing key (Phase 2).

use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
use aes_gcm::aead::OsRng;

pub struct FrameCipher {
    cipher: Aes256Gcm,
}

impl FrameCipher {
    /// Create a cipher from a 32-byte key (ECDH shared secret).
    pub fn new(key: &[u8; 32]) -> Self {
        let cipher = Aes256Gcm::new(key.into());
        Self { cipher }
    }

    /// Encrypt plaintext, returns (nonce, ciphertext).
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
        let mut nonce_bytes = [0u8; 12];
        use aes_gcm::aead::rand_core::RngCore;
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| format!("encrypt error: {}", e))?;
        Ok((nonce_bytes.to_vec(), ciphertext))
    }

    /// Decrypt ciphertext with given nonce.
    pub fn decrypt(&self, nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, String> {
        if nonce.len() != 12 {
            return Err("nonce must be 12 bytes".to_string());
        }
        let nonce = Nonce::from_slice(nonce);
        self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| format!("decrypt error: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = [0x42u8; 32];
        let cipher = FrameCipher::new(&key);
        let plaintext = b"Hello, remote terminal!";
        let (nonce, ciphertext) = cipher.encrypt(plaintext).unwrap();
        assert_ne!(&ciphertext[..], plaintext);
        let decrypted = cipher.decrypt(&nonce, &ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_decrypt_wrong_key() {
        let cipher1 = FrameCipher::new(&[0x01u8; 32]);
        let cipher2 = FrameCipher::new(&[0x02u8; 32]);
        let (nonce, ciphertext) = cipher1.encrypt(b"secret").unwrap();
        assert!(cipher2.decrypt(&nonce, &ciphertext).is_err());
    }

    #[test]
    fn test_decrypt_wrong_nonce() {
        let cipher = FrameCipher::new(&[0x01u8; 32]);
        let (_, ciphertext) = cipher.encrypt(b"secret").unwrap();
        let wrong_nonce = [0u8; 12];
        assert!(cipher.decrypt(&wrong_nonce, &ciphertext).is_err());
    }

    #[test]
    fn test_encrypt_large_data() {
        let cipher = FrameCipher::new(&[0xABu8; 32]);
        let plaintext = vec![0x55u8; 65536];
        let (nonce, ciphertext) = cipher.encrypt(&plaintext).unwrap();
        let decrypted = cipher.decrypt(&nonce, &ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }
}
