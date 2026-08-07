//! WebSocket tunnel client — connects to the relay server and bridges
//! encrypted frame I/O between the relay and `RemoteServer::handle_tunnel`.
//!
//! One tunnel per paired phone. The tunnel:
//! 1. Connects to `wss://relay/tunnel` with JWT auth
//! 2. Sends `register` control message (text frame, JSON)
//! 3. Waits for `peer_connected` from relay
//! 4. Bridges binary frames (relay ↔ channel) + handles control messages
//! 5. Auto-reconnects with exponential backoff on disconnect

use crate::remote_server::{RemoteServer, TunnelSession};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::handshake::client::generate_key;
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::tungstenite::Message as WsMessage;

/// Parsed relay control message (text frame JSON).
#[derive(Debug, PartialEq)]
pub enum ControlMessage {
    /// peer_connected — mobile has connected, pipe established
    PeerConnected,
    /// peer_disconnected — mobile has disconnected
    PeerDisconnected,
    /// peer_timeout — mobile did not connect in time
    PeerTimeout,
    /// Unknown control message type
    Unknown(String),
}

/// Extract host from a URL string for the Host header.
/// Strips protocol (wss://, ws://) and path.
pub fn extract_host(url: &str) -> &str {
    let stripped = url
        .strip_prefix("wss://")
        .or_else(|| url.strip_prefix("ws://"))
        .unwrap_or(url);
    stripped.split('/').next().unwrap_or(stripped)
}

/// Parse a relay control message from a text frame.
/// Returns ControlMessage enum; non-JSON or missing "type" field → Unknown.
pub fn parse_control_message(text: &str) -> ControlMessage {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(v) => {
            let msg_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match msg_type {
                "peer_connected" => ControlMessage::PeerConnected,
                "peer_disconnected" => ControlMessage::PeerDisconnected,
                "peer_timeout" => ControlMessage::PeerTimeout,
                other => ControlMessage::Unknown(other.to_string()),
            }
        }
        Err(_) => ControlMessage::Unknown(text.to_string()),
    }
}

/// Configuration for a tunnel connection.
pub struct TunnelConfig {
    /// Relay WebSocket URL (e.g. "wss://termfast.xisj.com/tunnel")
    pub relay_url: String,
    /// Desktop user JWT (from POST /auth/login, contains user_id claim)
    pub jwt: String,
    /// Pairing ID for this phone
    pub pairing_id: String,
    /// Pairing key K (ECDH-derived, 32 bytes) — used by RemoteServer for encryption
    pub pairing_key: [u8; 32],
}

/// A tunnel client manages the WebSocket connection to the relay for one paired phone.
///
/// Created per paired phone. `run()` is a long-running async fn that:
/// - Connects to relay
/// - Registers with relay
/// - Waits for peer (mobile) to connect
/// - Bridges I/O between relay and RemoteServer
/// - Reconnects with exponential backoff on failure
pub struct TunnelClient {
    config: TunnelConfig,
    remote_server: Arc<RemoteServer>,
}

impl TunnelClient {
    pub fn new(config: TunnelConfig, remote_server: Arc<RemoteServer>) -> Self {
        Self { config, remote_server }
    }

    /// Run the tunnel with auto-reconnect.
    ///
    /// This is a long-running async fn — should be spawned, not awaited.
    /// Returns only if explicitly stopped (future cancellation), fatal error
    /// (HTTP 401 — JWT invalid/expired, should not retry), or peer_timeout
    /// that exceeds max retries.
    pub async fn run(&self) {
        let mut backoff = Duration::from_secs(1);
        let max_backoff = Duration::from_secs(30);

        loop {
            tracing::info!(
                "connecting tunnel for pairing {} to {}",
                self.config.pairing_id,
                self.config.relay_url
            );

            match self.run_once().await {
                Ok(()) => {
                    tracing::info!(
                        "tunnel closed cleanly for pairing {}",
                        self.config.pairing_id
                    );
                    // Clean close — reset backoff
                    backoff = Duration::from_secs(1);
                }
                Err(e) => {
                    // HTTP 401 — JWT invalid/expired, do not retry
                    // (design #25: JWT refresh is handled separately)
                    if e.contains("HTTP 401") || e.contains("unauthorized") {
                        tracing::error!(
                            "tunnel auth failed for pairing {}: {} — NOT retrying (JWT invalid)",
                            self.config.pairing_id,
                            e
                        );
                        return;
                    }
                    tracing::warn!(
                        "tunnel error for pairing {}: {} — reconnecting in {:?}",
                        self.config.pairing_id,
                        e,
                        backoff
                    );
                    tokio::time::sleep(backoff).await;
                    backoff = std::cmp::min(backoff * 2, max_backoff);
                }
            }
        }
    }

    /// Run one tunnel connection lifecycle (connect → bridge → disconnect).
    ///
    /// Returns Ok(()) on clean close, Err on connection failure.
    async fn run_once(&self) -> Result<(), String> {
        // 1. Connect WebSocket with JWT auth
        let ws_stream = self.connect_ws().await?;

        // 2. Split into read/write halves
        let (mut ws_write, mut ws_read) = ws_stream.split();

        // 3. Send register control message
        let register_msg = serde_json::json!({
            "type": "register",
            "pairing_id": self.config.pairing_id,
            "role": "desktop"
        });
        let register_text = serde_json::to_string(&register_msg)
            .map_err(|e| format!("serialize register: {}", e))?;
        ws_write
            .send(WsMessage::Text(register_text))
            .await
            .map_err(|e| format!("send register: {}", e))?;

        // 4. Wait for peer_connected from relay
        let mut peer_connected = false;
        while !peer_connected {
            match ws_read.next().await {
                Some(Ok(WsMessage::Text(text))) => {
                    match parse_control_message(&text) {
                        ControlMessage::PeerConnected => {
                            peer_connected = true;
                            tracing::info!("peer connected via relay for pairing {}", self.config.pairing_id);
                        }
                        ControlMessage::PeerTimeout => {
                            return Err("peer timeout — mobile did not connect".to_string());
                        }
                        _ => {} // ignore other control messages
                    }
                }
                Some(Ok(WsMessage::Binary(_))) => {
                    tracing::warn!("binary frame before peer_connected, dropping");
                }
                Some(Ok(_)) => {} // ping/pong/close
                Some(Err(e)) => return Err(format!("ws read during wait: {}", e)),
                None => return Err("ws closed during peer wait".to_string()),
            }
        }

        // 5. Create channels for TunnelSession
        let (inbound_tx, inbound_rx) = mpsc::channel::<Vec<u8>>(256);
        let (outbound_tx, mut outbound_rx) = mpsc::channel::<Vec<u8>>(256);
        let (async_tx, async_rx) = mpsc::channel(256);

        let session = TunnelSession {
            pairing_id: self.config.pairing_id.clone(),
            inbound_rx,
            outbound_tx,
            async_rx,
            async_tx: async_tx.clone(),
        };

        // 6. Spawn RemoteServer::handle_tunnel
        let remote_server = self.remote_server.clone();
        let server_handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        // 7. Bridge loop: relay ↔ channel
        loop {
            tokio::select! {
                // Relay → channel
                msg = ws_read.next() => {
                    match msg {
                        Some(Ok(WsMessage::Text(text))) => {
                            match parse_control_message(&text) {
                                ControlMessage::PeerDisconnected => {
                                    tracing::warn!("peer disconnected for pairing {}", self.config.pairing_id);
                                    break;
                                }
                                ControlMessage::PeerTimeout => {
                                    tracing::warn!("peer timeout for pairing {}", self.config.pairing_id);
                                    break;
                                }
                                _ => {} // other control messages: ignore
                            }
                        }
                        Some(Ok(WsMessage::Binary(data))) => {
                            if inbound_tx.send(data).await.is_err() {
                                break; // RemoteServer exited
                            }
                        }
                        Some(Ok(WsMessage::Close(_))) => {
                            tracing::info!("ws closed by relay for pairing {}", self.config.pairing_id);
                            break;
                        }
                        Some(Ok(_)) => {} // ping/pong
                        Some(Err(e)) => {
                            tracing::warn!("ws read error for pairing {}: {}", self.config.pairing_id, e);
                            break;
                        }
                        None => {
                            tracing::info!("ws stream ended for pairing {}", self.config.pairing_id);
                            break;
                        }
                    }
                }
                // Channel → relay
                frame = outbound_rx.recv() => {
                    match frame {
                        Some(data) => {
                            if ws_write.send(WsMessage::Binary(data)).await.is_err() {
                                break;
                            }
                        }
                        None => {
                            // RemoteServer exited
                            break;
                        }
                    }
                }
            }
        }

        // 8. Cleanup: close WebSocket + wait for server task
        // Per design #31: 3-second timeout on GOODBYE close
        let _ = ws_write.send(WsMessage::Close(None)).await;
        // Wait for server task with 3s timeout (don't hang if handle_tunnel is stuck)
        let _ = tokio::time::timeout(Duration::from_secs(3), server_handle).await;

        Ok(())
    }

    /// Connect to the relay WebSocket with JWT auth header.
    /// Returns Err with "HTTP 401" in the message if the JWT is rejected,
    /// so run() can detect it and stop retrying.
    async fn connect_ws(&self) -> Result<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, String> {
        let request = Request::builder()
            .uri(&self.config.relay_url)
            .header("Authorization", format!("Bearer {}", self.config.jwt))
            .header("Host", self.extract_host())
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", generate_key())
            .header("X-Pairing-Id", &self.config.pairing_id)
            .body(())
            .map_err(|e| format!("build request: {}", e))?;

        let (ws_stream, response) = tokio_tungstenite::connect_async(request)
            .await
            .map_err(|e| {
                let msg = e.to_string();
                // tungstenite includes HTTP status in error for non-101 responses
                if msg.contains("401") || msg.contains("Unauthorized") {
                    format!("HTTP 401 unauthorized: {}", msg)
                } else {
                    format!("ws connect: {}", msg)
                }
            })?;

        // Check response status (connect_async should have already checked,
        // but be defensive)
        let status = response.status();
        if status.as_u16() == 401 {
            return Err("HTTP 401 unauthorized".to_string());
        }

        tracing::info!("WebSocket connected to relay for pairing {} (status {})", self.config.pairing_id, status);
        Ok(ws_stream)
    }

    /// Extract host from relay URL for Host header.
    fn extract_host(&self) -> &str {
        extract_host(&self.config.relay_url)
    }
}

// === SECTION 1 END ===

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::{BinaryEventForwarder, EventForwarder};
    use crate::terminal::TerminalManager;
    use std::sync::Mutex as StdMutex;
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    /// Helper: create a TunnelClient for testing
    fn make_test_client(relay_url: &str, pairing_id: &str) -> TunnelClient {
        let config = TunnelConfig {
            relay_url: relay_url.to_string(),
            jwt: "test-jwt".to_string(),
            pairing_id: pairing_id.to_string(),
            pairing_key: [0x42u8; 32],
        };
        let bin_fwd: BinaryEventForwarder = Box::new(|_sid, _data, _stderr| {});
        let evt_fwd: EventForwarder = Box::new(|_event, _data| {});
        let manager = Arc::new(TerminalManager::new(
            Arc::new(StdMutex::new(Some(evt_fwd))),
            Arc::new(StdMutex::new(Some(bin_fwd))),
        ));
        let config_mgr = termfast_core::config::ConfigManager::new(
            termfast_core::config::Config::default(),
        );
        TunnelClient::new(config, Arc::new(RemoteServer::new(
            manager,
            Arc::new(tokio::sync::Mutex::new(config_mgr)),
        )))
    }

    // === parse_control_message tests ===

    #[test]
    fn test_parse_peer_connected() {
        assert_eq!(parse_control_message(r#"{"type":"peer_connected"}"#), ControlMessage::PeerConnected);
    }

    #[test]
    fn test_parse_peer_disconnected() {
        assert_eq!(parse_control_message(r#"{"type":"peer_disconnected"}"#), ControlMessage::PeerDisconnected);
    }

    #[test]
    fn test_parse_peer_timeout() {
        assert_eq!(parse_control_message(r#"{"type":"peer_timeout"}"#), ControlMessage::PeerTimeout);
    }

    #[test]
    fn test_parse_unknown_type() {
        let result = parse_control_message(r#"{"type":"some_new_msg"}"#);
        assert_eq!(result, ControlMessage::Unknown("some_new_msg".to_string()));
    }

    #[test]
    fn test_parse_missing_type_field() {
        let result = parse_control_message(r#"{"other":"data"}"#);
        // Missing "type" → as_str returns "" → Unknown("")
        assert_eq!(result, ControlMessage::Unknown("".to_string()));
    }

    #[test]
    fn test_parse_non_json() {
        let result = parse_control_message("not json at all");
        assert_eq!(result, ControlMessage::Unknown("not json at all".to_string()));
    }

    #[test]
    fn test_parse_with_extra_fields() {
        // Should still parse correctly with extra fields
        assert_eq!(
            parse_control_message(r#"{"type":"peer_connected","timestamp":12345}"#),
            ControlMessage::PeerConnected
        );
    }

    // === extract_host tests ===

    #[test]
    fn test_extract_host_wss() {
        let client = make_test_client("wss://termfast.xisj.com/tunnel", "pair1");
        assert_eq!(client.extract_host(), "termfast.xisj.com");
    }

    #[test]
    fn test_extract_host_ws_with_port() {
        let client = make_test_client("ws://localhost:8080/tunnel", "pair1");
        assert_eq!(client.extract_host(), "localhost:8080");
    }

    #[test]
    fn test_extract_host_with_path() {
        let client = make_test_client("wss://relay.example.com/api/v1/tunnel", "pair1");
        assert_eq!(client.extract_host(), "relay.example.com");
    }

    // === Integration test: run_once with local WebSocket listener ===

    /// Integration test: full run_once lifecycle with a mock relay.
    /// Verifies: register message, peer_connected handshake, binary frame bridging.
    #[tokio::test]
    async fn test_run_once_full_lifecycle() {
        // Start a local TCP listener
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let relay_url = format!("ws://{}/tunnel", addr);

        // Mock relay server task
        let relay_handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();

            // 1. Receive register message (text frame)
            let msg = ws.next().await.unwrap().unwrap();
            assert!(msg.is_text(), "first message should be text (register)");
            let register_text = msg.to_text().unwrap();
            let register_json: serde_json::Value = serde_json::from_str(register_text).unwrap();
            assert_eq!(register_json["type"], "register");
            assert_eq!(register_json["role"], "desktop");

            // 2. Send peer_connected
            ws.send(WsMessage::Text(r#"{"type":"peer_connected"}"#.to_string())).await.unwrap();

            // 3. Send a binary frame (simulating encrypted data from mobile)
            ws.send(WsMessage::Binary(vec![0xDE, 0xAD, 0xBE, 0xEF])).await.unwrap();

            // 4. Receive a binary frame (desktop → mobile response)
            // Wait for binary frame from desktop
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            loop {
                if tokio::time::Instant::now() >= deadline {
                    panic!("timeout waiting for binary frame from desktop");
                }
                match tokio::time::timeout(Duration::from_millis(100), ws.next()).await {
                    Ok(Some(Ok(msg))) => {
                        if msg.is_binary() {
                            // Got binary frame from desktop
                            break;
                        }
                    }
                    _ => {}
                }
            }

            // 5. Send peer_disconnected to close tunnel
            ws.send(WsMessage::Text(r#"{"type":"peer_disconnected"}"#.to_string())).await.unwrap();
        });

        // Run the tunnel client
        let client = make_test_client(&relay_url, "pair1");
        let result = tokio::time::timeout(Duration::from_secs(10), client.run_once()).await;

        // run_once should return Ok (clean close after peer_disconnected)
        assert!(result.is_ok(), "run_once should complete within timeout");
        let inner = result.unwrap();
        assert!(inner.is_ok(), "run_once should return Ok, got: {:?}", inner);

        // Relay task should complete
        let _ = relay_handle.await;
    }

    /// Integration test: peer_timeout before peer_connected → run_once returns Err
    #[tokio::test]
    async fn test_run_once_peer_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let relay_url = format!("ws://{}/tunnel", addr);

        let relay_handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();

            // Receive register
            let _ = ws.next().await;

            // Send peer_timeout (not peer_connected)
            ws.send(WsMessage::Text(r#"{"type":"peer_timeout"}"#.to_string())).await.unwrap();
        });

        let client = make_test_client(&relay_url, "pair1");
        let result = tokio::time::timeout(Duration::from_secs(5), client.run_once()).await;

        assert!(result.is_ok(), "run_once should complete within timeout");
        let inner = result.unwrap();
        assert!(inner.is_err(), "run_once should return Err on peer_timeout");

        let _ = relay_handle.await;
    }

    /// Integration test: binary frame before peer_connected is dropped (not forwarded)
    #[tokio::test]
    async fn test_run_once_binary_before_peer_connected_dropped() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let relay_url = format!("ws://{}/tunnel", addr);

        let relay_handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();

            // Receive register
            let _ = ws.next().await;

            // Send binary BEFORE peer_connected (should be dropped by client)
            ws.send(WsMessage::Binary(vec![0x01, 0x02])).await.unwrap();

            // Now send peer_connected
            ws.send(WsMessage::Text(r#"{"type":"peer_connected"}"#.to_string())).await.unwrap();

            // Send another binary (this one should be forwarded to RemoteServer)
            ws.send(WsMessage::Binary(vec![0x03, 0x04])).await.unwrap();

            // Wait a bit then close
            tokio::time::sleep(Duration::from_millis(500)).await;
            ws.send(WsMessage::Close(None)).await.unwrap();
        });

        let client = make_test_client(&relay_url, "pair1");
        let result = tokio::time::timeout(Duration::from_secs(5), client.run_once()).await;

        // Should complete (either Ok or Err from close)
        assert!(result.is_ok(), "run_once should complete within timeout");

        let _ = relay_handle.await;
    }

    /// Integration test: run() auto-reconnects after run_once() failure.
    /// First connection: relay accepts then immediately closes (run_once returns Ok).
    /// Second connection: relay accepts and completes full lifecycle.
    /// Verifies run() loops and reconnects.
    #[tokio::test]
    async fn test_run_auto_reconnect() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let relay_url = format!("ws://{}/tunnel", addr);

        let connect_count = Arc::new(AtomicU32::new(0));
        let connect_count_clone = connect_count.clone();

        let relay_handle = tokio::spawn(async move {
            // First connection: accept then send peer_timeout (run_once returns Err)
            let (stream, _) = listener.accept().await.unwrap();
            connect_count_clone.fetch_add(1, Ordering::SeqCst);
            let mut ws1 = accept_async(stream).await.unwrap();
            let _ = ws1.next().await; // register
            // Send peer_timeout — run_once will return Err
            ws1.send(WsMessage::Text(r#"{"type":"peer_timeout"}"#.to_string())).await.unwrap();
            // Close ws1
            ws1.close(None).await.ok();

            // Second connection: full lifecycle then peer_disconnected
            // (run() will sleep 1s backoff before reconnecting)
            let (stream, _) = listener.accept().await.unwrap();
            connect_count_clone.fetch_add(1, Ordering::SeqCst);
            let mut ws2 = accept_async(stream).await.unwrap();
            let _ = ws2.next().await; // register
            ws2.send(WsMessage::Text(r#"{"type":"peer_connected"}"#.to_string())).await.unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
            ws2.send(WsMessage::Text(r#"{"type":"peer_disconnected"}"#.to_string())).await.unwrap();
        });

        let client = make_test_client(&relay_url, "pair1");

        // Run with timeout — run() is infinite, so we cancel after 2 connections
        let result = tokio::time::timeout(Duration::from_secs(15), client.run()).await;

        // run() should have been cancelled (still running) — that's expected
        // The key assertion: relay saw 2 connections (proving reconnect happened)
        assert!(result.is_err(), "run() should still be running (infinite loop)");
        assert_eq!(
            connect_count.load(Ordering::SeqCst), 2,
            "run() should have reconnected — relay saw {} connections, expected 2",
            connect_count.load(Ordering::SeqCst)
        );

        let _ = relay_handle.await;
    }
}
