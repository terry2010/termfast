//! Android credential store placeholder.
//!
//! The full implementation will call into Android Keystore via JNI.
//! For now we use an in-memory store so the crate compiles on desktop.

use termfast_credential::{CredentialStore, InMemoryCredentialStore};

pub fn android_credential_store() -> Box<dyn CredentialStore> {
    Box::new(InMemoryCredentialStore::new())
}
