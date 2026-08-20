//! Port forwarding — local (-L) and remote (-R) forwarders
//!
//! Local forward: listens on a local TCP port, forwards each connection
//! to a remote target via SSH direct-tcpip channel.
//!
//! Remote forward: requests the SSH server to listen on a remote port,
//! forwards each incoming connection to a local target.

use crate::config::PortForwardRule;
use crate::error::{Error, ErrorCode, IpcError, Result};
use crate::ssh::channel_opener::{ChannelOpener, SshChannelOpener};
use crate::ssh::forwarded_dispatch::{ForwardKey, ForwardedDispatch};
use russh::client;
use russh::Channel;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

/// Runtime status of a port forward rule
#[derive(Debug, Clone, serde::Serialize)]
pub struct PortForwardStatus {
    pub rule_id: String,
    pub running: bool,
    pub error: Option<String>,
    pub active_connections: u32,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

/// Trait for both local and remote forwarders
#[async_trait::async_trait]
pub trait Forwarder: Send + Sync {
    async fn stop(&self) -> Result<()>;
    fn status(&self) -> PortForwardStatus;
}

/// Local port forwarder (-L)
///
/// Listens on `local_host:local_port`, forwards each connection to
/// `remote_host:remote_port` via SSH direct-tcpip channel.
pub struct LocalForwarder {
    rule: PortForwardRule,
    channel_opener: Arc<SshChannelOpener>,
    listener: Mutex<Option<TcpListener>>,
    cancelled: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    active_connections: Arc<AtomicU32>,
    bytes_in: Arc<AtomicU64>,
    bytes_out: Arc<AtomicU64>,
}

impl LocalForwarder {
    /// Stop the forwarder from a shared reference (for use with Arc<LocalForwarder>).
    pub async fn stop_shared(&self) -> Result<()> {
        self.cancelled.store(true, Ordering::Relaxed);
        self.running.store(false, Ordering::Relaxed);
        *self.listener.lock().await = None;
        tracing::info!("local forward {} stopped", self.rule.id);
        Ok(())
    }

    pub fn new(rule: PortForwardRule, channel_opener: Arc<SshChannelOpener>) -> Self {
        Self {
            rule,
            channel_opener,
            listener: Mutex::new(None),
            cancelled: Arc::new(AtomicBool::new(false)),
            running: Arc::new(AtomicBool::new(false)),
            active_connections: Arc::new(AtomicU32::new(0)),
            bytes_in: Arc::new(AtomicU64::new(0)),
            bytes_out: Arc::new(AtomicU64::new(0)),
        }
    }

    pub async fn start(&self) -> Result<()> {
        let addr = format!("{}:{}", self.rule.local_host, self.rule.local_port);
        let listener = TcpListener::bind(&addr).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::AddrInUse {
                Error::Ipc(IpcError::new(
                    ErrorCode::ProxyPortInUse,
                    format!("port {} is already in use", self.rule.local_port),
                ))
            } else {
                Error::Io(e)
            }
        })?;

        tracing::info!(
            "local forward started: {}:{} -> {}:{} (rule {})",
            self.rule.local_host,
            self.rule.local_port,
            self.rule.remote_host,
            self.rule.remote_port,
            self.rule.id
        );

        *self.listener.lock().await = Some(listener);
        self.running.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Run the accept loop. Should be spawned as a tokio task.
    /// Takes ownership of the listener from the Mutex.
    pub async fn run_accept_loop(&self) {
        let listener = {
            let mut guard = self.listener.lock().await;
            match guard.take() {
                Some(l) => l,
                None => {
                    tracing::error!("local forward {}: listener not set", self.rule.id);
                    return;
                }
            }
        };

        loop {
            if self.cancelled.load(Ordering::Relaxed) {
                tracing::info!("local forward {} cancelled", self.rule.id);
                break;
            }
            match tokio::time::timeout(
                std::time::Duration::from_millis(500),
                listener.accept(),
            ).await {
                Ok(Ok((mut local_stream, client_addr))) => {
                    if self.cancelled.load(Ordering::Relaxed) {
                        break;
                    }
                    tracing::debug!(
                        "local forward {}: connection from {}",
                        self.rule.id,
                        client_addr
                    );
                    let opener = self.channel_opener.clone();
                    let remote_host = self.rule.remote_host.clone();
                    let remote_port = self.rule.remote_port;
                    let active = self.active_connections.clone();
                    let (bin, bout) = (self.bytes_in.clone(), self.bytes_out.clone());

                    tokio::spawn(async move {
                        // RAII guard: ensures active_connections is decremented
                        // even if the task panics (e.g. during copy_bidirectional).
                        struct ConnGuard<'a>(&'a AtomicU32);
                        impl<'a> Drop for ConnGuard<'a> {
                            fn drop(&mut self) {
                                self.0.fetch_sub(1, Ordering::Relaxed);
                            }
                        }
                        let _guard = ConnGuard(active.as_ref());
                        active.fetch_add(1, Ordering::Relaxed);
                        match opener.open_channel(&remote_host, remote_port).await {
                            Ok(channel) => {
                                copy_bidirectional_local(
                                    &mut local_stream,
                                    channel,
                                    bin,
                                    bout,
                                ).await;
                            }
                            Err(e) => {
                                tracing::error!(
                                    "local forward: failed to open channel to {}:{}: {}",
                                    remote_host, remote_port, e
                                );
                            }
                        }
                        // _guard dropped here, fetch_sub executed
                    });
                }
                Ok(Err(e)) => {
                    tracing::error!("local forward {} accept error: {}", self.rule.id, e);
                    break;
                }
                Err(_) => {
                    // Timeout — loop back to check cancel flag
                }
            }
        }
    }
}

/// Bidirectional copy between a local TcpStream and an SSH channel.
/// local_stream <-> channel
async fn copy_bidirectional_local(
    local_stream: &mut tokio::net::TcpStream,
    channel: Channel<client::Msg>,
    bytes_in: Arc<AtomicU64>,
    bytes_out: Arc<AtomicU64>,
) {
    let (cli_read, mut cli_write) = local_stream.split();
    let (mut chan_read, chan_write) = channel.split();
    let chan_reader = chan_read.make_reader();
    let mut chan_writer = chan_write.make_writer();

    let mut counting_cli_read: crate::proxy::manager::CountingReader<_> =
        crate::proxy::manager::CountingReader {
            inner: cli_read,
            counter: bytes_in,
        };
    let mut counting_chan_reader: crate::proxy::manager::CountingReader<_> =
        crate::proxy::manager::CountingReader {
            inner: chan_reader,
            counter: bytes_out,
        };

    let client_to_channel = async {
        tokio::io::copy(&mut counting_cli_read, &mut chan_writer).await
    };
    let channel_to_client = async {
        tokio::io::copy(&mut counting_chan_reader, &mut cli_write).await
    };

    tokio::select! {
        _ = client_to_channel => {}
        _ = channel_to_client => {}
    }
}
// === SECTION 1 END ===

#[async_trait::async_trait]
impl Forwarder for LocalForwarder {
    async fn stop(&self) -> Result<()> {
        self.stop_shared().await
    }

    fn status(&self) -> PortForwardStatus {
        PortForwardStatus {
            rule_id: self.rule.id.clone(),
            running: self.running.load(Ordering::Relaxed),
            error: None,
            active_connections: self.active_connections.load(Ordering::Relaxed),
            bytes_in: self.bytes_in.load(Ordering::Relaxed),
            bytes_out: self.bytes_out.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PortForwardType;

    #[test]
    fn test_local_forwarder_creation() {
        let rule = PortForwardRule {
            id: "pf_test".into(),
            name: "test".into(),
            forward_type: PortForwardType::Local,
            local_host: "127.0.0.1".into(),
            local_port: 13306,
            remote_host: "127.0.0.1".into(),
            remote_port: 3306,
            enabled: true,
            auto_start: false,
        };
        let opener = Arc::new(SshChannelOpener::empty());
        let fw = LocalForwarder::new(rule, opener);
        let status = fw.status();
        assert_eq!(status.rule_id, "pf_test");
        assert!(!status.running);
        assert_eq!(status.active_connections, 0);
        assert_eq!(status.bytes_in, 0);
        assert_eq!(status.bytes_out, 0);
    }

    #[tokio::test]
    async fn test_local_forwarder_port_in_use() {
        // Bind a port first to create conflict
        let listener = TcpListener::bind("127.0.0.1:13307").await.unwrap();
        let rule = PortForwardRule {
            id: "pf_conflict".into(),
            name: "conflict test".into(),
            forward_type: PortForwardType::Local,
            local_host: "127.0.0.1".into(),
            local_port: 13307,
            remote_host: "127.0.0.1".into(),
            remote_port: 3306,
            enabled: true,
            auto_start: false,
        };
        let opener = Arc::new(SshChannelOpener::empty());
        let fw = LocalForwarder::new(rule, opener);
        let result = fw.start().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Ipc(ipc) => assert_eq!(ipc.code, ErrorCode::ProxyPortInUse),
            other => panic!("expected ProxyPortInUse, got {:?}", other),
        }
        drop(listener);
    }

    #[tokio::test]
    async fn test_local_forwarder_start_success() {
        let rule = PortForwardRule {
            id: "pf_ok".into(),
            name: "ok test".into(),
            forward_type: PortForwardType::Local,
            local_host: "127.0.0.1".into(),
            local_port: 13308,
            remote_host: "127.0.0.1".into(),
            remote_port: 3306,
            enabled: true,
            auto_start: false,
        };
        let opener = Arc::new(SshChannelOpener::empty());
        let fw = LocalForwarder::new(rule, opener);
        let result = fw.start().await;
        assert!(result.is_ok());
        // Listener should be set
        assert!(fw.listener.lock().await.is_some());
    }

    #[tokio::test]
    async fn test_local_forwarder_stop() {
        let rule = PortForwardRule {
            id: "pf_stop".into(),
            name: "stop test".into(),
            forward_type: PortForwardType::Local,
            local_host: "127.0.0.1".into(),
            local_port: 13309,
            remote_host: "127.0.0.1".into(),
            remote_port: 3306,
            enabled: true,
            auto_start: false,
        };
        let opener = Arc::new(SshChannelOpener::empty());
        let fw = LocalForwarder::new(rule, opener);
        fw.start().await.unwrap();
        assert!(fw.listener.lock().await.is_some());
        fw.stop().await.unwrap();
        assert!(fw.listener.lock().await.is_none());
    }
}
// === SECTION 2 END ===

// === SECTION 3: RemoteForwarder ===

/// Remote port forwarder (-R)
///
/// Requests the SSH server to listen on `local_host:local_port` (remote side),
/// forwards each incoming connection to `remote_host:remote_port` (local target).
pub struct RemoteForwarder {
    rule: PortForwardRule,
    ssh_handle: Arc<client::Handle<crate::ssh::client::SshHandler>>,
    forwarded_dispatch: Arc<ForwardedDispatch>,
    /// Actual remote port assigned by the server (may differ if port 0 was requested)
    actual_remote_port: Mutex<Option<u16>>,
    cancelled: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    active_connections: Arc<AtomicU32>,
    bytes_in: Arc<AtomicU64>,
    bytes_out: Arc<AtomicU64>,
}

impl RemoteForwarder {
    pub fn new(
        rule: PortForwardRule,
        ssh_handle: Arc<client::Handle<crate::ssh::client::SshHandler>>,
        forwarded_dispatch: Arc<ForwardedDispatch>,
    ) -> Self {
        Self {
            rule,
            ssh_handle,
            forwarded_dispatch,
            actual_remote_port: Mutex::new(None),
            cancelled: Arc::new(AtomicBool::new(false)),
            running: Arc::new(AtomicBool::new(false)),
            active_connections: Arc::new(AtomicU32::new(0)),
            bytes_in: Arc::new(AtomicU64::new(0)),
            bytes_out: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Request the SSH server to listen on the remote port.
    /// Returns the actual port assigned by the server.
    /// If port 0 was requested, the server assigns a port; otherwise the requested port is returned.
    pub async fn start(&self) -> Result<u16> {
        let addr = self.rule.local_host.clone();
        let port = self.rule.local_port as u32;
        let returned_port = self
            .ssh_handle
            .tcpip_forward(&addr, port)
            .await
            .map_err(|e| {
                let msg = format!("tcpip_forward failed: {}", e);
                // SSH server rejected the request — usually because:
                // 1. sshd_config has AllowTcpForwarding no
                // 2. PermitListen restricts the port
                // 3. The port is already in use on the remote server
                if msg.contains("rejected by the other party") {
                    Error::Ipc(IpcError::new(
                        ErrorCode::Internal,
                        format!(
                            "remote forward rejected by server (check sshd AllowTcpForwarding, PermitListen, or port {} availability): {}",
                            port, e
                        ),
                    ))
                } else {
                    Error::Ipc(IpcError::new(ErrorCode::Internal, msg))
                }
            })?;

        // When a specific port is requested, the server reply has no data
        // and russh returns 0. Use the requested port in that case.
        // When port 0 is requested, the server assigns a port and returns it.
        let actual_port = if returned_port == 0 && port != 0 {
            port as u16
        } else {
            returned_port as u16
        };

        *self.actual_remote_port.lock().await = Some(actual_port);
        self.running.store(true, Ordering::Relaxed);

        tracing::info!(
            "remote forward started: remote {}:{} -> local {}:{} (rule {}, actual port {})",
            self.rule.local_host,
            self.rule.local_port,
            self.rule.remote_host,
            self.rule.remote_port,
            self.rule.id,
            actual_port
        );
        Ok(actual_port)
    }

    /// Run the receive loop for forwarded-tcpip channels.
    /// Should be spawned as a tokio task after `start()`.
    pub async fn run_receive_loop(&self) {
        // Use the actual remote port (assigned by server) for dispatch key,
        // not the requested port (which may be 0 for auto-assign).
        let actual_port = self.actual_remote_port.lock().await.unwrap_or(self.rule.local_port);
        let key: ForwardKey = (self.rule.local_host.clone(), actual_port);
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Channel<client::Msg>>(16);
        self.forwarded_dispatch.register(key.clone(), tx).await;

        let remote_host = self.rule.remote_host.clone();
        let remote_port = self.rule.remote_port;
        let active = self.active_connections.clone();
        let (bin, bout) = (self.bytes_in.clone(), self.bytes_out.clone());

        loop {
            if self.cancelled.load(Ordering::Relaxed) {
                tracing::info!("remote forward {} cancelled", self.rule.id);
                break;
            }
            match tokio::time::timeout(
                std::time::Duration::from_millis(500),
                rx.recv(),
            ).await {
                Ok(Some(channel)) => {
                    if self.cancelled.load(Ordering::Relaxed) {
                        break;
                    }
                    let rh = remote_host.clone();
                    let active = active.clone();
                    let bin = bin.clone();
                    let bout = bout.clone();
                    tokio::spawn(async move {
                        // RAII guard: ensures active_connections is decremented
                        // even if the task panics.
                        struct ConnGuard<'a>(&'a AtomicU32);
                        impl<'a> Drop for ConnGuard<'a> {
                            fn drop(&mut self) {
                                self.0.fetch_sub(1, Ordering::Relaxed);
                            }
                        }
                        let _guard = ConnGuard(active.as_ref());
                        active.fetch_add(1, Ordering::Relaxed);
                        match TcpStream::connect((rh.as_str(), remote_port)).await {
                            Ok(mut local_stream) => {
                                copy_bidirectional_remote(
                                    &mut local_stream,
                                    channel,
                                    bin,
                                    bout,
                                ).await;
                            }
                            Err(e) => {
                                tracing::error!(
                                    "remote forward: failed to connect to local target {}:{}: {}",
                                    rh, remote_port, e
                                );
                            }
                        }
                        // _guard dropped here, fetch_sub executed
                    });
                }
                Ok(None) => {
                    // Channel closed — dispatcher unregistered or SSH disconnected
                    tracing::info!("remote forward {} dispatch channel closed", self.rule.id);
                    break;
                }
                Err(_) => {
                    // Timeout — check cancel flag
                }
            }
        }
        self.forwarded_dispatch.unregister(&key).await;
    }

    /// Get the actual remote port assigned by the server
    pub async fn actual_remote_port(&self) -> Option<u16> {
        *self.actual_remote_port.lock().await
    }

    /// Stop from a shared reference
    pub async fn stop_shared(&self) -> Result<()> {
        self.cancelled.store(true, Ordering::Relaxed);
        self.running.store(false, Ordering::Relaxed);
        // Cancel the remote forwarding on the server
        if let Some(port) = *self.actual_remote_port.lock().await {
            let addr = self.rule.local_host.clone();
            let _ = self.ssh_handle.cancel_tcpip_forward(&addr, port as u32).await;
        }
        tracing::info!("remote forward {} stopped", self.rule.id);
        Ok(())
    }
}

/// Bidirectional copy between a local TcpStream and an SSH channel (remote forward).
/// channel <-> local_stream
async fn copy_bidirectional_remote(
    local_stream: &mut TcpStream,
    channel: Channel<client::Msg>,
    bytes_in: Arc<AtomicU64>,
    bytes_out: Arc<AtomicU64>,
) {
    let (cli_read, mut cli_write) = local_stream.split();
    let (mut chan_read, chan_write) = channel.split();
    let chan_reader = chan_read.make_reader();
    let mut chan_writer = chan_write.make_writer();

    let mut counting_chan_reader: crate::proxy::manager::CountingReader<_> =
        crate::proxy::manager::CountingReader {
            inner: chan_reader,
            counter: bytes_in,
        };
    let mut counting_cli_read: crate::proxy::manager::CountingReader<_> =
        crate::proxy::manager::CountingReader {
            inner: cli_read,
            counter: bytes_out,
        };

    let channel_to_local = async {
        tokio::io::copy(&mut counting_chan_reader, &mut cli_write).await
    };
    let local_to_channel = async {
        tokio::io::copy(&mut counting_cli_read, &mut chan_writer).await
    };

    tokio::select! {
        _ = channel_to_local => {}
        _ = local_to_channel => {}
    }
}

#[async_trait::async_trait]
impl Forwarder for RemoteForwarder {
    async fn stop(&self) -> Result<()> {
        self.stop_shared().await
    }

    fn status(&self) -> PortForwardStatus {
        PortForwardStatus {
            rule_id: self.rule.id.clone(),
            running: self.running.load(Ordering::Relaxed),
            error: None,
            active_connections: self.active_connections.load(Ordering::Relaxed),
            bytes_in: self.bytes_in.load(Ordering::Relaxed),
            bytes_out: self.bytes_out.load(Ordering::Relaxed),
        }
    }
}
// === SECTION 3 END ===

// === SECTION 4: PortForwardManager ===

use std::collections::HashMap;

/// Manages the lifecycle of port forwarding rules for a single server.
pub struct PortForwardManager {
    /// Running forwarders, keyed by rule_id
    forwarders: Mutex<HashMap<String, Arc<dyn Forwarder>>>,
    /// The SSH channel opener (shared with proxy layer)
    channel_opener: Arc<SshChannelOpener>,
    /// The SSH handle (for remote forwarding)
    ssh_handle: Mutex<Option<Arc<client::Handle<crate::ssh::client::SshHandler>>>>,
    /// The forwarded-tcpip dispatcher
    forwarded_dispatch: Arc<ForwardedDispatch>,
}

impl PortForwardManager {
    pub fn new(
        channel_opener: Arc<SshChannelOpener>,
        forwarded_dispatch: Arc<ForwardedDispatch>,
    ) -> Self {
        Self {
            forwarders: Mutex::new(HashMap::new()),
            channel_opener,
            ssh_handle: Mutex::new(None),
            forwarded_dispatch,
        }
    }

    /// Set the SSH handle (called when SSH connects)
    pub async fn set_ssh_handle(&self, handle: Arc<client::Handle<crate::ssh::client::SshHandler>>) {
        *self.ssh_handle.lock().await = Some(handle);
    }

    /// Clear the SSH handle (called when SSH disconnects)
    pub async fn clear_ssh_handle(&self) {
        *self.ssh_handle.lock().await = None;
    }

    /// Start a port forwarding rule
    pub async fn start_rule(&self, rule: &PortForwardRule) -> Result<()> {
        // Hold the lock for the entire start operation to prevent TOCTOU
        let mut forwarders = self.forwarders.lock().await;
        if forwarders.contains_key(&rule.id) {
            return Err(Error::Other(format!("port forward rule {} already running", rule.id)));
        }

        match rule.forward_type {
            crate::config::PortForwardType::Local => {
                let fw = LocalForwarder::new(rule.clone(), self.channel_opener.clone());
                fw.start().await?;
                let fw_arc = Arc::new(fw);
                let fw_for_loop = fw_arc.clone();
                tokio::spawn(async move {
                    fw_for_loop.run_accept_loop().await;
                });
                forwarders.insert(rule.id.clone(), fw_arc as Arc<dyn Forwarder>);
            }
            crate::config::PortForwardType::Remote => {
                let handle = self.ssh_handle.lock().await.clone()
                    .ok_or_else(|| Error::Ipc(IpcError::new(
                        ErrorCode::SshDisconnected,
                        "SSH not connected, please connect first".to_string(),
                    )))?;
                let fw = RemoteForwarder::new(rule.clone(), handle, self.forwarded_dispatch.clone());
                fw.start().await?;
                let fw_arc = Arc::new(fw);
                let fw_for_loop = fw_arc.clone();
                tokio::spawn(async move {
                    fw_for_loop.run_receive_loop().await;
                });
                forwarders.insert(rule.id.clone(), fw_arc as Arc<dyn Forwarder>);
            }
        }
        Ok(())
    }

    /// Stop a port forwarding rule
    pub async fn stop_rule(&self, rule_id: &str) -> Result<()> {
        let mut forwarders = self.forwarders.lock().await;
        if let Some(fw) = forwarders.remove(rule_id) {
            fw.stop().await?;
        }
        Ok(())
    }

    /// Stop all port forwarding rules
    pub async fn stop_all(&self) -> Result<()> {
        let mut forwarders = self.forwarders.lock().await;
        for (_, fw) in forwarders.drain() {
            let _ = fw.stop().await;
        }
        Ok(())
    }

    /// Start all auto-start rules (called after SSH connects)
    pub async fn start_auto_rules(&self, rules: &[PortForwardRule]) -> Result<()> {
        for rule in rules {
            if rule.auto_start && rule.enabled {
                if let Err(e) = self.start_rule(rule).await {
                    tracing::warn!("failed to auto-start port forward {}: {}", rule.id, e);
                }
            }
        }
        Ok(())
    }

    /// Get the status of all running forwarders
    pub async fn get_all_statuses(&self) -> Vec<PortForwardStatus> {
        let forwarders = self.forwarders.lock().await;
        forwarders.values().map(|fw| fw.status()).collect()
    }

    /// Get the status of a specific forwarder
    pub async fn get_status(&self, rule_id: &str) -> Option<PortForwardStatus> {
        let forwarders = self.forwarders.lock().await;
        forwarders.get(rule_id).map(|fw| fw.status())
    }

    /// Check if any port forward rules are currently running
    pub async fn has_running(&self) -> bool {
        let forwarders = self.forwarders.lock().await;
        !forwarders.is_empty()
    }
}
// === SECTION 4 END ===

// === SECTION 4 TESTS ===

#[cfg(test)]
mod manager_tests {
    use super::*;
    use crate::ssh::channel_opener::SshChannelOpener;
    use crate::ssh::forwarded_dispatch::ForwardedDispatch;

    fn make_manager() -> PortForwardManager {
        let channel_opener = Arc::new(SshChannelOpener::empty());
        let dispatch = Arc::new(ForwardedDispatch::new());
        PortForwardManager::new(channel_opener, dispatch)
    }

    fn make_local_rule(id: &str, local_port: u16) -> PortForwardRule {
        PortForwardRule {
            id: id.into(),
            name: format!("Local {}", id),
            forward_type: crate::config::PortForwardType::Local,
            local_host: "127.0.0.1".into(),
            local_port,
            remote_host: "127.0.0.1".into(),
            remote_port: 80,
            enabled: true,
            auto_start: false,
        }
    }

    #[tokio::test]
    async fn test_manager_start_and_stop_local_rule() {
        let mgr = make_manager();
        let rule = make_local_rule("pf_mgr_1", 13601);

        // Start should succeed
        let result = mgr.start_rule(&rule).await;
        assert!(result.is_ok(), "start_rule should succeed: {:?}", result.err());

        // Status should show running
        let status = mgr.get_status("pf_mgr_1").await;
        assert!(status.is_some());
        assert!(status.unwrap().running);

        // Stop should succeed
        let stop_result = mgr.stop_rule("pf_mgr_1").await;
        assert!(stop_result.is_ok());

        // Status should be None after stop
        let status_after = mgr.get_status("pf_mgr_1").await;
        assert!(status_after.is_none());
    }

    #[tokio::test]
    async fn test_manager_start_duplicate_rule_fails() {
        let mgr = make_manager();
        let rule = make_local_rule("pf_mgr_2", 13602);

        mgr.start_rule(&rule).await.unwrap();
        // Starting again should fail
        let result = mgr.start_rule(&rule).await;
        assert!(result.is_err(), "starting duplicate rule should fail");

        mgr.stop_rule("pf_mgr_2").await.unwrap();
    }

    #[tokio::test]
    async fn test_manager_stop_nonexistent_rule() {
        let mgr = make_manager();
        // Stopping a non-existent rule should not error
        let result = mgr.stop_rule("nonexistent").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_manager_stop_all() {
        let mgr = make_manager();
        let rule1 = make_local_rule("pf_mgr_3", 13603);
        let rule2 = make_local_rule("pf_mgr_4", 13604);

        mgr.start_rule(&rule1).await.unwrap();
        mgr.start_rule(&rule2).await.unwrap();

        let statuses = mgr.get_all_statuses().await;
        assert_eq!(statuses.len(), 2);

        mgr.stop_all().await.unwrap();

        let statuses_after = mgr.get_all_statuses().await;
        assert_eq!(statuses_after.len(), 0);
    }

    #[tokio::test]
    async fn test_manager_remote_rule_without_ssh_handle_fails() {
        let mgr = make_manager();
        let rule = PortForwardRule {
            id: "pf_mgr_5".into(),
            name: "Remote".into(),
            forward_type: crate::config::PortForwardType::Remote,
            local_host: "127.0.0.1".into(),
            local_port: 18080,
            remote_host: "127.0.0.1".into(),
            remote_port: 80,
            enabled: true,
            auto_start: false,
        };

        // No SSH handle set — should fail
        let result = mgr.start_rule(&rule).await;
        assert!(result.is_err(), "remote rule without SSH handle should fail");
    }

    #[tokio::test]
    async fn test_manager_start_auto_rules_skips_disabled() {
        let mgr = make_manager();
        let mut rule_enabled = make_local_rule("pf_mgr_6", 13605);
        rule_enabled.auto_start = true;
        let mut rule_disabled = make_local_rule("pf_mgr_7", 13606);
        rule_disabled.auto_start = true;
        rule_disabled.enabled = false;
        let mut rule_no_autostart = make_local_rule("pf_mgr_8", 13607);
        rule_no_autostart.auto_start = false;

        let rules = vec![rule_enabled, rule_disabled, rule_no_autostart];
        mgr.start_auto_rules(&rules).await.unwrap();

        // Only the enabled + auto_start rule should be running
        let statuses = mgr.get_all_statuses().await;
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].rule_id, "pf_mgr_6");

        mgr.stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn test_manager_clear_ssh_handle() {
        let mgr = make_manager();
        // Should not panic
        mgr.clear_ssh_handle().await;
    }
}
// === SECTION 4 TESTS END ===
