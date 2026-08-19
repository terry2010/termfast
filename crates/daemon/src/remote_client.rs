//! Remote client — Desktop A as client connecting to Desktop B (server) via relay.
//!
//! This is the client-side counterpart to `tunnel_client.rs` (which is the server side).
//! The RemoteClient:
//! 1. Connects to relay with a pairing JWT (scope=tunnel) — relay treats it as "mobile" role
//! 2. Sends `connect` control message (not `register` — that's for the server side)
//! 3. Waits for `peer_connected` from relay
//! 4. Performs HELLO exchange (client side: send HELLO, receive server HELLO, derive session key)
//! 5. Sends LIST/SUBSCRIBE/INPUT/RESIZE frames (encrypted)
//! 6. Receives OUTPUT/HISTORY/NOTIFY/ERROR frames (decrypted) and forwards via callback
//! 7. Auto-reconnects with exponential backoff on disconnect
//!
//! B5: The relay determines role from JWT scope, not from the control message `role` field.
//!     pairing_jwt has scope=tunnel → relay assigns mobile (client) role.

use crate::frame_crypto::{
    generate_random_32, FrameCipher, DIR_DESKTOP_TO_MOBILE, DIR_MOBILE_TO_DESKTOP,
};
use crate::remote_frame::{self, Frame};
use crate::tunnel_client::parse_control_message;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::handshake::client::generate_key;
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::tungstenite::Message as WsMessage;

/// Capabilities advertised in HELLO (same as mobile client).
const CLIENT_CAPABILITIES: u16 = 0x0001;

/// Configuration for a RemoteClient connection.
pub struct RemoteClientConfig {
    /// Relay WebSocket URL (e.g. "wss://termfast.xisj.com/tunnel")
    pub relay_url: String,
    /// Pairing JWT (scope=tunnel) — relay uses this to identify us as the client (mobile role).
    pub pairing_jwt: String,
    /// Pairing ID for this desktop-to-desktop pairing.
    pub pairing_id: String,
    /// 32-byte pairing key K (shared secret, used for HELLO encryption + session key derivation).
    pub pairing_key: [u8; 32],
}

/// Callback for receiving decrypted frames from the remote desktop (server).
/// The callback receives (pairing_id, frame_type, terminal_id, payload).
pub type FrameCallback = Arc<
    dyn Fn(&str, u8, u32, &[u8]) + Send + Sync,
>;

/// Callback for connection state changes.
/// (pairing_id, connected: bool)
pub type StateCallback = Arc<dyn Fn(&str, bool) + Send + Sync>;

/// Manages RemoteClient instances for all desktop-to-desktop pairings
/// where this desktop is the client (Desktop A).
/// One RemoteClient per pairing_id. Auto-reconnects on disconnect.
pub struct RemoteClientManager {
    /// Active client tasks: pairing_id → JoinHandle
    clients: Arc<Mutex<std::collections::HashMap<String, tokio::task::JoinHandle<()>>>>,
    /// Send channels: pairing_id → mpsc::Sender for sending frames to the ws task
    senders: Arc<Mutex<std::collections::HashMap<String, tokio::sync::mpsc::Sender<OutboundFrame>>>>,
    /// Connected state: pairing_id → bool (true if HELLO completed)
    connected: Arc<Mutex<std::collections::HashSet<String>>>,
}

/// An outbound frame to be encrypted and sent via WebSocket.
#[derive(Debug)]
pub enum OutboundFrame {
    List,
    Subscribe(u32),
    Unsubscribe(u32),
    Input(u32, Vec<u8>),
    Resize(u32, u16, u16),
    Goodbye,
    InfoRequest,
    NewTerminal { shell: Option<String>, name: Option<String> },
    CloseTerminal(u32),
}

impl RemoteClientManager {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(Mutex::new(std::collections::HashMap::new())),
            senders: Arc::new(Mutex::new(std::collections::HashMap::new())),
            connected: Arc::new(Mutex::new(std::collections::HashSet::new())),
        }
    }

    /// Check if a remote client for the given pairing_id is connected (HELLO completed).
    pub async fn is_connected(&self, pairing_id: &str) -> bool {
        self.connected.lock().await.contains(pairing_id)
    }

    /// Start a remote client for a desktop-to-desktop pairing.
    /// `frame_callback` is called when decrypted frames arrive from the remote desktop.
    /// `state_callback` is called when connection state changes (connected/disconnected).
    pub async fn start_client<F, S>(
        &self,
        config: RemoteClientConfig,
        frame_callback: F,
        state_callback: S,
    ) -> Result<(), String>
    where
        F: Fn(&str, u8, u32, &[u8]) + Send + Sync + 'static,
        S: Fn(&str, bool) + Send + Sync + 'static,
    {
        let pairing_id = config.pairing_id.clone();

        // Stop existing client for this pairing_id
        self.stop_client(&pairing_id).await;

        let (tx, rx) = tokio::sync::mpsc::channel::<OutboundFrame>(256);
        self.senders.lock().await.insert(pairing_id.clone(), tx.clone());

        let frame_cb: FrameCallback = Arc::new(frame_callback);
        let state_cb: StateCallback = Arc::new(state_callback);
        let pairing_id_clone = pairing_id.clone();
        let senders = self.senders.clone();
        let connected_set = self.connected.clone();

        let handle = tokio::spawn(async move {
            run_client_loop(config, frame_cb, state_cb, rx, connected_set).await;
            senders.lock().await.remove(&pairing_id_clone);
        });

        self.clients.lock().await.insert(pairing_id, handle);
        Ok(())
    }

    /// Stop a remote client.
    pub async fn stop_client(&self, pairing_id: &str) {
        if let Some(handle) = self.clients.lock().await.remove(pairing_id) {
            handle.abort();
        }
        self.senders.lock().await.remove(pairing_id);
        self.connected.lock().await.remove(pairing_id);
    }

    /// Send a frame to a specific remote client (by pairing_id).
    pub async fn send_frame(&self, pairing_id: &str, frame: OutboundFrame) -> Result<(), String> {
        let senders = self.senders.lock().await;
        let tx = senders
            .get(pairing_id)
            .ok_or_else(|| format!("no remote client for pairing_id={}", pairing_id))?;
        tracing::info!("remote_client: send_frame pairing={} type={:?}", pairing_id, frame);
        tx.send(frame).await.map_err(|e| format!("send: {}", e))
    }

    /// Stop all clients (on app shutdown).
    pub async fn stop_all(&self) {
        let mut clients = self.clients.lock().await;
        for (id, handle) in clients.drain() {
            handle.abort();
            tracing::info!("Stopped remote client for pairing {} (shutdown)", id);
        }
        self.senders.lock().await.clear();
    }
}

impl Default for RemoteClientManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Run the client loop: connect → HELLO → bridge + outbound frame sending.
/// Auto-reconnects on disconnect.
async fn run_client_loop(
    config: RemoteClientConfig,
    frame_cb: FrameCallback,
    state_cb: StateCallback,
    mut rx: tokio::sync::mpsc::Receiver<OutboundFrame>,
    connected_set: Arc<Mutex<std::collections::HashSet<String>>>,
) {
    let mut backoff = std::time::Duration::from_secs(1);
    let max_backoff = std::time::Duration::from_secs(30);

    loop {
        tracing::info!(
            "remote_client_loop: connecting for pairing {} to {}",
            config.pairing_id,
            config.relay_url
        );

        match run_client_once(&config, &frame_cb, &state_cb, &mut rx, &connected_set).await {
            Ok(()) => {
                tracing::info!(
                    "remote_client_loop: closed cleanly for pairing {}",
                    config.pairing_id
                );
                // Remove from connected set on clean close
                connected_set.lock().await.remove(&config.pairing_id);
                // Sleep before reconnecting to avoid tight loop if relay closes immediately
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                backoff = std::time::Duration::from_secs(1);
            }
            Err(e) => {
                if e.contains("HTTP 401") || e.contains("unauthorized") {
                    tracing::error!(
                        "remote_client_loop: auth failed for pairing {}: {} — NOT retrying",
                        config.pairing_id,
                        e
                    );
                    connected_set.lock().await.remove(&config.pairing_id);
                    return;
                }
                tracing::warn!(
                    "remote_client_loop: error for pairing {}: {} — reconnecting in {:?}",
                    config.pairing_id,
                    e,
                    backoff
                );
                tokio::time::sleep(backoff).await;
                backoff = std::cmp::min(backoff * 2, max_backoff);
            }
        }
    }
}

// === SECTION 3 END ===

/// Run one connection lifecycle: connect → HELLO → bridge (inbound + outbound) → disconnect.
async fn run_client_once(
    config: &RemoteClientConfig,
    frame_cb: &FrameCallback,
    state_cb: &StateCallback,
    rx: &mut tokio::sync::mpsc::Receiver<OutboundFrame>,
    connected_set: &Arc<Mutex<std::collections::HashSet<String>>>,
) -> Result<(), String> {
    // 1. Connect WebSocket
    let host = crate::tunnel_client::extract_host(&config.relay_url);
    let auth_header = format!("Bearer {}", config.pairing_jwt);
    let request = Request::builder()
        .method("GET")
        .uri(&config.relay_url)
        .header("Host", host)
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", generate_key())
        .header("Authorization", &auth_header)
        .body(())
        .map_err(|e| format!("build request: {}", e))?;

    let ws_stream = tokio_tungstenite::connect_async(request)
        .await
        .map(|(ws, _)| ws)
        .map_err(|e| format!("ws connect: {}", e))?;

    // 2. Send connect control message
    let (mut ws_write, mut ws_read) = ws_stream.split();
    let connect_msg = serde_json::json!({
        "type": "connect",
        "pairing_id": config.pairing_id,
    });
    ws_write
        .send(WsMessage::Text(serde_json::to_string(&connect_msg).unwrap()))
        .await
        .map_err(|e| format!("send connect: {}", e))?;

    // 3. Wait for peer_connected
    let mut peer_connected = false;
    while !peer_connected {
        match ws_read.next().await {
            Some(Ok(WsMessage::Text(text))) => {
                match parse_control_message(&text) {
                    crate::tunnel_client::ControlMessage::PeerConnected => {
                        peer_connected = true;
                    }
                    crate::tunnel_client::ControlMessage::PeerTimeout => {
                        return Err("peer timeout".to_string());
                    }
                    _ => {}
                }
            }
            Some(Ok(WsMessage::Binary(_))) => {}
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(format!("ws wait: {}", e)),
            None => return Err("ws closed during wait".to_string()),
        }
    }

    // 4. HELLO exchange (client side)
    let client_random = generate_random_32();
    let hello_frame = Frame::hello(CLIENT_CAPABILITIES, &client_random);
    let mut hello_cipher =
        FrameCipher::from_pairing_key(&config.pairing_key, DIR_MOBILE_TO_DESKTOP);
    let encrypted_hello = hello_cipher.encrypt(&hello_frame.serialize())?;
    ws_write
        .send(WsMessage::Binary(encrypted_hello))
        .await
        .map_err(|e| format!("send HELLO: {}", e))?;

    // Receive server HELLO
    let encrypted_response = loop {
        match ws_read.next().await {
            Some(Ok(WsMessage::Binary(data))) => break data,
            Some(Ok(WsMessage::Text(text))) => {
                if let crate::tunnel_client::ControlMessage::PeerDisconnected = parse_control_message(&text) {
                    return Err("peer disconnected during HELLO".to_string());
                }
            }
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(format!("ws read HELLO: {}", e)),
            None => return Err("ws closed during HELLO".to_string()),
        }
    };

    let resp_cipher =
        FrameCipher::from_pairing_key(&config.pairing_key, DIR_DESKTOP_TO_MOBILE);
    let plaintext = resp_cipher.decrypt(&encrypted_response)?;
    let server_hello = Frame::deserialize(&plaintext).map_err(|e| format!("deserialize: {}", e))?;
    if server_hello.frame_type != remote_frame::HELLO {
        return Err(format!("expected HELLO, got 0x{:02X}", server_hello.frame_type));
    }
    let (_server_caps, server_random) = server_hello
        .parse_hello()
        .ok_or_else(|| "invalid HELLO payload".to_string())?;

    let mut send_cipher = FrameCipher::from_session_key(
        &config.pairing_key,
        &client_random,
        &server_random,
        DIR_MOBILE_TO_DESKTOP,
    );
    let recv_cipher = FrameCipher::from_session_key(
        &config.pairing_key,
        &client_random,
        &server_random,
        DIR_DESKTOP_TO_MOBILE,
    );

    tracing::info!("remote_client: HELLO complete for pairing {}", config.pairing_id);
    connected_set.lock().await.insert(config.pairing_id.clone());
    state_cb(&config.pairing_id, true);

    // 5. Bridge loop: inbound (ws_read → decrypt → callback) + outbound (rx → encrypt → ws_write)
    loop {
        tokio::select! {
            msg = ws_read.next() => {
                match msg {
                    Some(Ok(WsMessage::Text(text))) => {
                        match parse_control_message(&text) {
                            crate::tunnel_client::ControlMessage::PeerDisconnected => break,
                            crate::tunnel_client::ControlMessage::PeerTimeout => break,
                            _ => {}
                        }
                    }
                    Some(Ok(WsMessage::Binary(data))) => {
                        let pt = match recv_cipher.decrypt(&data) {
                            Ok(pt) => pt,
                            Err(e) => {
                                tracing::warn!("remote_client: decrypt error: {}", e);
                                break;
                            }
                        };
                        let frame = match Frame::deserialize(&pt) {
                            Ok(f) => f,
                            Err(e) => {
                                tracing::warn!("remote_client: deserialize error: {}", e);
                                break;
                            }
                        };
                        frame_cb(&config.pairing_id, frame.frame_type, frame.terminal_id, &frame.payload);
                    }
                    Some(Ok(WsMessage::Close(_))) => break,
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        tracing::warn!("remote_client: ws error: {}", e);
                        break;
                    }
                    None => break,
                }
            }
            frame = rx.recv() => {
                match frame {
                    Some(outbound) => {
                        let proto_frame = match outbound {
                            OutboundFrame::List => Frame::list_request(),
                            OutboundFrame::Subscribe(tid) => Frame::subscribe(tid),
                            OutboundFrame::Unsubscribe(tid) => Frame::unsubscribe(tid),
                            OutboundFrame::Input(tid, data) => Frame::input(tid, &data),
                            OutboundFrame::Resize(tid, cols, rows) => Frame::resize(tid, cols, rows),
                            OutboundFrame::Goodbye => Frame::goodbye(),
                            OutboundFrame::InfoRequest => Frame::info_request(),
                            OutboundFrame::NewTerminal { shell, name } => {
                                Frame::new_terminal(shell.as_deref(), name.as_deref())
                            }
                            OutboundFrame::CloseTerminal(tid) => Frame::close_terminal(tid),
                        };
                        let encrypted = match send_cipher.encrypt(&proto_frame.serialize()) {
                            Ok(data) => data,
                            Err(e) => {
                                tracing::warn!("remote_client: encrypt error: {}", e);
                                break;
                            }
                        };
                        if let Err(e) = ws_write.send(WsMessage::Binary(encrypted)).await {
                            tracing::warn!("remote_client: send error: {}", e);
                            break;
                        }
                    }
                    None => {
                        // Manager dropped the sender — stop
                        break;
                    }
                }
            }
        }
    }

    state_cb(&config.pairing_id, false);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test RemoteClientManager creation and basic state
    #[tokio::test]
    async fn test_remote_client_manager_new() {
        let mgr = RemoteClientManager::new();
        // Should start with no clients
        assert!(mgr.clients.lock().await.is_empty());
        assert!(mgr.senders.lock().await.is_empty());
    }

    /// Test RemoteClientManager default() equals new()
    #[tokio::test]
    async fn test_remote_client_manager_default() {
        let mgr = RemoteClientManager::default();
        assert!(mgr.clients.lock().await.is_empty());
    }

    /// Test RemoteClientManager stop_client on non-existent pairing (no-op)
    #[tokio::test]
    async fn test_remote_client_manager_stop_nonexistent() {
        let mgr = RemoteClientManager::new();
        // Should not panic
        mgr.stop_client("nonexistent").await;
    }

    /// Test RemoteClientManager stop_all on empty manager
    #[tokio::test]
    async fn test_remote_client_manager_stop_all_empty() {
        let mgr = RemoteClientManager::new();
        mgr.stop_all().await;
        assert!(mgr.clients.lock().await.is_empty());
    }

    /// Test RemoteClientManager send_frame to non-existent client returns error
    #[tokio::test]
    async fn test_remote_client_manager_send_nonexistent() {
        let mgr = RemoteClientManager::new();
        let result = mgr.send_frame("nonexistent", OutboundFrame::List).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no remote client"));
    }

    /// Test OutboundFrame enum variants can be constructed
    #[test]
    fn test_outbound_frame_variants() {
        let _list = OutboundFrame::List;
        let _sub = OutboundFrame::Subscribe(42);
        let _unsub = OutboundFrame::Unsubscribe(42);
        let _input = OutboundFrame::Input(42, vec![0x41, 0x42]);
        let _resize = OutboundFrame::Resize(42, 80, 24);
        let _goodbye = OutboundFrame::Goodbye;
    }

    /// Test OutboundFrame → Frame conversion produces correct frame types
    #[test]
    fn test_outbound_frame_to_frame_conversion() {
        use crate::remote_frame;

        let frame = Frame::list_request();
        assert_eq!(frame.frame_type, remote_frame::LIST_REQUEST);

        let frame = Frame::subscribe(42);
        assert_eq!(frame.frame_type, remote_frame::SUBSCRIBE);

        let frame = Frame::unsubscribe(42);
        assert_eq!(frame.frame_type, remote_frame::UNSUBSCRIBE);

        let frame = Frame::input(42, b"test");
        assert_eq!(frame.frame_type, remote_frame::INPUT);

        let frame = Frame::resize(42, 80, 24);
        assert_eq!(frame.frame_type, remote_frame::RESIZE);

        let frame = Frame::goodbye();
        assert_eq!(frame.frame_type, remote_frame::GOODBYE);
    }

    /// Test RemoteClientConfig construction
    #[test]
    fn test_remote_client_config_construction() {
        let config = RemoteClientConfig {
            relay_url: "wss://relay.example.com/tunnel".to_string(),
            pairing_jwt: "jwt-token".to_string(),
            pairing_id: "pair-123".to_string(),
            pairing_key: [0x42u8; 32],
        };
        assert_eq!(config.relay_url, "wss://relay.example.com/tunnel");
        assert_eq!(config.pairing_jwt, "jwt-token");
        assert_eq!(config.pairing_id, "pair-123");
        assert_eq!(config.pairing_key, [0x42u8; 32]);
    }

    /// Test HELLO exchange logic: client generates HELLO, server responds,
    /// both derive the same session key.
    #[test]
    fn test_hello_session_key_derivation() {
        use crate::frame_crypto::{
            FrameCipher, DIR_DESKTOP_TO_MOBILE, DIR_MOBILE_TO_DESKTOP,
        };

        let pairing_key = [0x42u8; 32];
        let client_random = generate_random_32();
        let server_random = generate_random_32();

        // Client creates send/recv ciphers
        let mut client_send = FrameCipher::from_session_key(
            &pairing_key,
            &client_random,
            &server_random,
            DIR_MOBILE_TO_DESKTOP,
        );
        let client_recv = FrameCipher::from_session_key(
            &pairing_key,
            &client_random,
            &server_random,
            DIR_DESKTOP_TO_MOBILE,
        );

        // Server creates send/recv ciphers (opposite directions)
        let mut server_send = FrameCipher::from_session_key(
            &pairing_key,
            &client_random,
            &server_random,
            DIR_DESKTOP_TO_MOBILE,
        );
        let server_recv = FrameCipher::from_session_key(
            &pairing_key,
            &client_random,
            &server_random,
            DIR_MOBILE_TO_DESKTOP,
        );

        // Client encrypts a frame, server decrypts it
        let test_frame = Frame::list_request();
        let encrypted = client_send.encrypt(&test_frame.serialize()).unwrap();
        let decrypted = server_recv.decrypt(&encrypted).unwrap();
        let decoded = Frame::deserialize(&decrypted).unwrap();
        assert_eq!(decoded.frame_type, crate::remote_frame::LIST_REQUEST);

        // Server encrypts a frame, client decrypts it
        let resp_frame = Frame::ok(42);
        let encrypted = server_send.encrypt(&resp_frame.serialize()).unwrap();
        let decrypted = client_recv.decrypt(&encrypted).unwrap();
        let decoded = Frame::deserialize(&decrypted).unwrap();
        assert_eq!(decoded.frame_type, crate::remote_frame::OK);
        assert_eq!(decoded.terminal_id, 42);
    }

    /// Test HELLO frame construction and parsing (client side)
    #[test]
    fn test_hello_frame_construction() {
        let client_random = generate_random_32();
        let hello = Frame::hello(CLIENT_CAPABILITIES, &client_random);
        assert_eq!(hello.frame_type, crate::remote_frame::HELLO);
        let (caps, random) = hello.parse_hello().unwrap();
        assert_eq!(caps, CLIENT_CAPABILITIES);
        assert_eq!(random, client_random);
    }

    /// Test that extract_host correctly parses relay URLs
    #[test]
    fn test_extract_host() {
        assert_eq!(crate::tunnel_client::extract_host("wss://relay.example.com/tunnel"), "relay.example.com");
        assert_eq!(crate::tunnel_client::extract_host("ws://localhost:8080/tunnel"), "localhost:8080");
        assert_eq!(crate::tunnel_client::extract_host("wss://termfast.xisj.com/tunnel"), "termfast.xisj.com");
    }

    /// Test RemoteClientManager start_client + stop_client lifecycle
    /// Uses a non-existent relay URL so connection fails, but manager state is still testable
    #[tokio::test]
    async fn test_remote_client_manager_start_stop() {
        let mgr = RemoteClientManager::new();
        let config = RemoteClientConfig {
            relay_url: "ws://127.0.0.1:1/tunnel".to_string(), // Port 1 — will fail to connect
            pairing_jwt: "jwt".to_string(),
            pairing_id: "test-pair".to_string(),
            pairing_key: [0x42u8; 32],
        };

        // Start client — the task will spawn but connection will fail
        mgr.start_client(
            config,
            |_pid, _ft, _tid, _payload| {},
            |_pid, _connected| {},
        ).await.unwrap();

        // Manager should have one client
        assert_eq!(mgr.clients.lock().await.len(), 1);
        assert_eq!(mgr.senders.lock().await.len(), 1);

        // Stop the client
        mgr.stop_client("test-pair").await;
        assert_eq!(mgr.clients.lock().await.len(), 0);
    }

    /// Test RemoteClientManager stop_all clears everything
    #[tokio::test]
    async fn test_remote_client_manager_stop_all() {
        let mgr = RemoteClientManager::new();
        let config = RemoteClientConfig {
            relay_url: "ws://127.0.0.1:1/tunnel".to_string(),
            pairing_jwt: "jwt".to_string(),
            pairing_id: "test-pair-all".to_string(),
            pairing_key: [0x42u8; 32],
        };

        mgr.start_client(
            config,
            |_pid, _ft, _tid, _payload| {},
            |_pid, _connected| {},
        ).await.unwrap();

        assert_eq!(mgr.clients.lock().await.len(), 1);
        mgr.stop_all().await;
        assert_eq!(mgr.clients.lock().await.len(), 0);
        assert!(mgr.senders.lock().await.is_empty());
    }
}
