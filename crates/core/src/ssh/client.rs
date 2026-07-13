//! SSH client core — FP-2.1
//!
//! Connection management with heartbeat and reconnection.
//! Uses russh client for SSH protocol.

use crate::error::{Error, ErrorCode, IpcError, Result};
use russh::client;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// Connection state (§7.2, §17.2)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Connected,
    Reconnecting { attempt: u32, max: u32 },
    AuthFailed,
    Disconnected,
}

/// SSH client configuration
#[derive(Clone)]
pub struct SshClientConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub heartbeat_interval: u64,
    pub max_attempts: u32,
    pub initial_backoff_secs: u64,
    pub max_backoff_secs: u64,
    pub skip_hostkey_verify: bool,
    /// Callback invoked on hostkey mismatch (§17.2)
    pub hostkey_mismatch_callback: Option<Arc<dyn Fn(String, String) + Send + Sync>>,
}

impl Default for SshClientConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 22,
            user: "root".into(),
            heartbeat_interval: 15,
            max_attempts: 10,
            initial_backoff_secs: 1,
            max_backoff_secs: 30,
            skip_hostkey_verify: false,
            hostkey_mismatch_callback: None,
        }
    }
}

/// Handler for russh client callbacks
pub struct SshHandler {
    pub known_host_key: Option<String>,
    pub skip_hostkey_verify: bool,
    pub host_key_recorded: Arc<tokio::sync::Mutex<Option<String>>>,
    /// Callback invoked on hostkey mismatch (§17.2: triple notification)
    /// Parameters: (expected_key, actual_key)
    pub hostkey_mismatch_callback: Option<Arc<dyn Fn(String, String) + Send + Sync>>,
}

impl client::Handler for SshHandler {
    type Error = russh::Error;

    fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKey,
    ) -> impl std::future::Future<Output = std::result::Result<bool, Self::Error>> + Send {
        let fp = fingerprint(server_public_key);
        let skip = self.skip_hostkey_verify;
        let known = self.known_host_key.clone();
        let recorded = self.host_key_recorded.clone();
        let mismatch_cb = self.hostkey_mismatch_callback.clone();
        async move {
            if skip {
                return Ok(true);
            }
            if let Some(k) = &known {
                let matches = fp == *k;
                if !matches {
                    tracing::warn!("hostkey mismatch: expected {}, got {}", k, fp);
                    // Triple notification (§17.2): system notification + tray red + log highlight
                    if let Some(ref cb) = mismatch_cb {
                        cb(k.clone(), fp.clone());
                    }
                }
                return Ok(matches);
            }
            // First connection — record the key
            tracing::info!("first connection, recording hostkey: {}", fp);
            *recorded.lock().await = Some(fp);
            Ok(true)
        }
    }
}

/// Compute SHA256 fingerprint of a public key
pub fn fingerprint(key: &russh::keys::PublicKey) -> String {
    let fp = key.fingerprint(russh::keys::HashAlg::Sha256);
    format!("SHA256:{}", fp)
}

/// SSH client handle — wraps russh client handle with reconnection logic.
pub struct SshClientHandle {
    config: Mutex<SshClientConfig>,
    handle: Mutex<Option<Arc<client::Handle<SshHandler>>>>,
    state: Mutex<ConnectionState>,
}

impl SshClientHandle {
    /// Create a new SSH client handle (not yet connected)
    pub fn new(config: SshClientConfig) -> Self {
        Self {
            config: Mutex::new(config),
            handle: Mutex::new(None),
            state: Mutex::new(ConnectionState::Disconnected),
        }
    }

    /// Set the hostkey mismatch callback (§17.2)
    pub async fn set_hostkey_mismatch_callback(&self, cb: Arc<dyn Fn(String, String) + Send + Sync>) {
        self.config.lock().await.hostkey_mismatch_callback = Some(cb);
    }

    /// Connect to the SSH server with the given auth method
    pub async fn connect(&self, auth: &super::auth::AuthMethod) -> Result<()> {
        self.set_state(ConnectionState::Connecting).await;

        let config = self.config.lock().await;
        let russh_config = Arc::new(client::Config {
            keepalive_interval: Some(Duration::from_secs(config.heartbeat_interval)),
            keepalive_max: 3,
            ..Default::default()
        });

        let handler = SshHandler {
            known_host_key: None,
            skip_hostkey_verify: config.skip_hostkey_verify,
            host_key_recorded: Arc::new(tokio::sync::Mutex::new(None)),
            hostkey_mismatch_callback: config.hostkey_mismatch_callback.clone(),
        };

        let addr = format!("{}:{}", config.host, config.port);
        let user = config.user.clone();
        drop(config);

        let mut handle = client::connect(russh_config, &addr, handler)
            .await
            .map_err(|e| {
                self.set_state_sync(ConnectionState::Disconnected);
                Error::Ipc(IpcError::new(
                    ErrorCode::SshConnectFailed,
                    format!("SSH connect to {} failed: {}", addr, e),
                ))
            })?;

        // Authenticate
        let auth_ok = super::auth::authenticate(&mut handle, &user, auth)
            .await
            .inspect_err(|_e| {
                self.set_state_sync(ConnectionState::AuthFailed);
            })?;

        if !auth_ok {
            self.set_state(ConnectionState::AuthFailed).await;
            return Err(Error::Ipc(IpcError::new(
                ErrorCode::AuthFailed,
                "authentication rejected by server",
            )));
        }

        *self.handle.lock().await = Some(Arc::new(handle));
        self.set_state(ConnectionState::Connected).await;
        Ok(())
    }

    /// Connect with reconnection logic (exponential backoff)
    pub async fn connect_with_reconnect(
        &self,
        auth: &super::auth::AuthMethod,
    ) -> Result<()> {
        let config = self.config.lock().await;
        let max_attempts = config.max_attempts;
        let initial_backoff = config.initial_backoff_secs;
        let max_backoff = config.max_backoff_secs;
        drop(config);

        let mut attempt = 0u32;
        let mut backoff = initial_backoff;

        loop {
            attempt += 1;
            if attempt > 1 {
                self.set_state(ConnectionState::Reconnecting {
                    attempt: attempt - 1,
                    max: max_attempts,
                })
                .await;
                tracing::info!(
                    "reconnect attempt {}/{} after {}s",
                    attempt - 1,
                    max_attempts,
                    backoff
                );
                tokio::time::sleep(Duration::from_secs(backoff)).await;
            }

            match self.connect(auth).await {
                Ok(()) => return Ok(()),
                Err(Error::Ipc(ipc)) if ipc.code == ErrorCode::AuthFailed => {
                    return Err(Error::Ipc(ipc));
                }
                Err(e) => {
                    if attempt >= max_attempts {
                        self.set_state(ConnectionState::Disconnected).await;
                        return Err(e);
                    }
                    backoff = (backoff * 2).min(max_backoff);
                }
            }
        }
    }

    /// Disconnect from the SSH server
    pub async fn disconnect(&self) -> Result<()> {
        let mut handle_guard = self.handle.lock().await;
        if let Some(handle) = handle_guard.take() {
            let _ = handle
                .disconnect(russh::Disconnect::ByApplication, "", "en")
                .await;
        }
        self.set_state(ConnectionState::Disconnected).await;
        Ok(())
    }

    /// Get a reference to the handle (for sharing with channel opener)
    pub async fn get_handle(&self) -> Option<Arc<client::Handle<SshHandler>>> {
        self.handle.lock().await.clone()
    }

    /// Get the current connection state
    pub async fn state(&self) -> ConnectionState {
        self.state.lock().await.clone()
    }

    /// Check if connected
    pub async fn is_connected(&self) -> bool {
        let guard = self.handle.lock().await;
        guard.as_ref().map(|h| !h.is_closed()).unwrap_or(false)
    }

    /// Execute a command on the remote server
    pub async fn exec(&self, command: &str, timeout_secs: u64) -> Result<super::exec::ExecResult> {
        let guard = self.handle.lock().await;
        let handle = guard.as_ref().ok_or_else(|| {
            Error::Ipc(IpcError::new(
                ErrorCode::SshDisconnected,
                "SSH connection is not established",
            ))
        })?;
        super::exec::exec(handle, command, timeout_secs).await
    }

    /// Open a direct-tcpip channel (for proxy)
    pub async fn open_direct_tcpip(
        &self,
        host: &str,
        port: u32,
    ) -> Result<russh::Channel<russh::client::Msg>> {
        let guard = self.handle.lock().await;
        let handle = guard.as_ref().ok_or_else(|| {
            Error::Ipc(IpcError::new(
                ErrorCode::SshDisconnected,
                "SSH connection is not established",
            ))
        })?;
        handle
            .channel_open_direct_tcpip(host, port, "127.0.0.1", 0)
            .await
            .map_err(|e| {
                Error::Ssh(format!("failed to open direct-tcpip channel: {}", e))
            })
    }

    /// Set the connection state (async)
    async fn set_state(&self, state: ConnectionState) {
        *self.state.lock().await = state;
    }

    /// Set the connection state (sync, for use in error paths)
    fn set_state_sync(&self, state: ConnectionState) {
        // Use try_lock to avoid deadlock in sync context
        if let Ok(mut guard) = self.state.try_lock() {
            *guard = state;
        }
    }

    /// Get the client config (cloned, since it's behind a Mutex)
    pub async fn config(&self) -> SshClientConfig {
        self.config.lock().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_state_equality() {
        assert_eq!(ConnectionState::Connected, ConnectionState::Connected);
        assert_ne!(ConnectionState::Connecting, ConnectionState::Connected);
    }

    #[test]
    fn test_ssh_client_config_default() {
        let config = SshClientConfig::default();
        assert_eq!(config.port, 22);
        assert_eq!(config.heartbeat_interval, 15);
        assert_eq!(config.max_attempts, 10);
        assert_eq!(config.initial_backoff_secs, 1);
        assert_eq!(config.max_backoff_secs, 30);
    }

    #[tokio::test]
    async fn test_new_client_starts_disconnected() {
        let client = SshClientHandle::new(SshClientConfig::default());
        assert_eq!(client.state().await, ConnectionState::Disconnected);
        assert!(!client.is_connected().await);
    }

    #[test]
    fn test_exponential_backoff_sequence() {
        let mut backoff = 1u64;
        let max = 30u64;
        let sequence: Vec<u64> = (0..10).map(|_| {
            let current = backoff;
            backoff = (backoff * 2).min(max);
            current
        }).collect();

        assert_eq!(sequence, vec![1, 2, 4, 8, 16, 30, 30, 30, 30, 30]);
    }

    #[tokio::test]
    async fn test_disconnect_when_not_connected() {
        let client = SshClientHandle::new(SshClientConfig::default());
        client.disconnect().await.unwrap();
        assert_eq!(client.state().await, ConnectionState::Disconnected);
    }
}
