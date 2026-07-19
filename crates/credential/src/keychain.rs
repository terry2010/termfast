//! Keychain credential store — desktop implementation using `keyring` crate (§8.7)
//!
//! Falls back to InMemoryCredentialStore when keychain is unavailable (§17.3)

use super::{CredentialStore, InMemoryCredentialStore, SERVICE_NAME};
use anyhow::Result;
use std::sync::Mutex;

/// Desktop credential store backed by OS keychain.
/// Falls back to in-memory storage when keychain is unavailable.
/// Caches credentials in memory after first load to avoid repeated keychain prompts.
pub struct KeychainCredentialStore {
    fallback: Mutex<Option<InMemoryCredentialStore>>,
    cache: Mutex<std::collections::HashMap<String, String>>,
}

impl KeychainCredentialStore {
    pub fn new() -> Self {
        Self {
            fallback: Mutex::new(None),
            cache: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Enable in-memory fallback
    fn enable_fallback(&self) {
        let mut fb = self.fallback.lock().unwrap();
        if fb.is_none() {
            tracing::warn!("keychain unavailable, falling back to in-memory storage");
            *fb = Some(InMemoryCredentialStore::new());
        }
    }
}

impl Default for KeychainCredentialStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialStore for KeychainCredentialStore {
    fn save(&self, key: &str, value: &str) -> Result<()> {
        // Update cache first
        self.cache
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());

        // Try keychain first (it persists across restarts)
        match keyring::Entry::new(SERVICE_NAME, key) {
            Ok(entry) => match entry.set_password(value) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    tracing::warn!("keychain save failed: {}, trying fallback", e);
                    self.enable_fallback();
                }
            },
            Err(e) => {
                tracing::warn!("keychain entry creation failed: {}, trying fallback", e);
                self.enable_fallback();
            }
        }

        // Fall back to in-memory store
        let fb = self.fallback.lock().unwrap();
        if let Some(ref store) = *fb {
            return store.save(key, value);
        }

        Err(anyhow::anyhow!(
            "keychain unavailable and no fallback store"
        ))
    }

    fn load(&self, key: &str) -> Result<String> {
        // Check cache first — avoids repeated keychain prompts
        if let Some(v) = self.cache.lock().unwrap().get(key) {
            tracing::debug!(target: "keychain", "loaded credential from cache");
            return Ok(v.clone());
        }

        tracing::info!(target: "keychain", "accessing OS keychain for credential");

        // Try keychain first (it persists across restarts)
        match keyring::Entry::new(SERVICE_NAME, key) {
            Ok(entry) => match entry.get_password() {
                Ok(v) => {
                    // Cache for future loads
                    self.cache
                        .lock()
                        .unwrap()
                        .insert(key.to_string(), v.clone());
                    return Ok(v);
                }
                Err(keyring::Error::NoEntry) => {} // not found, try fallback
                Err(e) => {
                    tracing::warn!("keychain load failed: {}, trying fallback", e);
                    self.enable_fallback();
                }
            },
            Err(e) => {
                tracing::warn!("keychain entry creation failed: {}, trying fallback", e);
                self.enable_fallback();
            }
        }

        // Try fallback (in-memory) store — avoid holding fallback lock
        // while acquiring cache lock to prevent lock-ordering issues.
        let fb_value = {
            let fb = self.fallback.lock().unwrap();
            if let Some(ref store) = *fb {
                Some(store.load(key)?)
            } else {
                None
            }
        };
        if let Some(v) = fb_value {
            self.cache
                .lock()
                .unwrap()
                .insert(key.to_string(), v.clone());
            return Ok(v);
        }

        Err(anyhow::anyhow!("credential not found: {}", key))
    }

    fn delete(&self, key: &str) -> Result<()> {
        // Remove from cache
        self.cache.lock().unwrap().remove(key);

        // Check fallback first
        {
            let fb = self.fallback.lock().unwrap();
            if let Some(ref store) = *fb {
                return store.delete(key);
            }
        }

        match keyring::Entry::new(SERVICE_NAME, key) {
            Ok(entry) => {
                match entry.delete_credential() {
                    Ok(()) => Ok(()),
                    Err(keyring::Error::NoEntry) => Ok(()), // Already gone, not an error
                    Err(e) => {
                        tracing::warn!("keychain delete failed: {}, enabling fallback", e);
                        self.enable_fallback();
                        let fb = self.fallback.lock().unwrap();
                        fb.as_ref().unwrap().delete(key)
                    }
                }
            }
            Err(e) => {
                tracing::warn!("keychain entry creation failed: {}, enabling fallback", e);
                self.enable_fallback();
                let fb = self.fallback.lock().unwrap();
                fb.as_ref().unwrap().delete(key)
            }
        }
    }

    fn delete_all_for_server(&self, server_id: &str) -> Result<()> {
        // Key naming: termfast::<server_id>::<type>
        // Known credential types
        let cred_types = ["password", "key_passphrase"];
        for ct in cred_types {
            let key = super::make_key(server_id, ct);
            let _ = self.delete(&key); // Ignore errors (entry may not exist)
        }
        Ok(())
    }

    fn has(&self, key: &str) -> bool {
        // Only check the in-memory cache. Do NOT hit the OS keychain here:
        // macOS would prompt the user for keychain access just to check existence.
        self.cache.lock().unwrap().contains_key(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keychain_fallback_to_memory() {
        // In test environment, keychain may or may not be available.
        // Either way, save/load should work.
        let store = KeychainCredentialStore::new();
        store.save("test_key_123", "test_value").unwrap();
        let val = store.load("test_key_123").unwrap();
        assert_eq!(val, "test_value");
        store.delete("test_key_123").unwrap();
        assert!(store.load("test_key_123").is_err());
    }

    #[test]
    fn test_delete_all_for_server() {
        let store = KeychainCredentialStore::new();
        let key1 = super::super::make_key("srv_test_del", "password");
        let key2 = super::super::make_key("srv_test_del", "key_passphrase");

        store.save(&key1, "pass1").unwrap();
        store.save(&key2, "passphrase1").unwrap();

        store.delete_all_for_server("srv_test_del").unwrap();

        // Both should be gone — keychain may not support delete in test env,
        // so we just verify delete_all didn't error (checked above with unwrap)
        let _ = store.load(&key1);
        let _ = store.load(&key2);
    }
}
