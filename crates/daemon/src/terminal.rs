//! Terminal session manager — interactive SSH terminals
//!
//! Manages PTY shell sessions on top of existing SSH connections.
//! Each session has a unique ID; output is streamed via the event forwarder.

use crate::server::EventForwarder;
use base64::Engine;
use russh::client;
use russh::ChannelMsg;
use std::collections::HashMap;
use std::sync::Arc;
use termfast_core::ssh::pty;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task::JoinHandle;
use uuid::Uuid;

/// Commands sent to a terminal session's write task
enum TerminalCmd {
    Input(Vec<u8>, Option<oneshot::Sender<()>>),
    Resize(u32, u32),
    Close,
}

/// A single terminal session
struct TerminalSession {
    server_id: String,
    /// Send commands (input/resize/close) to the session's write task
    cmd_tx: mpsc::UnboundedSender<TerminalCmd>,
    /// Handles to the background tasks — aborted on close
    tasks: Vec<JoinHandle<()>>,
}

// === SECTION 1 END ===

/// Manages all active terminal sessions
pub struct TerminalManager {
    sessions: Mutex<HashMap<String, TerminalSession>>,
    forwarder: Arc<std::sync::Mutex<Option<EventForwarder>>>,
}

impl TerminalManager {
    pub fn new(forwarder: Arc<std::sync::Mutex<Option<EventForwarder>>>) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            forwarder,
        }
    }

    /// Open a new terminal session on the given server's SSH connection.
    /// Returns the session ID.
    pub async fn open(
        &self,
        ssh_handle: &client::Handle<termfast_core::ssh::client::SshHandler>,
        server_id: &str,
        cols: u32,
        rows: u32,
    ) -> Result<(String, String), String> {
        let session_id = Uuid::new_v4().to_string();
        let sid = session_id.clone();
        let fwd = self.forwarder.clone();

        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<TerminalCmd>();

        let (channel, first_output) = try_open_pty_or_fallback(ssh_handle, cols, rows)
            .await
            .map_err(|e| format!("failed to open terminal: {}", e))?;

        // Split the channel into independent read/write halves so input and output
        // can run concurrently without lock contention.
        let (mut read_half, write_half) = channel.split();

        // Read initial shell output (MOTD/prompt) before starting the read
        // task. This output is returned to the caller via the IPC response so
        // the frontend can write it directly to the terminal — avoiding a
        // race condition where the read_task emits "terminal:output" events
        // before the frontend has registered its event listener.
        let mut initial_output_bytes = first_output.clone();
        if !initial_output_bytes.is_empty() {
            tracing::info!(
                "terminal initial data from open: {} bytes for session {}",
                initial_output_bytes.len(),
                sid
            );
        }
        let collect_deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(800);
        loop {
            let now = tokio::time::Instant::now();
            if now >= collect_deadline {
                break;
            }
            let remaining = collect_deadline - now;
            match tokio::time::timeout(remaining, read_half.wait()).await {
                Ok(Some(ChannelMsg::Data { ref data })) => {
                    initial_output_bytes.extend_from_slice(data);
                }
                Ok(Some(ChannelMsg::ExtendedData { ref data, .. })) => {
                    initial_output_bytes.extend_from_slice(data);
                }
                Ok(Some(ChannelMsg::Success)) => {
                    // Shell request confirmed — keep reading for MOTD data
                }
                Ok(Some(ChannelMsg::Eof)) | Ok(Some(ChannelMsg::Close)) | Ok(None) => {
                    break;
                }
                Err(_) => {
                    // Timeout — we've collected what we can
                    break;
                }
                Ok(Some(_)) => {
                    // Other messages (WindowAdjusted, etc.) — ignore
                }
            }
        }
        // Encode initial output as base64 so binary data (e.g. ZMODEM) is preserved
        let initial_output =
            base64::engine::general_purpose::STANDARD.encode(&initial_output_bytes);
        if !initial_output_bytes.is_empty() {
            tracing::info!(
                "terminal collected initial output: {} bytes (base64 len={}) for session {}",
                initial_output_bytes.len(),
                initial_output.len(),
                sid
            );
        }

        let read_sid = sid.clone();
        let read_fwd = fwd.clone();
        let read_task = tokio::spawn(async move {
            tracing::info!("terminal read task started for {}", read_sid);
            // Buffer for merging small Data packets within a short time window.
            // This reduces the number of emit events (and base64 encode calls)
            // when the server outputs many small chunks in rapid succession.
            let mut data_buf: Vec<u8> = Vec::new();
            let mut stderr_buf: Vec<u8> = Vec::new();
            let mut has_pending: bool = false;

            // Helper closure to flush pending buffers
            macro_rules! flush_buffers {
                () => {{
                    if !data_buf.is_empty() {
                        let output = base64::engine::general_purpose::STANDARD.encode(&data_buf);
                        tracing::trace!(
                            "terminal data len={} (base64={}) for session {}",
                            data_buf.len(),
                            output.len(),
                            read_sid
                        );
                        forward_terminal_output(&read_fwd, &read_sid, &output, false);
                        data_buf.clear();
                    }
                    if !stderr_buf.is_empty() {
                        let output = base64::engine::general_purpose::STANDARD.encode(&stderr_buf);
                        tracing::trace!(
                            "terminal ext data len={} (base64={}) for session {}",
                            stderr_buf.len(),
                            output.len(),
                            read_sid
                        );
                        forward_terminal_output(&read_fwd, &read_sid, &output, true);
                        stderr_buf.clear();
                    }
                    has_pending = false;
                    let _ = has_pending;
                }};
            }

            loop {
                // If we have pending data, race a flush timer against the next
                // channel message. This batches small Data packets within a 5ms
                // window to reduce emit events and base64 encode calls.
                let flush_delay_ms = if has_pending { 5u64 } else { u64::MAX };

                tokio::select! {
                    biased; // Check timer first so we flush before processing new data
                    _ = tokio::time::sleep(std::time::Duration::from_millis(flush_delay_ms)) => {
                        flush_buffers!();
                    }
                    msg = read_half.wait() => {
                        match msg {
                            Some(ChannelMsg::Data { ref data }) => {
                                data_buf.extend_from_slice(data);
                                has_pending = true;
                            }
                            Some(ChannelMsg::ExtendedData { ref data, .. }) => {
                                stderr_buf.extend_from_slice(data);
                                has_pending = true;
                            }
                            Some(ChannelMsg::Success) => {
                                tracing::debug!("terminal Success for session {}", read_sid);
                            }
                            Some(ChannelMsg::Failure) => {
                                tracing::warn!("terminal Failure for session {}", read_sid);
                            }
                            Some(ChannelMsg::ExitStatus { exit_status }) => {
                                flush_buffers!();
                                tracing::info!("terminal exit_status={} for session {}", exit_status, read_sid);
                            }
                            Some(ChannelMsg::Eof) => {
                                flush_buffers!();
                                tracing::info!("terminal EOF for session {}", read_sid);
                            }
                            Some(ChannelMsg::Close) => {
                                flush_buffers!();
                                tracing::info!("terminal Close for session {}", read_sid);
                                forward_terminal_closed(&read_fwd, &read_sid);
                                break;
                            }
                            None => {
                                flush_buffers!();
                                tracing::info!("terminal channel None for session {}", read_sid);
                                forward_terminal_closed(&read_fwd, &read_sid);
                                break;
                            }
                            Some(other) => {
                                tracing::debug!("terminal other msg: {:?} for session {}", other, read_sid);
                            }
                        }
                    }
                }
            }
            tracing::info!("terminal read task ended for {}", read_sid);
        });

        let write_task = tokio::spawn(async move {
            tracing::info!("terminal write task started for {}", sid);
            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    TerminalCmd::Input(data, ack) => {
                        // With a PTY the remote tty line discipline handles
                        // echo and CR/LF translation, so input bytes are sent
                        // raw (xterm.js sends \r on Enter, which is correct).
                        // Skip noisy logging for large ZMODEM payloads.
                        if data.len() <= 64 {
                            tracing::info!(
                                "terminal write task input len={} data={:?} for session {}",
                                data.len(),
                                String::from_utf8_lossy(&data),
                                sid
                            );
                        }
                        // Retry loop: during large ZMODEM transfers the SSH send
                        // window can temporarily fill up.  A short timeout would
                        // silently drop bytes, corrupting the transfer.  Use a
                        // generous per-attempt timeout and retry until the data
                        // is sent or the channel is truly dead.
                        let payload = data.clone();
                        let mut attempts = 0u32;
                        loop {
                            match tokio::time::timeout(
                                std::time::Duration::from_secs(120),
                                write_half.data_bytes(payload.clone()),
                            )
                            .await
                            {
                                Ok(Ok(())) => {
                                    if attempts > 0 {
                                        tracing::info!(
                                            "terminal input sent after {} retries for session {}",
                                            attempts,
                                            sid,
                                        );
                                    }
                                    break;
                                }
                                Ok(Err(e)) => {
                                    tracing::warn!(
                                        "terminal input error: {} for session {}",
                                        e,
                                        sid
                                    );
                                    break;
                                }
                                Err(_) => {
                                    attempts += 1;
                                    tracing::warn!(
                                        "terminal input timed out (attempt {}) for session {}, {} bytes remaining",
                                        attempts, sid, payload.len(),
                                    );
                                    if attempts >= 3 {
                                        tracing::error!(
                                            "terminal input giving up after {} timeouts for session {}",
                                            attempts, sid,
                                        );
                                        break;
                                    }
                                    // retry with the same data
                                }
                            }
                        }
                        // Notify the caller that the data has been sent (or failed).
                        if let Some(tx) = ack {
                            let _ = tx.send(());
                        }
                    }
                    TerminalCmd::Resize(c, r) => {
                        if let Err(e) = write_half.window_change(c, r, 0, 0).await {
                            tracing::warn!("terminal resize error: {} for session {}", e, sid);
                        }
                    }
                    TerminalCmd::Close => {
                        tracing::info!("terminal close cmd for session {}", sid);
                        let _ = write_half.eof().await;
                        let _ = write_half.close().await;
                        forward_terminal_closed(&fwd, &sid);
                        break;
                    }
                }
            }
            tracing::info!("terminal write task ended for {}", sid);
        });

        let session = TerminalSession {
            server_id: server_id.to_string(),
            cmd_tx,
            tasks: vec![read_task, write_task],
        };
        self.sessions
            .lock()
            .await
            .insert(session_id.clone(), session);
        Ok((session_id, initial_output))
    }

    /// Send user input to the terminal.
    ///
    /// When `wait_for_send` is true, this function blocks until the SSH write
    /// task has actually sent the bytes over the channel (or timed out).  This
    /// provides backpressure for large ZMODEM transfers so the caller doesn't
    /// queue data faster than SSH can transmit it.
    pub async fn input(&self, session_id: &str, data: &str) -> Result<(), String> {
        self.input_with_ack(session_id, data, true).await
    }

    /// Same as `input` but optionally waits for SSH send completion.
    pub async fn input_with_ack(
        &self,
        session_id: &str,
        data: &str,
        wait_for_send: bool,
    ) -> Result<(), String> {
        let (ack_tx, ack_rx) = if wait_for_send {
            let (tx, rx) = oneshot::channel();
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };

        {
            let sessions = self.sessions.lock().await;
            let session = sessions
                .get(session_id)
                .ok_or_else(|| format!("terminal session not found: {}", session_id))?;
            // Data is base64-encoded to support binary input (e.g. ZMODEM file transfers)
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|e| format!("failed to decode base64 input: {}", e))?;
            if decoded.len() <= 64 {
                tracing::info!(
                    "TerminalManager::input sending {} bytes to session {}",
                    decoded.len(),
                    session_id,
                );
            }
            session
                .cmd_tx
                .send(TerminalCmd::Input(decoded, ack_tx))
                .map_err(|e| format!("failed to send terminal input: {}", e))?;
        }

        // Wait for the write task to confirm the bytes were sent over SSH.
        // This provides backpressure: the IPC call won't resolve until SSH
        // has actually transmitted the data (or the 120s timeout fires).
        if let Some(rx) = ack_rx {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(130), rx).await;
        }

        Ok(())
    }

    /// Resize the terminal PTY
    pub async fn resize(&self, session_id: &str, cols: u32, rows: u32) -> Result<(), String> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| format!("terminal session not found: {}", session_id))?;
        session
            .cmd_tx
            .send(TerminalCmd::Resize(cols, rows))
            .map_err(|e| format!("failed to resize terminal: {}", e))
    }

    /// Close a terminal session
    pub async fn close(&self, session_id: &str) -> Result<(), String> {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.remove(session_id) {
            let _ = session.cmd_tx.send(TerminalCmd::Close);
            for task in session.tasks {
                task.abort();
            }
            Ok(())
        } else {
            Err(format!("terminal session not found: {}", session_id))
        }
    }

    /// Close all terminal sessions for a given server (called on disconnect)
    pub async fn close_all_for_server(&self, server_id: &str) {
        let mut sessions = self.sessions.lock().await;
        let to_remove: Vec<String> = sessions
            .iter()
            .filter(|(_, s)| s.server_id == server_id)
            .map(|(id, _)| id.clone())
            .collect();
        for id in to_remove {
            if let Some(session) = sessions.remove(&id) {
                let _ = session.cmd_tx.send(TerminalCmd::Close);
                for task in session.tasks {
                    task.abort();
                }
            }
        }
    }
}

// === SECTION 2 END ===

async fn try_open_pty_or_fallback(
    ssh_handle: &client::Handle<termfast_core::ssh::client::SshHandler>,
    cols: u32,
    rows: u32,
) -> Result<(russh::Channel<client::Msg>, Vec<u8>), String> {
    // Strategy: request a PTY + shell first (the canonical interactive
    // terminal pattern). A PTY is required for a usable terminal — without
    // one the remote shell runs non-interactively and stdout is fully
    // buffered (not a tty), so the terminal shows nothing.
    //
    // If the PTY+shell path fails (e.g. a server that genuinely refuses
    // pty-req), fall back to exec("bash -i").

    // --- Attempt 1: PTY + shell ---
    match pty::open_pty_shell(ssh_handle, cols, rows).await {
        Ok(mut channel) => {
            tracing::info!(
                "pty+shell opened (id={}), waiting for first msg...",
                channel.id()
            );
            let first_msg =
                tokio::time::timeout(std::time::Duration::from_secs(5), channel.wait()).await;
            match first_msg {
                Ok(Some(ChannelMsg::Success)) => {
                    tracing::info!("pty+shell ready (Success)");
                    return Ok((channel, Vec::new()));
                }
                Ok(Some(ChannelMsg::Data { data })) => {
                    tracing::info!("pty+shell data len={}", data.len());
                    return Ok((channel, data.to_vec()));
                }
                Ok(Some(ChannelMsg::Failure)) => {
                    tracing::warn!("pty+shell rejected by server (Failure), falling back to exec");
                }
                Ok(Some(other)) => {
                    tracing::info!("pty+shell first msg: {:?}, proceeding", other);
                    return Ok((channel, Vec::new()));
                }
                Ok(None) => {
                    tracing::warn!("pty+shell channel closed immediately, falling back to exec");
                }
                Err(_) => {
                    tracing::warn!("pty+shell timed out, using it anyway");
                    return Ok((channel, Vec::new()));
                }
            }
        }
        Err(e) => {
            tracing::warn!("pty+shell failed ({}), falling back to exec", e);
        }
    }

    // --- Attempt 2: exec("bash -i") ---
    let mut channel = pty::open_shell_via_exec(ssh_handle)
        .await
        .map_err(|e| format!("all terminal open methods failed: {}", e))?;
    let first_msg = tokio::time::timeout(std::time::Duration::from_secs(5), channel.wait()).await;
    match first_msg {
        Ok(Some(ChannelMsg::Data { data })) => {
            tracing::info!("exec fallback data len={}", data.len());
            Ok((channel, data.to_vec()))
        }
        Ok(Some(other)) => {
            tracing::info!("exec fallback first msg: {:?}", other);
            Ok((channel, Vec::new()))
        }
        Ok(None) => Err("exec fallback channel closed immediately".to_string()),
        Err(_) => Ok((channel, Vec::new())),
    }
}

// === SECTION 3 END ===

/// Forward terminal output to the GUI via the event forwarder
fn forward_terminal_output(
    forwarder: &Arc<std::sync::Mutex<Option<EventForwarder>>>,
    session_id: &str,
    data: &str,
    is_stderr: bool,
) {
    if let Ok(fwd) = forwarder.lock() {
        if let Some(ref f) = *fwd {
            f(
                "terminal:output",
                serde_json::json!({
                    "sessionId": session_id,
                    "data": data,
                    "stderr": is_stderr,
                }),
            );
        } else {
            tracing::warn!(
                "terminal output: event forwarder is None, dropping {} bytes",
                data.len()
            );
        }
    } else {
        tracing::warn!("terminal output: failed to lock event forwarder");
    }
}

/// Forward terminal closed event to the GUI
fn forward_terminal_closed(
    forwarder: &Arc<std::sync::Mutex<Option<EventForwarder>>>,
    session_id: &str,
) {
    if let Ok(fwd) = forwarder.lock() {
        if let Some(ref f) = *fwd {
            f(
                "terminal:closed",
                serde_json::json!({ "sessionId": session_id }),
            );
        }
    }
}
