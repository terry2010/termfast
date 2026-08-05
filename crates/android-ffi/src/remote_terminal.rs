//! Remote terminal tunnel session management for Android.
//!
//! Manages per-pairing_id crypto state and protocol frame processing.
//! The Kotlin TunnelClient handles WebSocket transport (connect/disconnect/
//! reconnect + relay control messages); this module handles frame
//! encryption/decryption and protocol logic (HELLO exchange, LIST/SUBSCRIBE/
//! OUTPUT/HISTORY/RESIZE/INPUT).
//!
//! Architecture:
//! 1. Kotlin TunnelClient connects WebSocket, receives `peer_connected`
//! 2. Kotlin calls `init_tunnel` → gets encrypted HELLO → sends via WebSocket
//! 3. Kotlin receives binary frame → calls `process_binary` → Rust decrypts,
//!    processes, dispatches events to Kotlin
//! 4. Kotlin sends user input → calls `send_input` → gets encrypted INPUT
//!    frame → sends via WebSocket

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use base64::Engine;
use termfast_daemon::frame_crypto::{
    generate_random_32, FrameCipher, DIR_DESKTOP_TO_MOBILE, DIR_MOBILE_TO_DESKTOP,
};
use termfast_daemon::remote_frame::Frame;

/// Capabilities flags advertised in HELLO.
/// Currently no optional capabilities (compression etc. is future work).
const CLIENT_CAPABILITIES: u16 = 0x0001;

/// Phase of the tunnel handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TunnelPhase {
    /// HELLO sent, waiting for server HELLO response.
    WaitingHello,
    /// Session key established, ready for protocol frames.
    Ready,
}

/// Per-pairing_id tunnel session state.
struct RemoteTunnelSession {
    pairing_key: [u8; 32],
    phase: TunnelPhase,
    /// Client random generated for HELLO (kept for session key derivation).
    client_random: [u8; 32],
    /// Send cipher (mobile → desktop, direction = 1).
    /// Created after HELLO exchange completes.
    send_cipher: Option<FrameCipher>,
    /// Receive cipher (desktop → mobile, direction = 0).
    /// Created after HELLO exchange completes.
    recv_cipher: Option<FrameCipher>,
}

/// Global registry of tunnel sessions (pairing_id → session).
static TUNNEL_SESSIONS: OnceLock<Mutex<HashMap<String, RemoteTunnelSession>>> = OnceLock::new();

fn sessions() -> &'static Mutex<HashMap<String, RemoteTunnelSession>> {
    TUNNEL_SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

// === SECTION 1 END ===

/// Initialize a tunnel session: generate client_random, create encrypted HELLO.
///
/// Returns the encrypted HELLO bytes to send via WebSocket binary frame.
/// The caller (Kotlin) should send this immediately after `peer_connected`.
pub fn init_tunnel(pairing_id: &str, pairing_key: &[u8; 32]) -> Result<Vec<u8>, String> {
    let client_random = generate_random_32();

    // Create HELLO frame (version=1, capabilities, client_random)
    let hello_frame = Frame::hello(CLIENT_CAPABILITIES, &client_random);

    // Encrypt HELLO with pairing key K (session key not yet derived)
    let mut hello_cipher = FrameCipher::from_pairing_key(pairing_key, DIR_MOBILE_TO_DESKTOP);
    let encrypted = hello_cipher.encrypt(&hello_frame.serialize())?;

    // Store session state
    let session = RemoteTunnelSession {
        pairing_key: *pairing_key,
        phase: TunnelPhase::WaitingHello,
        client_random,
        send_cipher: None,
        recv_cipher: None,
    };
    sessions()
        .lock()
        .unwrap()
        .insert(pairing_id.to_string(), session);

    Ok(encrypted)
}

/// Process a binary frame received from the relay (via Kotlin TunnelClient).
///
/// Decrypts the frame and dispatches events to Kotlin via the event callback.
/// Returns `Ok(())` on success, `Err` on decryption/parsing errors.
pub fn process_binary(pairing_id: &str, data: &[u8]) -> Result<(), String> {
    let mut map = sessions().lock().unwrap();
    let session = map
        .get_mut(pairing_id)
        .ok_or_else(|| format!("no tunnel session for pairing_id={}", pairing_id))?;

    match session.phase {
        TunnelPhase::WaitingHello => {
            // Decrypt HELLO response with pairing key K
            let hello_cipher =
                FrameCipher::from_pairing_key(&session.pairing_key, DIR_DESKTOP_TO_MOBILE);
            let plaintext = hello_cipher.decrypt(data)?;
            let frame = Frame::deserialize(&plaintext).map_err(|e| e.to_string())?;

            if frame.frame_type != termfast_daemon::remote_frame::HELLO {
                return Err(format!(
                    "expected HELLO response, got frame_type=0x{:02X}",
                    frame.frame_type
                ));
            }

            let (_server_caps, server_random) = frame
                .parse_hello()
                .ok_or_else(|| "invalid HELLO payload".to_string())?;

            // Derive session key and create ciphers for both directions
            let send_cipher = FrameCipher::from_session_key(
                &session.pairing_key,
                &session.client_random,
                &server_random,
                DIR_MOBILE_TO_DESKTOP,
            );
            let recv_cipher = FrameCipher::from_session_key(
                &session.pairing_key,
                &session.client_random,
                &server_random,
                DIR_DESKTOP_TO_MOBILE,
            );

            session.send_cipher = Some(send_cipher);
            session.recv_cipher = Some(recv_cipher);
            session.phase = TunnelPhase::Ready;

            // Dispatch RemoteTunnelReady event
            drop(map);
            dispatch_ready(pairing_id);
            Ok(())
        }
        TunnelPhase::Ready => {
            // Decrypt with session key
            let recv_cipher = session
                .recv_cipher
                .as_ref()
                .ok_or_else(|| "recv_cipher not initialized".to_string())?;
            let plaintext = recv_cipher.decrypt(data)?;
            let frame = Frame::deserialize(&plaintext).map_err(|e| e.to_string())?;

            // Process frame based on type, dispatch events
            let frame_type = frame.frame_type;
            let terminal_id = frame.terminal_id;
            let payload = frame.payload.clone();

            drop(map);
            dispatch_frame_event(pairing_id, frame_type, terminal_id, &payload);
            Ok(())
        }
    }
}

// === SECTION 2 END ===

/// Dispatch a RemoteTunnelReady event to Kotlin.
fn dispatch_ready(pairing_id: &str) {
    #[cfg(target_os = "android")]
    {
        let event = crate::event::RustEvent::RemoteTunnelReady {
            pairing_id: pairing_id.to_string(),
        };
        crate::jni::dispatch_event_to_kotlin(&event.to_json());
    }
    let _ = pairing_id;
}

/// Dispatch a protocol frame event to Kotlin based on frame type.
fn dispatch_frame_event(pairing_id: &str, frame_type: u8, terminal_id: u32, payload: &[u8]) {
    #[cfg(target_os = "android")]
    {
        use termfast_daemon::remote_frame::{
            ERROR, HISTORY, LIST_RESPONSE, NOTIFY, OK, OUTPUT, RESIZE,
        };
        match frame_type {
            LIST_RESPONSE => {
                let json = String::from_utf8_lossy(payload).to_string();
                let event = crate::event::RustEvent::RemoteTerminalList {
                    pairing_id: pairing_id.to_string(),
                    terminals: json,
                };
                crate::jni::dispatch_event_to_kotlin(&event.to_json());
            }
            NOTIFY => {
                // Desktop broadcasts NOTIFY(list_changed) when terminals open/close.
                // Mobile should re-send LIST_REQUEST to refresh the terminal list.
                let json = String::from_utf8_lossy(payload).to_string();
                let event = crate::event::RustEvent::RemoteTerminalNotify {
                    pairing_id: pairing_id.to_string(),
                    message: json,
                };
                crate::jni::dispatch_event_to_kotlin(&event.to_json());
            }
            OUTPUT => {
                let data_b64 = base64::engine::general_purpose::STANDARD.encode(payload);
                let event = crate::event::RustEvent::RemoteTerminalOutput {
                    pairing_id: pairing_id.to_string(),
                    terminal_id,
                    data: data_b64,
                    encoding: "base64".to_string(),
                };
                crate::jni::dispatch_event_to_kotlin(&event.to_json());
            }
            HISTORY => {
                if payload.len() >= 5 {
                    let seq = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                    let is_last = payload[4] != 0;
                    let data_b64 =
                        base64::engine::general_purpose::STANDARD.encode(&payload[5..]);
                    let event = crate::event::RustEvent::RemoteTerminalHistory {
                        pairing_id: pairing_id.to_string(),
                        terminal_id,
                        seq,
                        is_last,
                        data: data_b64,
                        encoding: "base64".to_string(),
                    };
                    crate::jni::dispatch_event_to_kotlin(&event.to_json());
                }
            }
            RESIZE => {
                if payload.len() >= 4 {
                    let cols = u16::from_be_bytes([payload[0], payload[1]]);
                    let rows = u16::from_be_bytes([payload[2], payload[3]]);
                    let event = crate::event::RustEvent::RemoteTerminalResize {
                        pairing_id: pairing_id.to_string(),
                        terminal_id,
                        cols,
                        rows,
                    };
                    crate::jni::dispatch_event_to_kotlin(&event.to_json());
                }
            }
            ERROR => {
                let msg = String::from_utf8_lossy(payload).to_string();
                let event = crate::event::RustEvent::RemoteTerminalError {
                    pairing_id: pairing_id.to_string(),
                    error: msg,
                };
                crate::jni::dispatch_event_to_kotlin(&event.to_json());
            }
            OK => {
                // Confirmation frame, no event needed
            }
            _ => {
                tracing::warn!(
                    "remote_terminal: unhandled frame_type=0x{:02X} terminal_id={}",
                    frame_type,
                    terminal_id
                );
            }
        }
    }
    // Suppress unused variable warnings on non-Android targets
    let _ = (pairing_id, frame_type, terminal_id, payload);
}

/// Encrypt a protocol frame and return ciphertext for sending via WebSocket.
fn encrypt_outgoing(pairing_id: &str, frame: Frame) -> Result<Vec<u8>, String> {
    let mut map = sessions().lock().unwrap();
    let session = map
        .get_mut(pairing_id)
        .ok_or_else(|| format!("no tunnel session for pairing_id={}", pairing_id))?;

    if session.phase != TunnelPhase::Ready {
        return Err("tunnel not ready (HELLO exchange not complete)".to_string());
    }

    let cipher = session
        .send_cipher
        .as_mut()
        .ok_or_else(|| "send_cipher not initialized".to_string())?;
    cipher.encrypt(&frame.serialize())
}

/// Create and encrypt a LIST_REQUEST frame.
pub fn send_list_request(pairing_id: &str) -> Result<Vec<u8>, String> {
    encrypt_outgoing(pairing_id, Frame::list_request())
}

/// Create and encrypt a SUBSCRIBE frame for a terminal.
pub fn send_subscribe(pairing_id: &str, terminal_id: u32) -> Result<Vec<u8>, String> {
    encrypt_outgoing(pairing_id, Frame::subscribe(terminal_id))
}

/// Create and encrypt an UNSUBSCRIBE frame for a terminal.
pub fn send_unsubscribe(pairing_id: &str, terminal_id: u32) -> Result<Vec<u8>, String> {
    encrypt_outgoing(pairing_id, Frame::unsubscribe(terminal_id))
}

/// Create and encrypt an INPUT frame with user input data.
pub fn send_input(pairing_id: &str, terminal_id: u32, data: &[u8]) -> Result<Vec<u8>, String> {
    encrypt_outgoing(pairing_id, Frame::input(terminal_id, data))
}

/// Create and encrypt a RESIZE frame.
pub fn send_resize(
    pairing_id: &str,
    terminal_id: u32,
    cols: u16,
    rows: u16,
) -> Result<Vec<u8>, String> {
    encrypt_outgoing(pairing_id, Frame::resize(terminal_id, cols, rows))
}

/// Create and encrypt a GOODBYE frame, then remove the session.
pub fn close_tunnel(pairing_id: &str) -> Result<Vec<u8>, String> {
    let mut map = sessions().lock().unwrap();
    let session = map
        .get_mut(pairing_id)
        .ok_or_else(|| format!("no tunnel session for pairing_id={}", pairing_id))?;

    let ciphertext = if session.phase == TunnelPhase::Ready {
        let cipher = session
            .send_cipher
            .as_mut()
            .ok_or_else(|| "send_cipher not initialized".to_string())?;
        cipher.encrypt(&Frame::goodbye().serialize())?
    } else {
        Vec::new()
    };

    map.remove(pairing_id);
    Ok(ciphertext)
}

/// Check if a tunnel session exists and is ready.
pub fn is_ready(pairing_id: &str) -> bool {
    let map = sessions().lock().unwrap();
    map.get(pairing_id)
        .map(|s| s.phase == TunnelPhase::Ready)
        .unwrap_or(false)
}

// === SECTION 3 END ===

#[cfg(test)]
mod tests {
    use super::*;
    use termfast_daemon::frame_crypto::{
        FrameCipher, DIR_DESKTOP_TO_MOBILE, DIR_MOBILE_TO_DESKTOP,
    };
    use termfast_daemon::remote_frame::{Frame, HELLO};

    /// Simulate a desktop-side HELLO response: decrypt the mobile's HELLO,
    /// extract client_random, generate server_random, derive session key,
    /// and send back an encrypted HELLO response.
    fn simulate_desktop_hello(
        pairing_key: &[u8; 32],
        mobile_hello_ciphertext: &[u8],
    ) -> (Vec<u8>, [u8; 32]) {
        // Desktop decrypts HELLO with K
        let desktop_recv = FrameCipher::from_pairing_key(pairing_key, DIR_MOBILE_TO_DESKTOP);
        let plaintext = desktop_recv.decrypt(mobile_hello_ciphertext).unwrap();
        let frame = Frame::deserialize(&plaintext).unwrap();
        assert_eq!(frame.frame_type, HELLO);
        let (_client_caps, client_random) = frame.parse_hello().unwrap();

        // Desktop generates server_random and sends HELLO response
        let server_random = generate_random_32();
        let hello_resp = Frame::hello(CLIENT_CAPABILITIES, &server_random);
        let mut desktop_send = FrameCipher::from_pairing_key(pairing_key, DIR_DESKTOP_TO_MOBILE);
        let ciphertext = desktop_send.encrypt(&hello_resp.serialize()).unwrap();

        (ciphertext, client_random)
    }

    /// Full HELLO exchange: init_tunnel → simulate desktop → process_binary.
    /// Returns after session is Ready.
    fn do_hello_exchange(pairing_id: &str, pairing_key: &[u8; 32]) {
        // Mobile sends HELLO
        let hello_ct = init_tunnel(pairing_id, pairing_key).unwrap();

        // Desktop responds
        let (hello_resp_ct, _client_random) =
            simulate_desktop_hello(pairing_key, &hello_ct);

        // Mobile processes HELLO response
        process_binary(pairing_id, &hello_resp_ct).unwrap();
        assert!(is_ready(pairing_id));
    }

    #[test]
    fn test_init_tunnel_creates_session_and_encrypted_hello() {
        let pairing_id = "test-init-pid";
        let pairing_key = [0x42u8; 32];

        let hello_ct = init_tunnel(pairing_id, &pairing_key).unwrap();

        // Ciphertext should be non-empty and different from plaintext
        assert!(!hello_ct.is_empty());
        // Should be nonce(12) + encrypted(HELLO frame) + tag(16)
        // HELLO frame = version(1) + type(1) + terminal_id(4) + payload_len(4) + payload(34) = 44 bytes
        // Total = 12 + 44 + 16 = 72 bytes
        assert_eq!(hello_ct.len(), 72);

        // Session should exist in WaitingHello phase
        let map = sessions().lock().unwrap();
        let session = map.get(pairing_id).unwrap();
        assert_eq!(session.phase, TunnelPhase::WaitingHello);

        // Cleanup
        drop(map);
        close_tunnel(pairing_id).unwrap();
    }

    #[test]
    fn test_hello_exchange_completes_and_session_becomes_ready() {
        let pairing_id = "test-hello-pid";
        let pairing_key = [0x55u8; 32];

        do_hello_exchange(pairing_id, &pairing_key);

        // Session should be Ready
        assert!(is_ready(pairing_id));

        close_tunnel(pairing_id).unwrap();
    }

    // === SECTION: Tests part 1 END ===

    #[test]
    fn test_process_binary_wrong_key_fails() {
        let pairing_id = "test-wrong-key-pid";
        let pairing_key = [0x42u8; 32];
        let wrong_key = [0x99u8; 32];

        let _hello_ct = init_tunnel(pairing_id, &pairing_key).unwrap();

        // Desktop responds with wrong key
        let server_random = generate_random_32();
        let hello_resp = Frame::hello(CLIENT_CAPABILITIES, &server_random);
        let mut desktop_send = FrameCipher::from_pairing_key(&wrong_key, DIR_DESKTOP_TO_MOBILE);
        let wrong_ct = desktop_send.encrypt(&hello_resp.serialize()).unwrap();

        // Mobile should fail to decrypt
        let result = process_binary(pairing_id, &wrong_ct);
        assert!(result.is_err());

        close_tunnel(pairing_id).unwrap();
    }

    #[test]
    fn test_process_binary_non_hello_frame_before_ready_fails() {
        let pairing_id = "test-non-hello-pid";
        let pairing_key = [0x33u8; 32];

        let _hello_ct = init_tunnel(pairing_id, &pairing_key).unwrap();

        // Desktop sends a non-HELLO frame encrypted with K
        let list_resp = Frame::list_response(0, r#"[{"id":"t1"}]"#);
        let mut desktop_send = FrameCipher::from_pairing_key(&pairing_key, DIR_DESKTOP_TO_MOBILE);
        let bad_ct = desktop_send.encrypt(&list_resp.serialize()).unwrap();

        // Mobile should reject it (expected HELLO, got LIST_RESPONSE)
        let result = process_binary(pairing_id, &bad_ct);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected HELLO"));

        close_tunnel(pairing_id).unwrap();
    }

    #[test]
    fn test_send_list_request_after_hello() {
        let pairing_id = "test-list-req-pid";
        let pairing_key = [0x77u8; 32];
        do_hello_exchange(pairing_id, &pairing_key);

        // Send LIST_REQUEST
        let ct = send_list_request(pairing_id).unwrap();
        assert!(!ct.is_empty());

        // Decrypt and verify it's a LIST_REQUEST
        let map = sessions().lock().unwrap();
        let session = map.get(pairing_id).unwrap();
        let recv_cipher = FrameCipher::from_session_key(
            &session.pairing_key,
            &session.client_random,
            // We don't have server_random stored, but we can verify by
            // checking that the ciphertext is non-empty and different
            // from the plaintext LIST_REQUEST frame.
            &[0u8; 32], // placeholder — actual verification in integration test
            DIR_MOBILE_TO_DESKTOP,
        );
        // The ciphertext should not decrypt with wrong session key
        assert!(recv_cipher.decrypt(&ct).is_err());

        drop(map);
        close_tunnel(pairing_id).unwrap();
    }

    #[test]
    fn test_send_list_request_before_hello_fails() {
        let pairing_id = "test-list-req-fail-pid";
        let pairing_key = [0x88u8; 32];

        // Init tunnel but don't complete HELLO exchange
        init_tunnel(pairing_id, &pairing_key).unwrap();

        // Should fail because session is not Ready
        let result = send_list_request(pairing_id);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not ready"));

        close_tunnel(pairing_id).unwrap();
    }

    // === SECTION: Tests part 2 END ===

    #[test]
    fn test_send_subscribe_and_unsubscribe() {
        let pairing_id = "test-sub-pid";
        let pairing_key = [0xAAu8; 32];
        do_hello_exchange(pairing_id, &pairing_key);

        let sub_ct = send_subscribe(pairing_id, 42).unwrap();
        assert!(!sub_ct.is_empty());

        let unsub_ct = send_unsubscribe(pairing_id, 42).unwrap();
        assert!(!unsub_ct.is_empty());

        close_tunnel(pairing_id).unwrap();
    }

    #[test]
    fn test_send_input() {
        let pairing_id = "test-input-pid";
        let pairing_key = [0xBBu8; 32];
        do_hello_exchange(pairing_id, &pairing_key);

        let input_data = b"ls -la\r";
        let ct = send_input(pairing_id, 5, input_data).unwrap();
        assert!(!ct.is_empty());

        close_tunnel(pairing_id).unwrap();
    }

    #[test]
    fn test_send_resize() {
        let pairing_id = "test-resize-pid";
        let pairing_key = [0xCCu8; 32];
        do_hello_exchange(pairing_id, &pairing_key);

        let ct = send_resize(pairing_id, 3, 120, 40).unwrap();
        assert!(!ct.is_empty());

        close_tunnel(pairing_id).unwrap();
    }

    #[test]
    fn test_close_tunnel_removes_session() {
        let pairing_id = "test-close-pid";
        let pairing_key = [0xDDu8; 32];
        do_hello_exchange(pairing_id, &pairing_key);

        // Close should return GOODBYE ciphertext
        let goodbye_ct = close_tunnel(pairing_id).unwrap();
        assert!(!goodbye_ct.is_empty());

        // Session should be removed
        assert!(!is_ready(pairing_id));
        let map = sessions().lock().unwrap();
        assert!(!map.contains_key(pairing_id));
    }

    #[test]
    fn test_close_tunnel_nonexistent_fails() {
        let result = close_tunnel("nonexistent-pid");
        assert!(result.is_err());
    }

    #[test]
    fn test_process_binary_nonexistent_session_fails() {
        let result = process_binary("nonexistent-pid", &[0u8; 100]);
        assert!(result.is_err());
    }

    // === SECTION: Tests part 3 END ===

    /// Integration test: full HELLO exchange + frame roundtrip.
    /// Verifies that frames encrypted by the mobile side can be decrypted
    /// by a desktop-side cipher using the same session key.
    #[test]
    fn test_full_roundtrip_mobile_to_desktop() {
        let pairing_id = "test-roundtrip-pid";
        let pairing_key = [0xEEu8; 32];

        // Mobile sends HELLO
        let hello_ct = init_tunnel(pairing_id, &pairing_key).unwrap();

        // Desktop decrypts HELLO, gets client_random
        let desktop_recv = FrameCipher::from_pairing_key(&pairing_key, DIR_MOBILE_TO_DESKTOP);
        let hello_pt = desktop_recv.decrypt(&hello_ct).unwrap();
        let hello_frame = Frame::deserialize(&hello_pt).unwrap();
        let (_caps, client_random) = hello_frame.parse_hello().unwrap();

        // Desktop sends HELLO response
        let server_random = generate_random_32();
        let hello_resp = Frame::hello(CLIENT_CAPABILITIES, &server_random);
        let mut desktop_send = FrameCipher::from_pairing_key(&pairing_key, DIR_DESKTOP_TO_MOBILE);
        let hello_resp_ct = desktop_send.encrypt(&hello_resp.serialize()).unwrap();

        // Mobile processes HELLO response → session key derived
        process_binary(pairing_id, &hello_resp_ct).unwrap();
        assert!(is_ready(pairing_id));

        // Mobile sends LIST_REQUEST
        let list_req_ct = send_list_request(pairing_id).unwrap();

        // Desktop should be able to decrypt it with the same session key
        let desktop_session_recv = FrameCipher::from_session_key(
            &pairing_key,
            &client_random,
            &server_random,
            DIR_MOBILE_TO_DESKTOP,
        );
        let list_req_pt = desktop_session_recv.decrypt(&list_req_ct).unwrap();
        let list_req_frame = Frame::deserialize(&list_req_pt).unwrap();
        assert_eq!(list_req_frame.frame_type, termfast_daemon::remote_frame::LIST_REQUEST);

        // Desktop sends LIST_RESPONSE
        let list_resp = Frame::list_response(0, r#"[{"id":"t1","name":"Terminal 1"}]"#);
        let mut desktop_session_send = FrameCipher::from_session_key(
            &pairing_key,
            &client_random,
            &server_random,
            DIR_DESKTOP_TO_MOBILE,
        );
        let list_resp_ct = desktop_session_send.encrypt(&list_resp.serialize()).unwrap();

        // Mobile processes LIST_RESPONSE → should succeed (events only dispatched on Android)
        let result = process_binary(pairing_id, &list_resp_ct);
        assert!(result.is_ok());

        close_tunnel(pairing_id).unwrap();
    }

    /// Integration test: OUTPUT frame roundtrip (desktop → mobile).
    #[test]
    fn test_output_frame_roundtrip() {
        let pairing_id = "test-output-pid";
        let pairing_key = [0x11u8; 32];

        // HELLO exchange
        let hello_ct = init_tunnel(pairing_id, &pairing_key).unwrap();
        let desktop_recv = FrameCipher::from_pairing_key(&pairing_key, DIR_MOBILE_TO_DESKTOP);
        let hello_pt = desktop_recv.decrypt(&hello_ct).unwrap();
        let hello_frame = Frame::deserialize(&hello_pt).unwrap();
        let (_caps, client_random) = hello_frame.parse_hello().unwrap();

        let server_random = generate_random_32();
        let hello_resp = Frame::hello(CLIENT_CAPABILITIES, &server_random);
        let mut desktop_send = FrameCipher::from_pairing_key(&pairing_key, DIR_DESKTOP_TO_MOBILE);
        let hello_resp_ct = desktop_send.encrypt(&hello_resp.serialize()).unwrap();
        process_binary(pairing_id, &hello_resp_ct).unwrap();

        // Desktop sends OUTPUT frame
        let output_data = b"\x1b[32mhello world\x1b[0m";
        let output_frame = Frame::output(7, output_data);
        let mut desktop_session_send = FrameCipher::from_session_key(
            &pairing_key,
            &client_random,
            &server_random,
            DIR_DESKTOP_TO_MOBILE,
        );
        let output_ct = desktop_session_send.encrypt(&output_frame.serialize()).unwrap();

        // Mobile processes OUTPUT → should succeed
        let result = process_binary(pairing_id, &output_ct);
        assert!(result.is_ok());

        close_tunnel(pairing_id).unwrap();
    }

    /// Integration test: RESIZE frame roundtrip (desktop → mobile).
    #[test]
    fn test_resize_frame_roundtrip() {
        let pairing_id = "test-resize-rt-pid";
        let pairing_key = [0x22u8; 32];

        // HELLO exchange
        let hello_ct = init_tunnel(pairing_id, &pairing_key).unwrap();
        let desktop_recv = FrameCipher::from_pairing_key(&pairing_key, DIR_MOBILE_TO_DESKTOP);
        let hello_pt = desktop_recv.decrypt(&hello_ct).unwrap();
        let hello_frame = Frame::deserialize(&hello_pt).unwrap();
        let (_caps, client_random) = hello_frame.parse_hello().unwrap();

        let server_random = generate_random_32();
        let hello_resp = Frame::hello(CLIENT_CAPABILITIES, &server_random);
        let mut desktop_send = FrameCipher::from_pairing_key(&pairing_key, DIR_DESKTOP_TO_MOBILE);
        let hello_resp_ct = desktop_send.encrypt(&hello_resp.serialize()).unwrap();
        process_binary(pairing_id, &hello_resp_ct).unwrap();

        // Desktop sends RESIZE frame
        let resize_frame = Frame::resize(3, 100, 30);
        let mut desktop_session_send = FrameCipher::from_session_key(
            &pairing_key,
            &client_random,
            &server_random,
            DIR_DESKTOP_TO_MOBILE,
        );
        let resize_ct = desktop_session_send.encrypt(&resize_frame.serialize()).unwrap();

        // Mobile processes RESIZE → should succeed
        let result = process_binary(pairing_id, &resize_ct);
        assert!(result.is_ok());

        close_tunnel(pairing_id).unwrap();
    }

    /// Integration test: HISTORY frame roundtrip (desktop → mobile).
    #[test]
    fn test_history_frame_roundtrip() {
        let pairing_id = "test-history-pid";
        let pairing_key = [0x33u8; 32];

        // HELLO exchange
        let hello_ct = init_tunnel(pairing_id, &pairing_key).unwrap();
        let desktop_recv = FrameCipher::from_pairing_key(&pairing_key, DIR_MOBILE_TO_DESKTOP);
        let hello_pt = desktop_recv.decrypt(&hello_ct).unwrap();
        let hello_frame = Frame::deserialize(&hello_pt).unwrap();
        let (_caps, client_random) = hello_frame.parse_hello().unwrap();

        let server_random = generate_random_32();
        let hello_resp = Frame::hello(CLIENT_CAPABILITIES, &server_random);
        let mut desktop_send = FrameCipher::from_pairing_key(&pairing_key, DIR_DESKTOP_TO_MOBILE);
        let hello_resp_ct = desktop_send.encrypt(&hello_resp.serialize()).unwrap();
        process_binary(pairing_id, &hello_resp_ct).unwrap();

        // Desktop sends HISTORY frame
        let history_frame = Frame::history(5, 0, true, b"history snapshot");
        let mut desktop_session_send = FrameCipher::from_session_key(
            &pairing_key,
            &client_random,
            &server_random,
            DIR_DESKTOP_TO_MOBILE,
        );
        let history_ct = desktop_session_send.encrypt(&history_frame.serialize()).unwrap();

        // Mobile processes HISTORY → should succeed
        let result = process_binary(pairing_id, &history_ct);
        assert!(result.is_ok());

        close_tunnel(pairing_id).unwrap();
    }

    /// Integration test: ERROR frame roundtrip (desktop → mobile).
    #[test]
    fn test_error_frame_roundtrip() {
        let pairing_id = "test-error-pid";
        let pairing_key = [0x44u8; 32];

        // HELLO exchange
        let hello_ct = init_tunnel(pairing_id, &pairing_key).unwrap();
        let desktop_recv = FrameCipher::from_pairing_key(&pairing_key, DIR_MOBILE_TO_DESKTOP);
        let hello_pt = desktop_recv.decrypt(&hello_ct).unwrap();
        let hello_frame = Frame::deserialize(&hello_pt).unwrap();
        let (_caps, client_random) = hello_frame.parse_hello().unwrap();

        let server_random = generate_random_32();
        let hello_resp = Frame::hello(CLIENT_CAPABILITIES, &server_random);
        let mut desktop_send = FrameCipher::from_pairing_key(&pairing_key, DIR_DESKTOP_TO_MOBILE);
        let hello_resp_ct = desktop_send.encrypt(&hello_resp.serialize()).unwrap();
        process_binary(pairing_id, &hello_resp_ct).unwrap();

        // Desktop sends ERROR frame
        let error_frame = Frame::error("terminal not found");
        let mut desktop_session_send = FrameCipher::from_session_key(
            &pairing_key,
            &client_random,
            &server_random,
            DIR_DESKTOP_TO_MOBILE,
        );
        let error_ct = desktop_session_send.encrypt(&error_frame.serialize()).unwrap();

        // Mobile processes ERROR → should succeed
        let result = process_binary(pairing_id, &error_ct);
        assert!(result.is_ok());

        close_tunnel(pairing_id).unwrap();
    }

    // === SECTION: Tests part 4 END ===
}
