//! HTTP proxy server — FP-3.3
//!
//! Two-phase processing:
//! - Plain HTTP GET: parse request, extract target, open channel, forward
//! - HTTPS CONNECT: open channel, return 200, transparent TLS forwarding

use crate::error::{Error, ErrorCode, IpcError, Result};
use crate::proxy::manager::ChannelManager;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// HTTP proxy server
pub struct HttpProxyServer {
    port: u16,
    channel_manager: Arc<ChannelManager>,
}

impl HttpProxyServer {
    pub fn new(port: u16, channel_manager: Arc<ChannelManager>) -> Self {
        Self {
            port,
            channel_manager,
        }
    }

    /// Start the HTTP proxy server
    pub async fn start(&self) -> Result<()> {
        let addr = format!("127.0.0.1:{}", self.port);
        let listener = TcpListener::bind(&addr).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::AddrInUse {
                Error::Ipc(IpcError::new(
                    ErrorCode::ProxyPortInUse,
                    format!("HTTP proxy port {} is in use", self.port),
                ))
            } else {
                Error::Io(e)
            }
        })?;

        tracing::info!("HTTP proxy listening on {}", addr);

        loop {
            match listener.accept().await {
                Ok((socket, _)) => {
                    let mgr = self.channel_manager.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(socket, mgr, Vec::new()).await {
                            tracing::debug!("HTTP proxy connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("HTTP proxy accept error: {}", e);
                }
            }
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

/// Handle a single HTTP proxy connection.
/// `peeked` contains bytes already read from the socket (e.g., from mixed proxy).
pub async fn handle_connection(
    mut socket: tokio::net::TcpStream,
    mgr: Arc<ChannelManager>,
    peeked: Vec<u8>,
) -> Result<()> {
    // Read the request — combine peeked data with new data
    let mut buf = vec![0u8; 8192];
    let n = if !peeked.is_empty() {
        let len = peeked.len().min(buf.len());
        buf[..len].copy_from_slice(&peeked[..len]);
        let extra = socket.read(&mut buf[len..]).await?;
        len + extra
    } else {
        socket.read(&mut buf).await?
    };
    if n == 0 {
        return Ok(());
    }

    let request = String::from_utf8_lossy(&buf[..n]);
    let first_line = request.lines().next().unwrap_or("");

    if first_line.starts_with("CONNECT ") {
        // HTTPS CONNECT
        handle_connect(&mut socket, &mgr, &buf[..n], first_line).await
    } else if first_line.starts_with("GET ")
        || first_line.starts_with("POST ")
        || first_line.starts_with("PUT ")
        || first_line.starts_with("DELETE ")
        || first_line.starts_with("HEAD ")
        || first_line.starts_with("PATCH ")
    {
        // Plain HTTP request
        handle_http(&mut socket, &mgr, &buf[..n], first_line).await
    } else {
        // Bad request
        let response = "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n";
        socket.write_all(response.as_bytes()).await?;
        Err(Error::Other(format!(
            "invalid HTTP request: {}",
            first_line
        )))
    }
}

/// Handle HTTPS CONNECT method
async fn handle_connect(
    socket: &mut tokio::net::TcpStream,
    mgr: &ChannelManager,
    _initial_data: &[u8],
    first_line: &str,
) -> Result<()> {
    // Parse "CONNECT host:port HTTP/1.1"
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        let response = "HTTP/1.1 400 Bad Request\r\n\r\n";
        socket.write_all(response.as_bytes()).await?;
        return Err(Error::Other("invalid CONNECT request".into()));
    }

    let target = parts[1];
    let (host, port) = parse_host_port(target, 443)?;

    tracing::info!("HTTP CONNECT {} : {}", host, port);

    let managed = match mgr.open(&host, port).await {
        Ok(m) => {
            tracing::info!("HTTP CONNECT {}:{} channel opened", host, port);
            m
        }
        Err(e) => {
            tracing::warn!(
                "HTTP CONNECT {}:{} failed to open channel: {}",
                host,
                port,
                e
            );
            let response = "HTTP/1.1 502 Bad Gateway\r\n\r\n";
            let _ = socket.write_all(response.as_bytes()).await;
            return Err(e);
        }
    };

    // Send 200 Connection Established
    let response = "HTTP/1.1 200 Connection Established\r\n\r\n";
    socket.write_all(response.as_bytes()).await?;

    // Bidirectional forwarding
    let idle_timeout = managed.idle_timeout();
    let active_clients = managed.active_clients_clone();
    let (bytes_in, bytes_out) = managed.byte_counters();
    let channel = managed.channel;
    let (cli_read, mut cli_write) = socket.split();
    let (mut chan_read, chan_write) = channel.split();
    let chan_reader = chan_read.make_reader();
    let mut chan_writer = chan_write.make_writer();

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
            tracing::debug!("HTTP channel closed or idle timeout {:?}", idle_timeout);
        }
        _ = channel_to_client => {
            tracing::debug!("HTTP channel closed or idle timeout {:?}", idle_timeout);
        }
    }

    active_clients.fetch_sub(1, Ordering::Relaxed);
    Ok(())
}

/// Handle plain HTTP request (GET, POST, etc.)
async fn handle_http(
    socket: &mut tokio::net::TcpStream,
    mgr: &ChannelManager,
    initial_data: &[u8],
    first_line: &str,
) -> Result<()> {
    // Parse the target URL from the request line
    // e.g., "GET http://example.com/path HTTP/1.1"
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        let response = "HTTP/1.1 400 Bad Request\r\n\r\n";
        socket.write_all(response.as_bytes()).await?;
        return Err(Error::Other("invalid HTTP request".into()));
    }

    let url = parts[1];
    let (host, port, path) = parse_http_url(url)?;

    tracing::debug!("HTTP {} {} : {} {}", parts[0], host, port, path);

    let managed = match mgr.open(&host, port).await {
        Ok(m) => m,
        Err(e) => {
            let response = "HTTP/1.1 502 Bad Gateway\r\n\r\n";
            let _ = socket.write_all(response.as_bytes()).await;
            return Err(e);
        }
    };

    let idle_timeout = managed.idle_timeout();
    let active_clients = managed.active_clients_clone();
    let (bytes_in, bytes_out) = managed.byte_counters();
    let channel = managed.channel;

    // Rewrite the request line to use relative path
    let request_str = String::from_utf8_lossy(initial_data);
    let rewritten = rewrite_http_request(&request_str, &path);

    // Count the initial request bytes as upload
    bytes_in.fetch_add(rewritten.len() as u64, Ordering::Relaxed);

    // Send rewritten request to remote
    channel
        .data_bytes(rewritten.as_bytes().to_vec())
        .await
        .map_err(|e| Error::Ssh(format!("failed to send data: {}", e)))?;

    // Bidirectional forwarding
    let (cli_read, mut cli_write) = socket.split();
    let (mut chan_read, chan_write) = channel.split();
    let chan_reader = chan_read.make_reader();
    let mut chan_writer = chan_write.make_writer();

    let mut counting_cli_read = crate::proxy::manager::CountingReader {
        inner: cli_read,
        counter: bytes_in,
    };
    let mut counting_chan_reader = crate::proxy::manager::CountingReader {
        inner: chan_reader,
        counter: bytes_out,
    };

    // Forward remaining client data (if any) and response
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
            tracing::debug!("HTTP channel closed or idle timeout {:?}", idle_timeout);
        }
        _ = channel_to_client => {
            tracing::debug!("HTTP channel closed or idle timeout {:?}", idle_timeout);
        }
    }

    active_clients.fetch_sub(1, Ordering::Relaxed);
    Ok(())
}

/// Parse host:port from a CONNECT target
fn parse_host_port(target: &str, default_port: u16) -> Result<(String, u16)> {
    if let Some(colon) = target.rfind(':') {
        let host = &target[..colon];
        let port_str = &target[colon + 1..];
        // Handle IPv6 addresses with brackets
        let host = host.trim_start_matches('[').trim_end_matches(']');
        let port: u16 = port_str.parse().unwrap_or(default_port);
        Ok((host.to_string(), port))
    } else {
        Ok((target.to_string(), default_port))
    }
}

/// Parse an HTTP proxy URL (e.g., "http://example.com:80/path")
fn parse_http_url(url: &str) -> Result<(String, u16, String)> {
    let url = url.strip_prefix("http://").unwrap_or(url);
    let (host_port, path) = if let Some(slash) = url.find('/') {
        (&url[..slash], &url[slash..])
    } else {
        (url, "/")
    };

    let (host, port) = parse_host_port(host_port, 80)?;
    Ok((host, port, path.to_string()))
}

/// Rewrite HTTP request to use relative path instead of absolute URL.
/// Preserves the original \r\n line endings and ensures the header/body separator is intact.
fn rewrite_http_request(request: &str, path: &str) -> String {
    // Find the request line (first line)
    let line_end = request.find("\r\n").unwrap_or(request.len());
    let first_line = &request[..line_end];
    let parts: Vec<&str> = first_line.splitn(3, ' ').collect();
    if parts.len() != 3 {
        return request.to_string();
    }
    let new_first_line = format!("{} {} {}", parts[0], path, parts[2]);
    // Replace only the first line, keep the rest of the request (headers + body) as-is
    if line_end < request.len() {
        format!("{}{}", new_first_line, &request[line_end..])
    } else {
        format!("{}\r\n\r\n", new_first_line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_host_port_with_port() {
        let (host, port) = parse_host_port("example.com:8080", 443).unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(port, 8080);
    }

    #[test]
    fn test_parse_host_port_without_port() {
        let (host, port) = parse_host_port("example.com", 443).unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(port, 443);
    }

    #[test]
    fn test_parse_http_url_with_path() {
        let (host, port, path) = parse_http_url("http://example.com/path/to/page").unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(port, 80);
        assert_eq!(path, "/path/to/page");
    }

    #[test]
    fn test_parse_http_url_with_port() {
        let (host, port, path) = parse_http_url("http://example.com:8080/page").unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(port, 8080);
        assert_eq!(path, "/page");
    }

    #[test]
    fn test_parse_http_url_no_path() {
        let (host, port, path) = parse_http_url("http://example.com").unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(port, 80);
        assert_eq!(path, "/");
    }

    #[test]
    fn test_rewrite_http_request() {
        let request = "GET http://example.com/path HTTP/1.1\r\nHost: example.com\r\n\r\n";
        let rewritten = rewrite_http_request(request, "/path");
        assert!(rewritten.starts_with("GET /path HTTP/1.1"));
        assert!(!rewritten.contains("http://example.com/path"));
        // Must end with \r\n\r\n (header terminator)
        assert!(
            rewritten.ends_with("\r\n\r\n"),
            "rewritten must end with \\r\\n\\r\\n, got: {:?}",
            rewritten
        );
    }

    #[test]
    fn test_rewrite_http_request_preserves_body() {
        let request = "POST http://example.com/api HTTP/1.1\r\nHost: example.com\r\nContent-Length: 5\r\n\r\nhello";
        let rewritten = rewrite_http_request(request, "/api");
        assert!(rewritten.starts_with("POST /api HTTP/1.1"));
        assert!(rewritten.ends_with("hello"));
        assert!(rewritten.contains("\r\n\r\nhello"));
    }

    #[test]
    fn test_http_proxy_server_creation() {
        use crate::ssh::channel_opener::SshChannelOpener;
        let opener = Arc::new(SshChannelOpener::empty());
        let mgr = Arc::new(ChannelManager::new(opener, 64, 300));
        let server = HttpProxyServer::new(8080, mgr);
        assert_eq!(server.port, 8080);
    }
}
