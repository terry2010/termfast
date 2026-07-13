//! Credential store — FP-1.5
//!
//! Trait abstraction for credential storage (§8.7).
//! Desktop implementation uses OS keychain via `keyring` crate.
//! Falls back to in-memory storage when keychain is unavailable (§17.3).

pub mod keychain;
pub mod memory;

pub use keychain::KeychainCredentialStore;
pub use memory::InMemoryCredentialStore;

/// Credential type prefix in key naming
pub const SERVICE_NAME: &str = "vps-guard";

/// Credential store trait (§8.7)
pub trait CredentialStore: Send + Sync {
    fn save(&self, key: &str, value: &str) -> anyhow::Result<()>;
    fn load(&self, key: &str) -> anyhow::Result<String>;
    fn delete(&self, key: &str) -> anyhow::Result<()>;
    fn delete_all_for_server(&self, server_id: &str) -> anyhow::Result<()>;
    fn has(&self, key: &str) -> bool {
        self.load(key).is_ok()
    }
}

/// Build credential key: `vps-guard::<server_id>::<type>` (§8.7)
pub fn make_key(server_id: &str, credential_type: &str) -> String {
    format!("{}::{}::{}", SERVICE_NAME, server_id, credential_type)
}

/// Credential types
pub mod cred_type {
    pub const PASSWORD: &str = "password";
    pub const KEY_PASSPHRASE: &str = "key_passphrase";
}
