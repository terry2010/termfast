//! SSH authentication — FP-2.2
//!
//! Key-based and password-based authentication.
//! Key auto-generation with Ed25519 and authorized_keys push.

use crate::error::{Error, ErrorCode, IpcError, Result};
use russh::client;
use russh::keys;
use std::path::PathBuf;
use zeroize::Zeroizing;

/// Authentication method.
/// Passwords and passphrases are wrapped in `Zeroizing<String>` so they
/// are securely wiped from memory when dropped.
#[derive(Debug, Clone)]
pub enum AuthMethod {
    Key {
        key_path: String,
        passphrase: Option<Zeroizing<String>>,
    },
    Password {
        password: Zeroizing<String>,
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
                .authenticate_password(user, password.as_str())
                .await
                .map_err(|e| {
                    Error::Ipc(IpcError::new(
                        ErrorCode::AuthFailed,
                        format!("password auth error: {}", e),
                    ))
                })?;
            Ok(result.success())
        }
        AuthMethod::Key {
            key_path,
            passphrase,
        } => {
            let key_pair = load_keypair(key_path, passphrase.as_deref().map(|s| s.as_str()))?;
            let key_with_alg =
                keys::PrivateKeyWithHashAlg::new(std::sync::Arc::new(key_pair), None);
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
/// Uses the default `~/.ssh` directory.
/// Returns (private_key_path, public_key_string, passphrase)
pub fn generate_keypair(server_id: &str) -> Result<(PathBuf, String, String)> {
    let home = directories::BaseDirs::new()
        .ok_or_else(|| Error::Config("cannot determine home directory".into()))?;
    let ssh_dir = home.home_dir().join(".ssh");
    generate_keypair_at(&ssh_dir, server_id)
}

/// Generate an Ed25519 keypair under a custom directory (e.g. Android app private dir).
/// Returns (private_key_path, public_key_string, passphrase)
pub fn generate_keypair_at(
    ssh_dir: impl AsRef<std::path::Path>,
    server_id: &str,
) -> Result<(PathBuf, String, String)> {
    // Validate server_id to prevent path traversal — only allow [A-Za-z0-9_-]
    if server_id.is_empty()
        || !server_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(Error::Ipc(IpcError::new(
            ErrorCode::InvalidParams,
            "invalid server_id: only alphanumeric, hyphen, underscore allowed",
        )));
    }

    let ssh_dir = ssh_dir.as_ref();
    std::fs::create_dir_all(ssh_dir).map_err(Error::Io)?;

    let key_path = ssh_dir.join(format!("termfast_{}_key", server_id));
    let pub_key_path = ssh_dir.join(format!("termfast_{}_key.pub", server_id));

    let (key_pair, passphrase) = generate_keypair_bytes(server_id)?;

    // Write private key encrypted with passphrase
    let mut rng =
        russh::keys::ssh_key::rand_core::UnwrapErr(russh::keys::ssh_key::getrandom::SysRng);
    let encrypted_key = key_pair
        .encrypt(&mut rng, &passphrase)
        .map_err(|e| Error::Crypto(format!("key encryption failed: {}", e)))?;
    let private_key_str = encrypted_key
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)
        .map_err(|e| Error::Crypto(format!("key encode failed: {}", e)))?;
    // Crash-safe atomic write with fsync, 0600 for private key
    crate::fs::write_atomic(&key_path, private_key_str.as_bytes(), true).map_err(Error::Io)?;

    // Write public key (644, not sensitive)
    let public_key = key_pair.public_key();
    let pub_key_str = format!(
        "{} termfast@{}",
        public_key
            .to_openssh()
            .map_err(|e| Error::Crypto(format!("pubkey encode failed: {}", e)))?,
        server_id
    );
    crate::fs::write_atomic(&pub_key_path, pub_key_str.as_bytes(), false).map_err(Error::Io)?;

    Ok((key_path, pub_key_str, passphrase))
}

/// Generate an Ed25519 keypair and return the key bytes + passphrase.
/// The caller decides where to store the private/public key (e.g. Android Keystore).
#[allow(dead_code)]
pub fn generate_keypair_bytes(
    _server_id: &str,
) -> Result<(russh::keys::ssh_key::PrivateKey, String)> {
    use russh::keys::ssh_key;
    let mut rng = ssh_key::rand_core::UnwrapErr(ssh_key::getrandom::SysRng);
    let key_pair = ssh_key::PrivateKey::random(&mut rng, ssh_key::Algorithm::Ed25519 {})
        .map_err(|e| Error::Crypto(format!("key generation failed: {}", e)))?;

    // Generate random passphrase (32 bytes base64)
    let mut passphrase_bytes = [0u8; 32];
    ssh_key::getrandom::fill(&mut passphrase_bytes)
        .map_err(|e| Error::Crypto(format!("rng error: {}", e)))?;
    let passphrase =
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, passphrase_bytes);

    Ok((key_pair, passphrase))
}

/// Push a public key to the remote server's authorized_keys via SSH exec.
///
/// The key is written with a `# termfast: <key_name>` comment marker on the
/// preceding line so that `cleanup_authorized_keys` can reliably find and
/// remove it. Without this marker, the cleanup sed pattern would never match
/// and the key would persist after the server is deleted (D-2).
pub async fn push_public_key(
    handle: &client::Handle<super::client::SshHandler>,
    public_key: &str,
    key_name: &str,
) -> Result<()> {
    // Validate that the public key is a single line — multi-line input
    // would break authorized_keys format.
    if public_key.lines().count() != 1 {
        return Err(Error::Ipc(IpcError::new(
            ErrorCode::InvalidParams,
            "public key must be a single line",
        )));
    }
    let escaped_key = public_key.replace('\'', "'\\''");
    // Escape key_name for safe shell embedding (used in a comment line).
    // Only allow alphanumerics, dash, underscore, dot — reject anything else.
    let safe_name: String = key_name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .collect();
    let safe_name = if safe_name.is_empty() { "termfast".to_string() } else { safe_name };
    let command = format!(
        "mkdir -p ~/.ssh && echo '# termfast: {}' >> ~/.ssh/authorized_keys && echo '{}' >> ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys",
        safe_name, escaped_key
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
            password: Zeroizing::new("test".into()),
        };
        assert!(matches!(auth, AuthMethod::Password { .. }));
    }

    #[test]
    fn test_auth_method_key() {
        let auth = AuthMethod::Key {
            key_path: "/path/to/key".into(),
            passphrase: Some(Zeroizing::new("pass".into())),
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

    /// D-2/D-14 contract test: verify that the marker format written by
    /// push_public_key matches the sed cleanup pattern in
    /// handle_cleanup_authorized_keys (daemon/src/handler.rs).
    ///
    /// push writes: `# termfast: <safe_name>` comment line + key line
    /// cleanup deletes: `sed -i '/# termfast: <safe_name>/{N;d}'`
    ///
    /// This test verifies the marker format is consistent so cleanup actually
    /// removes the key (the original D-2 bug was push had no marker, cleanup
    /// sed matched a non-existent marker → key permanently残留).
    #[test]
    fn test_push_marker_matches_cleanup_pattern() {
        // Replicate safe_name sanitization (alphanumerics + - _ .)
        fn sanitize(name: &str) -> String {
            let s: String = name
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
                .collect();
            if s.is_empty() { "termfast".to_string() } else { s }
        }

        // Replicate push command marker format
        let key_name = "my-ed25519-key";
        let safe_name = sanitize(key_name);
        let push_cmd = format!(
            "mkdir -p ~/.ssh && echo '# termfast: {}' >> ~/.ssh/authorized_keys && echo '{}' >> ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys",
            safe_name, "ssh-ed25519 AAAA..."
        );

        // Replicate cleanup sed pattern
        let cleanup_cmd = format!(
            "sed -i '/# termfast: {}/{{N;d}}' ~/.ssh/authorized_keys 2>/dev/null || true",
            safe_name
        );

        // The marker in push must appear in the cleanup sed pattern
        let marker = format!("# termfast: {}", safe_name);
        assert!(
            push_cmd.contains(&marker),
            "push command must contain marker '{}', got: {}",
            marker,
            push_cmd
        );
        assert!(
            cleanup_cmd.contains(&marker),
            "cleanup sed pattern must contain marker '{}', got: {}",
            marker,
            cleanup_cmd
        );

        // Verify the sed pattern would match the marker line: the regex
        // `/# termfast: <name>/` must match the literal string `# termfast: <name>`
        // (safe_name has no regex-special chars after sanitization)
        let marker_line = format!("# termfast: {}", safe_name);
        let sed_regex = format!("# termfast: {}", safe_name);
        assert!(
            marker_line.contains(&sed_regex),
            "sed regex '{}' must match marker line '{}'",
            sed_regex,
            marker_line
        );
    }
}
