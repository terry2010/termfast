//! SSH authentication — FP-2.2
//!
//! Key-based and password-based authentication.
//! Key auto-generation with Ed25519 and authorized_keys push.

use crate::error::{Error, ErrorCode, IpcError, Result};
use russh::client;
use russh::keys;
use std::path::PathBuf;

/// Authentication method
#[derive(Debug, Clone)]
pub enum AuthMethod {
    Key {
        key_path: String,
        passphrase: Option<String>,
    },
    Password {
        password: String,
    },
}

/// Authenticate with the SSH server. Returns true if authenticated, false if rejected.
pub async fn authenticate(
    handle: &mut client::Handle<super::client::SshHandler>,
    user: &str,
    auth: &AuthMethod,
) -> Result<bool> {
    match auth {
        AuthMethod::Password { password } => {
            let result = handle
                .authenticate_password(user, password)
                .await
                .map_err(|e| {
                    Error::Ipc(IpcError::new(
                        ErrorCode::AuthFailed,
                        format!("password auth error: {}", e),
                    ))
                })?;
            Ok(result.success())
        }
        AuthMethod::Key { key_path, passphrase } => {
            let key_pair = load_keypair(key_path, passphrase.as_deref())?;
            let key_with_alg = keys::PrivateKeyWithHashAlg::new(
                std::sync::Arc::new(key_pair),
                None,
            );
            let result = handle
                .authenticate_publickey(user, key_with_alg)
                .await
                .map_err(|e| {
                    Error::Ipc(IpcError::new(
                        ErrorCode::AuthFailed,
                        format!("key auth error: {}", e),
                    ))
                })?;
            Ok(result.success())
        }
    }
}

/// Load a keypair from a file
fn load_keypair(key_path: &str, passphrase: Option<&str>) -> Result<keys::PrivateKey> {
    let expanded = expand_tilde(key_path);
    let path = std::path::Path::new(&expanded);

    if !path.exists() {
        return Err(Error::Ipc(IpcError::new(
            ErrorCode::CredentialNotFound,
            format!("key file not found: {}", key_path),
        )));
    }

    let key_pair = keys::load_secret_key(path, passphrase).map_err(|e| {
        Error::Ipc(IpcError::new(
            ErrorCode::AuthFailed,
            format!("failed to load key from {}: {}", key_path, e),
        ))
    })?;

    Ok(key_pair)
}

/// Expand ~ to home directory
fn expand_tilde(path: &str) -> String {
    if path.starts_with("~/") {
        if let Some(home) = directories::BaseDirs::new() {
            return format!("{}{}", home.home_dir().display(), &path[1..]);
        }
    }
    path.to_string()
}

/// Generate an Ed25519 keypair for a server (§8.2-8.5)
/// Returns (private_key_path, public_key_string, passphrase)
pub fn generate_keypair(server_id: &str) -> Result<(PathBuf, String, String)> {
    let home = directories::BaseDirs::new().ok_or_else(|| {
        Error::Config("cannot determine home directory".into())
    })?;

    let ssh_dir = home.home_dir().join(".ssh");
    std::fs::create_dir_all(&ssh_dir).map_err(Error::Io)?;

    let key_path = ssh_dir.join(format!("vps_guard_{}_key", server_id));
    let pub_key_path = ssh_dir.join(format!("vps_guard_{}_key.pub", server_id));

    // Generate Ed25519 key using russh's re-exported ssh_key
    use russh::keys::ssh_key;
    let mut rng = ssh_key::rand_core::UnwrapErr(ssh_key::getrandom::SysRng);
    let key_pair = ssh_key::PrivateKey::random(&mut rng, ssh_key::Algorithm::Ed25519 {})
        .map_err(|e| Error::Crypto(format!("key generation failed: {}", e)))?;

    // Generate random passphrase (32 bytes base64)
    let mut passphrase_bytes = [0u8; 32];
    ssh_key::getrandom::fill(&mut passphrase_bytes)
        .map_err(|e| Error::Crypto(format!("rng error: {}", e)))?;
    let passphrase = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        passphrase_bytes,
    );

    // Write private key encrypted with passphrase
    let encrypted_key = key_pair
        .encrypt(&mut rng, &passphrase)
        .map_err(|e| Error::Crypto(format!("key encryption failed: {}", e)))?;
    let private_key_str = encrypted_key
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)
        .map_err(|e| Error::Crypto(format!("key encode failed: {}", e)))?;
    std::fs::write(&key_path, private_key_str.as_bytes()).map_err(Error::Io)?;

    // Write public key
    let public_key = key_pair.public_key();
    let pub_key_str = format!(
        "{} vps-guard@{}",
        public_key
            .to_openssh()
            .map_err(|e| Error::Crypto(format!("pubkey encode failed: {}", e)))?,
        server_id
    );
    std::fs::write(&pub_key_path, &pub_key_str).map_err(Error::Io)?;

    // Set permissions: private 600, public 644
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))
            .map_err(Error::Io)?;
        std::fs::set_permissions(&pub_key_path, std::fs::Permissions::from_mode(0o644))
            .map_err(Error::Io)?;
    }

    Ok((key_path, pub_key_str, passphrase))
}

/// Push a public key to the remote server's authorized_keys via SSH exec
pub async fn push_public_key(
    handle: &client::Handle<super::client::SshHandler>,
    public_key: &str,
) -> Result<()> {
    let escaped_key = public_key.replace('\'', "'\\''");
    let command = format!(
        "mkdir -p ~/.ssh && echo '{}' >> ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys",
        escaped_key
    );

    let result = super::exec::exec(handle, &command, 30).await?;
    if result.exit_code != 0 {
        return Err(Error::Ipc(IpcError::new(
            ErrorCode::Internal,
            format!("failed to push public key: exit code {}", result.exit_code),
        )));
    }
    Ok(())
}

/// Check if a key file exists
pub fn check_key_exists(key_path: &str) -> bool {
    let expanded = expand_tilde(key_path);
    std::path::Path::new(&expanded).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_method_password() {
        let auth = AuthMethod::Password {
            password: "test".into(),
        };
        assert!(matches!(auth, AuthMethod::Password { .. }));
    }

    #[test]
    fn test_auth_method_key() {
        let auth = AuthMethod::Key {
            key_path: "/path/to/key".into(),
            passphrase: Some("pass".into()),
        };
        assert!(matches!(auth, AuthMethod::Key { .. }));
    }

    #[test]
    fn test_expand_tilde() {
        let result = expand_tilde("~/test");
        assert!(!result.starts_with("~"));
    }

    #[test]
    fn test_expand_tilde_no_tilde() {
        let result = expand_tilde("/absolute/path");
        assert_eq!(result, "/absolute/path");
    }

    #[test]
    fn test_check_key_exists_nonexistent() {
        assert!(!check_key_exists("/nonexistent/key/path/12345"));
    }
}
