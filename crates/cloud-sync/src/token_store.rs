//! Token store — persists OAuth tokens as plaintext JSON with 0600 permissions.
//!
//! Tokens are stored as a plain JSON file (no encryption). This is safe because:
//! - The synced config data is already encrypted with the user's master password
//! - Even if a token leaks, the attacker can only access ciphertext in the cloud
//! - The file has 0600 permissions on Unix (owner-only read/write)
//!
//! Previously tokens were encrypted with a separate passphrase, but that added
//! UX friction (extra password to remember) without meaningful security gain
//! since the cloud data is already encrypted.

use crate::{CloudProvider, OAuthToken};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// On-disk format: a map of provider → token.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenStoreData {
    pub tokens: HashMap<String, StoredToken>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredToken {
    pub provider: CloudProvider,
    pub token: OAuthToken,
    /// When this token was stored (unix timestamp seconds)
    pub stored_at: i64,
}

/// Save tokens to disk as plaintext JSON (0600 permissions on Unix).
pub fn save_tokens(
    path: &std::path::Path,
    data: &TokenStoreData,
) -> Result<(), crate::CloudSyncError> {
    let json = serde_json::to_vec_pretty(data)
        .map_err(|e| crate::CloudSyncError::Api(format!("serialize error: {}", e)))?;

    // Write atomically: write to temp file, then rename
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &json)
        .map_err(|e| crate::CloudSyncError::Io(e.to_string()))?;

    // Set permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| crate::CloudSyncError::Io(e.to_string()))?;
    }

    std::fs::rename(&tmp, path)
        .map_err(|e| crate::CloudSyncError::Io(e.to_string()))?;

    Ok(())
}

/// Load tokens from disk.
pub fn load_tokens(
    path: &std::path::Path,
) -> Result<TokenStoreData, crate::CloudSyncError> {
    let json = std::fs::read(path)
        .map_err(|e| crate::CloudSyncError::Io(e.to_string()))?;

    serde_json::from_slice(&json)
        .map_err(|e| crate::CloudSyncError::Api(format!("deserialize error: {}", e)))
}

/// Check if a token file exists.
pub fn token_file_exists(path: &std::path::Path) -> bool {
    path.exists()
}

// === SECTION token_store END ===
