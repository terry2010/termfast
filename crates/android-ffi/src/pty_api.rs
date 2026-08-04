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
    /// Resize the PTY window (sends SIGWINCH on the remote side).
    Resize(u32, u32),
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
                // Receive commands (write input, resize, or close)
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(PtyCommand::Write(data)) => {
                            if let Err(e) = channel.data_bytes(data.clone()).await {
                                log_to_kotlin("error", &format!("PTY write error: {:?}", e));
                                break;
                            }
                        }
                        Some(PtyCommand::Resize(cols, rows)) => {
                            // window_change sends SSH_MSG_CHANNEL_REQUEST with
                            // "pty-req" type update, which triggers SIGWINCH on
                            // the remote side. This is how the terminal learns
                            // about orientation changes / keyboard show-hide.
                            if let Err(e) = channel.window_change(cols, rows, 0, 0).await {
                                log_to_kotlin("warn", &format!("PTY resize failed: {:?}", e));
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
///
/// Sends a window-change request to the remote shell via the reader task's
/// command channel. The reader task calls `channel.window_change()` which
/// triggers SIGWINCH on the remote side, allowing full-screen programs
/// (vim/tmux/htop) to redraw at the new dimensions.
pub fn resize_session(session_id: &str, cols: u32, rows: u32) -> Result<(), String> {
    let st = sessions().lock().unwrap();
    let session = st.get(session_id).ok_or_else(|| format!("session {} not found", session_id))?;
    session
        .command_tx
        .send(PtyCommand::Resize(cols, rows))
        .map_err(|e| format!("failed to send resize: {}", e))
}

// === tmux session management ===

/// Detect if tmux is installed on the remote server (3s timeout).
pub async fn tmux_detect(server_id: &str) -> Result<bool, String> {
    let handle = get_ssh_handle(server_id).await?;
    Ok(termfast_core::ssh::tmux::detect_tmux(&handle).await)
}

/// List TermFast-tagged tmux sessions on the server.
/// Returns JSON string: {"sessions":[...],"tmux_installed":bool}
pub async fn tmux_list_sessions(server_id: &str) -> Result<String, String> {
    let handle = get_ssh_handle(server_id).await?;
    let installed = termfast_core::ssh::tmux::detect_tmux(&handle).await;
    if !installed {
        return Ok(serde_json::json!({
            "sessions": [],
            "tmux_installed": false
        }).to_string());
    }
    let sessions = termfast_core::ssh::tmux::list_termfast_sessions(&handle)
        .await
        .map_err(|e| format!("{:?}", e))?;
    let sessions_json: Vec<serde_json::Value> = sessions.iter().map(|s| {
        serde_json::json!({
            "name": s.name,
            "description": s.description,
            "created": s.created,
            "server": s.server,
            "size": s.size,
            "windows": s.windows,
            "attached_count": s.attached_count,
            "last_activity": s.last_activity,
        })
    }).collect();
    Ok(serde_json::json!({
        "sessions": sessions_json,
        "tmux_installed": true
    }).to_string())
}

/// Create a new tmux session and open a PTY attached to it.
/// Returns JSON: {"session_id":"...","tmux_session_name":"..."}
pub async fn tmux_new_session(
    server_id: &str,
    session_id: &str,
    description: &str,
    cols: u32,
    rows: u32,
) -> Result<String, String> {
    let handle = get_ssh_handle(server_id).await?;

    // Generate unique tmux session name — fallback to plain shell on collision
    let tmux_name = match termfast_core::ssh::tmux::generate_unique_session_name(&handle).await {
        Ok(name) => name,
        Err(_) => {
            // Fallback: open plain shell
            open_session(server_id, session_id, cols, rows).await?;
            return Ok(serde_json::json!({
                "session_id": session_id,
                "tmux_session_name": null
            }).to_string());
        }
    };

    // Create tmux session via exec (detached)
    let create_cmd = termfast_core::ssh::tmux::build_new_session_exec_command(
        &tmux_name, description, "", cols as u16, rows as u16,
    );
    let create_result = termfast_core::ssh::exec::exec(&handle, &create_cmd, 10)
        .await
        .map_err(|e| format!("{:?}", e))?;
    if !create_result.is_success() {
        return Err(format!("failed to create tmux session: {}", create_result.stderr));
    }

    // Open PTY and inject `tmux attach -t <name>`
    open_session(server_id, session_id, cols, rows).await?;
    let attach_cmd = termfast_core::ssh::tmux::build_attach_command(&tmux_name);
    if let Err(e) = write_session(session_id, attach_cmd.as_bytes()) {
        log_to_kotlin("warn", &format!("failed to inject attach command: {:?}", e));
    }

    Ok(serde_json::json!({
        "session_id": session_id,
        "tmux_session_name": tmux_name
    }).to_string())
}

/// Attach to an existing tmux session and open a PTY.
/// Returns JSON: {"session_id":"...","tmux_session_name":"..."}
pub async fn tmux_attach_session(
    server_id: &str,
    session_id: &str,
    tmux_session_name: &str,
    cols: u32,
    rows: u32,
) -> Result<String, String> {
    let handle = get_ssh_handle(server_id).await?;

    // Verify session exists
    let exists = termfast_core::ssh::tmux::session_exists(&handle, tmux_session_name)
        .await
        .map_err(|e| format!("{:?}", e))?;
    if !exists {
        return Err(format!("tmux session {} not found", tmux_session_name));
    }

    // Open PTY and inject `tmux attach -t <name>`
    open_session(server_id, session_id, cols, rows).await?;
    let attach_cmd = termfast_core::ssh::tmux::build_attach_command(tmux_session_name);
    if let Err(e) = write_session(session_id, attach_cmd.as_bytes()) {
        log_to_kotlin("warn", &format!("failed to inject attach command: {:?}", e));
    }

    Ok(serde_json::json!({
        "session_id": session_id,
        "tmux_session_name": tmux_session_name
    }).to_string())
}

/// Kill a tmux session by name on the remote server.
pub async fn tmux_kill_session(server_id: &str, tmux_session_name: &str) -> Result<bool, String> {
    let handle = get_ssh_handle(server_id).await?;
    let cmd = format!(
        "tmux kill-session -t {} 2>/dev/null; echo \"EXIT:$?\"",
        termfast_core::ssh::tmux::shell_escape(tmux_session_name)
    );
    let result = termfast_core::ssh::exec::exec(&handle, &cmd, 5)
        .await
        .map_err(|e| format!("{:?}", e))?;
    let killed = if let Some(pos) = result.stdout.rfind("EXIT:") {
        let code_str = &result.stdout[pos + 5..].trim();
        code_str.parse::<u32>().map(|c| c == 0).unwrap_or(false)
    } else {
        result.exit_code == 0
    };
    Ok(killed)
}

/// Helper: get SSH handle for a server (async to avoid nested block_on).
async fn get_ssh_handle(server_id: &str) -> Result<std::sync::Arc<russh::client::Handle<termfast_core::ssh::client::SshHandler>>, String> {
    use crate::jni::state;
    let instance = {
        let st = state().lock().unwrap();
        st.servers.get(server_id).cloned()
    };
    let instance = instance.ok_or_else(|| format!("server {} not found", server_id))?;
    let connected = instance.ssh_client.is_connected().await;
    if !connected {
        return Err("SSH not connected".to_string());
    }
    let handle = instance.ssh_client.get_handle().await
        .ok_or_else(|| "SSH handle not available".to_string())?;
    Ok(handle)
}