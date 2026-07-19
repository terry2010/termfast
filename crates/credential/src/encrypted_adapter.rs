//! Adapter that implements `CredentialStore` on top of
//! `EncryptedCredentialStore`, caching the decrypted map in memory.
//!
//! Workflow:
//! 1. `EncryptedCredentialStore::open(path)` — opens the file (may not exist).
//! 2a. If file absent: store starts in **pending** mode — credentials are held
//!     in memory only, not persisted. `save`/`load`/`delete` work on the
//!     in-memory map. When `initialize()` is called, the pending map is
//!     flushed to the encrypted file.
//! 2b. If file exists: `unlock(master_password)` — derives key, decrypts file,
//!     caches map.
//! 3. `CredentialStore` trait ops — read/write in-memory map, re-encrypt file on writes.
//! 4. `lock()` — clears cached key + map.
//! 5. `flush()` — explicit write (normally auto on save/delete).

use super::CredentialStore;
use super::encrypted::{DerivedKey, EncryptedCredentialStore};
use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use zeroize::Zeroize;

/// In-memory state held behind a mutex.
struct Inner {
    /// None when locked; Some when unlocked.
    key: Option<DerivedKey>,
    /// Decrypted credential map. None when locked.
    /// In pending mode (no password set), this is Some(empty) and key is None.
    map: Option<HashMap<String, String>>,
    /// True when no encrypted file exists yet (pending mode).
    /// In this mode, save/load/delete work on memory only.
    pending: bool,
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Zeroize sensitive data when the Inner struct is dropped.
        self.key.zeroize();
        if let Some(ref mut map) = self.map {
            for (_, v) in map.iter_mut() {
                v.zeroize();
            }
            map.clear();
        }
    }
}

/// Adapter implementing `CredentialStore` over an encrypted file.
pub struct EncryptedFileCredentialStore {
    store: EncryptedCredentialStore,
    inner: Mutex<Inner>,
}

impl EncryptedFileCredentialStore {
    /// Open an encrypted credential store at the given path.
    /// If the file doesn't exist, starts in **pending** mode (memory-only).
    /// If the file exists, starts in locked state — call `unlock()` before using.
    pub fn open(path: PathBuf) -> Self {
        let store = EncryptedCredentialStore::open(path);
        let pending = store.is_absent();
        Self {
            store,
            inner: Mutex::new(Inner {
                key: None,
                // In pending mode, map is Some(empty) so save/load work.
                map: if pending { Some(HashMap::new()) } else { None },
                pending,
            }),
        }
    }

    /// Whether the encrypted file exists and is initialized.
    pub fn is_initialized(&self) -> bool {
        self.store.is_initialized()
    }

    /// Whether the file is a legacy plaintext file needing migration.
    pub fn is_legacy_plaintext(&self) -> bool {
        self.store.is_legacy_plaintext()
    }

    /// Whether no credential file exists at all (fresh install).
    pub fn is_absent(&self) -> bool {
        self.store.is_absent()
    }

    /// First-time setup: initialize the encrypted store with the pending
    /// in-memory credentials (if any) and the given master password.
    /// Also unlocks (caches key + map). Transitions out of pending mode.
    pub fn initialize(&self, master_password: &str) -> Result<()> {
        // Grab the pending map (if in pending mode) so we can persist it.
        let pending_map = {
            let mut inner = self.inner.lock().unwrap();
            if inner.pending {
                inner.map.take()
            } else {
                None
            }
        };
        let initial_json = if let Some(ref map) = pending_map {
            serde_json::to_vec(map)?
        } else {
            b"{}".to_vec()
        };
        self.store.initialize(master_password, &initial_json)?;
        let key = self.store.unlock(master_password)?;
        let mut inner = self.inner.lock().unwrap();
        inner.key = Some(key);
        inner.map = Some(pending_map.unwrap_or_default());
        inner.pending = false;
        Ok(())
    }

    /// Unlock with a master password: derive key, decrypt file, cache map.
    pub fn unlock(&self, master_password: &str) -> Result<()> {
        let key = self.store.unlock(master_password)?;
        let plaintext = self.store.decrypt(&key)?;
        let map: HashMap<String, String> = if plaintext.is_empty() {
            HashMap::new()
        } else {
            serde_json::from_slice(&plaintext)
                .map_err(|e| anyhow!("failed to parse decrypted credentials: {}", e))?
        };
        let mut inner = self.inner.lock().unwrap();
        inner.key = Some(key);
        inner.map = Some(map);
        Ok(())
    }

    /// Unlock using a pre-cached derived key (e.g. from OS keychain).
    pub fn unlock_with_key(&self, key: DerivedKey) -> Result<()> {
        let plaintext = self.store.decrypt(&key)?;
        let map: HashMap<String, String> = if plaintext.is_empty() {
            HashMap::new()
        } else {
            serde_json::from_slice(&plaintext)
                .map_err(|e| anyhow!("failed to parse decrypted credentials: {}", e))?
        };
        let mut inner = self.inner.lock().unwrap();
        inner.key = Some(key);
        inner.map = Some(map);
        Ok(())
    }

    /// Lock: clear cached key and map from memory.
    pub fn lock(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.key.zeroize();
        if let Some(ref mut map) = inner.map {
            for (_, v) in map.iter_mut() {
                v.zeroize();
            }
        }
        inner.key = None;
        inner.map = None;
    }

    /// Whether the store is currently unlocked (or in pending mode).
    /// In pending mode, credentials are accessible (in memory) even
    /// though no key is set.
    pub fn is_unlocked(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.pending || inner.key.is_some()
    }

    /// Whether the store is in pending mode (no password set, memory-only).
    pub fn is_pending(&self) -> bool {
        self.inner.lock().unwrap().pending
    }

    /// Get the derived key (for caching in OS keychain).
    /// Returns None if locked.
    pub fn derived_key(&self) -> Option<DerivedKey> {
        self.inner.lock().unwrap().key.clone()
    }

    /// Migrate a legacy plaintext file to encrypted format, then unlock.
    pub fn migrate(&self, master_password: &str) -> Result<()> {
        self.store.migrate(master_password)?;
        self.unlock(master_password)?;
        Ok(())
    }

    /// Reset: delete the encrypted file and return to pending mode.
    pub fn reset(&self) -> Result<()> {
        self.store.reset()?;
        let mut inner = self.inner.lock().unwrap();
        inner.key.zeroize();
        if let Some(ref mut map) = inner.map {
            for (_, v) in map.iter_mut() {
                v.zeroize();
            }
        }
        inner.key = None;
        inner.map = Some(HashMap::new());
        inner.pending = true;
        Ok(())
    }

    /// Change master password. Re-encrypts the file with the new password.
    /// The cached derived key is updated.
    pub fn change_password(&self, old: &str, new: &str) -> Result<DerivedKey> {
        self.store.change_password(old, new)?;
        // Re-unlock with new password to refresh cached key.
        let key = self.store.unlock(new)?;
        let mut inner = self.inner.lock().unwrap();
        inner.key = Some(key.clone());
        Ok(key)
    }

    /// Export the encrypted file to a destination path.
    pub fn export_to(&self, dest: &std::path::Path) -> Result<()> {
        self.store.export_to(dest)
    }

    /// Import an encrypted file from a source path.
    /// Verifies the master password can decrypt the file before overwriting.
    /// On success, unlocks with the imported credentials.
    pub fn import_from(&self, src: &std::path::Path, master_password: &str) -> Result<()> {
        // Read and validate the import file format.
        let data = std::fs::read(src).context("failed to read import file")?;
        let header = super::encrypted::read_header_pub(&data)
            .context("import file is not a valid encrypted credential file")?;
        // Verify password can decrypt before overwriting local file.
        let key = super::encrypted::derive_key_pub(master_password, &header.salt)?;
        let _plaintext = super::encrypted::decrypt_with_key_pub(&key, &header, &data[super::encrypted::HEADER_LEN..])
            .context("wrong master password or corrupted import file")?;
        // Password verified — safe to overwrite.
        super::encrypted::write_atomic_pub(&self.store.path(), &data)?;
        self.store.set_sync_version(header.sync_version);
        // Unlock with the imported file.
        self.unlock(master_password)?;
        let mut inner = self.inner.lock().unwrap();
        inner.pending = false;
        Ok(())
    }

    /// Re-encrypt the in-memory map to the file. Called automatically on
    /// save/delete, but can be called explicitly.
    fn flush(&self) -> Result<()> {
        let inner = self.inner.lock().unwrap();
        let key = inner
            .key
            .as_ref()
            .ok_or_else(|| anyhow!("credential store is locked"))?;
        let map = inner
            .map
            .as_ref()
            .ok_or_else(|| anyhow!("credential store is locked"))?;
        // Zeroizing so the plaintext JSON is wiped after encryption.
        let json = zeroize::Zeroizing::new(serde_json::to_vec(map)?);
        // encrypt() needs a non-locked store; clone key to avoid holding the
        // mutex across the file write.
        let key_clone = key.clone();
        drop(inner);
        self.store.encrypt(&key_clone, &json)?;
        Ok(())
    }
}

impl CredentialStore for EncryptedFileCredentialStore {
    fn save(&self, key: &str, value: &str) -> Result<()> {
        let pending = {
            let mut inner = self.inner.lock().unwrap();
            let map = inner
                .map
                .as_mut()
                .ok_or_else(|| anyhow!("credential store is locked"))?;
            map.insert(key.to_string(), value.to_string());
            inner.pending
        };
        // In pending mode, don't flush to file (no key yet).
        if !pending {
            self.flush()?;
        }
        Ok(())
    }

    fn load(&self, key: &str) -> Result<String> {
        let inner = self.inner.lock().unwrap();
        let map = inner
            .map
            .as_ref()
            .ok_or_else(|| anyhow!("credential store is locked"))?;
        map.get(key)
            .cloned()
            .ok_or_else(|| anyhow!("credential not found: {}", key))
    }

    fn delete(&self, key: &str) -> Result<()> {
        let (changed, pending) = {
            let mut inner = self.inner.lock().unwrap();
            let map = inner
                .map
                .as_mut()
                .ok_or_else(|| anyhow!("credential store is locked"))?;
            let c = map.remove(key).is_some();
            (c, inner.pending)
        };
        if changed && !pending {
            self.flush()?;
        }
        Ok(())
    }

    fn delete_all_for_server(&self, server_id: &str) -> Result<()> {
        let prefix = format!("{}::{}::", super::SERVICE_NAME, server_id);
        let (changed, pending) = {
            let mut inner = self.inner.lock().unwrap();
            let map = inner
                .map
                .as_mut()
                .ok_or_else(|| anyhow!("credential store is locked"))?;
            let before = map.len();
            map.retain(|k, _| !k.starts_with(&prefix));
            (map.len() != before, inner.pending)
        };
        if changed && !pending {
            self.flush()?;
        }
        Ok(())
    }

    fn has(&self, key: &str) -> bool {
        let inner = self.inner.lock().unwrap();
        inner
            .map
            .as_ref()
            .map(|m| m.contains_key(key))
            .unwrap_or(false)
    }
}

// === SECTION 1 END ===

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn store_path(dir: &std::path::Path) -> PathBuf {
        dir.join("credentials.enc")
    }

    #[test]
    fn test_adapter_save_load_unlocked() {
        let dir = tempdir().unwrap();
        let store = EncryptedFileCredentialStore::open(store_path(dir.path()));
        store.initialize("pw").unwrap();
        assert!(store.is_unlocked());

        store.save("key1", "value1").unwrap();
        assert_eq!(store.load("key1").unwrap(), "value1");
        assert!(store.has("key1"));
    }

    #[test]
    fn test_adapter_persistence_across_reopen() {
        let dir = tempdir().unwrap();
        let path = store_path(dir.path());
        {
            let store = EncryptedFileCredentialStore::open(path.clone());
            store.initialize("pw").unwrap();
            store.save("k1", "v1").unwrap();
            store.save("k2", "v2").unwrap();
        }
        // Reopen and unlock — data should persist.
        let store2 = EncryptedFileCredentialStore::open(path);
        assert!(!store2.is_unlocked());
        store2.unlock("pw").unwrap();
        assert_eq!(store2.load("k1").unwrap(), "v1");
        assert_eq!(store2.load("k2").unwrap(), "v2");
    }

    #[test]
    fn test_adapter_locked_ops_fail() {
        let dir = tempdir().unwrap();
        let store = EncryptedFileCredentialStore::open(store_path(dir.path()));
        store.initialize("pw").unwrap();
        store.lock();
        assert!(!store.is_unlocked());
        assert!(store.save("k", "v").is_err());
        assert!(store.load("k").is_err());
    }

    #[test]
    fn test_adapter_delete() {
        let dir = tempdir().unwrap();
        let store = EncryptedFileCredentialStore::open(store_path(dir.path()));
        store.initialize("pw").unwrap();
        store.save("k1", "v1").unwrap();
        store.delete("k1").unwrap();
        assert!(!store.has("k1"));
        // Reopen to verify deletion persisted.
        let store2 = EncryptedFileCredentialStore::open(store_path(dir.path()));
        store2.unlock("pw").unwrap();
        assert!(!store2.has("k1"));
    }

    #[test]
    fn test_adapter_delete_all_for_server() {
        let dir = tempdir().unwrap();
        let store = EncryptedFileCredentialStore::open(store_path(dir.path()));
        store.initialize("pw").unwrap();
        store.save("termfast::srv1::password", "p1").unwrap();
        store.save("termfast::srv1::key_passphrase", "kp1").unwrap();
        store.save("termfast::srv2::password", "p2").unwrap();

        store.delete_all_for_server("srv1").unwrap();

        assert!(!store.has("termfast::srv1::password"));
        assert!(!store.has("termfast::srv1::key_passphrase"));
        assert!(store.has("termfast::srv2::password"));
    }

    #[test]
    fn test_adapter_migrate_then_unlock() {
        let dir = tempdir().unwrap();
        let path = store_path(dir.path());
        // Write a legacy plaintext file.
        let original = br#"{"termfast::srv1::password":"plain"}"#;
        std::fs::write(&path, original).unwrap();

        let store = EncryptedFileCredentialStore::open(path);
        assert!(store.is_legacy_plaintext());
        store.migrate("newpw").unwrap();
        assert!(store.is_unlocked());
        assert_eq!(
            store.load("termfast::srv1::password").unwrap(),
            "plain"
        );
    }

    #[test]
    fn test_adapter_change_password() {
        let dir = tempdir().unwrap();
        let store = EncryptedFileCredentialStore::open(store_path(dir.path()));
        store.initialize("oldpw").unwrap();
        store.save("k", "v").unwrap();

        store.change_password("oldpw", "newpw").unwrap();

        // Old password should fail on reopen.
        let store2 = EncryptedFileCredentialStore::open(store_path(dir.path()));
        assert!(store2.unlock("oldpw").is_err());
        store2.unlock("newpw").unwrap();
        assert_eq!(store2.load("k").unwrap(), "v");
    }

    #[test]
    fn test_adapter_reset() {
        let dir = tempdir().unwrap();
        let path = store_path(dir.path());
        let store = EncryptedFileCredentialStore::open(path.clone());
        store.initialize("pw").unwrap();
        store.save("k", "v").unwrap();
        store.reset().unwrap();
        // Reset returns to pending mode (memory-only, no file).
        assert!(store.is_pending());
        assert!(!store.is_initialized());
        assert!(!path.exists());
    }

    #[test]
    fn test_adapter_export_import() {
        let dir = tempdir().unwrap();
        let path = store_path(dir.path());
        let export = dir.path().join("export.enc");

        let store = EncryptedFileCredentialStore::open(path.clone());
        store.initialize("pw").unwrap();
        store.save("k", "v").unwrap();
        store.export_to(&export).unwrap();

        let dir2 = tempdir().unwrap();
        let path2 = store_path(dir2.path());
        let store2 = EncryptedFileCredentialStore::open(path2);
        // Import with correct password — should unlock automatically.
        store2.import_from(&export, "pw").unwrap();
        assert!(store2.is_unlocked()); // unlocked after import
        assert_eq!(store2.load("k").unwrap(), "v");
    }

    #[test]
    fn test_adapter_import_wrong_password_fails() {
        let dir = tempdir().unwrap();
        let path = store_path(dir.path());
        let export = dir.path().join("export.enc");

        let store = EncryptedFileCredentialStore::open(path.clone());
        store.initialize("pw").unwrap();
        store.save("k", "v").unwrap();
        store.export_to(&export).unwrap();

        let dir2 = tempdir().unwrap();
        let path2 = store_path(dir2.path());
        let store2 = EncryptedFileCredentialStore::open(path2.clone());
        // Import with wrong password — should fail, local file untouched.
        assert!(store2.import_from(&export, "wrong").is_err());
        assert!(!path2.exists()); // local file not overwritten
    }

    #[test]
    fn test_adapter_unlock_with_cached_key() {
        let dir = tempdir().unwrap();
        let path = store_path(dir.path());
        let store = EncryptedFileCredentialStore::open(path.clone());
        store.initialize("pw").unwrap();
        store.save("k", "v").unwrap();
        let cached_key = store.derived_key().unwrap();
        store.lock();

        // Reopen with cached key (simulating OS keychain path).
        let store2 = EncryptedFileCredentialStore::open(path);
        store2.unlock_with_key(cached_key).unwrap();
        assert_eq!(store2.load("k").unwrap(), "v");
    }

    #[test]
    fn test_pending_mode_memory_only() {
        let dir = tempdir().unwrap();
        let path = store_path(dir.path());
        let store = EncryptedFileCredentialStore::open(path.clone());

        // No file exists → pending mode.
        assert!(store.is_pending());
        assert!(store.is_unlocked()); // accessible via memory
        assert!(!store.is_initialized());

        // Save/load works in memory.
        store.save("k1", "v1").unwrap();
        assert_eq!(store.load("k1").unwrap(), "v1");

        // File should NOT exist on disk yet.
        assert!(!path.exists());

        // Initialize with password → pending credentials flushed to file.
        store.initialize("pw").unwrap();
        assert!(!store.is_pending());
        assert!(store.is_initialized());
        assert!(path.exists());

        // Credentials survived the transition.
        assert_eq!(store.load("k1").unwrap(), "v1");
    }

    #[test]
    fn test_pending_mode_then_initialize_persists() {
        let dir = tempdir().unwrap();
        let path = store_path(dir.path());

        // Phase 1: pending mode, save some credentials.
        let store = EncryptedFileCredentialStore::open(path.clone());
        store.save("a", "1").unwrap();
        store.save("b", "2").unwrap();

        // Initialize with password.
        store.initialize("secret").unwrap();
        assert!(!store.is_pending());

        // Phase 2: reopen, unlock, verify credentials persisted.
        let store2 = EncryptedFileCredentialStore::open(path);
        assert!(!store2.is_pending());
        assert!(!store2.is_unlocked());
        store2.unlock("secret").unwrap();
        assert_eq!(store2.load("a").unwrap(), "1");
        assert_eq!(store2.load("b").unwrap(), "2");
    }
}
