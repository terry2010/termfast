//! File-based credential store — persists credentials to a JSON file.
//!
//! Used on Android where OS keychain is not directly accessible from Rust.
//! The file is stored in the app's private data directory (mode 0700).

use super::CredentialStore;
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// File-based credential store backed by a JSON file.
pub struct FileCredentialStore {
    path: PathBuf,
    store: Mutex<HashMap<String, String>>,
}

impl FileCredentialStore {
    /// Create a new file credential store at the given path.
    /// Loads existing data if the file exists.
    pub fn new(path: PathBuf) -> Self {
        let store = Self::load_file(&path).unwrap_or_default();
        Self {
            path,
            store: Mutex::new(store),
        }
    }

    fn load_file(path: &PathBuf) -> Result<HashMap<String, String>> {
        if !path.exists() {
            return Ok(HashMap::new());
        }
        let data = std::fs::read_to_string(path)?;
        let map: HashMap<String, String> = serde_json::from_str(&data)?;
        Ok(map)
    }

    fn save_file(&self) -> Result<()> {
        let store = self.store.lock().unwrap();
        let data = serde_json::to_string_pretty(&*store)?;
        // Crash-safe atomic write with fsync
        termfast_core::fs::write_atomic(&self.path, data.as_bytes(), true)?;
        Ok(())
    }
}

impl CredentialStore for FileCredentialStore {
    fn save(&self, key: &str, value: &str) -> Result<()> {
        self.store
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());
        self.save_file()
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
        self.save_file()
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
        drop(store);
        self.save_file()
    }

    fn has(&self, key: &str) -> bool {
        self.store.lock().unwrap().contains_key(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_file_store_save_load() {
        let tmp = NamedTempFile::new().unwrap();
        let store = FileCredentialStore::new(tmp.path().to_path_buf());
        store.save("key1", "value1").unwrap();
        assert_eq!(store.load("key1").unwrap(), "value1");
    }

    #[test]
    fn test_file_store_persistence() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        {
            let store = FileCredentialStore::new(path.clone());
            store.save("key1", "value1").unwrap();
        }
        // Re-create store from same file
        let store2 = FileCredentialStore::new(path);
        assert_eq!(store2.load("key1").unwrap(), "value1");
    }

    #[test]
    fn test_file_store_delete() {
        let tmp = NamedTempFile::new().unwrap();
        let store = FileCredentialStore::new(tmp.path().to_path_buf());
        store.save("key1", "value1").unwrap();
        store.delete("key1").unwrap();
        assert!(store.load("key1").is_err());
    }
}
