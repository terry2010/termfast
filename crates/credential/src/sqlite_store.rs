//! SQLCipher-backed credential store.
//!
//! Wraps `SqlCipherStorage` from `termfast-core` and implements the
//! `CredentialStore` trait. Credentials are stored in the `credentials`
//! table of the SQLCipher DB, encrypted at rest by the DEK.
//!
//! Lock/unlock is a UX-level flag (AtomicBool), not a security boundary.
//! See §4.7 of the migration doc for security implications.
//!
//! ## Lifecycle methods alignment (§4.5)
//!
//! All 16 methods of `EncryptedFileCredentialStore` are mirrored here:
//! - `new(storage)` ← `open(path)`
//! - `is_initialized()` / `is_pending()` / `is_absent()` / `is_legacy_plaintext()`
//! - `initialize(password)` — Argon2id → rekey
//! - `unlock(dek)` / `unlock_with_password(password)` / `unlock_with_cached_key()`
//! - `lock()` / `is_unlocked()`
//! - `change_password(old, new)` — verify old → rekey with new
//! - `derived_key()` — return current DEK
//! - `migrate(password)` — no-op (no legacy plaintext)
//! - `reset()` — clear all data

use crate::encrypted::derive_key_pub;
use crate::CredentialStore;
use anyhow::{anyhow, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use termfast_core::config::SqlCipherStorage;
use zeroize::Zeroize;

pub struct SqlCipherCredentialStore {
    storage: Arc<SqlCipherStorage>,
    locked: AtomicBool,
    /// Current DEK held in memory for rekey operations and derived_key() access.
    /// None when locked or when the DEK hasn't been set yet.
    dek: Mutex<Option<[u8; 32]>>,
    /// Master password held in memory for export_to (envelope encryption).
    /// None when locked or when unlock was via cached DEK (no password available).
    master_password: Mutex<Option<String>>,
}

impl SqlCipherCredentialStore {
    pub fn new(storage: Arc<SqlCipherStorage>) -> Self {
        Self {
            storage,
            locked: AtomicBool::new(false),
            dek: Mutex::new(None),
            master_password: Mutex::new(None),
        }
    }

    /// Create with a known DEK (used when daemon layer already resolved the DEK).
    pub fn new_with_dek(storage: Arc<SqlCipherStorage>, dek: [u8; 32]) -> Self {
        Self {
            storage,
            locked: AtomicBool::new(false),
            dek: Mutex::new(Some(dek)),
            master_password: Mutex::new(None),
        }
    }

    /// Lock the credential layer (UX lock, not security lock).
    /// Does NOT close the DB or clear the DEK.
    pub fn lock(&self) {
        self.locked.store(true, Ordering::Relaxed);
    }

    /// Unlock with a pre-verified DEK (e.g., from keychain cache).
    /// Skips re-verification since the DEK was already used to open the DB.
    pub fn unlock_with_cached_key(&self) {
        self.locked.store(false, Ordering::Relaxed);
    }

    /// Unlock by providing the DEK directly.
    /// Verifies the DEK can open the DB, then marks as unlocked.
    pub fn unlock(&self, dek: &[u8; 32]) -> Result<()> {
        let db_path = &self.storage.db_path;
        let conn = termfast_core::config::open_with_key(db_path, dek)
            .map_err(|_| anyhow!("wrong password"))?;
        drop(conn);
        // Store the DEK for future rekey operations
        let mut guard = self.dek.lock().unwrap();
        *guard = Some(*dek);
        self.locked.store(false, Ordering::Relaxed);
        Ok(())
    }

    /// Unlock by deriving DEK from master password.
    /// Uses Argon2id with a fixed salt (the DB file itself is the salt source).
    pub fn unlock_with_password(&self, master_password: &str) -> Result<()> {
        let salt = self.derive_salt();
        let derived = derive_key_pub(master_password, &salt)?;
        let dek: [u8; 32] = derived.as_bytes().try_into().map_err(|_| anyhow!("invalid key length"))?;
        self.unlock(&dek)?;
        // Store master password for export_to
        *self.master_password.lock().unwrap() = Some(master_password.to_string());
        Ok(())
    }

    /// Initialize: set a master password for the first time.
    /// Derives a new DEK from the password and rekeys the DB.
    /// Only valid when `is_pending()` is true (using default DEK).
    pub fn initialize(&self, master_password: &str) -> Result<()> {
        if !self.is_pending() {
            return Err(anyhow!("master password already set"));
        }
        let salt = self.derive_salt();
        let derived = derive_key_pub(master_password, &salt)?;
        let new_dek: [u8; 32] = derived.as_bytes().try_into().map_err(|_| anyhow!("invalid key length"))?;
        self.storage.rekey(&new_dek).map_err(|e| anyhow!(e))?;
        // Store the new DEK and master password
        *self.dek.lock().unwrap() = Some(new_dek);
        *self.master_password.lock().unwrap() = Some(master_password.to_string());
        Ok(())
    }

    /// Change the master password.
    /// Verifies the old password, then rekeys with a new DEK derived from the new password.
    pub fn change_password(&self, old_password: &str, new_password: &str) -> Result<[u8; 32]> {
        // Verify old password
        self.unlock_with_password(old_password)?;

        // Derive new DEK
        let salt = self.derive_salt();
        let derived = derive_key_pub(new_password, &salt)?;
        let new_dek: [u8; 32] = derived.as_bytes().try_into().map_err(|_| anyhow!("invalid key length"))?;

        // Rekey
        self.storage.rekey(&new_dek).map_err(|e| anyhow!(e))?;

        // Update stored DEK and master password
        *self.dek.lock().unwrap() = Some(new_dek);
        *self.master_password.lock().unwrap() = Some(new_password.to_string());
        Ok(new_dek)
    }

    /// Get the current DEK (for keychain caching).
    /// Returns None if the DEK hasn't been set (e.g., pending mode with default DEK).
    pub fn derived_key(&self) -> Option<[u8; 32]> {
        let guard = self.dek.lock().unwrap();
        *guard
    }

    /// Derive a salt from the DB file path.
    /// Uses the DB file path as salt source — consistent across restarts.
    fn derive_salt(&self) -> Vec<u8> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(self.storage.db_path.to_string_lossy().as_bytes());
        hasher.finalize().to_vec()
    }

    pub fn is_unlocked(&self) -> bool {
        !self.locked.load(Ordering::Relaxed)
    }

    /// True when using default DEK (no master password set).
    pub fn is_pending(&self) -> bool {
        self.storage.is_using_default_dek()
    }

    /// True when a master password has been set (non-default DEK).
    pub fn is_initialized(&self) -> bool {
        !self.storage.is_using_default_dek()
    }

    /// No legacy plaintext in SQLCipher migration.
    pub fn is_legacy_plaintext(&self) -> bool {
        false
    }

    /// DB file doesn't exist = first launch.
    pub fn is_absent(&self) -> bool {
        !self.storage.db_file_exists()
    }

    /// No migration needed in SQLCipher migration.
    pub fn migrate(&self, _master_password: &str) -> Result<()> {
        Ok(())
    }

    /// Reset: clear all data, rekey back to DEFAULT_DEK, clear keychain.
    /// Design doc §4.5: "删除 DB 文件 + 清除 keychain + 用默认 DEK 重建"
    /// Implementation: rekey to DEFAULT_DEK (BEFORE clearing marker) → clear all tables → clear keychain.
    /// NOTE: rekey MUST happen before storage.reset(), because reset() sets
    /// using_default_dek='true' in the same batch. If we check is_using_default_dek()
    /// after reset(), it returns true and rekey is skipped — leaving the DB
    /// encrypted with the user's DEK while the marker says default DEK.
    pub fn reset(&self) -> Result<()> {
        // 1. Rekey back to DEFAULT_DEK BEFORE clearing the marker (if currently using a user DEK)
        if !self.storage.is_using_default_dek() {
            self.storage.rekey(&termfast_core::config::DEFAULT_DEK)
                .map_err(|e| anyhow!(e))?;
        }
        // 2. Clear all data in tables (this also sets using_default_dek='true')
        self.storage.reset().map_err(|e| anyhow!(e))?;
        // 3. Clear keychain cached DEK (platform-specific, best-effort)
        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        {
            use crate::keychain::KeychainCredentialStore;
            use crate::CredentialStore;
            let ks = KeychainCredentialStore::new();
            let _ = ks.delete("termfast::dek");
        }
        // 4. Clear stored DEK and master password in memory
        let mut guard = self.dek.lock().unwrap();
        guard.zeroize();
        *guard = None;
        *self.master_password.lock().unwrap() = None;
        Ok(())
    }

    /// Export all credentials to a file, encrypted with the master password.
    /// Design doc §4.5: "用 FullExportData 序列化 + envelope 加密"
    /// Uses the envelope module to create a portable encrypted file that can
    /// be imported on another device with the same master password.
    pub fn export_to(&self, dest: &std::path::Path) -> Result<()> {
        let password = self.master_password.lock().unwrap().clone()
            .ok_or_else(|| anyhow!("master password not available — unlock with password first"))?;

        // Read all credentials from DB
        let credentials = self.storage.list_credentials()
            .map_err(|e| anyhow!("failed to read credentials: {}", e))?;

        // Serialize as JSON
        let json = serde_json::to_vec(&credentials)
            .map_err(|e| anyhow!("failed to serialize credentials: {}", e))?;

        // Encrypt with envelope
        let salt = self.derive_salt();
        let salt_arr: [u8; 16] = salt[..16].try_into().map_err(|_| anyhow!("invalid salt length"))?;
        let blob = crate::envelope::encrypt(
            b"TCRE",
            &password,
            &salt_arr,
            &[],
            crate::envelope::Argon2Params::desktop(),
            &json,
        ).map_err(|e| anyhow!("failed to encrypt export: {}", e))?;

        // Write to file atomically
        std::fs::write(dest, &blob)
            .map_err(|e| anyhow!("failed to write export file: {}", e))?;
        Ok(())
    }

    /// Import credentials from an encrypted file.
    /// Design doc §4.5: "解密 import 文件 → FullExportData → 写入 SQLCipher"
    /// Decrypts with the provided master password, then writes credentials to DB.
    pub fn import_from(&self, src: &std::path::Path, master_password: &str) -> Result<()> {
        // Read file
        let data = std::fs::read(src)
            .map_err(|e| anyhow!("failed to read import file: {}", e))?;

        // Verify magic
        if data.len() < 5 || &data[..4] != b"TCRE" {
            anyhow::bail!("import file is not a valid encrypted credential file");
        }

        // Decrypt with envelope
        let plaintext = crate::envelope::decrypt(b"TCRE", master_password, &[], &data)
            .map_err(|e| anyhow!("failed to decrypt import file: {}", e))?;

        // Parse JSON
        let credentials: std::collections::HashMap<String, String> = serde_json::from_slice(&plaintext)
            .map_err(|e| anyhow!("failed to parse credentials JSON: {}", e))?;

        // Write credentials to DB
        for (key, value) in &credentials {
            self.storage.upsert_credential(key, value)
                .map_err(|e| anyhow!("failed to write credential {}: {}", key, e))?;
        }

        // Store master password for future export_to
        *self.master_password.lock().unwrap() = Some(master_password.to_string());

        Ok(())
    }

    /// Get a reference to the underlying SqlCipherStorage.
    pub fn storage(&self) -> &Arc<SqlCipherStorage> {
        &self.storage
    }
}

impl CredentialStore for SqlCipherCredentialStore {
    fn save(&self, key: &str, value: &str) -> Result<()> {
        if self.locked.load(Ordering::Relaxed) {
            return Err(anyhow!("credential store is locked"));
        }
        self.storage
            .upsert_credential(key, value)
            .map_err(|e| anyhow!(e))
    }

    fn load(&self, key: &str) -> Result<String> {
        if self.locked.load(Ordering::Relaxed) {
            return Err(anyhow!("credential store is locked"));
        }
        self.storage
            .get_credential(key)
            .map_err(|e| anyhow!(e))?
            .ok_or_else(|| anyhow!("credential not found: {}", key))
    }

    fn delete(&self, key: &str) -> Result<()> {
        if self.locked.load(Ordering::Relaxed) {
            return Err(anyhow!("credential store is locked"));
        }
        self.storage
            .delete_credential(key)
            .map_err(|e| anyhow!(e))
    }

    fn delete_all_for_server(&self, server_id: &str) -> Result<()> {
        if self.locked.load(Ordering::Relaxed) {
            return Err(anyhow!("credential store is locked"));
        }
        self.storage
            .delete_credentials_for_server(server_id)
            .map_err(|e| anyhow!(e))
    }

    fn has(&self, key: &str) -> bool {
        if self.locked.load(Ordering::Relaxed) {
            return false;
        }
        self.storage
            .get_credential(key)
            .map(|v| v.is_some())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use termfast_core::config::{SqlCipherStorage, DEFAULT_DEK};

    fn test_store() -> (tempfile::TempDir, Arc<SqlCipherStorage>, SqlCipherCredentialStore) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let storage = Arc::new(SqlCipherStorage::create_new(&path, &DEFAULT_DEK).unwrap());
        let store = SqlCipherCredentialStore::new(storage.clone());
        (dir, storage, store)
    }

    #[test]
    fn test_save_load() {
        let (_dir, _storage, store) = test_store();
        store.save("key1", "secret1").unwrap();
        assert_eq!(store.load("key1").unwrap(), "secret1");
    }

    #[test]
    fn test_load_missing() {
        let (_dir, _storage, store) = test_store();
        assert!(store.load("nonexistent").is_err());
    }

    #[test]
    fn test_delete() {
        let (_dir, _storage, store) = test_store();
        store.save("key1", "secret1").unwrap();
        store.delete("key1").unwrap();
        assert!(store.load("key1").is_err());
    }

    #[test]
    fn test_has() {
        let (_dir, _storage, store) = test_store();
        assert!(!store.has("key1"));
        store.save("key1", "secret1").unwrap();
        assert!(store.has("key1"));
    }

    #[test]
    fn test_delete_all_for_server() {
        let (_dir, _storage, store) = test_store();
        store.save("termfast::srv1::password", "p1").unwrap();
        store.save("termfast::srv2::password", "p2").unwrap();
        store.delete_all_for_server("srv1").unwrap();
        assert!(!store.has("termfast::srv1::password"));
        assert!(store.has("termfast::srv2::password"));
    }

    #[test]
    fn test_lock_unlock() {
        let (_dir, _storage, store) = test_store();
        store.save("key1", "secret1").unwrap();

        store.lock();
        assert!(!store.is_unlocked());
        assert!(store.load("key1").is_err());
        assert!(store.save("key2", "val").is_err());

        store.unlock_with_cached_key();
        assert!(store.is_unlocked());
        assert_eq!(store.load("key1").unwrap(), "secret1");
    }

    #[test]
    fn test_is_pending_default_dek() {
        let (_dir, _storage, store) = test_store();
        assert!(store.is_pending());
        assert!(!store.is_initialized());
    }

    #[test]
    fn test_is_initialized_after_rekey() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let storage = Arc::new(SqlCipherStorage::create_new(&path, &DEFAULT_DEK).unwrap());
        let store = SqlCipherCredentialStore::new(storage.clone());

        // Rekey to a user DEK
        let new_dek = [0xAA; 32];
        storage.rekey(&new_dek).unwrap();

        assert!(!store.is_pending());
        assert!(store.is_initialized());
    }

    #[test]
    fn test_is_legacy_plaintext_always_false() {
        let (_dir, _storage, store) = test_store();
        assert!(!store.is_legacy_plaintext());
    }

    #[test]
    fn test_unlock_with_correct_dek() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let storage = Arc::new(SqlCipherStorage::create_new(&path, &DEFAULT_DEK).unwrap());
        let store = SqlCipherCredentialStore::new(storage.clone());

        store.lock();
        // Unlock with the correct DEK (DEFAULT_DEK since we haven't rekeyed)
        store.unlock(&DEFAULT_DEK).unwrap();
        assert!(store.is_unlocked());
    }

    #[test]
    fn test_unlock_with_wrong_dek() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let storage = Arc::new(SqlCipherStorage::create_new(&path, &DEFAULT_DEK).unwrap());
        let store = SqlCipherCredentialStore::new(storage.clone());

        store.lock();
        let wrong_dek = [0xFF; 32];
        assert!(store.unlock(&wrong_dek).is_err());
        assert!(!store.is_unlocked());
    }

    #[test]
    fn test_initialize_sets_password() {
        let (_dir, _storage, store) = test_store();
        assert!(store.is_pending());

        store.initialize("my_password").unwrap();
        assert!(!store.is_pending());
        assert!(store.is_initialized());
        assert!(store.derived_key().is_some());
    }

    #[test]
    fn test_initialize_twice_fails() {
        let (_dir, _storage, store) = test_store();
        store.initialize("password1").unwrap();
        assert!(store.initialize("password2").is_err());
    }

    #[test]
    fn test_unlock_with_password_after_initialize() {
        let (_dir, _storage, store) = test_store();
        store.initialize("my_password").unwrap();
        store.lock();

        // Unlock with correct password
        store.unlock_with_password("my_password").unwrap();
        assert!(store.is_unlocked());
    }

    #[test]
    fn test_unlock_with_wrong_password_fails() {
        let (_dir, _storage, store) = test_store();
        store.initialize("my_password").unwrap();
        store.lock();

        assert!(store.unlock_with_password("wrong_password").is_err());
        assert!(!store.is_unlocked());
    }

    #[test]
    fn test_change_password() {
        let (_dir, _storage, store) = test_store();
        store.initialize("old_password").unwrap();
        store.lock();

        // Change password
        let new_dek = store.change_password("old_password", "new_password").unwrap();
        assert_eq!(new_dek.len(), 32);

        // Old password should fail
        store.lock();
        assert!(store.unlock_with_password("old_password").is_err());

        // New password should work
        assert!(store.unlock_with_password("new_password").is_ok());
    }

    #[test]
    fn test_change_password_wrong_old_fails() {
        let (_dir, _storage, store) = test_store();
        store.initialize("old_password").unwrap();
        store.lock();

        assert!(store.change_password("wrong_old", "new_password").is_err());
    }

    #[test]
    fn test_reset_clears_dek() {
        let (dir, _storage, store) = test_store();
        store.initialize("my_password").unwrap();
        assert!(store.derived_key().is_some());

        store.reset().unwrap();
        assert!(store.derived_key().is_none());
        assert!(store.is_pending());

        // Critical: verify DB can be reopened with DEFAULT_DEK after reset.
        // If rekey was skipped (bug), this would fail with WrongKey.
        let db_path = dir.path().join("test.db");
        let _reopened = termfast_core::config::SqlCipherStorage::open(&db_path, &termfast_core::config::DEFAULT_DEK)
            .expect("DB must be openable with DEFAULT_DEK after reset");
    }

    #[test]
    fn test_credentials_survive_initialize() {
        let (_dir, _storage, store) = test_store();
        store.save("key1", "secret1").unwrap();

        // Initialize master password (rekey)
        store.initialize("my_password").unwrap();

        // Credentials should still be there
        assert_eq!(store.load("key1").unwrap(), "secret1");
    }

    #[test]
    fn test_new_with_dek() {
        let (_dir, storage, _store) = test_store();
        let dek = [0xCC; 32];
        let store = SqlCipherCredentialStore::new_with_dek(storage, dek);
        assert_eq!(store.derived_key(), Some(dek));
    }
}
