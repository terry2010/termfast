//! Encrypted credential store — AES-256-GCM with Argon2id key derivation.
//!
//! File format (all little-endian):
//! ```text
//! offset  size    field
//! 0       4B      magic = b"TCRE"
//! 4       1B      version = 1
//! 5       16B     salt (Argon2id)
//! 21      12B     nonce (AES-GCM)
//! 33      8B      sync_version (u64)
//! 41      N B     ciphertext (includes 16B GCM auth tag at the end)
//! ```
//!
//! The header (magic + version + salt + nonce + sync_version) is used as
//! Additional Authenticated Data (AAD) for AES-GCM, so any tampering with
//! the header invalidates the ciphertext.
//!
//! Legacy plaintext detection: a file whose first 4 bytes are NOT `b"TCRE"`
//! is treated as a legacy plaintext JSON file that needs migration.

use anyhow::{anyhow, bail, Context, Result};
use rand::Rng;
use rand::rng;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use zeroize::Zeroize;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use argon2::Argon2;

/// File magic: "TermFast CREDential".
const MAGIC: &[u8; 4] = b"TCRE";
/// Current format version.
const FORMAT_VERSION: u8 = 1;
/// Argon2id salt length in bytes.
const SALT_LEN: usize = 16;
/// AES-GCM nonce length in bytes.
const NONCE_LEN: usize = 12;
/// Header size before ciphertext: magic(4) + version(1) + salt(16) + nonce(12) + sync_version(8).
pub const HEADER_LEN: usize = 4 + 1 + SALT_LEN + NONCE_LEN + 8;
/// Argon2id parameters.
/// 32 MiB memory / 3 iterations / 1 lane — balances security and responsiveness
/// across desktop and mobile. Above OWASP minimum (16 MiB / 2 iter) while
/// avoiding the ANR risk of 64 MiB on low-end Android. Argon2id runs on
/// background threads (spawn_blocking) so UI is not blocked.
const ARGON2_M_COST: u32 = 32768; // 32 MiB
const ARGON2_T_COST: u32 = 3;
const ARGON2_P_COST: u32 = 1;
/// Derived key length (AES-256).
const KEY_LEN: usize = 32;

/// Argon2id-derived 32-byte key. Cached in Keystore / OS keychain after unlock.
#[derive(Clone, Zeroize)]
pub struct DerivedKey([u8; KEY_LEN]);

impl DerivedKey {
    fn from_slice(s: &[u8]) -> Self {
        let mut arr = [0u8; KEY_LEN];
        arr.copy_from_slice(s);
        Self(arr)
    }
    /// Expose the raw key bytes (used by keychain caching layer).
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
    /// Reconstruct a `DerivedKey` from 32 raw bytes (e.g. loaded from OS
    /// keychain). Panics if the input is not exactly 32 bytes.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        assert_eq!(bytes.len(), KEY_LEN, "DerivedKey must be 32 bytes");
        Self::from_slice(bytes)
    }
}

/// In-memory sync version cache so encrypt() can self-increment without
/// re-reading the file header.
struct State {
    sync_version: u64,
}

/// Encrypted credential store backed by a single file.
pub struct EncryptedCredentialStore {
    path: PathBuf,
    state: Mutex<State>,
}

impl EncryptedCredentialStore {
    /// Open a store at the given path. The file may or may not exist yet.
    pub fn open(path: PathBuf) -> Self {
        let sync_version = std::fs::read(&path)
            .ok()
            .and_then(|data| read_header(&data).ok().map(|h| h.sync_version))
            .unwrap_or(0);
        Self {
            path,
            state: Mutex::new(State { sync_version }),
        }
    }

    /// True if the file exists and has a valid encrypted format header.
    pub fn is_initialized(&self) -> bool {
        match std::fs::read(&self.path) {
            Ok(data) => read_header(&data).is_ok(),
            Err(_) => false,
        }
    }

    /// True if the file exists but is NOT an encrypted store (i.e. legacy
    /// plaintext JSON that needs migration).
    pub fn is_legacy_plaintext(&self) -> bool {
        match std::fs::read(&self.path) {
            Ok(data) => {
                if data.len() < 4 {
                    return !data.is_empty();
                }
                &data[..4] != MAGIC
            }
            Err(_) => false,
        }
    }

    /// True if the file does not exist at all (fresh install).
    pub fn is_absent(&self) -> bool {
        !self.path.exists()
    }

    /// First-time setup: encrypt the given credentials JSON with the
    /// provided master password and write to disk. Generates a random
    /// salt and sets sync_version = 0.
    pub fn initialize(&self, master_password: &str, credentials_json: &[u8]) -> Result<()> {
        if self.path.exists() && self.is_initialized() {
            bail!("credential store already initialized");
        }
        let mut salt = [0u8; SALT_LEN];
        rng().fill_bytes(&mut salt);
        let key = derive_key(master_password, &salt)?;
        let ciphertext = encrypt_with_key(&key, &salt, 0, credentials_json)?;
        let mut out = Vec::with_capacity(HEADER_LEN + ciphertext.ct.len());
        out.extend_from_slice(MAGIC);
        out.push(FORMAT_VERSION);
        out.extend_from_slice(&salt);
        out.extend_from_slice(&ciphertext.nonce);
        out.extend_from_slice(&0u64.to_le_bytes());
        out.extend_from_slice(&ciphertext.ct);
        write_atomic(&self.path, &out)?;
        self.state.lock().unwrap().sync_version = 0;
        Ok(())
    }

    /// Verify the master password and return the derived key for caching.
    /// Fails if the password is wrong (AES-GCM auth tag mismatch).
    pub fn unlock(&self, master_password: &str) -> Result<DerivedKey> {
        let data = std::fs::read(&self.path).context("credential file not found")?;
        let header = read_header(&data)?;
        let key = derive_key(master_password, &header.salt)?;
        // Verify by attempting decryption.
        let _ = decrypt_with_key(&key, &header, &data[HEADER_LEN..])?;
        Ok(key)
    }

    /// Decrypt the credentials file using a previously unlocked key.
    /// Returns `Zeroizing<Vec<u8>>` so the plaintext is wiped from memory
    /// when dropped.
    pub fn decrypt(&self, key: &DerivedKey) -> Result<zeroize::Zeroizing<Vec<u8>>> {
        let data = std::fs::read(&self.path).context("credential file not found")?;
        let header = read_header(&data)?;
        let plaintext = decrypt_with_key(key, &header, &data[HEADER_LEN..])?;
        Ok(zeroize::Zeroizing::new(plaintext))
    }

    /// Encrypt new credentials JSON with a cached key and write to disk.
    /// Increments sync_version by 1.
    pub fn encrypt(&self, key: &DerivedKey, credentials_json: &[u8]) -> Result<()> {
        let data = std::fs::read(&self.path).context("credential file not found")?;
        let header = read_header(&data)?;
        let next_sync = header.sync_version.wrapping_add(1);
        let ct = encrypt_with_key(key, &header.salt, next_sync, credentials_json)?;
        let mut out = Vec::with_capacity(HEADER_LEN + ct.ct.len());
        out.extend_from_slice(MAGIC);
        out.push(FORMAT_VERSION);
        out.extend_from_slice(&header.salt);
        out.extend_from_slice(&ct.nonce);
        out.extend_from_slice(&next_sync.to_le_bytes());
        out.extend_from_slice(&ct.ct);
        write_atomic(&self.path, &out)?;
        self.state.lock().unwrap().sync_version = next_sync;
        Ok(())
    }

    /// Migrate a legacy plaintext JSON file to encrypted format.
    /// Reads the old plaintext, encrypts with the new master password,
    /// writes the encrypted file, then deletes the old plaintext file.
    pub fn migrate(&self, master_password: &str) -> Result<()> {
        if !self.is_legacy_plaintext() {
            bail!("no legacy plaintext file to migrate");
        }
        let plaintext = std::fs::read_to_string(&self.path)
            .context("failed to read legacy plaintext file")?;
        // Validate it's actually JSON before encrypting.
        let _: serde_json::Value =
            serde_json::from_str(&plaintext).context("legacy file is not valid JSON")?;
        self.initialize(master_password, plaintext.as_bytes())?;
        Ok(())
    }

    /// Reset: delete the encrypted file entirely. The server list in
    /// config.json is preserved; only credentials are removed.
    pub fn reset(&self) -> Result<()> {
        if self.path.exists() {
            std::fs::remove_file(&self.path).context("failed to delete credential file")?;
        }
        self.state.lock().unwrap().sync_version = 0;
        Ok(())
    }

    /// Change the master password: decrypt with old, re-encrypt with new
    /// (new random salt). sync_version is preserved.
    pub fn change_password(&self, old_password: &str, new_password: &str) -> Result<()> {
        let data = std::fs::read(&self.path).context("credential file not found")?;
        let header = read_header(&data)?;
        let old_key = derive_key(old_password, &header.salt)?;
        let plaintext = decrypt_with_key(&old_key, &header, &data[HEADER_LEN..])?;
        // Re-encrypt with new password + new salt.
        let mut new_salt = [0u8; SALT_LEN];
        rng().fill_bytes(&mut new_salt);
        let new_key = derive_key(new_password, &new_salt)?;
        let ct = encrypt_with_key(&new_key, &new_salt, header.sync_version, &plaintext)?;
        let mut out = Vec::with_capacity(HEADER_LEN + ct.ct.len());
        out.extend_from_slice(MAGIC);
        out.push(FORMAT_VERSION);
        out.extend_from_slice(&new_salt);
        out.extend_from_slice(&ct.nonce);
        out.extend_from_slice(&header.sync_version.to_le_bytes());
        out.extend_from_slice(&ct.ct);
        write_atomic(&self.path, &out)?;
        Ok(())
    }

    /// Export the encrypted file to `dest` (raw copy).
    pub fn export_to(&self, dest: &Path) -> Result<()> {
        std::fs::copy(&self.path, dest).context("failed to export credential file")?;
        Ok(())
    }

    /// Import an encrypted file from `src`, overwriting the local file.
    /// Caller is responsible for re-unlocking with the appropriate master
    /// password afterwards (the imported file may have a different salt).
    pub fn import_from(&self, src: &Path) -> Result<()> {
        let data = std::fs::read(src).context("failed to read import file")?;
        // Validate it's a valid encrypted file.
        let _ = read_header(&data).context("import file is not a valid encrypted credential file")?;
        write_atomic(&self.path, &data)?;
        let header = read_header(&data)?;
        self.state.lock().unwrap().sync_version = header.sync_version;
        Ok(())
    }

    /// Return the current sync_version (for future cloud sync).
    pub fn sync_version(&self) -> u64 {
        self.state.lock().unwrap().sync_version
    }

    /// Get the credential file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Set the sync_version (used after import).
    pub fn set_sync_version(&self, v: u64) {
        self.state.lock().unwrap().sync_version = v;
    }
}

// === SECTION 1 END ===

/// Parsed header fields.
pub struct Header {
    pub salt: [u8; SALT_LEN],
    pub nonce: [u8; NONCE_LEN],
    pub sync_version: u64,
}

/// Public wrappers for use by encrypted_adapter.
pub fn read_header_pub(data: &[u8]) -> Result<Header> { read_header(data) }
pub fn derive_key_pub(password: &str, salt: &[u8]) -> Result<DerivedKey> { derive_key(password, salt) }
pub fn decrypt_with_key_pub(key: &DerivedKey, header: &Header, ciphertext: &[u8]) -> Result<Vec<u8>> {
    decrypt_with_key(key, header, ciphertext)
}
pub fn write_atomic_pub(path: &Path, data: &[u8]) -> Result<()> { write_atomic(path, data) }

/// Read and validate the header from file bytes.
fn read_header(data: &[u8]) -> Result<Header> {
    if data.len() < HEADER_LEN {
        bail!("file too short for encrypted credential header");
    }
    if &data[..4] != MAGIC {
        bail!("bad magic: not an encrypted credential file");
    }
    let version = data[4];
    if version != FORMAT_VERSION {
        bail!("unsupported credential file version: {}", version);
    }
    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(&data[5..5 + SALT_LEN]);
    let mut nonce = [0u8; NONCE_LEN];
    nonce.copy_from_slice(&data[5 + SALT_LEN..5 + SALT_LEN + NONCE_LEN]);
    let mut sv_bytes = [0u8; 8];
    sv_bytes.copy_from_slice(&data[5 + SALT_LEN + NONCE_LEN..HEADER_LEN]);
    let sync_version = u64::from_le_bytes(sv_bytes);
    Ok(Header {
        salt,
        nonce,
        sync_version,
    })
}

/// Derive a 32-byte key from password + salt using Argon2id.
fn derive_key(password: &str, salt: &[u8]) -> Result<DerivedKey> {
    let argon2 = Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        argon2::Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, Some(KEY_LEN))
            .map_err(|e| anyhow!("invalid argon2 params: {}", e))?,
    );
    let mut out = [0u8; KEY_LEN];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut out)
        .map_err(|e| anyhow!("argon2 key derivation failed: {}", e))?;
    Ok(DerivedKey::from_slice(&out))
}

/// Result of encrypting: nonce + ciphertext (with auth tag).
struct EncryptedPayload {
    nonce: [u8; NONCE_LEN],
    ct: Vec<u8>,
}

/// Encrypt plaintext with the given key. The header fields (salt, sync_version)
/// are included as AAD to bind ciphertext to a specific file header.
fn encrypt_with_key(
    key: &DerivedKey,
    salt: &[u8],
    sync_version: u64,
    plaintext: &[u8],
) -> Result<EncryptedPayload> {
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rng().fill_bytes(&mut nonce_bytes);
    // AAD = magic + version + salt + nonce + sync_version (the full header).
    let mut aad = Vec::with_capacity(HEADER_LEN);
    aad.extend_from_slice(MAGIC);
    aad.push(FORMAT_VERSION);
    aad.extend_from_slice(salt);
    aad.extend_from_slice(&nonce_bytes);
    aad.extend_from_slice(&sync_version.to_le_bytes());
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_bytes()));
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, Payload { msg: plaintext, aad: &aad })
        .map_err(|e| anyhow!("aes-gcm encrypt failed: {}", e))?;
    Ok(EncryptedPayload {
        nonce: nonce_bytes,
        ct,
    })
}

/// Decrypt ciphertext with the given key, using the header for AAD.
fn decrypt_with_key(key: &DerivedKey, header: &Header, ciphertext: &[u8]) -> Result<Vec<u8>> {
    let mut aad = Vec::with_capacity(HEADER_LEN);
    aad.extend_from_slice(MAGIC);
    aad.push(FORMAT_VERSION);
    aad.extend_from_slice(&header.salt);
    aad.extend_from_slice(&header.nonce);
    aad.extend_from_slice(&header.sync_version.to_le_bytes());
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_bytes()));
    let nonce = Nonce::from_slice(&header.nonce);
    cipher
        .decrypt(nonce, Payload { msg: ciphertext, aad: &aad })
        .map_err(|_| anyhow!("wrong master password or corrupted credential file"))
}

/// Write data to a file atomically (write to temp, then rename).
fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data).with_context(|| format!("write tmp {:?}", tmp))?;
    std::fs::rename(&tmp, path).with_context(|| format!("rename {:?} -> {:?}", tmp, path))?;
    Ok(())
}

// === SECTION 2 END ===

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn store_path(dir: &Path) -> PathBuf {
        dir.join("credentials.enc")
    }

    #[test]
    fn test_initialize_and_decrypt_round_trip() {
        let dir = tempdir().unwrap();
        let path = store_path(dir.path());
        let store = EncryptedCredentialStore::open(path.clone());
        assert!(store.is_absent());
        assert!(!store.is_initialized());

        let creds = br#"{"srv1::password":"secret123"}"#;
        store.initialize("masterpw", creds).unwrap();

        assert!(store.is_initialized());
        assert!(!store.is_legacy_plaintext());

        let key = store.unlock("masterpw").unwrap();
        let decrypted = store.decrypt(&key).unwrap();
        assert_eq!(&*decrypted, creds);
    }

    #[test]
    fn test_wrong_master_password_fails() {
        let dir = tempdir().unwrap();
        let store = EncryptedCredentialStore::open(store_path(dir.path()));
        store.initialize("correct", b"{}").unwrap();
        assert!(store.unlock("wrong").is_err());
    }

    #[test]
    fn test_encrypt_increments_sync_version() {
        let dir = tempdir().unwrap();
        let store = EncryptedCredentialStore::open(store_path(dir.path()));
        store.initialize("pw", b"v0").unwrap();
        assert_eq!(store.sync_version(), 0);

        let key = store.unlock("pw").unwrap();
        store.encrypt(&key, b"v1").unwrap();
        assert_eq!(store.sync_version(), 1);
        store.encrypt(&key, b"v2").unwrap();
        assert_eq!(store.sync_version(), 2);

        // Re-open to verify persistence.
        let store2 = EncryptedCredentialStore::open(store_path(dir.path()));
        assert_eq!(store2.sync_version(), 2);
        let decrypted = store2.decrypt(&key).unwrap();
        assert_eq!(&*decrypted, b"v2");
    }

    #[test]
    fn test_empty_credentials_json() {
        let dir = tempdir().unwrap();
        let store = EncryptedCredentialStore::open(store_path(dir.path()));
        store.initialize("pw", b"").unwrap();
        let key = store.unlock("pw").unwrap();
        assert_eq!(&*store.decrypt(&key).unwrap(), b"");
    }

    #[test]
    fn test_is_legacy_plaintext() {
        let dir = tempdir().unwrap();
        let path = store_path(dir.path());
        std::fs::write(&path, br#"{"srv::password":"plain"}"#).unwrap();
        let store = EncryptedCredentialStore::open(path);
        assert!(store.is_legacy_plaintext());
        assert!(!store.is_initialized());
    }

    #[test]
    fn test_migrate_from_plaintext() {
        let dir = tempdir().unwrap();
        let path = store_path(dir.path());
        let original = br#"{"srv1::password":"plain","srv2::password":"also"}"#;
        std::fs::write(&path, original).unwrap();

        let store = EncryptedCredentialStore::open(path.clone());
        assert!(store.is_legacy_plaintext());

        store.migrate("newmaster").unwrap();

        // File should now be encrypted (starts with magic).
        let data = std::fs::read(&path).unwrap();
        assert_eq!(&data[..4], MAGIC);
        assert!(!store.is_legacy_plaintext());
        assert!(store.is_initialized());

        // Content should match original.
        let key = store.unlock("newmaster").unwrap();
        let decrypted = store.decrypt(&key).unwrap();
        assert_eq!(&*decrypted, original);
    }

    #[test]
    fn test_migrate_non_json_fails() {
        let dir = tempdir().unwrap();
        let path = store_path(dir.path());
        std::fs::write(&path, b"not json at all").unwrap();
        let store = EncryptedCredentialStore::open(path);
        assert!(store.is_legacy_plaintext());
        assert!(store.migrate("pw").is_err());
    }

    #[test]
    fn test_reset_deletes_file() {
        let dir = tempdir().unwrap();
        let path = store_path(dir.path());
        let store = EncryptedCredentialStore::open(path.clone());
        store.initialize("pw", b"{}").unwrap();
        assert!(path.exists());
        store.reset().unwrap();
        assert!(!path.exists());
        assert!(!store.is_initialized());
        assert!(store.is_absent());
    }

    #[test]
    fn test_change_password() {
        let dir = tempdir().unwrap();
        let path = store_path(dir.path());
        let store = EncryptedCredentialStore::open(path);
        store.initialize("oldpw", b"secret data").unwrap();

        store.change_password("oldpw", "newpw").unwrap();

        // Old password should fail.
        assert!(store.unlock("oldpw").is_err());
        // New password should work and preserve content.
        let key = store.unlock("newpw").unwrap();
        assert_eq!(&*store.decrypt(&key).unwrap(), b"secret data");
    }

    #[test]
    fn test_change_password_wrong_old_fails() {
        let dir = tempdir().unwrap();
        let store = EncryptedCredentialStore::open(store_path(dir.path()));
        store.initialize("realpw", b"data").unwrap();
        assert!(store.change_password("wrong", "new").is_err());
        // Original should still work.
        let key = store.unlock("realpw").unwrap();
        assert_eq!(&*store.decrypt(&key).unwrap(), b"data");
    }

    #[test]
    fn test_export_import_round_trip() {
        let dir = tempdir().unwrap();
        let path = store_path(dir.path());
        let export_path = dir.path().join("export.enc");

        let store = EncryptedCredentialStore::open(path.clone());
        store.initialize("pw", b"export me").unwrap();
        store.export_to(&export_path).unwrap();
        assert!(export_path.exists());

        // Import into a new store at a different path.
        let dir2 = tempdir().unwrap();
        let path2 = store_path(dir2.path());
        let store2 = EncryptedCredentialStore::open(path2);
        store2.import_from(&export_path).unwrap();
        assert!(store2.is_initialized());
        let key = store2.unlock("pw").unwrap();
        assert_eq!(&*store2.decrypt(&key).unwrap(), b"export me");
    }

    #[test]
    fn test_import_invalid_file_fails() {
        let dir = tempdir().unwrap();
        let bad_path = dir.path().join("bad.enc");
        std::fs::write(&bad_path, b"not an encrypted file").unwrap();

        let store = EncryptedCredentialStore::open(store_path(dir.path()));
        assert!(store.import_from(&bad_path).is_err());
    }

    #[test]
    fn test_tampered_header_fails_decryption() {
        let dir = tempdir().unwrap();
        let path = store_path(dir.path());
        let store = EncryptedCredentialStore::open(path.clone());
        store.initialize("pw", b"secret").unwrap();

        // Tamper with the sync_version byte in the header.
        let mut data = std::fs::read(&path).unwrap();
        data[HEADER_LEN - 1] ^= 0xFF;
        std::fs::write(&path, &data).unwrap();

        // unlock() re-reads the file; the tampered AAD should make decrypt fail.
        // Re-derive key manually to test decrypt path:
        let store2 = EncryptedCredentialStore::open(path.clone());
        // Read salt from tampered file to derive key without verifying.
        let raw = std::fs::read(&path).unwrap();
        let mut salt = [0u8; SALT_LEN];
        salt.copy_from_slice(&raw[5..5 + SALT_LEN]);
        let derived = derive_key("pw", &salt).unwrap();
        assert!(store2.decrypt(&derived).is_err());
    }

    #[test]
    fn test_format_header_layout() {
        // Verify header field offsets are exactly as documented.
        assert_eq!(MAGIC.len(), 4);
        assert_eq!(HEADER_LEN, 41);
        assert_eq!(SALT_LEN, 16);
        assert_eq!(NONCE_LEN, 12);
    }
}

