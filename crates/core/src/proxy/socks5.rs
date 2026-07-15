//! SOCKS5 proxy server — FP-3.2
//!
//! RFC 1928 SOCKS5 protocol implementation.
//! Supports CONNECT, NO AUTH, USERNAME/PASSWORD auth.
//! BIND and UDP ASSOCIATE return COMMAND NOT SUPPORTED.

use crate::error::{Error, ErrorCode, IpcError, Result};
use crate::proxy::manager::ChannelManager;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// SOCKS5 reply codes
mod reply {
    #![allow(dead_code)]
    pub const SUCCEEDED: u8 = 0x00;
    pub const GENERAL_FAILURE: u8 = 0x01;
    pub const NOT_ALLOWED: u8 = 0x02;
    pub const NETWORK_UNREACHABLE: u8 = 0x03;
    pub const HOST_UNREACHABLE: u8 = 0x04;
    pub const CONNECTION_REFUSED: u8 = 0x05;
    pub const TTL_EXPIRED: u8 = 0x06;
    pub const COMMAND_NOT_SUPPORTED: u8 = 0x07;
    pub const ADDRESS_TYPE_NOT_SUPPORTED: u8 = 0x08;
}

/// SOCKS5 command codes
mod cmd {
    pub const CONNECT: u8 = 0x01;
    pub const BIND: u8 = 0x02;
    pub const UDP_ASSOCIATE: u8 = 0x03;
}

/// Address types
mod atyp {
    pub const IPV4: u8 = 0x01;
    pub const DOMAINNAME: u8 = 0x03;
    pub const IPV6: u8 = 0x04;
}

/// SOCKS5 proxy server
pub struct Socks5Server {
    port: u16,
    channel_manager: Arc<ChannelManager>,
    auth: Option<Arc<Socks5Auth>>,
}

/// SOCKS5 authentication credentials
pub struct Socks5Auth {
    pub username: String,
    pub password: String,
}

impl Socks5Server {
    pub fn new(port: u16, channel_manager: Arc<ChannelManager>) -> Self {
        Self {
            port,
            channel_manager,
            auth: None,
        }
    }

    pub fn with_auth(mut self, auth: Socks5Auth) -> Self {
        self.auth = Some(Arc::new(auth));
        self
    }

    /// Start the SOCKS5 server
    pub async fn start(&self) -> Result<()> {
        let addr = format!("127.0.0.1:{}", self.port);
        let listener = TcpListener::bind(&addr).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::AddrInUse {
                Error::Ipc(IpcError::new(
                    ErrorCode::ProxyPortInUse,
                    format!("SOCKS5 port {} is in use", self.port),
                ))
            } else {
                Error::Io(e)
            }
        })?;

        tracing::info!("SOCKS5 proxy listening on {}", addr);

        loop {
            match listener.accept().await {
                Ok((socket, _)) => {
                    let mgr = self.channel_manager.clone();
                    let auth = self.auth.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(socket, mgr, auth, Vec::new()).await {
                            tracing::debug!("SOCKS5 connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("SOCKS5 accept error: {}", e);
                }
            }
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

/// Handle a single SOCKS5 connection.
/// `peeked` contains bytes already read from the socket (e.g., first byte from mixed proxy).
pub async fn handle_connection(
    mut socket: tokio::net::TcpStream,
    mgr: Arc<ChannelManager>,
    auth: Option<Arc<Socks5Auth>>,
    peeked: Vec<u8>,
) -> Result<()> {
    // --- Authentication negotiation ---
    // Need at least 2 bytes: version + nmethods
    let mut buf = [0u8; 2];
    if peeked.len() >= 2 {
        buf.copy_from_slice(&peeked[..2]);
    } else if peeked.len() == 1 {
        buf[0] = peeked[0];
        socket.read_exact(&mut buf[1..]).await?;
    } else {
        socket.read_exact(&mut buf).await?;
    }

    if buf[0] != 0x05 {
        return Err(Error::Other("not a SOCKS5 request".into()));
    }

    let nmethods = buf[1] as usize;
    // If we peeked more than 2 bytes, use them
    let methods = if peeked.len() > 2 {
        let peeked_methods = &peeked[2..];
        if peeked_methods.len() >= nmethods {
            peeked_methods[..nmethods].to_vec()
        } else {
            let mut m = peeked_methods.to_vec();
            m.resize(nmethods, 0);
            socket.read_exact(&mut m[peeked_methods.len()..]).await?;
            m
        }
    } else {
        let mut m = vec![0u8; nmethods];
        socket.read_exact(&mut m).await?;
        m
    };

    if let Some(auth) = &auth {
        // Require username/password auth
        if !methods.contains(&0x02) {
            socket.write_all(&[0x05, 0xFF]).await?;
            return Err(Error::Other(
                "client doesn't support username/password auth".into(),
            ));
        }
        socket.write_all(&[0x05, 0x02]).await?;

        // Username/password sub-negotiation (RFC 1929)
        let mut ver = [0u8; 1];
        socket.read_exact(&mut ver).await?;
        if ver[0] != 0x01 {
            return Err(Error::Other("invalid auth version".into()));
        }
        let mut ulen = [0u8; 1];
        socket.read_exact(&mut ulen).await?;
        let mut username = vec![0u8; ulen[0] as usize];
        socket.read_exact(&mut username).await?;
        let mut plen = [0u8; 1];
        socket.read_exact(&mut plen).await?;
        let mut password = vec![0u8; plen[0] as usize];
        socket.read_exact(&mut password).await?;

        if username != auth.username.as_bytes() || password != auth.password.as_bytes() {
            socket.write_all(&[0x01, 0x01]).await?; // auth failure
            return Err(Error::Other("SOCKS5 auth failed".into()));
        }
        socket.write_all(&[0x01, 0x00]).await?; // auth success
    } else {
        // No auth
        if !methods.contains(&0x00) {
            socket.write_all(&[0x05, 0xFF]).await?;
            return Err(Error::Other("client doesn't support no-auth".into()));
        }
        socket.write_all(&[0x05, 0x00]).await?;
    }

    // --- Request ---
    let mut req = [0u8; 4];
    socket.read_exact(&mut req).await?;

    if req[0] != 0x05 {
        return Err(Error::Other("invalid SOCKS5 version in request".into()));
    }

    let command = req[1];
    // req[2] is reserved (0x00)
    let addr_type = req[3];

    // Parse destination address
    let (host, port) = parse_address(&mut socket, addr_type).await?;

    // Handle command
    match command {
        cmd::CONNECT => handle_connect(&mut socket, &mgr, &host, port).await,
        cmd::BIND | cmd::UDP_ASSOCIATE => {
            // Return command not supported
            let reply = [
                0x05,
                reply::COMMAND_NOT_SUPPORTED,
                0x00,
                0x01,
                0,
                0,
                0,
                0,
                0,
                0,
            ];
            socket.write_all(&reply).await?;
            Err(Error::Other(format!("command {} not supported", command)))
        }
        _ => {
            let reply = [
                0x05,
                reply::COMMAND_NOT_SUPPORTED,
                0x00,
                0x01,
                0,
                0,
                0,
                0,
                0,
                0,
            ];
            socket.write_all(&reply).await?;
            Err(Error::Other(format!("unknown command {}", command)))
        }
    }
}

/// Parse the destination address from the SOCKS5 request
async fn parse_address(socket: &mut tokio::net::TcpStream, addr_type: u8) -> Result<(String, u16)> {
    match addr_type {
        atyp::IPV4 => {
            let mut addr = [0u8; 4];
            socket.read_exact(&mut addr).await?;
            let mut port_buf = [0u8; 2];
            socket.read_exact(&mut port_buf).await?;
            let port = u16::from_be_bytes(port_buf);
            Ok((
                format!("{}.{}.{}.{}", addr[0], addr[1], addr[2], addr[3]),
                port,
            ))
        }
        atyp::DOMAINNAME => {
            let mut len_buf = [0u8; 1];
            socket.read_exact(&mut len_buf).await?;
            let mut domain = vec![0u8; len_buf[0] as usize];
            socket.read_exact(&mut domain).await?;
            let mut port_buf = [0u8; 2];
            socket.read_exact(&mut port_buf).await?;
            let port = u16::from_be_bytes(port_buf);
            let host = String::from_utf8(domain)
                .map_err(|e| Error::Other(format!("invalid domain name: {}", e)))?;
            Ok((host, port))
        }
        atyp::IPV6 => {
            let mut addr = [0u8; 16];
            socket.read_exact(&mut addr).await?;
            let mut port_buf = [0u8; 2];
            socket.read_exact(&mut port_buf).await?;
            let port = u16::from_be_bytes(port_buf);
            let ip = std::net::Ipv6Addr::from(addr);
            Ok((ip.to_string(), port))
        }
        _ => Err(Error::Other(format!(
            "unsupported address type: {}",
            addr_type
        ))),
    }
}

/// Handle CONNECT command
async fn handle_connect(
    socket: &mut tokio::net::TcpStream,
    mgr: &ChannelManager,
    host: &str,
    port: u16,
) -> Result<()> {
    tracing::debug!("SOCKS5 CONNECT {} : {}", host, port);

    let managed = match mgr.open(host, port).await {
        Ok(m) => m,
        Err(e) => {
            let reply = [0x05, reply::GENERAL_FAILURE, 0x00, 0x01, 0, 0, 0, 0, 0, 0];
            let _ = socket.write_all(&reply).await;
            return Err(e);
        }
    };

    // Send success reply
    let reply = [0x05, reply::SUCCEEDED, 0x00, 0x01, 0, 0, 0, 0, 0, 0];
    socket.write_all(&reply).await?;

    // Bidirectional data forwarding
    let idle_timeout = managed.idle_timeout();
    let active_clients = managed.active_clients_clone();
    let (bytes_in, bytes_out) = managed.byte_counters();
    let channel = managed.channel;
    let (cli_read, mut cli_write) = socket.split();
    let (mut chan_read, chan_write) = channel.split();
    let chan_reader = chan_read.make_reader();
    let mut chan_writer = chan_write.make_writer();

    // client→channel = upload (bytes_in), channel→client = download (bytes_out)
    let mut counting_cli_read = crate::proxy::manager::CountingReader {
        inner: cli_read,
        counter: bytes_in,
    };
    let mut counting_chan_reader = crate::proxy::manager::CountingReader {
        inner: chan_reader,
        counter: bytes_out,
    };

    let client_to_channel = async {
        tokio::time::timeout(
            idle_timeout,
            tokio::io::copy(&mut counting_cli_read, &mut chan_writer),
        )
        .await
    };
    let channel_to_client = async {
        tokio::time::timeout(
            idle_timeout,
            tokio::io::copy(&mut counting_chan_reader, &mut cli_write),
        )
        .await
    };

    tokio::select! {
        _ = client_to_channel => {
            tracing::debug!("SOCKS5 channel closed (client side) or idle timeout {:?}", idle_timeout);
        }
        _ = channel_to_client => {
            tracing::debug!("SOCKS5 channel closed (channel side) or idle timeout {:?}", idle_timeout);
        }
    }

    let prev = active_clients.fetch_sub(1, Ordering::Relaxed);
    tracing::info!(
        "SOCKS5 active_clients decremented: {} -> {}",
        prev,
        prev.saturating_sub(1)
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socks5_server_creation() {
        use crate::ssh::channel_opener::SshChannelOpener;
        use std::sync::Arc;
        let opener = Arc::new(SshChannelOpener::empty());
        let mgr = Arc::new(ChannelManager::new(opener, 64, 300));
        let server = Socks5Server::new(1080, mgr);
        assert_eq!(server.port, 1080);
        assert!(server.auth.is_none());
    }

    #[test]
    fn test_socks5_server_with_auth() {
        use crate::ssh::channel_opener::SshChannelOpener;
        use std::sync::Arc;
        let opener = Arc::new(SshChannelOpener::empty());
        let mgr = Arc::new(ChannelManager::new(opener, 64, 300));
        let server = Socks5Server::new(1080, mgr).with_auth(Socks5Auth {
            username: "user".into(),
            password: "pass".into(),
        });
        assert!(server.auth.is_some());
    }
}
