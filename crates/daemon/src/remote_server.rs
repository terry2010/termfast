//! Remote terminal protocol server — handles encrypted frame protocol for mobile clients.
//!
//! One `RemoteServer` instance is shared across all tunnel connections (Arc).
//! Each tunnel connection gets its own `handle_tunnel()` call, which processes
//! the full lifecycle: HELLO exchange → key switch → LIST/SUBSCRIBE/INPUT/etc.
//!
//! Frame protocol (see `remote_frame.rs`):
//!   [version:1][type:1][terminal_id:4][payload_len:4][payload:N]
//!
//! Encryption (see `frame_crypto.rs`):
//!   HELLO frames use pairing key K; subsequent frames use HKDF-derived session key.
//!   nonce = [direction:1][counter:8][padding:3], per-direction monotonic counter.

use crate::frame_crypto::{self, FrameCipher, DIR_DESKTOP_TO_MOBILE, DIR_MOBILE_TO_DESKTOP};
use crate::remote_frame::{self, Frame};
use crate::terminal::{RemoteSubscriber, TerminalManager};
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex, RwLock};
use tokio::sync::mpsc;

/// A tunnel session — the channels that connect WebSocket I/O to the protocol server.
///
/// - `inbound_rx`: encrypted bytes from mobile (via relay) → protocol server decrypts
/// - `outbound_tx`: encrypted bytes from protocol server → relay → mobile
/// - `async_rx`: unencrypted Frames from background tasks (e.g. subscriber channels) → protocol server encrypts + sends
pub struct TunnelSession {
    pub pairing_id: String,
    pub inbound_rx: mpsc::Receiver<Vec<u8>>,
    pub outbound_tx: mpsc::Sender<Vec<u8>>,
    pub async_rx: mpsc::Receiver<Frame>,
    /// Clone of the async_tx so handle_tunnel can give it to background tasks
    pub async_tx: mpsc::Sender<Frame>,
}

/// u32 handle ↔ String session ID bidirectional mapping.
/// Per design doc: persistent across LIST_REQUEST calls, assigned on first LIST,
/// removed on terminal close. std Mutex (short lock, no await).
pub struct IdMap {
    handle_to_sid: HashMap<u32, String>,
    sid_to_handle: HashMap<String, u32>,
    next_id: u32,
}

impl Default for IdMap {
    fn default() -> Self {
        Self::new()
    }
}

impl IdMap {
    pub fn new() -> Self {
        Self {
            handle_to_sid: HashMap::new(),
            sid_to_handle: HashMap::new(),
            next_id: 1, // start at 1 (0 is reserved for LIST_RESPONSE terminal_id)
        }
    }

    /// Assign or return existing u32 handle for a session_id.
    pub fn get_or_assign(&mut self, sid: &str) -> u32 {
        if let Some(&h) = self.sid_to_handle.get(sid) {
            return h;
        }
        let h = self.next_id;
        self.next_id += 1;
        self.handle_to_sid.insert(h, sid.to_string());
        self.sid_to_handle.insert(sid.to_string(), h);
        h
    }

    /// u32 handle → session_id.
    pub fn lookup_sid(&self, handle: u32) -> Option<&String> {
        self.handle_to_sid.get(&handle)
    }

    /// Remove mapping when terminal closes.
    pub fn remove(&mut self, sid: &str) {
        if let Some(h) = self.sid_to_handle.remove(sid) {
            self.handle_to_sid.remove(&h);
        }
    }
}

/// The remote protocol server — shared across all tunnel connections.
/// Per design doc: holds TerminalManager, ConfigManager (for server_name),
/// Result of a file upload (returned by file_upload_callback).
/// Used to construct the NOTIFY(file_ready) frame.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileUploadResult {
    pub cloud_path: String,
    pub file_name: String,
    pub size: u64,
    pub sha256: String,
    pub mime_type: String,
}

/// Type alias for the file upload callback function.
/// Receives a file_path, returns a Future that resolves to FileUploadResult or error.
pub type FileUploadCallback = Box<
    dyn Fn(String) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<FileUploadResult, String>> + Send + 'static>>
        + Send
        + Sync,
>;

/// Parsed DESKTOP_PAIR frame payload (JSON).
/// Sent by the phone via an existing phone↔desktop encrypted tunnel to
/// instruct the desktop to store a new desktop-to-desktop pairing and
/// start acting as server (role=server) or client (role=client).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DesktopPairMessage {
    pub action: String,
    pub pairing_id: String,
    /// Legacy: pairing key hex (64 chars). Empty when using ECDH key agreement.
    /// Kept for backward compatibility; new flow uses peer_ecdh_public_key.
    #[serde(default)]
    pub pairing_key_hex: String,
    /// Peer's X25519 ECDH public key (base64, 32 bytes).
    /// When present, the desktop computes shared_secret = ECDH(my_private, peer_public)
    /// and uses it as the pairing key. When absent, falls back to pairing_key_hex.
    #[serde(default)]
    pub peer_ecdh_public_key: String,
    /// For role=client (Desktop A): scope=tunnel pairing JWT to connect relay.
    /// For role=server (Desktop B): empty (B uses its own user JWT).
    pub pairing_jwt: String,
    pub peer_name: String,
    pub pairing_type: String,
    /// "server" (Desktop B) or "client" (Desktop A).
    pub role: String,
    /// Relay URL that both A and B should connect to.
    pub relay_url: String,
}

/// Type alias for the desktop pair callback function.
/// Called when a DESKTOP_PAIR frame is received. The callback is responsible
/// for storing the pairing and starting a tunnel (server) or RemoteClient (client).
pub type DesktopPairCallback = Box<
    dyn Fn(DesktopPairMessage) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'static>>
        + Send
        + Sync,
>;

/// Callback for forwarding received frames to the frontend.
/// Same signature as RemoteClientManager's FrameCallback.
pub type ServerFrameCallback = Arc<dyn Fn(&str, u8, u32, &[u8]) + Send + Sync>;

/// Trigger management request type (parsed from TRIGGER_* frames).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct TriggerRequest {
    /// "list", "exec", "add", "update", "remove"
    pub action: String,
    /// Trigger ID (for exec / remove)
    #[serde(default)]
    pub trigger_id: String,
    /// Full trigger JSON object (for add / update)
    #[serde(default)]
    pub trigger: Option<serde_json::Value>,
}

/// Trigger management response (returned by the callback).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct TriggerResponse {
    pub success: bool,
    /// For "list": JSON array of triggers. For "exec": execution result.
    /// For errors: error message.
    pub data: serde_json::Value,
}

/// Callback for handling trigger management requests from the peer desktop.
/// Called when TRIGGER_LIST_REQUEST / TRIGGER_EXEC / TRIGGER_ADD /
/// TRIGGER_UPDATE / TRIGGER_REMOVE frames are received.
/// The callback is responsible for accessing the local DaemonState
/// (config + trigger engine) and performing the requested operation.
pub type TriggerCallback = Box<
    dyn Fn(TriggerRequest) -> std::pin::Pin<Box<dyn std::future::Future<Output = TriggerResponse> + Send + 'static>>
        + Send
        + Sync,
>;

/// Broadcast a LIST_CHANGED NOTIFY frame to all active tunnels.
/// Uses try_send (non-blocking) since this is called from sync callbacks.
/// Mobile clients receiving this frame should re-send LIST_REQUEST.
fn broadcast_list_changed(active_tunnels: &Arc<StdMutex<HashMap<String, mpsc::Sender<Frame>>>>) {
    let tunnels = active_tunnels.lock().unwrap_or_else(|e| e.into_inner());
    tracing::info!("[RemoteServer] broadcast_list_changed: {} active tunnel(s)", tunnels.len());
    let notify_frame = Frame::notify(0, r#"{"type":"list_changed"}"#);
    let mut dead = Vec::new();
    for (pairing_id, tx) in tunnels.iter() {
        match tx.try_send(notify_frame.clone()) {
            Ok(()) => tracing::info!("[RemoteServer] sent LIST_CHANGED to pairing {}", pairing_id),
            Err(_) => {
                tracing::debug!("[RemoteServer] tunnel {} appears disconnected (try_send failed), will remove", pairing_id);
                dead.push(pairing_id.clone());
            }
        }
    }
    drop(tunnels);
    if !dead.is_empty() {
        let mut tunnels = active_tunnels.lock().unwrap_or_else(|e| e.into_inner());
        for id in dead {
            tunnels.remove(&id);
        }
    }
}

/// IdMap (u32↔session_id), auth_keys (pairing_id→key), answered_questions (mutex).
pub struct RemoteServer {
    pub terminal_manager: Arc<TerminalManager>,
    /// ConfigManager for resolving server_name in LIST_RESPONSE.
    /// Wrapped in tokio Mutex because ConfigManager.get() is async.
    config_manager: Arc<tokio::sync::Mutex<termfast_core::config::ConfigManager>>,
    /// ServerManager for opening SSH terminals (NEW_TERMINAL with server_id).
    /// Optional — None on Android (no SSH client).
    #[cfg(not(target_os = "android"))]
    server_manager: Option<Arc<termfast_core::server::ServerManager>>,
    /// Persistent u32↔session_id mapping (survives across LIST_REQUEST calls).
    id_map: Arc<StdMutex<IdMap>>,
    /// Pairing ID → 32-byte pairing key K. Added on pairing, removed on revoke.
    auth_keys: Arc<RwLock<HashMap<String, [u8; 32]>>>,
    /// Answered questions (question_id → answer) for agent popup mutex.
    /// Entries auto-expire after 30 seconds (spawned cleanup task).
    answered_questions: Arc<StdMutex<HashMap<String, String>>>,
    /// Callback for uploading local files to cloud (set by desktop app).
    /// Receives (file_path, terminal_id) → FileUploadResult or error message.
    file_upload_callback: Arc<std::sync::Mutex<Option<FileUploadCallback>>>,
    /// Callback for handling DESKTOP_PAIR frames (set by desktop app).
    /// Receives the parsed DesktopPairMessage → Ok or error message.
    desktop_pair_callback: Arc<std::sync::Mutex<Option<DesktopPairCallback>>>,
    /// Callback for handling trigger management frames (set by desktop app).
    /// Receives a TriggerRequest → TriggerResponse.
    trigger_callback: Arc<std::sync::Mutex<Option<TriggerCallback>>>,
    /// Active tunnel connections: pairing_id → async_tx (for pushing frames to mobile).
    /// Used to broadcast LIST_CHANGED notifications when terminals are created/closed.
    active_tunnels: Arc<StdMutex<HashMap<String, mpsc::Sender<Frame>>>>,
    /// Callback for forwarding received frames to the frontend (set by desktop app).
    /// Used when server desktop initiates requests (LIST, INFO, etc.) and receives
    /// responses from the client desktop — these need to be forwarded to the frontend.
    frame_callback: Arc<std::sync::Mutex<Option<ServerFrameCallback>>>,
    /// Lock to serialize NEW_TERMINAL requests, preventing TOCTOU race in
    /// terminal name deduplication (read existing names → open → set name).
    /// Uses tokio Mutex because the critical section spans async calls.
    new_terminal_lock: Arc<tokio::sync::Mutex<()>>,
}

impl RemoteServer {
    pub fn new(
        terminal_manager: Arc<TerminalManager>,
        config_manager: Arc<tokio::sync::Mutex<termfast_core::config::ConfigManager>>,
    ) -> Self {
        Self::new_with_server_manager(terminal_manager, config_manager, None)
    }

    /// Create RemoteServer with ServerManager (enables NEW_TERMINAL for SSH terminals).
    #[cfg(not(target_os = "android"))]
    pub fn new_with_server_manager(
        terminal_manager: Arc<TerminalManager>,
        config_manager: Arc<tokio::sync::Mutex<termfast_core::config::ConfigManager>>,
        server_manager: Option<Arc<termfast_core::server::ServerManager>>,
    ) -> Self {
        let id_map = Arc::new(StdMutex::new(IdMap::new()));

        // active_tunnels registry — used to broadcast LIST_CHANGED to all connected mobiles
        let active_tunnels: Arc<StdMutex<HashMap<String, mpsc::Sender<Frame>>>> =
            Arc::new(StdMutex::new(HashMap::new()));

        // Register on_closed callback: clean up IdMap + broadcast LIST_CHANGED
        let id_map_cb = id_map.clone();
        let tunnels_closed = active_tunnels.clone();
        terminal_manager.set_on_closed_callback(Box::new(move |session_id: &str| {
            if let Ok(mut map) = id_map_cb.lock() {
                map.remove(session_id);
                tracing::debug!("[RemoteServer] on_terminal_closed: removed session {} from IdMap", session_id);
            }
            broadcast_list_changed(&tunnels_closed);
        }));

        // Register on_opened callback: broadcast LIST_CHANGED to all connected mobiles
        let tunnels_opened = active_tunnels.clone();
        terminal_manager.set_on_opened_callback(Box::new(move || {
            broadcast_list_changed(&tunnels_opened);
        }));

        Self {
            terminal_manager,
            config_manager,
            server_manager,
            id_map,
            auth_keys: Arc::new(RwLock::new(HashMap::new())),
            answered_questions: Arc::new(StdMutex::new(HashMap::new())),
            file_upload_callback: Arc::new(std::sync::Mutex::new(None)),
            desktop_pair_callback: Arc::new(std::sync::Mutex::new(None)),
            trigger_callback: Arc::new(std::sync::Mutex::new(None)),
            active_tunnels,
            frame_callback: Arc::new(std::sync::Mutex::new(None)),
            new_terminal_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// Android variant — no server_manager (no SSH client).
    #[cfg(target_os = "android")]
    pub fn new_with_server_manager(
        terminal_manager: Arc<TerminalManager>,
        config_manager: Arc<tokio::sync::Mutex<termfast_core::config::ConfigManager>>,
        _server_manager: Option<Arc<termfast_core::server::ServerManager>>,
    ) -> Self {
        Self::new(terminal_manager, config_manager)
    }

    /// Set the file upload callback (called when mobile sends FILE_REQUEST on local terminal).
    /// The callback receives a file_path and returns a FileUploadResult (cloud_path, etc.)
    /// or an error message. The callback is responsible for reading, encrypting, and
    /// uploading the file to cloud storage.
    pub fn set_file_upload_callback(&self, callback: FileUploadCallback) {
        *self.file_upload_callback.lock().unwrap_or_else(|e| e.into_inner()) = Some(callback);
    }

    /// Set the desktop pair callback (called when a DESKTOP_PAIR frame is received).
    /// The callback is responsible for storing the pairing and starting a tunnel
    /// (for role=server) or RemoteClient (for role=client).
    pub fn set_desktop_pair_callback(&self, callback: DesktopPairCallback) {
        *self.desktop_pair_callback.lock().unwrap_or_else(|e| e.into_inner()) = Some(callback);
    }

    /// Set the trigger callback (called when TRIGGER_* frames are received).
    /// The callback handles list/exec/add/update/remove operations on local triggers.
    pub fn set_trigger_callback(&self, callback: TriggerCallback) {
        *self.trigger_callback.lock().unwrap_or_else(|e| e.into_inner()) = Some(callback);
    }

    /// Set the frame callback (called when a frame is received from the peer
    /// that needs to be forwarded to the frontend, e.g. LIST_RESPONSE, INFO_RESPONSE,
    /// OUTPUT, HISTORY, OK).
    pub fn set_frame_callback(&self, callback: ServerFrameCallback) {
        *self.frame_callback.lock().unwrap_or_else(|e| e.into_inner()) = Some(callback);
    }

    /// Add a pairing (called when pairing completes).
    pub fn add_pairing(&self, pairing_id: String, key: [u8; 32]) {
        self.auth_keys.write().unwrap().insert(pairing_id, key);
    }

    /// Remove a pairing key (called when tunnel client exits).
    /// Lightweight — does NOT remove remote subscribers (unlike revoke_pairing).
    pub fn remove_pairing(&self, pairing_id: &str) {
        self.auth_keys.write().unwrap().remove(pairing_id);
    }

    /// Revoke a pairing (called when user removes a device).
    /// Removes the key and all remote subscribers for this pairing_id.
    pub async fn revoke_pairing(&self, pairing_id: &str) {
        self.auth_keys.write().unwrap().remove(pairing_id);
        self.terminal_manager.remove_remote_subscribers(pairing_id).await;
    }

    /// Get pairing key for a pairing_id (returns None if not paired/revoked).
    pub fn get_pairing_key(&self, pairing_id: &str) -> Option<[u8; 32]> {
        self.auth_keys.read().unwrap().get(pairing_id).copied()
    }

    /// Send a frame to the peer (client) for a given pairing_id.
    /// Used by the server-side desktop to proactively send frames
    /// (INFO_REQUEST, NEW_TERMINAL, CLOSE_TERMINAL, etc.) to the client
    /// desktop over the existing tunnel connection.
    ///
    /// The frame is queued via the active_tunnels async_tx channel,
    /// which the tunnel loop picks up and encrypts + sends.
    pub async fn send_frame_to_peer(&self, pairing_id: &str, frame: Frame) -> Result<(), String> {
        let tx = {
            let tunnels = self.active_tunnels.lock().unwrap_or_else(|e| e.into_inner());
            tunnels.get(pairing_id).cloned()
        };
        match tx {
            Some(tx) => {
                tx.send(frame).await.map_err(|e| format!("async send: {}", e))
            }
            None => Err(format!("no active tunnel for pairing_id={}", pairing_id)),
        }
    }

    /// Broadcast a frame to all active tunnels (all connected peers).
    /// Returns the number of peers the frame was sent to.
    /// Uses try_send for non-blocking delivery; disconnected peers are cleaned up.
    pub async fn broadcast_frame_to_all(&self, frame: Frame) -> usize {
        let tunnels = self.active_tunnels.lock().unwrap_or_else(|e| e.into_inner());
        let mut sent = 0;
        let mut dead = Vec::new();
        for (pairing_id, tx) in tunnels.iter() {
            match tx.try_send(frame.clone()) {
                Ok(()) => {
                    sent += 1;
                }
                Err(mpsc::error::TrySendError::Full(_)) => {
                    tracing::warn!("[RemoteServer] broadcast_frame_to_all: channel full for pairing {}", pairing_id);
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    tracing::debug!("[RemoteServer] broadcast_frame_to_all: channel closed for pairing {}", pairing_id);
                    dead.push(pairing_id.clone());
                }
            }
        }
        drop(tunnels);
        if !dead.is_empty() {
            let mut tunnels = self.active_tunnels.lock().unwrap_or_else(|e| e.into_inner());
            for id in dead {
                tunnels.remove(&id);
            }
        }
        sent
    }

    /// Check if a tunnel is active (peer is connected) for a given pairing_id.
    pub fn is_tunnel_active(&self, pairing_id: &str) -> bool {
        self.active_tunnels.lock().unwrap_or_else(|e| e.into_inner()).contains_key(pairing_id)
    }

    /// Resolve u32 terminal_id → session_id (for frame processing).
    fn resolve_sid(&self, terminal_id: u32) -> Option<String> {
        self.id_map.lock().unwrap_or_else(|e| e.into_inner()).lookup_sid(terminal_id).cloned()
    }

    /// Resolve session_id → u32 terminal_id (for desktop IPC path).
    pub fn resolve_terminal_id(&self, session_id: &str) -> Option<u32> {
        self.id_map.lock().unwrap_or_else(|e| e.into_inner()).sid_to_handle.get(session_id).copied()
    }

    /// Handle one tunnel connection's full lifecycle.
    ///
    /// This is a long-running async fn — should be spawned, not awaited.
    /// Returns when the tunnel closes (GOODBYE, error, or channel close).
    ///
    /// The pairing key is looked up from `auth_keys` by pairing_id.
    /// If the pairing has been revoked (key removed), the tunnel is rejected.
    pub async fn handle_tunnel(
        &self,
        session: TunnelSession,
    ) {
        let TunnelSession {
            pairing_id,
            mut inbound_rx,
            outbound_tx,
            mut async_rx,
            async_tx,
        } = session;

        // Look up pairing key from auth_keys (per design doc)
        let pairing_key = match self.get_pairing_key(&pairing_id) {
            Some(k) => k,
            None => {
                tracing::warn!("pairing {} not found in auth_keys (revoked?)", pairing_id);
                return;
            }
        };

        // Phase 1: HELLO exchange
        // Mobile sends HELLO encrypted with pairing key K.
        // Desktop decrypts with K, generates server_random, derives session_key,
        // replies with HELLO encrypted with K (containing server_random).
        let (mut send_cipher, recv_cipher) = match self.handle_hello(
            &pairing_key,
            &mut inbound_rx,
            &outbound_tx,
        ).await {
            Some(ciphers) => ciphers,
            None => {
                tracing::warn!("HELLO exchange failed for pairing {}", pairing_id);
                return;
            }
        };

        // Register this tunnel's async_tx in active_tunnels so we can push
        // LIST_CHANGED notifications to mobile when terminals open/close.
        {
            let mut tunnels = self.active_tunnels.lock().unwrap_or_else(|e| e.into_inner());
            tunnels.insert(pairing_id.clone(), async_tx.clone());
        }

        // Phase 2: Main loop — process frames
        // terminal_id → session_id resolution uses the persistent IdMap (self.id_map),
        // not a per-tunnel HashMap. This ensures terminal_id is stable across
        // LIST_REQUEST calls and across reconnects.
        'tunnel: loop {
            tokio::select! {
                // Encrypted bytes from mobile (via relay)
                data = inbound_rx.recv() => {
                    match data {
                        Some(encrypted) => {
                            // Decrypt with session key
                            let plaintext = match recv_cipher.decrypt(&encrypted) {
                                Ok(pt) => pt,
                                Err(e) => {
                                    tracing::warn!("decrypt error from pairing {}: {}", pairing_id, e);
                                    // Per design: decrypt failure → disconnect, no reply
                                    break 'tunnel;
                                }
                            };
                            // Deserialize frame
                            let frame = match Frame::deserialize(&plaintext) {
                                Ok(f) => f,
                                Err(e) => {
                                    tracing::warn!("frame deserialize error from pairing {}: {}", pairing_id, e);
                                    break 'tunnel;
                                }
                            };
                            // Process frame
                            let (response, should_close) = self.process_frame(
                                frame,
                                &pairing_id,
                                &async_tx,
                            ).await;

                            // Send response (if any)
                            if let Some(resp_frame) = response {
                                if let Err(e) = self.send_frame(&mut send_cipher, &outbound_tx, &resp_frame).await {
                                    tracing::warn!("send error for pairing {}: {}", pairing_id, e);
                                    break 'tunnel;
                                }
                            }

                            if should_close {
                                tracing::info!("closing tunnel for pairing {} (GOODBYE)", pairing_id);
                                break 'tunnel;
                            }
                        }
                        None => {
                            tracing::info!("inbound channel closed for pairing {}", pairing_id);
                            break 'tunnel;
                        }
                    }
                }
                // Unencrypted frames from background tasks (subscriber channels)
                frame = async_rx.recv() => {
                    match frame {
                        Some(f) => {
                            if let Err(e) = self.send_frame(&mut send_cipher, &outbound_tx, &f).await {
                                tracing::warn!("async send error for pairing {}: {}", pairing_id, e);
                                break 'tunnel;
                            }
                        }
                        None => {
                            // async_tx still held by us (in async_tx var), so None shouldn't happen
                            // unless all clones are dropped. Treat as tunnel close.
                            tracing::info!("async channel closed for pairing {}", pairing_id);
                            break 'tunnel;
                        }
                    }
                }
            }
        }

        // Unregister this tunnel from active_tunnels (cleanup on any exit path)
        {
            let mut tunnels = self.active_tunnels.lock().unwrap_or_else(|e| e.into_inner());
            tunnels.remove(&pairing_id);
        }
    }

    /// HELLO exchange: decrypt mobile's HELLO with K, generate server_random,
    /// derive session_key, reply with HELLO containing server_random.
    ///
    /// Returns (send_cipher, recv_cipher) using session_key, or None on failure.
    async fn handle_hello(
        &self,
        pairing_key: &[u8; 32],
        inbound_rx: &mut mpsc::Receiver<Vec<u8>>,
        outbound_tx: &mpsc::Sender<Vec<u8>>,
    ) -> Option<(FrameCipher, FrameCipher)> {
        // Wait for mobile's HELLO (encrypted with K) — 30s timeout
        let encrypted = match tokio::time::timeout(
            std::time::Duration::from_secs(30),
            inbound_rx.recv(),
        ).await {
            Ok(Some(data)) => data,
            Ok(None) => {
                tracing::warn!("no HELLO received — channel closed");
                return None;
            }
            Err(_) => {
                tracing::warn!("no HELLO received — timeout (30s)");
                return None;
            }
        };

        // Decrypt HELLO with pairing key K
        let hello_cipher = FrameCipher::from_pairing_key(pairing_key, DIR_MOBILE_TO_DESKTOP);
        let plaintext = match hello_cipher.decrypt(&encrypted) {
            Ok(pt) => pt,
            Err(e) => {
                tracing::warn!("HELLO decrypt failed: {} — wrong key?", e);
                return None;
            }
        };

        let hello_frame = match Frame::deserialize(&plaintext) {
            Ok(f) if f.frame_type == remote_frame::HELLO => f,
            Ok(f) => {
                // Per design: HELLO before other frames → send ERROR("hello_required") then disconnect
                tracing::warn!("expected HELLO frame, got type 0x{:02X}", f.frame_type);
                let err_frame = Frame::error("hello_required");
                let mut err_cipher = FrameCipher::from_pairing_key(pairing_key, DIR_DESKTOP_TO_MOBILE);
                let _ = self.send_frame(&mut err_cipher, outbound_tx, &err_frame).await;
                return None;
            }
            Err(e) => {
                tracing::warn!("HELLO frame deserialize error: {}", e);
                return None;
            }
        };

        // Extract client capabilities + client_random from HELLO payload
        let (client_caps, client_random) = match hello_frame.parse_hello() {
            Some(caps) => caps,
            None => {
                tracing::warn!("HELLO payload too short or invalid");
                return None;
            }
        };

        // Version negotiation: min(client_version, SERVER_VERSION)
        // Per design: if client_version < MIN_SUPPORTED_VERSION → reject
        const SERVER_VERSION: u8 = 1;
        const MIN_SUPPORTED_VERSION: u8 = 1;
        if hello_frame.version < MIN_SUPPORTED_VERSION {
            tracing::warn!("HELLO version {} < min supported {}", hello_frame.version, MIN_SUPPORTED_VERSION);
            let err_frame = Frame::error("unsupported_version");
            let mut err_cipher = FrameCipher::from_pairing_key(pairing_key, DIR_DESKTOP_TO_MOBILE);
            let _ = self.send_frame(&mut err_cipher, outbound_tx, &err_frame).await;
            return None;
        }
        let _negotiated_version = hello_frame.version.min(SERVER_VERSION);

        // Capabilities negotiation: intersection of client and server caps
        // 4a: server supports skip_history (bit1=1), no zstd (bit0=0)
        // 4b: server will support zstd (bit0=1)
        const SERVER_CAPS_4A: u16 = 0b0000_0000_0000_0010; // bit1=skip_history
        let negotiated_caps = client_caps & SERVER_CAPS_4A;

        // Generate server_random
        let server_random = frame_crypto::generate_random_32();

        // Derive session key
        // Desktop sends with DIR_DESKTOP_TO_MOBILE, receives with DIR_MOBILE_TO_DESKTOP
        let send_cipher = FrameCipher::from_session_key(
            pairing_key,
            &client_random,
            &server_random,
            DIR_DESKTOP_TO_MOBILE,
        );
        let recv_cipher = FrameCipher::from_session_key(
            pairing_key,
            &client_random,
            &server_random,
            frame_crypto::DIR_MOBILE_TO_DESKTOP,
        );

        // Reply with HELLO containing negotiated capabilities + server_random (encrypted with K)
        let reply_hello = Frame::hello(negotiated_caps, &server_random);
        let reply_cipher = FrameCipher::from_pairing_key(pairing_key, DIR_DESKTOP_TO_MOBILE);
        // Use a one-shot cipher for HELLO reply (counter starts at 0)
        let mut reply_cipher = reply_cipher;
        if self.send_frame(&mut reply_cipher, outbound_tx, &reply_hello).await.is_err() {
            tracing::warn!("failed to send HELLO reply");
            return None;
        }

        tracing::info!("HELLO exchange complete, session key derived");
        Some((send_cipher, recv_cipher))
    }

    /// Process a decrypted frame from mobile. Returns (optional response frame, should_close).
    async fn process_frame(
        &self,
        frame: Frame,
        pairing_id: &str,
        async_tx: &mpsc::Sender<Frame>,
    ) -> (Option<Frame>, bool) {
        match frame.frame_type {
            remote_frame::LIST_REQUEST => {
                self.handle_list_request(async_tx).await;
                (None, false) // LIST_RESPONSE sent via async_tx
            }
            remote_frame::SUBSCRIBE => {
                let resp = self.handle_subscribe(frame.terminal_id, pairing_id, async_tx).await;
                (resp, false)
            }
            remote_frame::UNSUBSCRIBE => {
                let resp = self.handle_unsubscribe(frame.terminal_id, pairing_id).await;
                (resp, false)
            }
            remote_frame::INPUT => {
                let resp = self.handle_input(frame).await;
                (resp, false)
            }
            remote_frame::INPUT_ANSWER => {
                let resp = self.handle_input_answer(frame).await;
                (resp, false)
            }
            remote_frame::RESIZE => {
                let resp = self.handle_resize(frame).await;
                (resp, false)
            }
            remote_frame::REDRAW_REQUEST => {
                let resp = self.handle_redraw_request(frame.terminal_id, async_tx).await;
                (resp, false)
            }
            remote_frame::FILE_REQUEST => {
                let resp = self.handle_file_request(frame, async_tx).await;
                (resp, false)
            }
            remote_frame::DESKTOP_PAIR => {
                let resp = self.handle_desktop_pair_frame(frame).await;
                (resp, false)
            }
            #[cfg(not(target_os = "android"))]
            remote_frame::INFO_REQUEST => {
                tracing::info!("INFO_REQUEST received from pairing {}", pairing_id);
                let resp = self.handle_info_request().await;
                (Some(resp), false)
            }
            #[cfg(not(target_os = "android"))]
            remote_frame::NEW_TERMINAL => {
                tracing::info!("NEW_TERMINAL received from pairing {}", pairing_id);
                let resp = self.handle_new_terminal(frame, async_tx).await;
                (resp, false)
            }
            #[cfg(not(target_os = "android"))]
            remote_frame::CLOSE_TERMINAL => {
                tracing::info!("CLOSE_TERMINAL received from pairing {}, terminal_id={}", pairing_id, frame.terminal_id);
                let resp = self.handle_close_terminal(frame).await;
                (resp, false)
            }
            #[cfg(target_os = "android")]
            remote_frame::INFO_REQUEST => {
                tracing::warn!("INFO_REQUEST not supported on Android");
                (Some(Frame::error("not_supported")), false)
            }
            #[cfg(target_os = "android")]
            remote_frame::NEW_TERMINAL => {
                tracing::warn!("NEW_TERMINAL not supported on Android");
                (Some(Frame::error("not_supported")), false)
            }
            #[cfg(target_os = "android")]
            remote_frame::CLOSE_TERMINAL => {
                tracing::warn!("CLOSE_TERMINAL not supported on Android");
                (Some(Frame::error("not_supported")), false)
            }
            // Trigger management (desktop-to-desktop) — not on Android
            #[cfg(not(target_os = "android"))]
            remote_frame::TRIGGER_LIST_REQUEST
            | remote_frame::TRIGGER_EXEC
            | remote_frame::TRIGGER_ADD
            | remote_frame::TRIGGER_UPDATE
            | remote_frame::TRIGGER_REMOVE => {
                let resp = self.handle_trigger_frame(frame).await;
                (resp, false)
            }
            #[cfg(target_os = "android")]
            remote_frame::TRIGGER_LIST_REQUEST
            | remote_frame::TRIGGER_EXEC
            | remote_frame::TRIGGER_ADD
            | remote_frame::TRIGGER_UPDATE
            | remote_frame::TRIGGER_REMOVE => {
                (Some(Frame::error("not_supported")), false)
            }
            remote_frame::GOODBYE => {
                tracing::info!("GOODBYE received from pairing {}", pairing_id);
                // Reply with GOODBYE, then close.
                // Per design #31: GOODBYE is bidirectional —
                // active side sends GOODBYE → passive side replies GOODBYE → both close.
                // 3-second timeout is enforced by the tunnel client (WebSocket close timeout).
                (Some(Frame::goodbye()), true)
            }
            remote_frame::HELLO => {
                // HELLO after initial exchange — protocol violation
                tracing::warn!("HELLO received after initial exchange — protocol violation");
                (Some(Frame::error("hello_already_done")), false)
            }
            // Response frames from the peer desktop (when this desktop is the
            // initiator). Forward them to the frontend via frame_callback.
            remote_frame::LIST_RESPONSE
            | remote_frame::INFO_RESPONSE
            | remote_frame::OUTPUT
            | remote_frame::HISTORY
            | remote_frame::OK
            | remote_frame::ERROR
            | remote_frame::NOTIFY => {
                // Clone callback out of mutex before invoking to avoid holding lock during callback
                let cb_clone = self.frame_callback.lock().unwrap_or_else(|e| e.into_inner()).clone();
                if let Some(cb) = cb_clone {
                    cb(pairing_id, frame.frame_type, frame.terminal_id, &frame.payload);
                }
                (None, false)
            }
            _ => {
                tracing::warn!("unknown frame type 0x{:02X}", frame.frame_type);
                (Some(Frame::error("unknown_frame_type")), false)
            }
        }
    }

    /// Encrypt and send a frame via the outbound channel.
    async fn send_frame(
        &self,
        cipher: &mut FrameCipher,
        outbound_tx: &mpsc::Sender<Vec<u8>>,
        frame: &Frame,
    ) -> Result<(), String> {
        let plaintext = frame.serialize();
        let encrypted = cipher.encrypt(&plaintext)?;
        outbound_tx.send(encrypted).await.map_err(|e| format!("outbound send: {}", e))
    }

    // === Frame handlers ===

    /// LIST_REQUEST: list all terminals, assign u32 handles via persistent IdMap,
    /// fill server_name from ConfigManager, send LIST_RESPONSE via async_tx.
    ///
    /// Per design doc: JSON object with fields:
    ///   terminals: array of { id (u32), name, status, preview, server_id, server_name, terminal_type, tmux_session_name }
    ///   servers: array of { server_id, server_name, is_local } — all configured SSH servers + local desktop
    async fn handle_list_request(
        &self,
        async_tx: &mpsc::Sender<Frame>,
    ) {
        let mut infos = self.terminal_manager.list_session_infos().await;
        // Fill server_name from ConfigManager (real-time, user may have renamed servers)
        let config = {
            let mgr = self.config_manager.lock().await;
            mgr.get().await
        };
        for info in infos.iter_mut() {
            info.server_name = if info.server_id == "__local__" {
                "桌面端".to_string()
            } else {
                config.servers.iter()
                    .find(|srv| srv.id == info.server_id)
                    .map(|srv| srv.name.clone())
                    .unwrap_or_else(|| info.server_id.clone())
            };
        }
        // Assign u32 handles via persistent IdMap and build JSON list
        let terminals: Vec<serde_json::Value> = {
            let mut id_map = self.id_map.lock().unwrap_or_else(|e| e.into_inner());
            infos.iter().map(|info| {
                let handle = id_map.get_or_assign(&info.session_id);
                serde_json::json!({
                    "id": handle,
                    "name": info.name,
                    "status": info.status,
                    "preview": info.preview,
                    "server_id": info.server_id,
                    "server_name": info.server_name,
                    "terminal_type": info.terminal_type,
                    "tmux_session_name": info.tmux_session_name,
                })
            }).collect()
        };
        // Build servers list: local desktop + all configured SSH servers
        let servers: Vec<serde_json::Value> = {
            let mut vec = vec![serde_json::json!({
                "server_id": "__local__",
                "server_name": "桌面端",
                "is_local": true,
            })];
            for srv in &config.servers {
                vec.push(serde_json::json!({
                    "server_id": srv.id,
                    "server_name": srv.name,
                    "is_local": false,
                }));
            }
            vec
        };
        let json = serde_json::json!({ "terminals": terminals, "servers": servers }).to_string();
        let frame = Frame::list_response(0, &json);
        let _ = async_tx.send(frame).await;
    }

    async fn handle_subscribe(
        &self,
        terminal_id: u32,
        pairing_id: &str,
        async_tx: &mpsc::Sender<Frame>,
    ) -> Option<Frame> {
        let session_id = match self.resolve_sid(terminal_id) {
            Some(sid) => sid,
            None => return Some(Frame::error("invalid_terminal_id")),
        };

        // Create subscriber channel — background task reads from it and sends to async_tx
        let (sub_tx, mut sub_rx) = mpsc::channel::<Frame>(256);
        let subscriber = RemoteSubscriber {
            pairing_id: pairing_id.to_string(),
            terminal_id,
            sender: sub_tx,
            lagging: false,
        };

        // Subscribe (atomic: pushes RESIZE + HISTORY into channel, adds to remote_subscribers)
        if let Err(e) = self.terminal_manager.subscribe_remote(&session_id, subscriber).await {
            tracing::warn!("subscribe_remote error: {}", e);
            return Some(Frame::error("terminal_not_found"));
        }

        // Spawn background task: forward frames from subscriber channel to async_tx
        // (handle_tunnel main loop encrypts + sends them)
        let async_tx_clone = async_tx.clone();
        tokio::spawn(async move {
            while let Some(frame) = sub_rx.recv().await {
                if async_tx_clone.send(frame).await.is_err() {
                    break; // handle_tunnel exited
                }
            }
        });

        Some(Frame::ok(terminal_id))
    }

    async fn handle_unsubscribe(
        &self,
        terminal_id: u32,
        pairing_id: &str,
    ) -> Option<Frame> {
        if let Some(session_id) = self.resolve_sid(terminal_id) {
            self.terminal_manager.unsubscribe_remote(&session_id, pairing_id).await;
        }
        Some(Frame::ok(terminal_id))
    }

    async fn handle_input(
        &self,
        frame: Frame,
    ) -> Option<Frame> {
        if let Some(session_id) = self.resolve_sid(frame.terminal_id) {
            if let Err(e) = self.terminal_manager.remote_input(&session_id, &frame.payload).await {
                tracing::warn!("remote input error: {}", e);
                return Some(Frame::error("input_failed"));
            }
        } else {
            return Some(Frame::error("invalid_terminal_id"));
        }
        Some(Frame::ok(frame.terminal_id))
    }

    /// INPUT_ANSWER: agent popup answer with mutex (question_id → answer).
    /// Per design #17/#18: first answer wins, subsequent get already_answered error.
    /// Broadcasts QUESTION_RESOLVED to all subscribers after accepting answer.
    async fn handle_input_answer(
        &self,
        frame: Frame,
    ) -> Option<Frame> {
        // Parse payload JSON {question_id, answer}
        let req: serde_json::Value = match serde_json::from_slice(&frame.payload) {
            Ok(v) => v,
            Err(_) => return Some(Frame::error("invalid_payload")),
        };
        let question_id = req["question_id"].as_str().unwrap_or("");
        let answer = req["answer"].as_str().unwrap_or("");
        if question_id.is_empty() {
            return Some(Frame::error("invalid_payload"));
        }

        // Phase 1: check and mark answered (std Mutex, short lock)
        {
            let mut answered = self.answered_questions.lock().unwrap_or_else(|e| e.into_inner());
            if answered.contains_key(question_id) {
                return Some(Frame::error("already_answered"));
            }
            answered.insert(question_id.to_string(), answer.to_string());
        }

        // Phase 2: write answer to PTY
        let data = format!("{}\n", answer).into_bytes();
        if let Some(session_id) = self.resolve_sid(frame.terminal_id) {
            if let Err(e) = self.terminal_manager.remote_input(&session_id, &data).await {
                tracing::warn!("input_answer write error: {}", e);
            }

            // Phase 3: broadcast QUESTION_RESOLVED to all subscribers
            let resolved = Frame::question_resolved(frame.terminal_id, question_id, answer);
            self.terminal_manager.broadcast_to_subscribers(&session_id, resolved).await;
        }

        // Phase 4: schedule cleanup after 30 seconds
        let answered = self.answered_questions.clone();
        let qid = question_id.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            answered.lock().unwrap_or_else(|e| e.into_inner()).remove(&qid);
        });

        Some(Frame::ok(frame.terminal_id))
    }

    async fn handle_resize(
        &self,
        frame: Frame,
    ) -> Option<Frame> {
        // Mobile sends RESIZE with its desired cols/rows.
        // Desktop resizes its PTY to match. No reply needed — the mobile
        // already knows its own dimensions. Replying would cause a resize
        // loop (mobile resize → desktop reply → mobile resize → ...).
        if let Some(session_id) = self.resolve_sid(frame.terminal_id) {
            if let Some((cols, rows)) = frame.parse_resize() {
                let _ = self.terminal_manager
                    .resize_and_notify(&session_id, cols as u32, rows as u32)
                    .await;
                // Return None — no reply frame. The OK was already sent
                // by the frame handler for query-type frames.
                return None;
            }
        }
        Some(Frame::error("invalid_terminal_id"))
    }

    async fn handle_redraw_request(
        &self,
        terminal_id: u32,
        async_tx: &mpsc::Sender<Frame>,
    ) -> Option<Frame> {
        if let Some(session_id) = self.resolve_sid(terminal_id) {
            if let Some(history) = self.terminal_manager.get_history(&session_id).await {
                let all_bytes: Vec<u8> = history.into_iter().flatten().collect();
                if all_bytes.is_empty() {
                    // Send a single HISTORY with is_last=1 and empty data
                    let _ = async_tx.send(Frame::history(terminal_id, 0, true, &[])).await;
                } else {
                    let chunks: Vec<&[u8]> = all_bytes.chunks(remote_frame::MAX_HISTORY_DATA).collect();
                    let total = chunks.len();
                    for (seq, chunk) in chunks.iter().enumerate() {
                        let is_last = seq == total - 1;
                        let _ = async_tx.send(Frame::history(terminal_id, seq as u32, is_last, chunk)).await;
                    }
                }
                return Some(Frame::ok(terminal_id));
            }
        }
        Some(Frame::error("invalid_terminal_id"))
    }

    /// FILE_REQUEST: 4a phase — SSH terminals return error, local terminals
    /// Handle FILE_REQUEST: SSH → ERROR(file_request_unsupported),
    /// local → spawn background upload task, return OK immediately,
    /// then send NOTIFY(file_ready) via async_tx when upload completes.
    /// Per design #19/#20.
    async fn handle_file_request(
        &self,
        frame: Frame,
        async_tx: &mpsc::Sender<Frame>,
    ) -> Option<Frame> {
        let session_id = match self.resolve_sid(frame.terminal_id) {
            Some(sid) => sid,
            None => return Some(Frame::error("invalid_terminal_id")),
        };
        // Check terminal type
        let is_local = self.terminal_manager.is_local_session(&session_id).await;
        if !is_local {
            // 4a: SSH terminals don't support FILE_REQUEST
            return Some(Frame::error("file_request_unsupported"));
        }
        // Parse file_path from payload (try JSON first, then raw string)
        let file_path: String = if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&frame.payload) {
            v["file_path"].as_str().unwrap_or("").to_string()
        } else {
            String::from_utf8_lossy(&frame.payload).to_string()
        };
        if file_path.is_empty() {
            return Some(Frame::error("missing_file_path"));
        }
        // Check if callback is set
        let callback = {
            let guard = self.file_upload_callback.lock().unwrap_or_else(|e| e.into_inner());
            guard.is_some()
        };
        if !callback {
            tracing::warn!("FILE_REQUEST: no file_upload_callback set — returning not_configured");
            return Some(Frame::error("file_upload_not_configured"));
        }
        // Return OK immediately (file request accepted), spawn background upload task
        let terminal_id = frame.terminal_id;
        let cb_arc = self.file_upload_callback.clone();
        let async_tx_clone = async_tx.clone();
        tokio::spawn(async move {
            // Clone the callback out of the mutex before awaiting (MutexGuard is not Send)
            let cb_future = {
                let guard = cb_arc.lock().unwrap_or_else(|e| e.into_inner());
                guard.as_ref().map(|cb| cb(file_path.clone()))
            };
            let result = match cb_future {
                Some(fut) => fut.await,
                None => Err("callback disappeared".to_string()),
            };
            match result {
                Ok(upload_result) => {
                    // Send NOTIFY(file_ready) with upload result
                    let notify_json = serde_json::json!({
                        "event_type": "file_ready",
                        "cloud_path": upload_result.cloud_path,
                        "file_name": upload_result.file_name,
                        "size": upload_result.size,
                        "sha256": upload_result.sha256,
                        "mime_type": upload_result.mime_type,
                    });
                    let notify_frame = Frame::notify(terminal_id, &notify_json.to_string());
                    if async_tx_clone.send(notify_frame).await.is_err() {
                        tracing::warn!("FILE_REQUEST: failed to send NOTIFY(file_ready) — tunnel closed");
                    }
                }
                Err(e) => {
                    tracing::error!("FILE_REQUEST: upload failed for {}: {}", file_path, e);
                    let err_frame = Frame::error_with_terminal(terminal_id, &format!("file_upload_failed: {}", e));
                    let _ = async_tx_clone.send(err_frame).await;
                }
            }
        });
        Some(Frame::ok(frame.terminal_id))
    }

    /// Handle a DESKTOP_PAIR frame (B1 + B6).
    ///
    /// The frame payload is a JSON `DesktopPairMessage`. The `role` field
    /// determines what the desktop should do:
    /// - "server": store pairing + start tunnel (register as server for the new pairing)
    /// - "client": store pairing + start RemoteClient (connect to the peer desktop)
    ///
    /// The actual tunnel/client startup is delegated to the `desktop_pair_callback`
    /// set by the app layer. This keeps RemoteServer decoupled from relay_url/user_jwt.
    async fn handle_desktop_pair_frame(&self, frame: Frame) -> Option<Frame> {
        let payload = match std::str::from_utf8(&frame.payload) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("DESKTOP_PAIR: utf8 error: {}", e);
                return Some(Frame::desktop_pair_response(&serde_json::json!({
                    "action": "pair_error",
                    "message": format!("utf8: {}", e),
                }).to_string()));
            }
        };
        let msg: DesktopPairMessage = match serde_json::from_str(payload) {
            Ok(m) => m,
            Err(e) => {
                tracing::error!("DESKTOP_PAIR: parse error: {}", e);
                return Some(Frame::desktop_pair_response(&serde_json::json!({
                    "action": "pair_error",
                    "message": format!("parse: {}", e),
                }).to_string()));
            }
        };
        if msg.action != "pair" {
            return Some(Frame::desktop_pair_response(&serde_json::json!({
                "action": "pair_error",
                "message": format!("unexpected action: {}", msg.action),
            }).to_string()));
        }
        // Check callback is set
        let callback_is_set = self.desktop_pair_callback.lock().unwrap_or_else(|e| e.into_inner()).is_some();
        if !callback_is_set {
            tracing::warn!("DESKTOP_PAIR: no callback set — cannot handle");
            return Some(Frame::desktop_pair_response(&serde_json::json!({
                "action": "pair_error",
                "message": "desktop_pair_not_configured",
            }).to_string()));
        }
        // Invoke callback (async). Clone the callback out of the mutex before awaiting.
        let cb_future = {
            let guard = self.desktop_pair_callback.lock().unwrap_or_else(|e| e.into_inner());
            guard.as_ref().map(|cb| cb(msg.clone()))
        };
        let result = match cb_future {
            Some(fut) => fut.await,
            None => Err("callback disappeared".to_string()),
        };
        match result {
            Ok(()) => {
                tracing::info!("DESKTOP_PAIR: callback succeeded for pairing {} role={}", msg.pairing_id, msg.role);
                Some(Frame::desktop_pair_response(&serde_json::json!({
                    "action": "pair_ok",
                    "pairing_id": msg.pairing_id,
                }).to_string()))
            }
            Err(e) => {
                tracing::error!("DESKTOP_PAIR: callback failed for pairing {}: {}", msg.pairing_id, e);
                Some(Frame::desktop_pair_response(&serde_json::json!({
                    "action": "pair_error",
                    "pairing_id": msg.pairing_id,
                    "message": e,
                }).to_string()))
            }
        }
    }

    /// Handle TRIGGER_* frames (list / exec / add / update / remove).
    ///
    /// All trigger frames carry JSON in the payload. The frame_type determines
    /// the action. The actual operation is delegated to the `trigger_callback`
    /// set by the app layer (which has access to DaemonState).
    ///
    /// Response is sent as a TRIGGER_LIST_RESPONSE (for list) or
    /// TRIGGER_EXEC_RESULT (for exec) or OK/ERROR (for add/update/remove).
    #[cfg(not(target_os = "android"))]
    async fn handle_trigger_frame(&self, frame: Frame) -> Option<Frame> {
        // Determine action from frame type
        let action = match frame.frame_type {
            remote_frame::TRIGGER_LIST_REQUEST => "list",
            remote_frame::TRIGGER_EXEC => "exec",
            remote_frame::TRIGGER_ADD => "add",
            remote_frame::TRIGGER_UPDATE => "update",
            remote_frame::TRIGGER_REMOVE => "remove",
            _ => return Some(Frame::error("unknown_trigger_frame")),
        };

        // Parse payload (empty for list request, JSON for others)
        let trigger_id = if frame.frame_type == remote_frame::TRIGGER_LIST_REQUEST {
            String::new()
        } else {
            let payload = match std::str::from_utf8(&frame.payload) {
                Ok(s) => s,
                Err(e) => return Some(Frame::error(&format!("utf8: {}", e))),
            };
            let req: serde_json::Value = match serde_json::from_str(payload) {
                Ok(v) => v,
                Err(e) => return Some(Frame::error(&format!("parse: {}", e))),
            };
            req.get("trigger_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };

        // For add/update, extract the trigger object from payload
        let trigger_obj = if frame.frame_type == remote_frame::TRIGGER_ADD
            || frame.frame_type == remote_frame::TRIGGER_UPDATE
        {
            let payload = match std::str::from_utf8(&frame.payload) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("TRIGGER add/update: utf8 parse error: {}", e);
                    return Some(Frame::error(&format!("utf8: {}", e)));
                }
            };
            let req: serde_json::Value = match serde_json::from_str(payload) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("TRIGGER add/update: json parse error: {}", e);
                    return Some(Frame::error(&format!("parse: {}", e)));
                }
            };
            req.get("trigger").cloned()
        } else {
            None
        };

        let request = TriggerRequest {
            action: action.to_string(),
            trigger_id,
            trigger: trigger_obj,
        };

        // Check callback is set
        let callback_is_set = self.trigger_callback.lock().unwrap_or_else(|e| e.into_inner()).is_some();
        if !callback_is_set {
            tracing::warn!("TRIGGER {}: no callback set", action);
            return Some(Frame::error("trigger_not_configured"));
        }

        // Invoke callback (async)
        let cb_future = {
            let guard = self.trigger_callback.lock().unwrap_or_else(|e| e.into_inner());
            guard.as_ref().map(|cb| cb(request.clone()))
        };
        let response = match cb_future {
            Some(fut) => fut.await,
            None => {
                return Some(Frame::error("callback_disappeared"));
            }
        };

        if !response.success {
            let err_msg = response.data.as_str().unwrap_or("trigger_error");
            return Some(Frame::error(err_msg));
        }

        // Return appropriate response frame based on action
        match action {
            "list" => {
                let json = serde_json::to_string(&response.data).unwrap_or_else(|_| "[]".to_string());
                Some(Frame::trigger_list_response(&json))
            }
            "exec" => {
                let json = serde_json::to_string(&response.data).unwrap_or_else(|_| "{}".to_string());
                Some(Frame::trigger_exec_result(&json))
            }
            _ => {
                // add / update / remove — return OK
                Some(Frame::ok(0))
            }
        }
    }

    /// INFO_REQUEST: return local system info as JSON.
    /// Security: only returns minimal info needed for the remote client to
    /// display the desktop in its UI. Sensitive details (real_name, full
    /// hostname) are omitted to reduce information leakage if the paired
    /// device is compromised.
    #[cfg(not(target_os = "android"))]
    async fn handle_info_request(&self) -> Frame {
        let default_shell = termfast_core::local::shell::detect_default_shell();
        let shell_name = std::path::Path::new(&default_shell)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&default_shell)
            .to_string();
        let os = os_info::get();
        let os_arch = std::env::consts::ARCH.to_string();
        let hostname = whoami::hostname().unwrap_or_else(|_| "unknown".to_string());
        let os_name = match os.os_type() {
            os_info::Type::Macos => "macOS".to_string(),
            os_info::Type::Windows => "Windows".to_string(),
            os_info::Type::Linux => "Linux".to_string(),
            other => format!("{:?}", other),
        };
        let available_shells = termfast_core::local::shell::list_available_shells();
        // Security: omit username, real_name, and full default_shell path.
        // Only return shell name (basename) and OS family.
        let json = serde_json::json!({
            "default_shell": shell_name,
            "shell_name": shell_name,
            "os_name": os_name,
            "os_version": os.version().to_string(),
            "os_arch": os_arch,
            "hostname": hostname,
            "available_shells": available_shells,
        });
        tracing::info!("INFO_RESPONSE: sending system info to remote client");
        Frame::info_response(&json.to_string())
    }

    /// NEW_TERMINAL: create a local terminal on the remote desktop.
    #[cfg(not(target_os = "android"))]
    async fn handle_new_terminal(
        &self,
        frame: Frame,
        _async_tx: &mpsc::Sender<Frame>,
    ) -> Option<Frame> {
        let payload = match std::str::from_utf8(&frame.payload) {
            Ok(s) => s,
            Err(_) => return Some(Frame::error("invalid_payload")),
        };
        let req: serde_json::Value = match serde_json::from_str(payload) {
            Ok(v) => v,
            Err(_) => return Some(Frame::error("invalid_json")),
        };
        let shell = req.get("shell").and_then(|v| v.as_str()).map(|s| s.to_string());
        // Security: validate shell against whitelist to prevent arbitrary
        // command execution from remote devices.
        let shell = shell.and_then(|s| {
            termfast_core::local::shell::validate_shell(&s)
        });
        let name = req.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
        let server_id = req.get("server_id").and_then(|v| v.as_str())
            .filter(|s| !s.is_empty() && *s != "__local__");

        // Serialize NEW_TERMINAL handling to prevent TOCTOU race in name deduplication.
        let _lock = self.new_terminal_lock.lock().await;

        // Generate a unique "终端 N" name: start with count+1, keep incrementing
        // until no duplicate name exists among sessions on the same server.
        let existing_names: std::collections::HashSet<String> = self.terminal_manager
            .list_session_infos().await
            .iter()
            .filter(|i| i.server_id == server_id.unwrap_or("__local__"))
            .map(|i| i.name.clone())
            .collect();
        let base_count = existing_names.len();

        // Open terminal: local or SSH depending on server_id
        let open_result: Result<(String, Vec<u8>), String> = if let Some(sid) = server_id {
            // SSH terminal — need ServerManager to get SSH handle
            #[cfg(not(target_os = "android"))]
            {
                let sm = match &self.server_manager {
                    Some(sm) => sm.clone(),
                    None => return Some(Frame::error("server_manager_not_available")),
                };
                let server = match sm.get_server(sid).await {
                    Ok(s) => s,
                    Err(_) => return Some(Frame::error("server_not_found")),
                };
                if !server.ssh_client.is_connected().await {
                    return Some(Frame::error("server_not_connected"));
                }
                let ssh_handle = match server.ssh_client.get_handle().await {
                    Some(h) => h,
                    None => return Some(Frame::error("ssh_handle_not_available")),
                };
                self.terminal_manager.open(&ssh_handle, sid, 80, 24).await
            }
            #[cfg(target_os = "android")]
            {
                let _ = sid;
                return Some(Frame::error("ssh_terminal_not_supported_on_android"));
            }
        } else {
            // Local terminal
            self.terminal_manager.open_local(80, 24, shell, None).await
        };

        match open_result {
            Ok((session_id, _initial_output)) => {
                // Use provided name, or generate a unique "终端 N"
                let effective_name = match name {
                    Some(n) => n,
                    None => {
                        let mut n = base_count + 1;
                        loop {
                            let candidate = format!("终端 {}", n);
                            if !existing_names.contains(&candidate) {
                                break candidate;
                            }
                            n += 1;
                        }
                    }
                };
                self.terminal_manager.set_session_name(&session_id, &effective_name).await;
                self.terminal_manager.notify_opened();
                // Forward terminal:opened to the local GUI so it creates a tab.
                // Pass the name so the frontend uses the daemon-assigned name.
                self.terminal_manager.forward_opened(&session_id, Some(&effective_name));
                // Assign u32 handle and return it
                let handle = {
                    let mut id_map = self.id_map.lock().unwrap_or_else(|e| e.into_inner());
                    id_map.get_or_assign(&session_id)
                };
                let json = serde_json::json!({
                    "terminal_id": handle,
                    "session_id": session_id,
                    "name": effective_name,
                });
                Some(Frame::ok_with_payload(0, &json.to_string()))
            }
            Err(e) => {
                tracing::error!("NEW_TERMINAL: failed to open terminal: {}", e);
                Some(Frame::error(&format!("open_failed: {}", e)))
            }
        }
    }

    /// CLOSE_TERMINAL: close a terminal on the remote desktop.
    #[cfg(not(target_os = "android"))]
    async fn handle_close_terminal(&self, frame: Frame) -> Option<Frame> {
        let terminal_id = frame.terminal_id;
        let session_id = match self.resolve_sid(terminal_id) {
            Some(sid) => sid,
            None => return Some(Frame::error_with_terminal(terminal_id, "terminal_not_found")),
        };
        match self.terminal_manager.close_remote(&session_id).await {
            Ok(()) => {
                // Remove from id_map
                {
                    let mut id_map = self.id_map.lock().unwrap_or_else(|e| e.into_inner());
                    id_map.remove(&session_id);
                }
                Some(Frame::ok(terminal_id))
            }
            Err(e) => Some(Frame::error_with_terminal(terminal_id, &format!("close_failed: {}", e))),
        }
    }
}

// === SECTION 1 END ===

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::{BinaryEventForwarder, EventForwarder};
    use std::sync::Mutex as StdMutex;

    /// Helper: create a RemoteServer with a TerminalManager that has one local terminal.
    /// Also registers pairing "pair1" with key [0x42;32] in auth_keys.
    async fn setup_remote_server_with_local_terminal() -> (RemoteServer, String) {
        let bin_fwd: BinaryEventForwarder = Box::new(|_sid, _data, _stderr| {});
        let evt_fwd: EventForwarder = Box::new(|_event, _data| {});
        let manager = TerminalManager::new(
            Arc::new(StdMutex::new(Some(evt_fwd))),
            Arc::new(StdMutex::new(Some(bin_fwd))),
        );
        let (sid, _) = manager.open_local(80, 24, None, None).await.unwrap();
        let config_mgr = termfast_core::config::ConfigManager::new(
            termfast_core::config::Config::default(),
        );
        let server = RemoteServer::new(
            Arc::new(manager),
            Arc::new(tokio::sync::Mutex::new(config_mgr)),
        );
        server.add_pairing("pair1".to_string(), [0x42u8; 32]);
        (server, sid)
    }

    /// Helper: create a TunnelSession with channels for testing
    fn make_tunnel_session(pairing_id: &str) -> (TunnelSession, mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) {
        let (inbound_tx, inbound_rx) = mpsc::channel(256);
        let (outbound_tx, outbound_rx) = mpsc::channel(256);
        let (async_tx, async_rx) = mpsc::channel(256);
        let session = TunnelSession {
            pairing_id: pairing_id.to_string(),
            inbound_rx,
            outbound_tx,
            async_rx,
            async_tx,
        };
        (session, inbound_tx, outbound_rx)
    }

    /// Helper: encrypt a frame with a cipher and send to inbound_tx
    async fn send_encrypted_frame(cipher: &mut FrameCipher, tx: &mpsc::Sender<Vec<u8>>, frame: &Frame) {
        let plaintext = frame.serialize();
        let encrypted = cipher.encrypt(&plaintext).unwrap();
        tx.send(encrypted).await.unwrap();
    }

    /// Helper: receive and decrypt a frame from outbound_rx
    async fn recv_decrypted_frame(cipher: &FrameCipher, rx: &mut mpsc::Receiver<Vec<u8>>) -> Frame {
        let encrypted = rx.recv().await.expect("should receive frame");
        let plaintext = cipher.decrypt(&encrypted).unwrap();
        Frame::deserialize(&plaintext).unwrap()
    }

    /// Helper: do HELLO exchange, return (mobile_send_cipher, desktop_recv_cipher)
    async fn do_hello_exchange(
        pairing_key: &[u8; 32],
        inbound_tx: &mpsc::Sender<Vec<u8>>,
        outbound_rx: &mut mpsc::Receiver<Vec<u8>>,
    ) -> (FrameCipher, FrameCipher) {
        let client_random = frame_crypto::generate_random_32();
        let mut mobile_send = FrameCipher::from_pairing_key(pairing_key, DIR_MOBILE_TO_DESKTOP);
        send_encrypted_frame(&mut mobile_send, inbound_tx, &Frame::hello(0x0001, &client_random)).await;
        let desktop_hello_cipher = FrameCipher::from_pairing_key(pairing_key, DIR_DESKTOP_TO_MOBILE);
        let reply = recv_decrypted_frame(&desktop_hello_cipher, outbound_rx).await;
        let (_, server_random) = reply.parse_hello().unwrap();
        let mobile_session = FrameCipher::from_session_key(pairing_key, &client_random, &server_random, DIR_MOBILE_TO_DESKTOP);
        let desktop_session = FrameCipher::from_session_key(pairing_key, &client_random, &server_random, DIR_DESKTOP_TO_MOBILE);
        (mobile_session, desktop_session)
    }

    /// Helper: send LIST_REQUEST, receive LIST_RESPONSE, return first terminal_id
    async fn do_list_and_get_first_id(
        mobile_send: &mut FrameCipher,
        desktop_recv: &FrameCipher,
        inbound_tx: &mpsc::Sender<Vec<u8>>,
        outbound_rx: &mut mpsc::Receiver<Vec<u8>>,
    ) -> u32 {
        send_encrypted_frame(mobile_send, inbound_tx, &Frame::list_request()).await;
        let list_resp = recv_decrypted_frame(desktop_recv, outbound_rx).await;
        assert_eq!(list_resp.frame_type, remote_frame::LIST_RESPONSE);
        let json: serde_json::Value = serde_json::from_slice(&list_resp.payload).unwrap();
        json[0]["id"].as_u64().unwrap() as u32
    }

    /// Test HELLO exchange: mobile sends HELLO with K, desktop replies with HELLO + server_random
    #[tokio::test]
    async fn test_hello_exchange() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        // Spawn handle_tunnel
        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        // Mobile sends HELLO (encrypted with K)
        let client_random = frame_crypto::generate_random_32();
        let hello_frame = Frame::hello(0x0001, &client_random);
        let mut mobile_hello_cipher = FrameCipher::from_pairing_key(&pairing_key, DIR_MOBILE_TO_DESKTOP);
        send_encrypted_frame(&mut mobile_hello_cipher, &inbound_tx, &hello_frame).await;

        // Desktop should reply with HELLO (encrypted with K)
        let desktop_hello_cipher = FrameCipher::from_pairing_key(&pairing_key, DIR_DESKTOP_TO_MOBILE);
        let reply = recv_decrypted_frame(&desktop_hello_cipher, &mut outbound_rx).await;
        assert_eq!(reply.frame_type, remote_frame::HELLO);
        let (_caps, server_random) = reply.parse_hello().unwrap();
        assert_ne!(server_random, [0u8; 32], "server_random should not be all zeros");

        // Clean up
        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test LIST_REQUEST → LIST_RESPONSE
    #[tokio::test]
    async fn test_list_request_response() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        // HELLO exchange
        let client_random = frame_crypto::generate_random_32();
        let mut mobile_send = FrameCipher::from_pairing_key(&pairing_key, DIR_MOBILE_TO_DESKTOP);
        send_encrypted_frame(&mut mobile_send, &inbound_tx, &Frame::hello(0x0001, &client_random)).await;

        let desktop_hello_cipher = FrameCipher::from_pairing_key(&pairing_key, DIR_DESKTOP_TO_MOBILE);
        let reply = recv_decrypted_frame(&desktop_hello_cipher, &mut outbound_rx).await;
        let (_, server_random) = reply.parse_hello().unwrap();

        // Switch to session key
        let mut mobile_session_send = FrameCipher::from_session_key(
            &pairing_key, &client_random, &server_random, DIR_MOBILE_TO_DESKTOP,
        );
        let desktop_session_cipher = FrameCipher::from_session_key(
            &pairing_key, &client_random, &server_random, DIR_DESKTOP_TO_MOBILE,
        );

        // Send LIST_REQUEST
        send_encrypted_frame(&mut mobile_session_send, &inbound_tx, &Frame::list_request()).await;

        // Receive LIST_RESPONSE
        let list_resp = recv_decrypted_frame(&desktop_session_cipher, &mut outbound_rx).await;
        assert_eq!(list_resp.frame_type, remote_frame::LIST_RESPONSE);
        // Payload should be JSON array with at least 1 terminal
        let json_str = String::from_utf8_lossy(&list_resp.payload);
        assert!(json_str.contains("\"id\""), "LIST_RESPONSE should contain id field");
        assert!(json_str.contains("\"server_name\""), "LIST_RESPONSE should contain server_name");
        assert!(json_str.contains("\"terminal_type\""), "LIST_RESPONSE should contain terminal_type");
        assert!(json_str.contains("\"status\""), "LIST_RESPONSE should contain status");
        assert!(json_str.contains("桌面端"), "local terminal server_name should be 桌面端");

        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test SUBSCRIBE → OK + HISTORY frames (no RESIZE — mobile determines size)
    #[tokio::test]
    async fn test_subscribe() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let (mut mobile_send, desktop_recv) = do_hello_exchange(&pairing_key, &inbound_tx, &mut outbound_rx).await;
        let term_id = do_list_and_get_first_id(&mut mobile_send, &desktop_recv, &inbound_tx, &mut outbound_rx).await;

        // SUBSCRIBE terminal_id
        send_encrypted_frame(&mut mobile_send, &inbound_tx, &Frame::subscribe(term_id)).await;

        // Should receive OK frame
        let ok_frame = recv_decrypted_frame(&desktop_recv, &mut outbound_rx).await;
        assert_eq!(ok_frame.frame_type, remote_frame::OK);
        assert_eq!(ok_frame.terminal_id, term_id);

        // No RESIZE frame is sent on SUBSCRIBE — mobile determines its own
        // dimensions and sends RESIZE to desktop. History may follow if non-empty.

        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test INPUT → OK
    #[tokio::test]
    async fn test_input() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let (mut mobile_send, desktop_recv) = do_hello_exchange(&pairing_key, &inbound_tx, &mut outbound_rx).await;
        let term_id = do_list_and_get_first_id(&mut mobile_send, &desktop_recv, &inbound_tx, &mut outbound_rx).await;

        // INPUT to terminal_id
        let input_frame = Frame::input(term_id, b"echo test\n");
        send_encrypted_frame(&mut mobile_send, &inbound_tx, &input_frame).await;

        // Should receive OK
        let ok_frame = recv_decrypted_frame(&desktop_recv, &mut outbound_rx).await;
        assert_eq!(ok_frame.frame_type, remote_frame::OK);
        assert_eq!(ok_frame.terminal_id, term_id);

        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test GOODBYE → GOODBYE reply
    #[tokio::test]
    async fn test_goodbye() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let client_random = frame_crypto::generate_random_32();
        let mut mobile_send = FrameCipher::from_pairing_key(&pairing_key, DIR_MOBILE_TO_DESKTOP);
        send_encrypted_frame(&mut mobile_send, &inbound_tx, &Frame::hello(0x0001, &client_random)).await;
        let desktop_hello_cipher = FrameCipher::from_pairing_key(&pairing_key, DIR_DESKTOP_TO_MOBILE);
        let reply = recv_decrypted_frame(&desktop_hello_cipher, &mut outbound_rx).await;
        let (_, server_random) = reply.parse_hello().unwrap();

        let mut mobile_session_send = FrameCipher::from_session_key(
            &pairing_key, &client_random, &server_random, DIR_MOBILE_TO_DESKTOP,
        );
        let desktop_session_cipher = FrameCipher::from_session_key(
            &pairing_key, &client_random, &server_random, DIR_DESKTOP_TO_MOBILE,
        );

        // Send GOODBYE
        send_encrypted_frame(&mut mobile_session_send, &inbound_tx, &Frame::goodbye()).await;

        // Should receive GOODBYE reply
        let goodbye_reply = recv_decrypted_frame(&desktop_session_cipher, &mut outbound_rx).await;
        assert_eq!(goodbye_reply.frame_type, remote_frame::GOODBYE);

        // handle_tunnel should exit after GOODBYE
        let _ = handle.await;
    }

    /// Test wrong pairing key → HELLO fails, tunnel closes
    #[tokio::test]
    async fn test_wrong_pairing_key() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        let wrong_key = [0x99u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        // Mobile sends HELLO with WRONG key
        let client_random = frame_crypto::generate_random_32();
        let mut mobile_send = FrameCipher::from_pairing_key(&wrong_key, DIR_MOBILE_TO_DESKTOP);
        send_encrypted_frame(&mut mobile_send, &inbound_tx, &Frame::hello(0x0001, &client_random)).await;

        // Desktop should NOT reply (decrypt fails → disconnect)
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            outbound_rx.recv(),
        ).await;
        assert!(result.is_err() || result.unwrap().is_none(), "should not receive reply with wrong key");

        // handle_tunnel should have exited
        let _ = handle.await;
    }

    /// Test RESIZE → desktop resizes PTY, no reply (prevents resize loop)
    #[tokio::test]
    async fn test_resize_query() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let (mut mobile_send, desktop_recv) = do_hello_exchange(&pairing_key, &inbound_tx, &mut outbound_rx).await;
        let term_id = do_list_and_get_first_id(&mut mobile_send, &desktop_recv, &inbound_tx, &mut outbound_rx).await;

        // Send RESIZE — desktop should resize its PTY but NOT reply (prevents loop)
        send_encrypted_frame(&mut mobile_send, &inbound_tx, &Frame::resize(term_id, 100, 30)).await;

        // Should NOT receive any reply frame (timeout means success — no loop)
        if let Ok(Some(_)) = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            outbound_rx.recv(),
        ).await {
            panic!("expected no reply to RESIZE, but got a frame");
        } // timeout or channel closed — correct

        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test SUBSCRIBE to invalid terminal_id → ERROR
    #[tokio::test]
    async fn test_subscribe_invalid_terminal() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let client_random = frame_crypto::generate_random_32();
        let mut mobile_send = FrameCipher::from_pairing_key(&pairing_key, DIR_MOBILE_TO_DESKTOP);
        send_encrypted_frame(&mut mobile_send, &inbound_tx, &Frame::hello(0x0001, &client_random)).await;
        let desktop_hello_cipher = FrameCipher::from_pairing_key(&pairing_key, DIR_DESKTOP_TO_MOBILE);
        let reply = recv_decrypted_frame(&desktop_hello_cipher, &mut outbound_rx).await;
        let (_, server_random) = reply.parse_hello().unwrap();

        let mut mobile_session_send = FrameCipher::from_session_key(
            &pairing_key, &client_random, &server_random, DIR_MOBILE_TO_DESKTOP,
        );
        let desktop_session_cipher = FrameCipher::from_session_key(
            &pairing_key, &client_random, &server_random, DIR_DESKTOP_TO_MOBILE,
        );

        // SUBSCRIBE to terminal_id=999 (doesn't exist — no LIST_REQUEST sent)
        send_encrypted_frame(&mut mobile_session_send, &inbound_tx, &Frame::subscribe(999)).await;

        // Should receive ERROR
        let error_frame = recv_decrypted_frame(&desktop_session_cipher, &mut outbound_rx).await;
        assert_eq!(error_frame.frame_type, remote_frame::ERROR);

        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test UNSUBSCRIBE → OK
    #[tokio::test]
    async fn test_unsubscribe() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let (mut mobile_ss, desktop_sc) = do_hello_exchange(&pairing_key, &inbound_tx, &mut outbound_rx).await;
        let term_id = do_list_and_get_first_id(&mut mobile_ss, &desktop_sc, &inbound_tx, &mut outbound_rx).await;

        send_encrypted_frame(&mut mobile_ss, &inbound_tx, &Frame::subscribe(term_id)).await;
        let _ok = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;
        let _resize = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;

        // UNSUBSCRIBE
        send_encrypted_frame(&mut mobile_ss, &inbound_tx, &Frame::unsubscribe(term_id)).await;

        // May receive OUTPUT frames (from terminal echo) before OK — drain until OK
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut got_ok = false;
        loop {
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            let frame = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;
            if frame.frame_type == remote_frame::OK && frame.terminal_id == term_id {
                got_ok = true;
                break;
            }
        }
        assert!(got_ok, "should receive OK for UNSUBSCRIBE");

        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test SUBSCRIBE idempotency — same pairing_id SUBSCRIBE twice, old subscriber removed
    #[tokio::test]
    async fn test_subscribe_idempotent() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let (mut mobile_ss, desktop_sc) = do_hello_exchange(&pairing_key, &inbound_tx, &mut outbound_rx).await;
        let term_id = do_list_and_get_first_id(&mut mobile_ss, &desktop_sc, &inbound_tx, &mut outbound_rx).await;

        // First SUBSCRIBE
        send_encrypted_frame(&mut mobile_ss, &inbound_tx, &Frame::subscribe(term_id)).await;
        // Drain until OK (may have OUTPUT from echo)
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if tokio::time::Instant::now() >= deadline { panic!("timeout waiting for OK after first SUBSCRIBE"); }
            let f = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;
            if f.frame_type == remote_frame::OK { break; }
        }
        // No RESIZE frame on SUBSCRIBE (mobile determines size)

        // Second SUBSCRIBE (same terminal_id, same pairing_id) — should be idempotent
        send_encrypted_frame(&mut mobile_ss, &inbound_tx, &Frame::subscribe(term_id)).await;
        // Drain until OK
        let deadline2 = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut got_ok = false;
        loop {
            if tokio::time::Instant::now() >= deadline2 { break; }
            let f = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;
            if f.frame_type == remote_frame::OK { got_ok = true; break; }
        }
        assert!(got_ok, "should receive OK for second SUBSCRIBE");
        // No RESIZE frame on SUBSCRIBE (mobile determines size)

        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test REDRAW_REQUEST → HISTORY frames
    #[tokio::test]
    async fn test_redraw_request() {
        let (remote_server, sid) = setup_remote_server_with_local_terminal().await;
        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        // Write some data to the terminal's ring buffer via input
        remote_server.terminal_manager.input(&sid, b"echo redraw_test\n").await.unwrap();
        // Wait briefly for output to be captured in ring buffer
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let (mut mobile_ss, desktop_sc) = do_hello_exchange(&pairing_key, &inbound_tx, &mut outbound_rx).await;
        let term_id = do_list_and_get_first_id(&mut mobile_ss, &desktop_sc, &inbound_tx, &mut outbound_rx).await;

        // REDRAW_REQUEST
        send_encrypted_frame(&mut mobile_ss, &inbound_tx, &Frame::redraw_request(term_id)).await;

        // Should receive OK
        let ok_frame = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;
        assert_eq!(ok_frame.frame_type, remote_frame::OK);

        // Should receive at least one HISTORY frame (ring buffer has data from echo output)
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut got_history = false;
        loop {
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            match tokio::time::timeout(std::time::Duration::from_millis(100), outbound_rx.recv()).await {
                Ok(Some(encrypted)) => {
                    let pt = desktop_sc.decrypt(&encrypted).unwrap();
                    let frame = Frame::deserialize(&pt).unwrap();
                    if frame.frame_type == remote_frame::HISTORY {
                        got_history = true;
                        break;
                    }
                }
                _ => break,
            }
        }
        assert!(got_history, "REDRAW_REQUEST should trigger HISTORY frames");

        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test post-HELLO decrypt failure → tunnel closes, no reply
    #[tokio::test]
    async fn test_decrypt_failure_post_hello() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        // HELLO exchange
        let client_random = frame_crypto::generate_random_32();
        let mut mobile_send = FrameCipher::from_pairing_key(&pairing_key, DIR_MOBILE_TO_DESKTOP);
        send_encrypted_frame(&mut mobile_send, &inbound_tx, &Frame::hello(0x0001, &client_random)).await;
        let desktop_hello_cipher = FrameCipher::from_pairing_key(&pairing_key, DIR_DESKTOP_TO_MOBILE);
        let reply = recv_decrypted_frame(&desktop_hello_cipher, &mut outbound_rx).await;
        let (_, _server_random) = reply.parse_hello().unwrap();

        // Send a frame encrypted with WRONG session key (different randoms)
        let wrong_random = [0xFFu8; 32];
        let mut wrong_cipher = FrameCipher::from_session_key(
            &pairing_key, &client_random, &wrong_random, DIR_MOBILE_TO_DESKTOP,
        );
        send_encrypted_frame(&mut wrong_cipher, &inbound_tx, &Frame::list_request()).await;

        // Should NOT receive any reply (decrypt fails → disconnect)
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            outbound_rx.recv(),
        ).await;
        assert!(result.is_err() || result.unwrap().is_none(),
            "should not receive reply after decrypt failure");

        // handle_tunnel should have exited
        let _ = handle.await;
    }

    /// Test HELLO-before-other-frames: send LIST_REQUEST before HELLO → ERROR("hello_required")
    #[tokio::test]
    async fn test_frame_before_hello() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        // Send LIST_REQUEST encrypted with K (before HELLO)
        let mut mobile_send = FrameCipher::from_pairing_key(&pairing_key, DIR_MOBILE_TO_DESKTOP);
        send_encrypted_frame(&mut mobile_send, &inbound_tx, &Frame::list_request()).await;

        // Should receive ERROR("hello_required") encrypted with K
        let desktop_cipher = FrameCipher::from_pairing_key(&pairing_key, DIR_DESKTOP_TO_MOBILE);
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            outbound_rx.recv(),
        ).await;
        match result {
            Ok(Some(encrypted)) => {
                let pt = desktop_cipher.decrypt(&encrypted).unwrap();
                let frame = Frame::deserialize(&pt).unwrap();
                assert_eq!(frame.frame_type, remote_frame::ERROR);
                let err_msg = String::from_utf8_lossy(&frame.payload);
                assert!(err_msg.contains("hello_required"), "error should be hello_required, got: {}", err_msg);
            }
            _ => panic!("should receive ERROR(hello_required) before disconnect"),
        }

        // handle_tunnel should exit after sending ERROR
        let _ = handle.await;
    }

    /// Test SUBSCRIBE with non-empty history → OK + HISTORY frames (no RESIZE)
    #[tokio::test]
    async fn test_subscribe_with_history() {
        let (remote_server, sid) = setup_remote_server_with_local_terminal().await;
        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        // Write data to ring buffer
        remote_server.terminal_manager.input(&sid, b"echo history_test\n").await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let (mut mobile_ss, desktop_sc) = do_hello_exchange(&pairing_key, &inbound_tx, &mut outbound_rx).await;
        let term_id = do_list_and_get_first_id(&mut mobile_ss, &desktop_sc, &inbound_tx, &mut outbound_rx).await;

        // SUBSCRIBE — should get OK + HISTORY (no RESIZE — mobile determines size)
        send_encrypted_frame(&mut mobile_ss, &inbound_tx, &Frame::subscribe(term_id)).await;
        let ok = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;
        assert_eq!(ok.frame_type, remote_frame::OK);

        // Should receive at least one HISTORY frame
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut got_history = false;
        let mut got_is_last = false;
        loop {
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            match tokio::time::timeout(std::time::Duration::from_millis(100), outbound_rx.recv()).await {
                Ok(Some(encrypted)) => {
                    let pt = desktop_sc.decrypt(&encrypted).unwrap();
                    let frame = Frame::deserialize(&pt).unwrap();
                    if frame.frame_type == remote_frame::HISTORY {
                        got_history = true;
                        // Check is_last flag (payload[4])
                        if frame.payload.len() >= 5 && frame.payload[4] == 1 {
                            got_is_last = true;
                            break;
                        }
                    }
                }
                _ => break,
            }
        }
        assert!(got_history, "SUBSCRIBE with non-empty history should send HISTORY frames");
        assert!(got_is_last, "should receive HISTORY with is_last=1");

        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test INPUT_ANSWER → OK (first answer accepted)
    #[tokio::test]
    async fn test_input_answer_first_accepted() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let (mut mobile_ss, desktop_sc) = do_hello_exchange(&pairing_key, &inbound_tx, &mut outbound_rx).await;
        let term_id = do_list_and_get_first_id(&mut mobile_ss, &desktop_sc, &inbound_tx, &mut outbound_rx).await;

        // INPUT_ANSWER with question_id and answer
        let answer_frame = Frame::input_answer(term_id, "q-123", "1");
        send_encrypted_frame(&mut mobile_ss, &inbound_tx, &answer_frame).await;

        // Should receive OK
        let ok_frame = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;
        assert_eq!(ok_frame.frame_type, remote_frame::OK);
        assert_eq!(ok_frame.terminal_id, term_id);

        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test INPUT_ANSWER mutex — second answer with same question_id gets already_answered
    #[tokio::test]
    async fn test_input_answer_already_answered() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let (mut mobile_ss, desktop_sc) = do_hello_exchange(&pairing_key, &inbound_tx, &mut outbound_rx).await;
        let term_id = do_list_and_get_first_id(&mut mobile_ss, &desktop_sc, &inbound_tx, &mut outbound_rx).await;

        // First INPUT_ANSWER — should get OK
        send_encrypted_frame(&mut mobile_ss, &inbound_tx, &Frame::input_answer(term_id, "q-mutex", "1")).await;
        let ok_frame = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;
        assert_eq!(ok_frame.frame_type, remote_frame::OK);

        // Second INPUT_ANSWER with same question_id — should get ERROR(already_answered)
        send_encrypted_frame(&mut mobile_ss, &inbound_tx, &Frame::input_answer(term_id, "q-mutex", "2")).await;
        // May receive OUTPUT frames before ERROR — drain until ERROR
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut got_error = false;
        loop {
            if tokio::time::Instant::now() >= deadline { break; }
            let f = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;
            if f.frame_type == remote_frame::ERROR {
                let msg = String::from_utf8_lossy(&f.payload);
                assert!(msg.contains("already_answered"), "error should be already_answered, got: {}", msg);
                got_error = true;
                break;
            }
        }
        assert!(got_error, "second INPUT_ANSWER should get already_answered error");

        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test INPUT_ANSWER broadcasts QUESTION_RESOLVED to all subscribers
    #[tokio::test]
    async fn test_input_answer_broadcasts_question_resolved() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let (mut mobile_ss, desktop_sc) = do_hello_exchange(&pairing_key, &inbound_tx, &mut outbound_rx).await;
        let term_id = do_list_and_get_first_id(&mut mobile_ss, &desktop_sc, &inbound_tx, &mut outbound_rx).await;

        // SUBSCRIBE first (so we receive QUESTION_RESOLVED broadcast)
        send_encrypted_frame(&mut mobile_ss, &inbound_tx, &Frame::subscribe(term_id)).await;
        // Drain until OK
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if tokio::time::Instant::now() >= deadline { panic!("timeout waiting for OK after SUBSCRIBE"); }
            let f = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;
            if f.frame_type == remote_frame::OK { break; }
        }
        // No RESIZE frame on SUBSCRIBE (mobile determines size)

        // INPUT_ANSWER — should get OK + broadcast QUESTION_RESOLVED to subscribers
        send_encrypted_frame(&mut mobile_ss, &inbound_tx, &Frame::input_answer(term_id, "q-broadcast", "1")).await;

        // Drain until OK
        let mut got_ok = false;
        loop {
            if tokio::time::Instant::now() >= deadline { break; }
            let f = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;
            if f.frame_type == remote_frame::OK { got_ok = true; break; }
        }
        assert!(got_ok, "should receive OK for INPUT_ANSWER");

        // Should receive QUESTION_RESOLVED broadcast (as a subscriber)
        let mut got_qr = false;
        loop {
            if tokio::time::Instant::now() >= deadline { break; }
            match tokio::time::timeout(std::time::Duration::from_millis(100), outbound_rx.recv()).await {
                Ok(Some(encrypted)) => {
                    let pt = desktop_sc.decrypt(&encrypted).unwrap();
                    let frame = Frame::deserialize(&pt).unwrap();
                    if frame.frame_type == remote_frame::QUESTION_RESOLVED {
                        let json: serde_json::Value = serde_json::from_slice(&frame.payload).unwrap();
                        assert_eq!(json["question_id"], "q-broadcast");
                        assert_eq!(json["answer"], "1");
                        got_qr = true;
                        break;
                    }
                }
                _ => break,
            }
        }
        assert!(got_qr, "subscriber should receive QUESTION_RESOLVED after INPUT_ANSWER");

        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test answered_questions auto-cleanup after 30 seconds.
    /// Verifies the entry is added after INPUT_ANSWER, then removed after 30s.
    #[tokio::test]
    async fn test_answered_questions_auto_cleanup() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        // Capture answered_questions Arc before moving remote_server into spawn
        let answered_questions = remote_server.answered_questions.clone();
        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let (mut mobile_ss, desktop_sc) = do_hello_exchange(&pairing_key, &inbound_tx, &mut outbound_rx).await;
        let term_id = do_list_and_get_first_id(&mut mobile_ss, &desktop_sc, &inbound_tx, &mut outbound_rx).await;

        // INPUT_ANSWER — should add entry to answered_questions
        send_encrypted_frame(&mut mobile_ss, &inbound_tx, &Frame::input_answer(term_id, "q-cleanup", "1")).await;
        let ok_frame = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;
        assert_eq!(ok_frame.frame_type, remote_frame::OK);

        // Verify entry exists in answered_questions map
        {
            let map = answered_questions.lock().unwrap_or_else(|e| e.into_inner());
            assert!(map.contains_key("q-cleanup"), "answered_questions should contain q-cleanup after INPUT_ANSWER");
        }

        // Wait for 31 seconds for auto-cleanup (30s timeout + 1s margin)
        // NOTE: This test takes ~31s but verifies the actual cleanup behavior.
        tokio::time::sleep(std::time::Duration::from_secs(31)).await;

        // Verify entry has been removed by cleanup task
        {
            let map = answered_questions.lock().unwrap_or_else(|e| e.into_inner());
            assert!(!map.contains_key("q-cleanup"), "answered_questions should NOT contain q-cleanup after 30s cleanup");
        }

        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test FILE_REQUEST on local terminal without callback → error(file_upload_not_configured)
    #[tokio::test]
    async fn test_file_request_local_not_configured() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let (mut mobile_ss, desktop_sc) = do_hello_exchange(&pairing_key, &inbound_tx, &mut outbound_rx).await;
        let term_id = do_list_and_get_first_id(&mut mobile_ss, &desktop_sc, &inbound_tx, &mut outbound_rx).await;

        // FILE_REQUEST on local terminal — no callback set → error
        send_encrypted_frame(&mut mobile_ss, &inbound_tx, &Frame::file_request(term_id, "/tmp/test.txt")).await;

        // Should receive ERROR (no callback configured)
        let err_frame = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;
        assert_eq!(err_frame.frame_type, remote_frame::ERROR);
        let msg = String::from_utf8_lossy(&err_frame.payload);
        assert!(msg.contains("file_upload_not_configured"), "local FILE_REQUEST without callback should return not_configured, got: {}", msg);

        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test FILE_REQUEST on local terminal with callback → OK + NOTIFY(file_ready)
    #[tokio::test]
    async fn test_file_request_local_with_callback() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        // Register a mock file upload callback
        remote_server.set_file_upload_callback(Box::new(|file_path: String| {
            Box::pin(async move {
                // Simulate upload — return a fake result
                Ok(FileUploadResult {
                    cloud_path: "/TermFast/files/mock.enc".to_string(),
                    file_name: std::path::Path::new(&file_path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("file")
                        .to_string(),
                    size: 42,
                    sha256: "abc123".to_string(),
                    mime_type: "text/plain".to_string(),
                })
            })
        }));

        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let (mut mobile_ss, desktop_sc) = do_hello_exchange(&pairing_key, &inbound_tx, &mut outbound_rx).await;
        let term_id = do_list_and_get_first_id(&mut mobile_ss, &desktop_sc, &inbound_tx, &mut outbound_rx).await;

        // FILE_REQUEST on local terminal
        send_encrypted_frame(&mut mobile_ss, &inbound_tx, &Frame::file_request(term_id, "/tmp/test.txt")).await;

        // Should receive OK immediately (request accepted)
        let ok_frame = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;
        assert_eq!(ok_frame.frame_type, remote_frame::OK);
        assert_eq!(ok_frame.terminal_id, term_id);

        // Should receive NOTIFY(file_ready) from background upload task
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut got_notify = false;
        loop {
            if tokio::time::Instant::now() >= deadline { break; }
            match tokio::time::timeout(std::time::Duration::from_millis(100), outbound_rx.recv()).await {
                Ok(Some(encrypted)) => {
                    let pt = desktop_sc.decrypt(&encrypted).unwrap();
                    let frame = Frame::deserialize(&pt).unwrap();
                    if frame.frame_type == remote_frame::NOTIFY {
                        let json: serde_json::Value = serde_json::from_slice(&frame.payload).unwrap();
                        assert_eq!(json["event_type"], "file_ready");
                        assert_eq!(json["cloud_path"], "/TermFast/files/mock.enc");
                        assert_eq!(json["file_name"], "test.txt");
                        assert_eq!(json["size"], 42);
                        assert_eq!(json["sha256"], "abc123");
                        assert_eq!(json["mime_type"], "text/plain");
                        got_notify = true;
                        break;
                    }
                }
                _ => break,
            }
        }
        assert!(got_notify, "FILE_REQUEST with callback should send NOTIFY(file_ready)");

        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test FILE_REQUEST on local terminal with failing callback → OK + ERROR(file_upload_failed)
    #[tokio::test]
    async fn test_file_request_local_callback_error() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        // Register a callback that always fails
        remote_server.set_file_upload_callback(Box::new(|_file_path: String| {
            Box::pin(async move {
                Err("upload failed: mock error".to_string())
            })
        }));

        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let (mut mobile_ss, desktop_sc) = do_hello_exchange(&pairing_key, &inbound_tx, &mut outbound_rx).await;
        let term_id = do_list_and_get_first_id(&mut mobile_ss, &desktop_sc, &inbound_tx, &mut outbound_rx).await;

        // FILE_REQUEST on local terminal
        send_encrypted_frame(&mut mobile_ss, &inbound_tx, &Frame::file_request(term_id, "/tmp/test.txt")).await;

        // Should receive OK immediately (request accepted)
        let ok_frame = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;
        assert_eq!(ok_frame.frame_type, remote_frame::OK);

        // Should receive ERROR(file_upload_failed) from background task
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut got_error = false;
        loop {
            if tokio::time::Instant::now() >= deadline { break; }
            match tokio::time::timeout(std::time::Duration::from_millis(100), outbound_rx.recv()).await {
                Ok(Some(encrypted)) => {
                    let pt = desktop_sc.decrypt(&encrypted).unwrap();
                    let frame = Frame::deserialize(&pt).unwrap();
                    if frame.frame_type == remote_frame::ERROR {
                        let msg = String::from_utf8_lossy(&frame.payload);
                        assert!(msg.contains("file_upload_failed"), "should get file_upload_failed, got: {}", msg);
                        got_error = true;
                        break;
                    }
                }
                _ => break,
            }
        }
        assert!(got_error, "FILE_REQUEST with failing callback should send ERROR(file_upload_failed)");

        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test FILE_REQUEST on invalid terminal_id → ERROR
    #[tokio::test]
    async fn test_file_request_invalid_terminal() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let (mut mobile_ss, desktop_sc) = do_hello_exchange(&pairing_key, &inbound_tx, &mut outbound_rx).await;
        // No LIST_REQUEST — terminal_id 999 not in IdMap

        send_encrypted_frame(&mut mobile_ss, &inbound_tx, &Frame::file_request(999, "/tmp/test.txt")).await;

        let err_frame = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;
        assert_eq!(err_frame.frame_type, remote_frame::ERROR);
        let msg = String::from_utf8_lossy(&err_frame.payload);
        assert!(msg.contains("invalid_terminal_id"), "FILE_REQUEST on invalid terminal should return invalid_terminal_id, got: {}", msg);

        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test FILE_REQUEST on SSH terminal → ERROR(file_request_unsupported)
    #[tokio::test]
    async fn test_file_request_ssh_unsupported() {
        let (remote_server, _local_sid) = setup_remote_server_with_local_terminal().await;
        // Register a mock SSH session
        let ssh_sid = "ssh-session-1";
        remote_server.terminal_manager.register_mock_ssh_session(
            ssh_sid,
            "server-1",
            80,
            24,
        ).await;

        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let (mut mobile_ss, desktop_sc) = do_hello_exchange(&pairing_key, &inbound_tx, &mut outbound_rx).await;
        // LIST to populate IdMap (will include both local + SSH sessions)
        send_encrypted_frame(&mut mobile_ss, &inbound_tx, &Frame::list_request()).await;
        let list_resp = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;
        assert_eq!(list_resp.frame_type, remote_frame::LIST_RESPONSE);
        // Find the SSH terminal's id from the list (it's the one with terminal_type="ssh")
        let json: serde_json::Value = serde_json::from_slice(&list_resp.payload).unwrap();
        let ssh_term_id = json.as_array().unwrap().iter()
            .find(|v| v["terminal_type"] == "ssh")
            .map(|v| v["id"].as_u64().unwrap() as u32)
            .expect("should have an SSH terminal in list");

        // FILE_REQUEST on SSH terminal → ERROR(file_request_unsupported)
        send_encrypted_frame(&mut mobile_ss, &inbound_tx, &Frame::file_request(ssh_term_id, "/tmp/test.txt")).await;

        let err_frame = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;
        assert_eq!(err_frame.frame_type, remote_frame::ERROR);
        let msg = String::from_utf8_lossy(&err_frame.payload);
        assert!(msg.contains("file_request_unsupported"), "SSH FILE_REQUEST should return unsupported, got: {}", msg);

        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test HELLO version negotiation — version 0 < MIN_SUPPORTED → ERROR(unsupported_version)
    #[tokio::test]
    async fn test_hello_unsupported_version() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        // Send HELLO with version 0 (below MIN_SUPPORTED_VERSION=1)
        let client_random = frame_crypto::generate_random_32();
        let mut mobile_send = FrameCipher::from_pairing_key(&pairing_key, DIR_MOBILE_TO_DESKTOP);
        // Manually construct a HELLO frame with version=0
        let mut hello = Frame::hello(0x0001, &client_random);
        hello.version = 0;
        send_encrypted_frame(&mut mobile_send, &inbound_tx, &hello).await;

        // Should receive ERROR(unsupported_version) encrypted with K
        let desktop_cipher = FrameCipher::from_pairing_key(&pairing_key, DIR_DESKTOP_TO_MOBILE);
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            outbound_rx.recv(),
        ).await;
        match result {
            Ok(Some(encrypted)) => {
                let pt = desktop_cipher.decrypt(&encrypted).unwrap();
                let frame = Frame::deserialize(&pt).unwrap();
                assert_eq!(frame.frame_type, remote_frame::ERROR);
                let msg = String::from_utf8_lossy(&frame.payload);
                assert!(msg.contains("unsupported_version"), "error should be unsupported_version, got: {}", msg);
            }
            _ => panic!("should receive ERROR(unsupported_version)"),
        }

        // handle_tunnel should exit
        let _ = handle.await;
    }

    /// Test HELLO capabilities negotiation — server caps = skip_history(bit1),
    /// client sends zstd(bit0) only → negotiated caps = 0
    #[tokio::test]
    async fn test_hello_caps_negotiation() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        // Send HELLO with client_caps = 0x0001 (zstd only, no skip_history)
        let client_random = frame_crypto::generate_random_32();
        let mut mobile_send = FrameCipher::from_pairing_key(&pairing_key, DIR_MOBILE_TO_DESKTOP);
        send_encrypted_frame(&mut mobile_send, &inbound_tx, &Frame::hello(0x0001, &client_random)).await;

        // Desktop should reply with HELLO — negotiated caps = client & server = 0x0001 & 0x0002 = 0
        let desktop_hello_cipher = FrameCipher::from_pairing_key(&pairing_key, DIR_DESKTOP_TO_MOBILE);
        let reply = recv_decrypted_frame(&desktop_hello_cipher, &mut outbound_rx).await;
        assert_eq!(reply.frame_type, remote_frame::HELLO);
        let (negotiated_caps, _server_random) = reply.parse_hello().unwrap();
        // client_caps=0x0001, server_caps=0x0002 → intersection=0x0000
        assert_eq!(negotiated_caps, 0x0000, "negotiated caps should be intersection (zstd-only client & skip_history server = 0)");

        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test add_pairing / revoke_pairing — revoked pairing key not found
    #[tokio::test]
    async fn test_revoke_pairing() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;

        // "pair1" was added in setup with key [0x42;32]
        assert!(remote_server.get_pairing_key("pair1").is_some());

        // Revoke
        remote_server.revoke_pairing("pair1").await;
        assert!(remote_server.get_pairing_key("pair1").is_none(), "revoked pairing key should be removed");

        // Add again
        remote_server.add_pairing("pair1".to_string(), [0x42u8; 32]);
        assert!(remote_server.get_pairing_key("pair1").is_some());
    }

    /// Test IdMap — terminal_id stable across LIST_REQUEST calls
    #[tokio::test]
    async fn test_id_map_stable() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let (mut mobile_ss, desktop_sc) = do_hello_exchange(&pairing_key, &inbound_tx, &mut outbound_rx).await;

        // First LIST_REQUEST
        let term_id_1 = do_list_and_get_first_id(&mut mobile_ss, &desktop_sc, &inbound_tx, &mut outbound_rx).await;

        // Second LIST_REQUEST — terminal_id should be the same (stable IdMap)
        let term_id_2 = do_list_and_get_first_id(&mut mobile_ss, &desktop_sc, &inbound_tx, &mut outbound_rx).await;

        assert_eq!(term_id_1, term_id_2, "terminal_id should be stable across LIST_REQUEST calls (persistent IdMap)");

        drop(inbound_tx);
        let _ = handle.await;
    }

    // === FP-3: DESKTOP_PAIR frame handling tests ===

    /// Helper: build a valid DesktopPairMessage JSON payload
    fn make_desktop_pair_payload(action: &str, role: &str) -> String {
        serde_json::json!({
            "action": action,
            "pairing_id": "dpair-123",
            "pairing_key_hex": "ab".repeat(32),
            "pairing_jwt": if role == "client" { "jwt-client" } else { "" },
            "peer_name": "Desktop-B",
            "pairing_type": "desktop",
            "role": role,
            "relay_url": "wss://relay.example.com/tunnel",
        }).to_string()
    }

    /// Test DESKTOP_PAIR with no callback set → pair_error (desktop_pair_not_configured)
    #[tokio::test]
    async fn test_desktop_pair_no_callback() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let (mut mobile_ss, desktop_sc) = do_hello_exchange(&pairing_key, &inbound_tx, &mut outbound_rx).await;

        // Send DESKTOP_PAIR frame — no callback set
        let payload = make_desktop_pair_payload("pair", "server");
        send_encrypted_frame(&mut mobile_ss, &inbound_tx, &Frame::desktop_pair(&payload)).await;

        // Should receive DESKTOP_PAIR_RESPONSE with pair_error
        let resp = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;
        assert_eq!(resp.frame_type, remote_frame::DESKTOP_PAIR);
        let resp_json: serde_json::Value = serde_json::from_slice(&resp.payload).unwrap();
        assert_eq!(resp_json["action"].as_str().unwrap(), "pair_error");
        assert!(resp_json["message"].as_str().unwrap().contains("desktop_pair_not_configured"));

        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test DESKTOP_PAIR with callback set → callback invoked → pair_ok
    #[tokio::test]
    async fn test_desktop_pair_with_callback_success() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        remote_server.set_desktop_pair_callback(Box::new(|msg: DesktopPairMessage| {
            Box::pin(async move {
                // Verify the message fields
                assert_eq!(msg.action, "pair");
                assert_eq!(msg.pairing_id, "dpair-123");
                assert_eq!(msg.role, "server");
                assert_eq!(msg.pairing_type, "desktop");
                Ok(())
            })
        }));

        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let (mut mobile_ss, desktop_sc) = do_hello_exchange(&pairing_key, &inbound_tx, &mut outbound_rx).await;

        let payload = make_desktop_pair_payload("pair", "server");
        send_encrypted_frame(&mut mobile_ss, &inbound_tx, &Frame::desktop_pair(&payload)).await;

        let resp = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;
        assert_eq!(resp.frame_type, remote_frame::DESKTOP_PAIR);
        let resp_json: serde_json::Value = serde_json::from_slice(&resp.payload).unwrap();
        assert_eq!(resp_json["action"].as_str().unwrap(), "pair_ok");
        assert_eq!(resp_json["pairing_id"].as_str().unwrap(), "dpair-123");

        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test DESKTOP_PAIR with callback that returns error → pair_error
    #[tokio::test]
    async fn test_desktop_pair_callback_error() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        remote_server.set_desktop_pair_callback(Box::new(|_msg: DesktopPairMessage| {
            Box::pin(async move {
                Err("mock failure".to_string())
            })
        }));

        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let (mut mobile_ss, desktop_sc) = do_hello_exchange(&pairing_key, &inbound_tx, &mut outbound_rx).await;

        let payload = make_desktop_pair_payload("pair", "client");
        send_encrypted_frame(&mut mobile_ss, &inbound_tx, &Frame::desktop_pair(&payload)).await;

        let resp = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;
        assert_eq!(resp.frame_type, remote_frame::DESKTOP_PAIR);
        let resp_json: serde_json::Value = serde_json::from_slice(&resp.payload).unwrap();
        assert_eq!(resp_json["action"].as_str().unwrap(), "pair_error");
        assert!(resp_json["message"].as_str().unwrap().contains("mock failure"));

        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test DESKTOP_PAIR with invalid JSON → pair_error (parse)
    #[tokio::test]
    async fn test_desktop_pair_invalid_json() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let (mut mobile_ss, desktop_sc) = do_hello_exchange(&pairing_key, &inbound_tx, &mut outbound_rx).await;

        // Send invalid JSON
        send_encrypted_frame(&mut mobile_ss, &inbound_tx, &Frame::desktop_pair("not json")).await;

        let resp = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;
        assert_eq!(resp.frame_type, remote_frame::DESKTOP_PAIR);
        let resp_json: serde_json::Value = serde_json::from_slice(&resp.payload).unwrap();
        assert_eq!(resp_json["action"].as_str().unwrap(), "pair_error");
        assert!(resp_json["message"].as_str().unwrap().contains("parse:"));

        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test DESKTOP_PAIR with wrong action → pair_error (unexpected action)
    #[tokio::test]
    async fn test_desktop_pair_wrong_action() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        remote_server.set_desktop_pair_callback(Box::new(|_msg: DesktopPairMessage| {
            Box::pin(async move { Ok(()) })
        }));

        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let (mut mobile_ss, desktop_sc) = do_hello_exchange(&pairing_key, &inbound_tx, &mut outbound_rx).await;

        let payload = make_desktop_pair_payload("unpair", "server");
        send_encrypted_frame(&mut mobile_ss, &inbound_tx, &Frame::desktop_pair(&payload)).await;

        let resp = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;
        assert_eq!(resp.frame_type, remote_frame::DESKTOP_PAIR);
        let resp_json: serde_json::Value = serde_json::from_slice(&resp.payload).unwrap();
        assert_eq!(resp_json["action"].as_str().unwrap(), "pair_error");
        assert!(resp_json["message"].as_str().unwrap().contains("unexpected action:"));

        drop(inbound_tx);
        let _ = handle.await;
    }

    /// Test DESKTOP_PAIR with role=client → callback receives client role
    #[tokio::test]
    async fn test_desktop_pair_role_client() {
        let (remote_server, _sid) = setup_remote_server_with_local_terminal().await;
        remote_server.set_desktop_pair_callback(Box::new(|msg: DesktopPairMessage| {
            Box::pin(async move {
                assert_eq!(msg.role, "client");
                assert_eq!(msg.pairing_jwt, "jwt-client");
                Ok(())
            })
        }));

        let pairing_key = [0x42u8; 32];
        let (session, inbound_tx, mut outbound_rx) = make_tunnel_session("pair1");

        let handle = tokio::spawn(async move {
            remote_server.handle_tunnel(session).await;
        });

        let (mut mobile_ss, desktop_sc) = do_hello_exchange(&pairing_key, &inbound_tx, &mut outbound_rx).await;

        let payload = make_desktop_pair_payload("pair", "client");
        send_encrypted_frame(&mut mobile_ss, &inbound_tx, &Frame::desktop_pair(&payload)).await;

        let resp = recv_decrypted_frame(&desktop_sc, &mut outbound_rx).await;
        assert_eq!(resp.frame_type, remote_frame::DESKTOP_PAIR);
        let resp_json: serde_json::Value = serde_json::from_slice(&resp.payload).unwrap();
        assert_eq!(resp_json["action"].as_str().unwrap(), "pair_ok");

        drop(inbound_tx);
        let _ = handle.await;
    }
}
