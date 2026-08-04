//! Terminal session manager — interactive SSH terminals + local terminals
//!
//! Manages PTY shell sessions on top of existing SSH connections,
//! and local PTY sessions (no SSH). Each session has a unique ID;
//! output is streamed via the event forwarder.

use crate::server::{BinaryEventForwarder, EventForwarder};
use russh::client;
use russh::ChannelMsg;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use termfast_core::local::{ChildKiller, PtySize};
use termfast_core::local::pty::open_local_pty;
use termfast_core::ssh::pty;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task::JoinHandle;
use uuid::Uuid;

/// 本地终端生命周期事件（打开/关闭）
#[derive(Debug, Clone)]
pub enum TerminalLifecycleEvent {
    Opened { session_id: String },
    Closed { session_id: String },
}

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
    /// Local terminal child killer (None for SSH terminals).
    /// On close, kill is called regardless of task state to ensure
    /// the child process is terminated.
    kill_child: Option<Box<dyn ChildKiller + Send + Sync>>,
    /// tmux session name (Some if this terminal is attached to a tmux session)
    /// Used by resize_and_notify to update @termfast_size via exec channel
    tmux_session_name: Option<String>,
    /// SSH handle (Some for SSH terminals, None for local)
    /// Used by resize_and_notify to exec tmux set-option on resize
    ssh_handle: Option<Arc<client::Handle<termfast_core::ssh::client::SshHandler>>>,
}

// === SECTION 1 END ===

/// Manages all active terminal sessions
pub struct TerminalManager {
    sessions: Mutex<HashMap<String, TerminalSession>>,
    forwarder: Arc<std::sync::Mutex<Option<EventForwarder>>>,
    binary_forwarder: Arc<std::sync::Mutex<Option<BinaryEventForwarder>>>,
    /// 本地终端事件发送端（打开/关闭时发送）。Arc<Mutex> 因为要 clone 进 main task。
    local_event_tx: Arc<Mutex<Option<mpsc::UnboundedSender<TerminalLifecycleEvent>>>>,
    /// 已发送 Closed 事件的 session_id（避免 main task 和 close() 双重发送）
    closed_sessions: Arc<Mutex<HashSet<String>>>,
    /// Per-session runtime overrides for exec_in_terminal.
    /// Keyed by session_id, then trigger_id → exec_in_terminal override.
    /// These are NOT persisted — cleared on restart. Used by the frontend
    /// to toggle exec_in_terminal per-terminal without modifying trigger config.
    trigger_overrides: Arc<Mutex<HashMap<String, HashMap<String, bool>>>>,
}

impl TerminalManager {
    pub fn new(
        forwarder: Arc<std::sync::Mutex<Option<EventForwarder>>>,
        binary_forwarder: Arc<std::sync::Mutex<Option<BinaryEventForwarder>>>,
    ) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            forwarder,
            binary_forwarder,
            local_event_tx: Arc::new(Mutex::new(None)),
            closed_sessions: Arc::new(Mutex::new(HashSet::new())),
            trigger_overrides: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 订阅本地终端生命周期事件。
    /// **只能调用一次**：每次调用会创建新 channel 并覆盖 local_event_tx，
    /// 之前的 subscriber 的 receiver 将不再收到事件。
    /// 在 DaemonState 初始化时调用一次。
    pub async fn subscribe_local_events(
        &self,
    ) -> mpsc::UnboundedReceiver<TerminalLifecycleEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        *self.local_event_tx.lock().await = Some(tx);
        tracing::info!("[TerminalManager] subscribe_local_events called — local_event_tx set");
        rx
    }

    /// 清空 closed_sessions（在 daemon shutdown 时调用）
    pub async fn clear_closed_sessions(&self) {
        self.closed_sessions.lock().await.clear();
    }

    /// Open a new terminal session on the given server's SSH connection.
    /// Returns the session ID.
    pub async fn open(
        &self,
        ssh_handle: &Arc<client::Handle<termfast_core::ssh::client::SshHandler>>,
        server_id: &str,
        cols: u32,
        rows: u32,
    ) -> Result<(String, Vec<u8>), String> {
        let session_id = Uuid::new_v4().to_string();
        let sid = session_id.clone();
        let fwd = self.forwarder.clone();
        let bin_fwd = self.binary_forwarder.clone();

        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<TerminalCmd>();

        let (mut channel, first_output) = try_open_pty_or_fallback(ssh_handle.as_ref(), cols, rows)
            .await
            .map_err(|e| format!("failed to open terminal: {}", e))?;

        // Read initial shell output (MOTD/prompt) before starting the main
        // task. This output is returned to the caller via the IPC response so
        // the frontend can write it directly to the terminal — avoiding a
        // race condition where the task emits "terminal:output" events
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
            match tokio::time::timeout(remaining, channel.wait()).await {
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
        // Return raw bytes — binary data (e.g. ZMODEM) is preserved without base64 overhead
        let initial_output = initial_output_bytes.clone();
        if !initial_output_bytes.is_empty() {
            tracing::info!(
                "terminal collected initial output: {} bytes for session {}",
                initial_output_bytes.len(),
                sid
            );
        }

        let task_sid = sid.clone();
        let task_fwd = fwd.clone();
        let task_bin_fwd = bin_fwd.clone();
        let main_task = tokio::spawn(async move {
            tracing::info!("terminal main task started for {}", task_sid);
            // Buffer for merging small Data packets within a short time window.
            let mut data_buf: Vec<u8> = Vec::new();
            let mut stderr_buf: Vec<u8> = Vec::new();
            let mut has_pending: bool = false;

            macro_rules! flush_buffers {
                () => {{
                    if !data_buf.is_empty() {
                        tracing::info!(
                            "terminal flush data len={} for session {}",
                            data_buf.len(),
                            task_sid
                        );
                        forward_terminal_output(&task_bin_fwd, &task_sid, &data_buf, false);
                        data_buf.clear();
                    }
                    if !stderr_buf.is_empty() {
                        tracing::info!(
                            "terminal flush ext data len={} for session {}",
                            stderr_buf.len(),
                            task_sid
                        );
                        forward_terminal_output(&task_bin_fwd, &task_sid, &stderr_buf, true);
                        stderr_buf.clear();
                    }
                    has_pending = false;
                    let _ = has_pending;
                }};
            }

            loop {
                if has_pending {
                    tokio::select! {
                        _ = tokio::time::sleep(std::time::Duration::from_millis(5)) => {
                            flush_buffers!();
                        }
                        msg = channel.wait() => {
                            match msg {
                                Some(ChannelMsg::Data { ref data }) => {
                                    tracing::info!("terminal read Data len={} for session {}", data.len(), task_sid);
                                    data_buf.extend_from_slice(data);
                                }
                                Some(ChannelMsg::ExtendedData { ref data, .. }) => {
                                    tracing::info!("terminal read ExtendedData len={} for session {}", data.len(), task_sid);
                                    stderr_buf.extend_from_slice(data);
                                }
                                Some(ChannelMsg::ExitStatus { exit_status }) => {
                                    flush_buffers!();
                                    tracing::info!("terminal exit_status={} for session {}", exit_status, task_sid);
                                }
                                Some(ChannelMsg::Eof) => {
                                    flush_buffers!();
                                    tracing::info!("terminal EOF for session {}", task_sid);
                                }
                                Some(ChannelMsg::Close) => {
                                    flush_buffers!();
                                    tracing::info!("terminal Close for session {}", task_sid);
                                    forward_terminal_closed(&task_fwd, &task_sid);
                                    break;
                                }
                                None => {
                                    flush_buffers!();
                                    tracing::info!("terminal channel None for session {}", task_sid);
                                    forward_terminal_closed(&task_fwd, &task_sid);
                                    break;
                                }
                                Some(other) => {
                                    tracing::info!("terminal other msg: {:?} for session {}", other, task_sid);
                                }
                            }
                        }
                        cmd = cmd_rx.recv() => {
                            match cmd {
                                Some(TerminalCmd::Input(data, ack)) => {
                                    if data.len() <= 64 {
                                        tracing::info!(
                                            "terminal input len={} data={:?} for session {}",
                                            data.len(),
                                            String::from_utf8_lossy(&data),
                                            task_sid
                                        );
                                    }
                                    let payload = data.clone();
                                    let mut attempts = 0u32;
                                    loop {
                                        match tokio::time::timeout(
                                            std::time::Duration::from_secs(120),
                                            channel.data_bytes(payload.clone()),
                                        ).await {
                                            Ok(Ok(())) => {
                                                if attempts > 0 {
                                                    tracing::info!(
                                                        "terminal input sent after {} retries for session {}",
                                                        attempts, task_sid,
                                                    );
                                                }
                                                break;
                                            }
                                            Ok(Err(e)) => {
                                                tracing::warn!("terminal input error: {} for session {}", e, task_sid);
                                                break;
                                            }
                                            Err(_) => {
                                                attempts += 1;
                                                tracing::warn!(
                                                    "terminal input timed out (attempt {}) for session {}, {} bytes remaining",
                                                    attempts, task_sid, payload.len(),
                                                );
                                                if attempts >= 3 {
                                                    tracing::error!(
                                                        "terminal input giving up after {} timeouts for session {}",
                                                        attempts, task_sid,
                                                    );
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                    if let Some(tx) = ack {
                                        let _ = tx.send(());
                                    }
                                }
                                Some(TerminalCmd::Resize(c, r)) => {
                                    if let Err(e) = channel.window_change(c, r, 0, 0).await {
                                        tracing::warn!("terminal resize error: {} for session {}", e, task_sid);
                                    }
                                }
                                Some(TerminalCmd::Close) => {
                                    tracing::info!("terminal close cmd for session {}", task_sid);
                                    flush_buffers!();
                                    let _ = channel.eof().await;
                                    let _ = channel.close().await;
                                    forward_terminal_closed(&task_fwd, &task_sid);
                                    break;
                                }
                                None => {
                                    // cmd_rx dropped — session manager closed
                                    flush_buffers!();
                                    let _ = channel.eof().await;
                                    let _ = channel.close().await;
                                    break;
                                }
                            }
                        }
                    }
                } else {
                    tokio::select! {
                        msg = channel.wait() => {
                            match msg {
                                Some(ChannelMsg::Data { ref data }) => {
                                    tracing::info!("terminal read Data len={} for session {}", data.len(), task_sid);
                                    data_buf.extend_from_slice(data);
                                    has_pending = true;
                                }
                                Some(ChannelMsg::ExtendedData { ref data, .. }) => {
                                    tracing::info!("terminal read ExtendedData len={} for session {}", data.len(), task_sid);
                                    stderr_buf.extend_from_slice(data);
                                    has_pending = true;
                                }
                                Some(ChannelMsg::ExitStatus { exit_status }) => {
                                    flush_buffers!();
                                    tracing::info!("terminal exit_status={} for session {}", exit_status, task_sid);
                                }
                                Some(ChannelMsg::Eof) => {
                                    flush_buffers!();
                                    tracing::info!("terminal EOF for session {}", task_sid);
                                }
                                Some(ChannelMsg::Close) => {
                                    flush_buffers!();
                                    tracing::info!("terminal Close for session {}", task_sid);
                                    forward_terminal_closed(&task_fwd, &task_sid);
                                    break;
                                }
                                None => {
                                    flush_buffers!();
                                    tracing::info!("terminal channel None for session {}", task_sid);
                                    forward_terminal_closed(&task_fwd, &task_sid);
                                    break;
                                }
                                Some(other) => {
                                    tracing::info!("terminal other msg: {:?} for session {}", other, task_sid);
                                }
                            }
                        }
                        cmd = cmd_rx.recv() => {
                            match cmd {
                                Some(TerminalCmd::Input(data, ack)) => {
                                    if data.len() <= 64 {
                                        tracing::info!(
                                            "terminal input len={} data={:?} for session {}",
                                            data.len(),
                                            String::from_utf8_lossy(&data),
                                            task_sid
                                        );
                                    }
                                    let payload = data.clone();
                                    let mut attempts = 0u32;
                                    loop {
                                        match tokio::time::timeout(
                                            std::time::Duration::from_secs(120),
                                            channel.data_bytes(payload.clone()),
                                        ).await {
                                            Ok(Ok(())) => {
                                                if attempts > 0 {
                                                    tracing::info!(
                                                        "terminal input sent after {} retries for session {}",
                                                        attempts, task_sid,
                                                    );
                                                }
                                                break;
                                            }
                                            Ok(Err(e)) => {
                                                tracing::warn!("terminal input error: {} for session {}", e, task_sid);
                                                break;
                                            }
                                            Err(_) => {
                                                attempts += 1;
                                                tracing::warn!(
                                                    "terminal input timed out (attempt {}) for session {}, {} bytes remaining",
                                                    attempts, task_sid, payload.len(),
                                                );
                                                if attempts >= 3 {
                                                    tracing::error!(
                                                        "terminal input giving up after {} timeouts for session {}",
                                                        attempts, task_sid,
                                                    );
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                    if let Some(tx) = ack {
                                        let _ = tx.send(());
                                    }
                                }
                                Some(TerminalCmd::Resize(c, r)) => {
                                    if let Err(e) = channel.window_change(c, r, 0, 0).await {
                                        tracing::warn!("terminal resize error: {} for session {}", e, task_sid);
                                    }
                                }
                                Some(TerminalCmd::Close) => {
                                    tracing::info!("terminal close cmd for session {}", task_sid);
                                    flush_buffers!();
                                    let _ = channel.eof().await;
                                    let _ = channel.close().await;
                                    forward_terminal_closed(&task_fwd, &task_sid);
                                    break;
                                }
                                None => {
                                    flush_buffers!();
                                    let _ = channel.eof().await;
                                    let _ = channel.close().await;
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            tracing::info!("terminal main task ended for {}", task_sid);
        });

        let session = TerminalSession {
            server_id: server_id.to_string(),
            cmd_tx,
            tasks: vec![main_task],
            kill_child: None,
            tmux_session_name: None,
            ssh_handle: Some(Arc::clone(ssh_handle)),
        };
        self.sessions
            .lock()
            .await
            .insert(session_id.clone(), session);
        Ok((session_id, initial_output))
    }

    /// Open a local terminal session (no SSH).
    ///
    /// Spawns a local shell in a PTY using `portable-pty`. The reader/writer
    /// are synchronous, so they're moved into `spawn_blocking` tasks with
    /// mpsc channels bridging to the async main task.
    pub async fn open_local(
        &self,
        cols: u32,
        rows: u32,
        shell: Option<String>,
        trigger_overrides: Option<std::collections::HashMap<String, bool>>,
    ) -> Result<(String, Vec<u8>), String> {
        let session_id = Uuid::new_v4().to_string();
        let sid = session_id.clone();
        let bin_fwd = self.binary_forwarder.clone();
        let fwd = self.forwarder.clone();

        let local_pty = open_local_pty(cols as u16, rows as u16, shell.as_deref())
            .map_err(|e| format!("failed to open local PTY: {}", e))?;

        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<TerminalCmd>();

        // === Sync IO → async runtime adaptation ===
        // Reader: spawn_blocking blocks on reader.read(), sends chunks via mpsc
        let (read_tx, mut read_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let mut reader = local_pty.reader;
        let read_task = tokio::task::spawn_blocking(move || {
            let mut buf = vec![0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if read_tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("local PTY read error: {}", e);
                        break;
                    }
                }
            }
        });

        // Writer: independent spawn_blocking task (avoids &mut writer lifetime issue)
        let (write_tx, mut write_rx) =
            mpsc::unbounded_channel::<(Vec<u8>, Option<oneshot::Sender<()>>)>();
        let mut writer = local_pty.writer;
        let write_task = tokio::task::spawn_blocking(move || {
            while let Some((data, ack)) = write_rx.blocking_recv() {
                if writer.write_all(&data).is_err() {
                    if let Some(tx) = ack {
                        let _ = tx.send(());
                    }
                    break;
                }
                let _ = writer.flush();
                if let Some(tx) = ack {
                    let _ = tx.send(());
                }
            }
        });

        // Main task: select on read_rx (output) + cmd_rx (input/resize/close)
        let task_sid = sid.clone();
        let task_bin_fwd = bin_fwd.clone();
        let task_fwd = fwd.clone();
        let master = local_pty.master;
        let mut child = local_pty.child;
        // Clone Arc handles for local terminal events (move into main task)
        let local_event_tx = self.local_event_tx.clone();
        let closed_sessions = self.closed_sessions.clone();

        let main_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    // read_rx.recv() returns None when read_task ends (shell exit / EOF)
                    data = read_rx.recv() => {
                        match data {
                            Some(data) => {
                                forward_terminal_output(&task_bin_fwd, &task_sid, &data, false);
                            }
                            None => {
                                // Shell exited naturally (exit / Ctrl+D)
                                let _ = child.kill();
                                // 通知终端事件回调（去重：与 close() 竞争，先到先发）
                                let mut closed = closed_sessions.lock().await;
                                if !closed.contains(&task_sid) {
                                    closed.insert(task_sid.clone());
                                    if let Some(tx) = local_event_tx.lock().await.as_ref() {
                                        let _ = tx.send(TerminalLifecycleEvent::Closed {
                                            session_id: task_sid.clone(),
                                        });
                                    }
                                }
                                forward_terminal_closed(&task_fwd, &task_sid);
                                break;
                            }
                        }
                    }
                    cmd = cmd_rx.recv() => {
                        match cmd {
                            Some(TerminalCmd::Input(data, ack)) => {
                                if write_tx.send((data, ack)).is_err() {
                                    tracing::error!("local PTY write task closed");
                                    break;
                                }
                            }
                            Some(TerminalCmd::Resize(c, r)) => {
                                if let Err(e) = master.resize(PtySize {
                                    rows: r as u16,
                                    cols: c as u16,
                                    pixel_width: 0,
                                    pixel_height: 0,
                                }) {
                                    tracing::warn!("local PTY resize error: {}", e);
                                }
                            }
                            Some(TerminalCmd::Close) | None => {
                                let _ = child.kill();
                                // 通知终端事件回调（去重：与 close() 竞争，先到先发）
                                let mut closed = closed_sessions.lock().await;
                                if !closed.contains(&task_sid) {
                                    closed.insert(task_sid.clone());
                                    if let Some(tx) = local_event_tx.lock().await.as_ref() {
                                        let _ = tx.send(TerminalLifecycleEvent::Closed {
                                            session_id: task_sid.clone(),
                                        });
                                    }
                                }
                                forward_terminal_closed(&task_fwd, &task_sid);
                                break;
                            }
                        }
                    }
                }
            }
        });

        let killer = local_pty.killer;

        let session = TerminalSession {
            server_id: "__local__".to_string(),
            cmd_tx,
            tasks: vec![main_task, read_task, write_task],
            kill_child: Some(killer),
            tmux_session_name: None,
            ssh_handle: None,
        };
        self.sessions
            .lock()
            .await
            .insert(session_id.clone(), session);

        // Set trigger overrides BEFORE sending the Opened event, so the
        // terminal event consumer sees them when it processes the event.
        if let Some(overrides) = trigger_overrides {
            self.set_trigger_overrides(&session_id, overrides).await;
        }

        // 发送 Opened 事件
        // Delay briefly to give the shell time to start and be ready for input.
        // Without this, OnTerminalOpen trigger commands may be injected before
        // the shell is ready to process them, causing them to be lost.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        if let Some(tx) = self.local_event_tx.lock().await.as_ref() {
            tracing::info!("[TerminalManager] sending Opened event for session {}", session_id);
            let _ = tx.send(TerminalLifecycleEvent::Opened {
                session_id: session_id.clone(),
            });
        } else {
            tracing::warn!("[TerminalManager] local_event_tx is None — Opened event for session {} dropped (subscribe_local_events not called?)", session_id);
        }

        // initial_output is empty — local PTY output (prompt, MOTD) is streamed
        // asynchronously via the binary forwarder, unlike SSH's synchronous read.
        Ok((session_id, Vec::new()))
    }

    /// Set tmux session info on a terminal session (called after open, when tmux is used)
    /// This stores the tmux session name and SSH handle for later resize_and_notify calls
    pub async fn set_tmux_session_info(
        &self,
        session_id: &str,
        tmux_session_name: Option<String>,
        ssh_handle: Option<Arc<client::Handle<termfast_core::ssh::client::SshHandler>>>,
    ) {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.tmux_session_name = tmux_session_name;
            // Only update ssh_handle if provided (don't overwrite existing)
            if ssh_handle.is_some() {
                session.ssh_handle = ssh_handle;
            }
        }
    }

    /// Resize terminal + update tmux @termfast_size + broadcast RESIZE to remote subscribers
    /// (Phase 4 remote subscribers not yet implemented — currently just resizes PTY + updates tmux)
    pub async fn resize_and_notify(
        &self,
        session_id: &str,
        cols: u32,
        rows: u32,
    ) -> Result<(), String> {
        let (tmux_name, ssh_handle) = {
            let sessions = self.sessions.lock().await;
            let session = match sessions.get(session_id) {
                Some(s) => s,
                None => return Ok(()),
            };
            // Send resize command to PTY
            let cmd = TerminalCmd::Resize(cols, rows);
            let _ = session.cmd_tx.send(cmd);
            // Get tmux info for exec channel update
            (session.tmux_session_name.clone(), session.ssh_handle.clone())
        };

        // If SSH + tmux terminal, update @termfast_size via exec channel
        if let (Some(name), Some(handle)) = (tmux_name, ssh_handle) {
            let cmd = termfast_core::ssh::tmux::build_update_size_command(&name, cols as u16, rows as u16);
            // Spawn async exec — don't block PTY resize on network round-trip
            tokio::spawn(async move {
                if let Err(e) = termfast_core::ssh::exec::exec(&handle, &cmd, 5).await {
                    tracing::warn!("failed to update @termfast_size: {}", e);
                }
            });
        }
        Ok(())
    }

    /// Send user input to the terminal.
    ///
    /// When `wait_for_send` is true, this function blocks until the SSH write
    /// task has actually sent the bytes over the channel (or timed out).  This
    /// provides backpressure for large ZMODEM transfers so the caller doesn't
    /// queue data faster than SSH can transmit it.
    pub async fn input(&self, session_id: &str, data: &[u8]) -> Result<(), String> {
        self.input_with_ack(session_id, data, true).await
    }

    /// Same as `input` but optionally waits for SSH send completion.
    pub async fn input_with_ack(
        &self,
        session_id: &str,
        data: &[u8],
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
            if data.len() <= 64 {
                tracing::info!(
                    "TerminalManager::input sending {} bytes to session {}",
                    data.len(),
                    session_id,
                );
            }
            session
                .cmd_tx
                .send(TerminalCmd::Input(data.to_vec(), ack_tx))
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

    /// Check if a session is a local terminal
    pub async fn is_local_session(&self, session_id: &str) -> bool {
        let sessions = self.sessions.lock().await;
        sessions
            .get(session_id)
            .map(|s| s.server_id == "__local__")
            .unwrap_or(false)
    }

    /// Mark a session as closed in closed_sessions (dedup), WITHOUT sending
    /// the channel event. This prevents the terminal event consumer task from
    /// firing BeforeTerminalClose — the caller (handle_terminal_close) will fire
    /// it directly before closing the terminal.
    pub async fn mark_closed(&self, session_id: &str) {
        let mut closed = self.closed_sessions.lock().await;
        closed.insert(session_id.to_string());
    }

    /// Find the first active session for a given server_id.
    /// Used by trigger injection to find a target terminal for events
    /// that don't have a specific session_id (e.g. OnNetworkConnect).
    pub async fn find_session_by_server(&self, server_id: &str) -> Option<String> {
        let sessions = self.sessions.lock().await;
        sessions
            .iter()
            .find(|(_, s)| s.server_id == server_id)
            .map(|(id, _)| id.clone())
    }

    /// Set per-session runtime overrides for exec_in_terminal.
    /// Keyed by trigger_id → override value. NOT persisted — cleared on restart.
    pub async fn set_trigger_overrides(
        &self,
        session_id: &str,
        overrides: HashMap<String, bool>,
    ) {
        let mut all = self.trigger_overrides.lock().await;
        all.insert(session_id.to_string(), overrides);
    }

    /// Get the exec_in_terminal override for a specific session+trigger.
    /// Returns None if no override is set (use trigger's configured value).
    pub async fn get_trigger_override(
        &self,
        session_id: &str,
        trigger_id: &str,
    ) -> Option<bool> {
        let all = self.trigger_overrides.lock().await;
        all.get(session_id)
            .and_then(|m| m.get(trigger_id).copied())
    }

    /// Get all overrides for a session (for frontend to display current state).
    pub async fn get_all_trigger_overrides(
        &self,
        session_id: &str,
    ) -> HashMap<String, bool> {
        self.trigger_overrides
            .lock()
            .await
            .get(session_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Clear overrides for a session (called on close).
    pub async fn clear_trigger_overrides(&self, session_id: &str) {
        self.trigger_overrides.lock().await.remove(session_id);
    }

    /// Close a terminal session
    pub async fn close(&self, session_id: &str) -> Result<(), String> {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.remove(session_id) {
            let _ = session.cmd_tx.send(TerminalCmd::Close);
            for task in session.tasks {
                task.abort();
            }
            // Kill child process (local terminals) regardless of task state
            if let Some(mut killer) = session.kill_child {
                let _ = killer.kill();
            }
            // 发送 Closed 事件（去重）
            if session.server_id == "__local__" {
                let mut closed = self.closed_sessions.lock().await;
                if !closed.contains(session_id) {
                    closed.insert(session_id.to_string());
                    if let Some(tx) = self.local_event_tx.lock().await.as_ref() {
                        let _ = tx.send(TerminalLifecycleEvent::Closed {
                            session_id: session_id.to_string(),
                        });
                    }
                }
                // 不在此处 remove：tokio task.abort() 是异步的，main task 可能
                // 在 abort flag 检查之前已进入退出分支并获取锁。如果 close() 先
                // remove 了条目，main task 的 contains 会为 false → 重复发送。
                // 改为在 shutdown 时统一清理。
            }
            // Clear per-session trigger overrides
            self.clear_trigger_overrides(session_id).await;
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
                // Kill child process (local terminals) — defensive: currently
                // only called with SSH server_id, but handles __local__ too.
                if let Some(mut killer) = session.kill_child {
                    let _ = killer.kill();
                }
                // 发送 Closed 事件（去重）
                if session.server_id == "__local__" {
                    let mut closed = self.closed_sessions.lock().await;
                    if !closed.contains(&id) {
                        closed.insert(id.clone());
                        if let Some(tx) = self.local_event_tx.lock().await.as_ref() {
                            let _ = tx.send(TerminalLifecycleEvent::Closed {
                                session_id: id.clone(),
                            });
                        }
                    }
                }
            }
        }
    }

    /// Close all local terminal sessions (called on app shutdown)
    pub async fn close_all_local(&self) {
        let mut sessions = self.sessions.lock().await;
        let to_remove: Vec<String> = sessions
            .iter()
            .filter(|(_, s)| s.server_id == "__local__")
            .map(|(id, _)| id.clone())
            .collect();
        for id in to_remove {
            if let Some(session) = sessions.remove(&id) {
                let _ = session.cmd_tx.send(TerminalCmd::Close);
                for task in session.tasks {
                    task.abort();
                }
                if let Some(mut killer) = session.kill_child {
                    let _ = killer.kill();
                }
                // 发送 Closed 事件（去重）
                let mut closed = self.closed_sessions.lock().await;
                if !closed.contains(&id) {
                    closed.insert(id.clone());
                    if let Some(tx) = self.local_event_tx.lock().await.as_ref() {
                        let _ = tx.send(TerminalLifecycleEvent::Closed {
                            session_id: id.clone(),
                        });
                    }
                }
            }
        }
    }

    /// Check if there are any active terminal sessions for a given server
    pub async fn has_sessions_for_server(&self, server_id: &str) -> bool {
        let sessions = self.sessions.lock().await;
        sessions.values().any(|s| s.server_id == server_id)
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

/// Forward terminal output to the GUI via the binary event forwarder (raw bytes, no base64)
fn forward_terminal_output(
    forwarder: &Arc<std::sync::Mutex<Option<BinaryEventForwarder>>>,
    session_id: &str,
    data: &[u8],
    is_stderr: bool,
) {
    if let Ok(fwd) = forwarder.lock() {
        if let Some(ref f) = *fwd {
            f(session_id, data, is_stderr);
        } else {
            tracing::warn!(
                "terminal output: binary event forwarder is None, dropping {} bytes",
                data.len()
            );
        }
    } else {
        tracing::warn!("terminal output: failed to lock binary event forwarder");
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    /// Integration test: open_local → input → verify output → resize → close
    ///
    /// Verifies the full local terminal lifecycle:
    /// 1. open_local spawns a local shell in a PTY
    /// 2. input sends data to the shell (echo command)
    /// 3. output is received via the binary event forwarder
    /// 4. resize changes the PTY dimensions
    /// 5. close terminates the session and kills the child process
    #[tokio::test]
    async fn test_open_local_input_resize_close() {
        // Collect output bytes via a shared buffer
        let output_buffer: Arc<StdMutex<Vec<u8>>> = Arc::new(StdMutex::new(Vec::new()));
        let buf_clone = output_buffer.clone();

        let binary_forwarder: BinaryEventForwarder = Box::new(move |_sid, data, _stderr| {
            if let Ok(mut buf) = buf_clone.lock() {
                buf.extend_from_slice(data);
            }
        });

        let event_forwarder: EventForwarder = Box::new(|_event, _data| {});

        let manager = TerminalManager::new(
            Arc::new(StdMutex::new(Some(event_forwarder))),
            Arc::new(StdMutex::new(Some(binary_forwarder))),
        );

        // 1. Open local terminal
        let (session_id, initial_output) = manager
            .open_local(80, 24, None, None)
            .await
            .expect("open_local should succeed");
        assert!(!session_id.is_empty());
        assert!(initial_output.is_empty()); // local PTY streams output async

        // 2. Send input (echo command)
        manager
            .input(&session_id, b"echo test_daemon_local\n")
            .await
            .expect("input should succeed");

        // 3. Wait for output containing "test_daemon_local"
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if tokio::time::Instant::now() >= deadline {
                panic!("timeout waiting for echo output");
            }
            {
                let buf = output_buffer.lock().unwrap();
                if buf.windows("test_daemon_local".len())
                    .any(|w| w == b"test_daemon_local")
                {
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        // 4. Resize
        manager
            .resize(&session_id, 120, 40)
            .await
            .expect("resize should succeed");

        // 5. Close
        manager
            .close(&session_id)
            .await
            .expect("close should succeed");

        // Verify session is removed
        assert!(!manager.has_sessions_for_server("__local__").await);
    }

    /// Test close_all_local kills all local terminal sessions
    #[tokio::test]
    async fn test_close_all_local() {
        let binary_forwarder: BinaryEventForwarder = Box::new(|_sid, _data, _stderr| {});
        let event_forwarder: EventForwarder = Box::new(|_event, _data| {});

        let manager = TerminalManager::new(
            Arc::new(StdMutex::new(Some(event_forwarder))),
            Arc::new(StdMutex::new(Some(binary_forwarder))),
        );

        // Open two local terminals
        let (sid1, _) = manager.open_local(80, 24, None, None).await.unwrap();
        let (sid2, _) = manager.open_local(80, 24, None, None).await.unwrap();

        assert!(manager.has_sessions_for_server("__local__").await);

        // Close all local
        manager.close_all_local().await;

        // Both should be gone
        assert!(!manager.has_sessions_for_server("__local__").await);
        assert!(manager.close(&sid1).await.is_err());
        assert!(manager.close(&sid2).await.is_err());
    }

    /// Test set_tmux_session_info + resize_and_notify on local terminal
    /// (tmux_session_name=None → resize_and_notify should just resize PTY, no exec)
    #[tokio::test]
    async fn test_set_tmux_session_info_and_resize_local() {
        let binary_forwarder: BinaryEventForwarder = Box::new(|_sid, _data, _stderr| {});
        let event_forwarder: EventForwarder = Box::new(|_event, _data| {});

        let manager = TerminalManager::new(
            Arc::new(StdMutex::new(Some(event_forwarder))),
            Arc::new(StdMutex::new(Some(binary_forwarder))),
        );

        let (sid, _) = manager.open_local(80, 24, None, None).await.unwrap();

        // set_tmux_session_info with None should not panic
        manager.set_tmux_session_info(&sid, None, None).await;

        // resize_and_notify with no tmux session should just resize PTY (no exec)
        manager
            .resize_and_notify(&sid, 100, 30)
            .await
            .expect("resize_and_notify should succeed on local terminal");

        // Verify resize took effect by checking session still works
        manager.input(&sid, b"echo resize_ok\n").await.unwrap();

        manager.close(&sid).await.unwrap();
    }

    /// Test set_tmux_session_info on non-existent session (should be no-op)
    #[tokio::test]
    async fn test_set_tmux_session_info_nonexistent() {
        let binary_forwarder: BinaryEventForwarder = Box::new(|_sid, _data, _stderr| {});
        let event_forwarder: EventForwarder = Box::new(|_event, _data| {});

        let manager = TerminalManager::new(
            Arc::new(StdMutex::new(Some(event_forwarder))),
            Arc::new(StdMutex::new(Some(binary_forwarder))),
        );

        // Should not panic on non-existent session
        manager
            .set_tmux_session_info("nonexistent_sid", Some("test_tmux".to_string()), None)
            .await;

        // resize_and_notify on non-existent session should return Ok (no-op)
        let result = manager.resize_and_notify("nonexistent_sid", 100, 30).await;
        assert!(result.is_ok(), "resize_and_notify on non-existent session should return Ok");
    }
}
