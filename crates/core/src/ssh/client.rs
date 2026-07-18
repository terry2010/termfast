//! SSH client core — FP-2.1
//!
//! Connection management with heartbeat and reconnection.
//! Uses russh client for SSH protocol.

use super::protector::SocketProtector;
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
    /// Known host key fingerprint (SHA256:xxx) from a previous connection.
    /// None on first connection (TOFU: trust on first use).
    pub known_host_key: Option<String>,
    /// Callback invoked on hostkey mismatch (§17.2)
    pub hostkey_mismatch_callback: Option<Arc<dyn Fn(String, String) + Send + Sync>>,
    /// Optional hook to protect the socket before `connect()` (Android VpnService).
    pub socket_protector: Option<Arc<dyn SocketProtector>>,
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
            known_host_key: None,
            hostkey_mismatch_callback: None,
            socket_protector: None,
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
    /// Auth banner received from the server during authentication (RFC4252 §5.4).
    /// Captured here so the caller can retrieve it after `connect()` returns.
    pub auth_banner: Arc<tokio::sync::Mutex<Option<String>>>,
}

impl client::Handler for SshHandler {
    type Error = russh::Error;

    fn auth_banner(
        &mut self,
        banner: &str,
        _session: &mut client::Session,
    ) -> impl std::future::Future<Output = std::result::Result<(), Self::Error>> + Send {
        let banner_store = self.auth_banner.clone();
        let banner_text = banner.to_string();
        async move {
            tracing::info!("received SSH auth banner ({} bytes)", banner_text.len());
            *banner_store.lock().await = Some(banner_text);
            Ok(())
        }
    }

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
                    // Store the actual key so the caller can retrieve it for
                    // the error message / accept-new-key flow.
                    *recorded.lock().await = Some(fp.clone());
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
    /// Auth banner captured during the most recent authentication (RFC4252 §5.4).
    auth_banner: Arc<tokio::sync::Mutex<Option<String>>>,
    /// Host key fingerprint recorded during the most recent connection attempt.
    /// On first connection: the server's key (to be persisted).
    /// On mismatch: the actual (new) key from the server.
    /// On matching reconnect: None (no change needed).
    recorded_host_key: Arc<tokio::sync::Mutex<Option<String>>>,
}

impl SshClientHandle {
    /// Create a new SSH client handle (not yet connected)
    pub fn new(config: SshClientConfig) -> Self {
        Self {
            config: Mutex::new(config),
            handle: Mutex::new(None),
            state: Mutex::new(ConnectionState::Disconnected),
            auth_banner: Arc::new(tokio::sync::Mutex::new(None)),
            recorded_host_key: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// Set the hostkey mismatch callback (§17.2)
    pub async fn set_hostkey_mismatch_callback(
        &self,
        cb: Arc<dyn Fn(String, String) + Send + Sync>,
    ) {
        self.config.lock().await.hostkey_mismatch_callback = Some(cb);
    }

    /// Set the socket protector (Android VpnService.protect hook)
    pub async fn set_socket_protector(&self, protector: Option<Arc<dyn SocketProtector>>) {
        self.config.lock().await.socket_protector = protector;
    }

    /// Connect to the SSH server with the given auth method
    pub async fn connect(&self, auth: &super::auth::AuthMethod) -> Result<()> {
        self.set_state(ConnectionState::Connecting).await;
        // Clear any banner from a previous connection attempt
        *self.auth_banner.lock().await = None;
        // Clear recorded host key from previous attempt
        *self.recorded_host_key.lock().await = None;

        let config = self.config.lock().await;
        let russh_config = Arc::new(client::Config {
            keepalive_interval: Some(Duration::from_secs(10)),
            keepalive_max: 3,
            ..Default::default()
        });

        let handler = SshHandler {
            known_host_key: config.known_host_key.clone(),
            skip_hostkey_verify: config.skip_hostkey_verify,
            host_key_recorded: Arc::new(tokio::sync::Mutex::new(None)),
            hostkey_mismatch_callback: config.hostkey_mismatch_callback.clone(),
            auth_banner: self.auth_banner.clone(),
        };
        // Clone the recorded-key Arc so we can read it after connect_stream
        // consumes the handler.
        let recorded_key = handler.host_key_recorded.clone();
        // Clone known_host_key for error handling after drop(config)
        let known_host_key = config.known_host_key.clone();

        let addr = format!("{}:{}", config.host, config.port);
        let user = config.user.clone();
        let protector = config.socket_protector.clone();
        drop(config);

        // Open ONE TCP connection and peek at the first bytes to capture any
        // pre-banner lines (e.g. "Not allowed at this time") that the server
        // sends before the SSH protocol identification string (RFC4253 §4.2).
        // The pre-read bytes + remaining stream are then handed to russh's
        // connect_stream, so we only make a single TCP connection.
        //
        // The old approach called client::connect() and then made a SECOND
        // TCP connection via read_server_banner() to retrieve the pre-banner.
        // That double-connection doubled the server's failed connection count,
        // which could trigger fail2ban / MaxStartups rate limiting and make
        // the rejection persistent.
        let (pre_banner, stream) = match tokio::time::timeout(
            Duration::from_secs(10),
            connect_and_peek(&addr, protector.as_deref()),
        )
        .await
        {
            Ok(Ok(result)) => result,
            Ok(Err(e)) => {
                self.set_state_sync(ConnectionState::Disconnected);
                return Err(Error::Ipc(IpcError::new(
                    ErrorCode::SshConnectFailed,
                    format!("SSH connect to {} failed: {}", addr, e),
                )));
            }
            Err(_) => {
                self.set_state_sync(ConnectionState::Disconnected);
                return Err(Error::Ipc(IpcError::new(
                    ErrorCode::SshConnectFailed,
                    format!("SSH connect to {} timed out", addr),
                )));
            }
        };

        let mut handle = match client::connect_stream(russh_config, stream, handler).await {
            Ok(h) => h,
            Err(e) => {
                self.set_state_sync(ConnectionState::Disconnected);
                // Check if this was a host key mismatch (russh returns UnknownKey
                // when check_server_key returns Ok(false)).
                let is_hostkey_mismatch = matches!(&e, russh::Error::UnknownKey);
                if is_hostkey_mismatch {
                    let known = known_host_key.clone();
                    let actual = recorded_key.lock().await.clone();
                    let detail = match (&known, &actual) {
                        (Some(k), Some(a)) => format!("expected: {}, got: {}", k, a),
                        (Some(k), None) => format!("expected: {}, got: <unknown>", k),
                        _ => "host key verification failed".to_string(),
                    };
                    return Err(Error::Ipc(IpcError::new(
                        ErrorCode::HostKeyMismatch,
                        detail,
                    )));
                }
                let base_msg = format!("SSH connect to {} failed: {}", addr, e);
                let enhanced = if pre_banner.is_empty() {
                    base_msg
                } else {
                    format!("{} (server message: {})", base_msg, pre_banner)
                };
                return Err(Error::Ipc(IpcError::new(
                    ErrorCode::SshConnectFailed,
                    enhanced,
                )));
            }
        };

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
        // Save the recorded host key (if any) so the caller can persist it.
        if let Some(k) = recorded_key.lock().await.clone() {
            *self.recorded_host_key.lock().await = Some(k);
        }
        self.set_state(ConnectionState::Connected).await;
        Ok(())
    }

    /// Get the host key fingerprint recorded during the last connection attempt.
    /// Returns Some(fp) if this was a first connection (new key to persist) or
    /// a mismatch (actual key from server). Returns None if the key matched.
    pub async fn get_recorded_host_key(&self) -> Option<String> {
        self.recorded_host_key.lock().await.clone()
    }

    /// Get the actual host key from a failed connection (mismatch case).
    /// Same as get_recorded_host_key but named for clarity in error handling.
    pub async fn get_mismatched_host_key(&self) -> Option<String> {
        self.recorded_host_key.lock().await.clone()
    }

    /// Accept a new host key (after user confirmation) by updating the known key.
    pub async fn accept_host_key(&self, fingerprint: String) {
        self.config.lock().await.known_host_key = Some(fingerprint);
        *self.recorded_host_key.lock().await = None;
    }

    /// Connect with reconnection logic (exponential backoff)
    pub async fn connect_with_reconnect(&self, auth: &super::auth::AuthMethod) -> Result<()> {
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

    /// Get the auth banner received during the most recent authentication
    /// (RFC4252 §5.4). Returns None if no banner was sent or if not yet connected.
    pub async fn auth_banner(&self) -> Option<String> {
        self.auth_banner.lock().await.clone()
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
            .map_err(|e| Error::Ssh(format!("failed to open direct-tcpip channel: {}", e)))
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
        let sequence: Vec<u64> = (0..10)
            .map(|_| {
                let current = backoff;
                backoff = (backoff * 2).min(max);
                current
            })
            .collect();

        assert_eq!(sequence, vec![1, 2, 4, 8, 16, 30, 30, 30, 30, 30]);
    }

    #[tokio::test]
    async fn test_disconnect_when_not_connected() {
        let client = SshClientHandle::new(SshClientConfig::default());
        client.disconnect().await.unwrap();
        assert_eq!(client.state().await, ConnectionState::Disconnected);
    }

    /// Verify the detail string format produced on HostKeyMismatch.
    /// The Android UI parses this with `Regex("got:\\s*(SHA256:\\S+)")`,
    /// so the format must remain stable.
    #[test]
    fn test_hostkey_mismatch_detail_format() {
        // Mirror the formatting logic in connect() (client.rs:248-252).
        let cases: Vec<(Option<&str>, Option<&str>, &str)> = vec![
            (
                Some("SHA256:aaa"),
                Some("SHA256:bbb"),
                "expected: SHA256:aaa, got: SHA256:bbb",
            ),
            (
                Some("SHA256:aaa"),
                None,
                "expected: SHA256:aaa, got: <unknown>",
            ),
            (None, Some("SHA256:bbb"), "host key verification failed"),
            (None, None, "host key verification failed"),
        ];
        for (known, actual, expected) in cases {
            let detail = match (known, actual) {
                (Some(k), Some(a)) => format!("expected: {}, got: {}", k, a),
                (Some(k), None) => format!("expected: {}, got: <unknown>", k),
                _ => "host key verification failed".to_string(),
            };
            assert_eq!(detail, expected);

            // Verify the Android-side regex contract: only the
            // (Some, Some) case yields a usable SHA256 fingerprint.
            // Android uses `Regex("got:\\s*(SHA256:\\S+)").find(raw)?.groupValues[1]`,
            // i.e. capture group 1, so we use `captures()` here.
            let re = regex::Regex::new(r"got:\s*(SHA256:\S+)").unwrap();
            let extracted = re
                .captures(&detail)
                .map(|c| c.get(1).unwrap().as_str().to_string());
            if let (Some(k), Some(a)) = (known, actual) {
                assert!(k.starts_with("SHA256:"), "known must be SHA256");
                assert!(a.starts_with("SHA256:"), "actual must be SHA256");
                assert_eq!(
                    extracted.as_deref(),
                    Some(a),
                    "regex should extract actual fingerprint"
                );
            } else if known.is_some() {
                // (Some, None): "got: <unknown>" — regex must NOT match.
                assert_eq!(
                    extracted, None,
                    "regex must not match when actual is <unknown>"
                );
            } else {
                assert_eq!(extracted, None, "regex must not match fallback detail");
            }
        }
    }
}

/// A stream wrapper that first returns pre-read bytes, then reads from the
/// underlying stream. This lets us peek at the server's pre-banner lines and
/// then hand the same stream (with those bytes still visible) to russh.
struct PrefixedStream {
    /// Bytes already read from the TCP stream (pre-banner + possibly SSH id)
    prefix: Vec<u8>,
    /// Position in the prefix buffer
    prefix_pos: usize,
    /// The underlying TCP stream
    inner: tokio::net::TcpStream,
}

impl tokio::io::AsyncRead for PrefixedStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.get_mut();

        // First, drain the prefix buffer
        if this.prefix_pos < this.prefix.len() {
            let remaining = &this.prefix[this.prefix_pos..];
            let n = remaining.len().min(buf.remaining());
            buf.put_slice(&remaining[..n]);
            this.prefix_pos += n;
            return std::task::Poll::Ready(Ok(()));
        }

        // Prefix exhausted — read from the underlying stream
        std::pin::Pin::new(&mut this.inner).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for PrefixedStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

/// Open a TCP connection to `addr`, read the first chunk of data, and extract
/// any pre-banner lines (lines that don't start with "SSH-"). Returns the
/// pre-banner text and a stream that replays the pre-read bytes followed by
/// the remaining TCP data, so russh can use it via `connect_stream`.
///
/// This makes exactly ONE TCP connection — unlike the old `read_server_banner`
/// which made a second connection just to read the pre-banner.
///
/// `protector` is called after the socket is created but before `connect()` is
/// issued. On Android this is used to call `VpnService.protect(fd)` so the SSH
/// control traffic is not routed back into the TUN.
async fn connect_and_peek(
    addr: &str,
    protector: Option<&dyn SocketProtector>,
) -> std::io::Result<(String, PrefixedStream)> {
    use tokio::io::AsyncReadExt;

    let mut stream = connect_socket(addr, protector).await?;

    // Read the first chunk (up to 2KB). The server may send:
    //   - Pre-banner lines + SSH version (e.g. "Not allowed\r\nSSH-2.0-...")
    //   - Just pre-banner lines then close (rejection)
    //   - Just SSH version (normal)
    let mut buf = vec![0u8; 2048];
    let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf))
        .await
        .unwrap_or(Ok(0))?;
    let data = &buf[..n];

    if data.is_empty() {
        // Server closed immediately — no pre-banner to extract
        return Ok((
            String::new(),
            PrefixedStream {
                prefix: Vec::new(),
                prefix_pos: 0,
                inner: stream,
            },
        ));
    }

    // Extract pre-banner lines (lines before the first "SSH-" line)
    let text = String::from_utf8_lossy(data);
    let pre_banner_lines: Vec<&str> = text
        .lines()
        .take_while(|line| !line.starts_with("SSH-"))
        .filter(|line| !line.trim().is_empty())
        .take(3)
        .collect();

    let pre_banner = if pre_banner_lines.is_empty() {
        String::new()
    } else {
        pre_banner_lines.join(" | ")
    };

    Ok((
        pre_banner,
        PrefixedStream {
            prefix: data.to_vec(),
            prefix_pos: 0,
            inner: stream,
        },
    ))
}

/// Create a TCP stream with an optional `SocketProtector` hook.
/// On Unix (Linux/macOS/Android) we use `socket2` to create the socket so the
/// protector can be called before `connect()`. On other platforms we fall back
/// to the standard `tokio::net::TcpStream::connect`.
async fn connect_socket(
    addr: &str,
    protector: Option<&dyn SocketProtector>,
) -> std::io::Result<tokio::net::TcpStream> {
    #[cfg(unix)]
    {
        use std::os::fd::FromRawFd;
        use std::os::fd::IntoRawFd;

        // Resolve host:port to a concrete SocketAddr (required by socket2).
        let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host(addr).await?.collect();
        if addrs.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AddrNotAvailable,
                format!("no address found for {}", addr),
            ));
        }
        let socket_addr = addrs[0];

        let domain = match socket_addr {
            std::net::SocketAddr::V4(_) => socket2::Domain::IPV4,
            std::net::SocketAddr::V6(_) => socket2::Domain::IPV6,
        };

        let socket = socket2::Socket::new(domain, socket2::Type::STREAM, None)?;
        socket.set_nonblocking(true)?;

        // Android: call VpnService.protect(fd) before connect.
        if let Some(p) = protector {
            p.protect_socket(&socket)?;
        }

        // Non-blocking connect: on Unix this returns EINPROGRESS.
        let std_stream = match socket.connect(&socket_addr.into()) {
            Ok(()) => {
                // Connect completed immediately; ownership transfers to TcpStream below.
                let fd = socket.into_raw_fd();
                unsafe { std::net::TcpStream::from_raw_fd(fd) }
            }
            Err(e) => {
                if e.raw_os_error() == Some(libc::EINPROGRESS) {
                    let fd = socket.into_raw_fd();
                    unsafe { std::net::TcpStream::from_raw_fd(fd) }
                } else {
                    return Err(e);
                }
            }
        };

        let stream = tokio::net::TcpStream::from_std(std_stream)?;

        // Wait for the non-blocking connect to complete (writable = connected or error).
        tokio::time::timeout(Duration::from_secs(10), stream.writable())
            .await
            .map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::TimedOut, "connect timed out")
            })??;
        if let Some(err) = stream.take_error()? {
            return Err(err);
        }

        Ok(stream)
    }

    #[cfg(not(unix))]
    {
        let _ = protector; // no-op on Windows; VpnService is not available there
        tokio::net::TcpStream::connect(addr).await
    }
}
