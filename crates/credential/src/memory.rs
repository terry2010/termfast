//! In-memory credential store — fallback when keychain unavailable (§17.3)

use super::CredentialStore;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Mutex;

/// In-memory credential store (for testing and keychain fallback)
pub struct InMemoryCredentialStore {
    store: Mutex<HashMap<String, String>>,
}

impl InMemoryCredentialStore {
    pub fn new() -> Self {
        Self {
            store: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryCredentialStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialStore for InMemoryCredentialStore {
    fn save(&self, key: &str, value: &str) -> Result<()> {
        self.store
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn load(&self, key: &str) -> Result<String> {
        self.store
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("credential not found: {}", key))
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.store.lock().unwrap().remove(key);
        Ok(())
    }

    fn delete_all_for_server(&self, server_id: &str) -> Result<()> {
        let prefix = format!("{}::{}::", super::SERVICE_NAME, server_id);
        let mut store = self.store.lock().unwrap();
        let keys_to_remove: Vec<String> = store
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect();
        for key in keys_to_remove {
            store.remove(&key);
        }
        Ok(())
    }

    fn has(&self, key: &str) -> bool {
        self.store.lock().unwrap().contains_key(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_load_round_trip() {
        let store = InMemoryCredentialStore::new();
        store.save("key1", "value1").unwrap();
        assert_eq!(store.load("key1").unwrap(), "value1");
    }

    #[test]
    fn test_load_not_found() {
        let store = InMemoryCredentialStore::new();
        assert!(store.load("nonexistent").is_err());
    }

    #[test]
    fn test_delete() {
        let store = InMemoryCredentialStore::new();
        store.save("key1", "value1").unwrap();
        store.delete("key1").unwrap();
        assert!(store.load("key1").is_err());
    }

    #[test]
    fn test_delete_all_for_server() {
        let store = InMemoryCredentialStore::new();
        let key1 = super::super::make_key("srv_1", "password");
        let key2 = super::super::make_key("srv_1", "key_passphrase");
        let key3 = super::super::make_key("srv_2", "password");

        store.save(&key1, "pass1").unwrap();
        store.save(&key2, "passphrase1").unwrap();
        store.save(&key3, "pass2").unwrap();

        store.delete_all_for_server("srv_1").unwrap();

        assert!(store.load(&key1).is_err());
        assert!(store.load(&key2).is_err());
        assert!(store.load(&key3).is_ok()); // srv_2 should survive
    }

    #[test]
    fn test_key_naming() {
        let key = super::super::make_key("srv_tokyo", "password");
        assert_eq!(key, "vps-guard::srv_tokyo::password");
    }

    #[test]
    fn test_has() {
        let store = InMemoryCredentialStore::new();
        store.save("key1", "value1").unwrap();
        assert!(store.has("key1"));
        assert!(!store.has("key2"));
    }
}
