//! Mixed proxy server — protocol multiplexing on a single port.
//!
//! Sniffs the first byte of an incoming connection to detect SOCKS5 vs HTTP,
//! then delegates to the appropriate handler.

use crate::error::{Error, ErrorCode, IpcError, Result};
use crate::proxy::manager::ChannelManager;
use crate::proxy::{http, socks5};
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;

/// Mixed proxy server (SOCKS5 + HTTP on the same port)
pub struct MixedProxyServer {
    port: u16,
    channel_manager: Arc<ChannelManager>,
}

impl MixedProxyServer {
    pub fn new(port: u16, channel_manager: Arc<ChannelManager>) -> Self {
        Self {
            port,
            channel_manager,
        }
    }

    /// Start the mixed proxy server
    pub async fn start(&self) -> Result<()> {
        let addr = format!("127.0.0.1:{}", self.port);
        let listener = TcpListener::bind(&addr).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::AddrInUse {
                Error::Ipc(IpcError::new(
                    ErrorCode::ProxyPortInUse,
                    format!("Mixed proxy port {} is in use", self.port),
                ))
            } else {
                Error::Io(e)
            }
        })?;

        tracing::info!("Mixed proxy listening on {}", addr);

        loop {
            match listener.accept().await {
                Ok((socket, _)) => {
                    let mgr = self.channel_manager.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(socket, mgr).await {
                            tracing::debug!("Mixed proxy connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("Mixed proxy accept error: {}", e);
                }
            }
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

/// Detect protocol by sniffing the first byte and delegate to the right handler.
async fn handle_connection(
    mut socket: tokio::net::TcpStream,
    mgr: Arc<ChannelManager>,
) -> Result<()> {
    // Read the first byte to detect protocol
    let mut first_byte = [0u8; 1];
    socket.read_exact(&mut first_byte).await?;

    if first_byte[0] == 0x05 {
        // SOCKS5
        tracing::info!("Mixed proxy: detected SOCKS5");
        socks5::handle_connection(socket, mgr, None, vec![0x05]).await
    } else {
        // HTTP (CONNECT, GET, POST, etc.)
        tracing::info!("Mixed proxy: detected HTTP");
        // Read more data to form a complete request line
        let mut buf = vec![0u8; 8191];
        let n = socket.read(&mut buf).await?;
        let mut peeked = vec![first_byte[0]];
        peeked.extend_from_slice(&buf[..n]);
        http::handle_connection(socket, mgr, peeked).await
    }
}
