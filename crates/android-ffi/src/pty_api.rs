//! SSH PTY terminal session management for Android.
//!
//! Provides JNI functions to open/close/write to interactive SSH shell sessions.
//! Output from the remote shell is delivered to Kotlin via the event callback
//! as `TerminalData` events.

#![cfg(target_os = "android")]

use crate::jni::{dispatch_event_to_kotlin, log_to_kotlin};
use crate::runtime::runtime;
use russh::ChannelMsg;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Commands sent to the PTY reader task.
enum PtyCommand {
    /// Write input data to the remote shell.
    Write(Vec<u8>),
    /// Close the session.
    Close,
}

/// A live PTY session.
struct PtySession {
    /// Channel for sending commands to the reader task.
    command_tx: tokio::sync::mpsc::UnboundedSender<PtyCommand>,
    /// Handle to the reader task so we can abort it on close.
    reader_task: tokio::task::JoinHandle<()>,
}

/// Global registry of active PTY sessions, keyed by session ID.
static SESSIONS: OnceLock<Mutex<HashMap<String, PtySession>>> = OnceLock::new();

fn sessions() -> &'static Mutex<HashMap<String, PtySession>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Open a PTY shell on the given server's SSH connection.
///
/// Returns a session ID (UUID string) on success, or empty string on failure.
pub async fn open_session(
    server_id: &str,
    session_id: &str,
    cols: u32,
    rows: u32,
) -> Result<(), String> {
    use crate::jni::state;

    // Get the server instance from FFI state
    let instance = {
        let st = state().lock().unwrap();
        st.servers.get(server_id).cloned()
    };
    let instance = instance.ok_or_else(|| format!("server {} not found", server_id))?;

    // Check if SSH is connected
    let connected = instance.ssh_client.is_connected().await;
    if !connected {
        return Err("SSH 未连接，请先启动 VPN 或代理".to_string());
    }

    // Get the SSH handle
    let handle = instance
        .ssh_client
        .get_handle()
        .await
        .ok_or_else(|| "SSH handle not available".to_string())?;

    // Open PTY shell
    let mut channel = termfast_core::ssh::open_pty_shell(&handle, cols, rows)
        .await
        .map_err(|e| format!("failed to open PTY: {:?}", e))?;

    log_to_kotlin("info", &format!("PTY session opened: {} (cols={}, rows={})", session_id, cols, rows));

    // Create a channel for sending commands to the reader task
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<PtyCommand>();

    let sid = session_id.to_string();

    // Spawn a task that reads output and writes input
    let reader_task = runtime().spawn(async move {
        loop {
            tokio::select! {
                // Read output from remote shell
                msg = channel.wait() => {
                    match msg {
                        Some(ChannelMsg::Data { data }) => {
                            // Base64-encode raw bytes to preserve binary data
                            // (ZMODEM, non-UTF-8 encodings, etc.) — using
                            // String::from_utf8_lossy here would corrupt any
                            // byte that isn't valid UTF-8.
                            use base64::Engine;
                            let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
                            let json = serde_json::json!({
                                "type": "TerminalData",
                                "session_id": sid,
                                "data": b64,
                                "encoding": "base64",
                            });
                            dispatch_event_to_kotlin(&json.to_string());
                        }
                        Some(ChannelMsg::ExtendedData { data, .. }) => {
                            use base64::Engine;
                            let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
                            let json = serde_json::json!({
                                "type": "TerminalData",
                                "session_id": sid,
                                "data": b64,
                                "encoding": "base64",
                                "is_stderr": true,
                            });
                            dispatch_event_to_kotlin(&json.to_string());
                        }
                        Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                            let json = serde_json::json!({
                                "type": "TerminalClosed",
                                "session_id": sid,
                            });
                            dispatch_event_to_kotlin(&json.to_string());
                            log_to_kotlin("info", &format!("PTY session ended: {}", sid));
                            break;
                        }
                        Some(_) => {}
                    }
                }
                // Receive commands (write input or close)
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(PtyCommand::Write(data)) => {
                            if let Err(e) = channel.data_bytes(data.clone()).await {
                                log_to_kotlin("error", &format!("PTY write error: {:?}", e));
                                break;
                            }
                        }
                        Some(PtyCommand::Close) | None => {
                            let _ = channel.close().await;
                            break;
                        }
                    }
                }
            }
        }
        // Remove from sessions map when done
        sessions().lock().unwrap().remove(&sid);
    });

    // Store the session
    sessions().lock().unwrap().insert(
        session_id.to_string(),
        PtySession {
            command_tx: cmd_tx,
            reader_task,
        },
    );

    Ok(())
}

/// Write input data to a PTY session.
pub fn write_session(session_id: &str, data: &[u8]) -> Result<(), String> {
    let st = sessions().lock().unwrap();
    let session = st.get(session_id).ok_or_else(|| format!("session {} not found", session_id))?;
    session.command_tx.send(PtyCommand::Write(data.to_vec())).map_err(|e| format!("failed to send input: {}", e))
}

/// Close a PTY session.
pub fn close_session(session_id: &str) {
    let session = sessions().lock().unwrap().remove(session_id);
    if let Some(session) = session {
        let _ = session.command_tx.send(PtyCommand::Close);
        session.reader_task.abort();
        log_to_kotlin("info", &format!("PTY session closed: {}", session_id));
    }
}

/// Resize a PTY session.
pub async fn resize_session(session_id: &str, _cols: u32, _rows: u32) -> Result<(), String> {
    // Resize requires access to the channel, which is owned by the reader task.
    // For now, we skip resize support — it can be added later by storing
    // a separate resize command channel.
    log_to_kotlin("info", &format!("PTY resize requested for {} (not yet implemented)", session_id));
    Ok(())
}