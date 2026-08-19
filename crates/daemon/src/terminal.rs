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
#[cfg(not(target_os = "android"))]
use termfast_core::local::{ChildKiller, PtySize};
#[cfg(not(target_os = "android"))]
use termfast_core::local::pty::open_local_pty;
use termfast_core::ssh::pty;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task::JoinHandle;
use uuid::Uuid;

/// Android stub for ChildKiller trait (local terminals not supported on Android).
#[cfg(target_os = "android")]
pub trait ChildKiller: Send {
    fn kill(&mut self) -> Result<(), std::io::Error>;
}

/// Android stub for PtySize (local terminals not supported on Android).
#[cfg(target_os = "android")]
#[derive(Clone, Copy, Debug)]
pub struct PtySize {
    pub rows: u16,
    pub cols: u16,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

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
    /// Display name for LIST_REQUEST (local label or server name + index)
    name: String,
    /// Ring buffer of recent output (256KB) for remote subscriber history recovery
    history: Arc<std::sync::RwLock<crate::remote_frame::RingBuffer>>,
    /// Remote subscribers (mobile clients via relay tunnel)
    remote_subscribers: Arc<std::sync::Mutex<Vec<RemoteSubscriber>>>,
    /// Current PTY size (cols, rows) — read by SUBSCRIBE, updated by resize
    pty_size: Arc<std::sync::Mutex<(u16, u16)>>,
}

/// A remote subscriber = one mobile client's output channel
pub struct RemoteSubscriber {
    pub pairing_id: String,
    pub terminal_id: u32,
    pub sender: tokio::sync::mpsc::Sender<crate::remote_frame::Frame>,
    pub lagging: bool,
}

// === SECTION 1 END ===

/// Type alias for the on_closed callback function.
pub type OnClosedCallback = Box<dyn Fn(&str) + Send + Sync>;

/// Type alias for the on_opened callback function.
pub type OnOpenedCallback = Box<dyn Fn() + Send + Sync>;

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
    /// Callback invoked when a terminal closes (for RemoteServer to clean up IdMap).
    /// Set by RemoteServer via set_on_closed_callback.
    on_closed_callback: Arc<std::sync::Mutex<Option<OnClosedCallback>>>,
    /// Callback invoked when a terminal is opened (for RemoteServer to broadcast LIST_CHANGED).
    on_opened_callback: Arc<std::sync::Mutex<Option<OnOpenedCallback>>>,
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
            on_closed_callback: Arc::new(std::sync::Mutex::new(None)),
            on_opened_callback: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Set a callback invoked when a terminal closes (for RemoteServer IdMap cleanup).
    pub fn set_on_closed_callback(&self, callback: OnClosedCallback) {
        *self.on_closed_callback.lock().unwrap() = Some(callback);
    }

    /// Set a callback invoked when a terminal is opened (for broadcasting LIST_CHANGED).
    pub fn set_on_opened_callback(&self, callback: OnOpenedCallback) {
        *self.on_opened_callback.lock().unwrap() = Some(callback);
    }

    /// Invoke the on_opened callback (if set).
    pub fn notify_opened(&self) {
        if let Ok(cb) = self.on_opened_callback.lock() {
            if let Some(ref f) = *cb {
                tracing::info!("[TerminalManager] notify_opened: broadcasting LIST_CHANGED");
                f();
            }
        }
    }

    /// Invoke the on_closed callback (if set) for a session_id.
    fn notify_closed(&self, session_id: &str) {
        if let Ok(cb) = self.on_closed_callback.lock() {
            if let Some(ref f) = *cb {
                f(session_id);
            }
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

        // Create shared state Arcs early so reader task and TerminalSession share them
        let history: Arc<std::sync::RwLock<crate::remote_frame::RingBuffer>> =
            Arc::new(std::sync::RwLock::new(crate::remote_frame::RingBuffer::new(256 * 1024)));
        let remote_subscribers: Arc<std::sync::Mutex<Vec<RemoteSubscriber>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let pty_size: Arc<std::sync::Mutex<(u16, u16)>> =
            Arc::new(std::sync::Mutex::new((cols as u16, rows as u16)));

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
        let task_history = history.clone();
        let task_remote_subs = remote_subscribers.clone();
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
                        forward_and_broadcast(&task_bin_fwd, &task_history, &task_remote_subs, &task_sid, &data_buf, false);
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
            name: format!("{} #?", server_id),
            history,
            remote_subscribers,
            pty_size,
        };
        self.sessions
            .lock()
            .await
            .insert(session_id.clone(), session);

        // Note: notify_opened() is called by the handler after set_session_name.

        Ok((session_id, initial_output))
    }

    /// Open a local terminal session (no SSH).
    ///
    /// Spawns a local shell in a PTY using `portable-pty`. The reader/writer
    /// are synchronous, so they're moved into `spawn_blocking` tasks with
    /// mpsc channels bridging to the async main task.
    #[cfg(not(target_os = "android"))]
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

        // Create shared state Arcs early so reader task and TerminalSession share them
        let history: Arc<std::sync::RwLock<crate::remote_frame::RingBuffer>> =
            Arc::new(std::sync::RwLock::new(crate::remote_frame::RingBuffer::new(256 * 1024)));
        let remote_subscribers: Arc<std::sync::Mutex<Vec<RemoteSubscriber>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let pty_size: Arc<std::sync::Mutex<(u16, u16)>> =
            Arc::new(std::sync::Mutex::new((cols as u16, rows as u16)));

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
        let task_history = history.clone();
        let task_remote_subs = remote_subscribers.clone();
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
                                // Note: Windows ConPTY DSR (ESC[6n) is handled
                                // preemptively in open_local_pty, which writes
                                // a CPR (ESC[1;1R) to the writer before returning
                                // LocalPty. By the time data reaches here, ConPTY
                                // is already unblocked.
                                forward_and_broadcast(&task_bin_fwd, &task_history, &task_remote_subs, &task_sid, &data, false);
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
            name: "Local".to_string(),
            history,
            remote_subscribers,
            pty_size,
        };
        self.sessions
            .lock()
            .await
            .insert(session_id.clone(), session);

        // Note: notify_opened() is NOT called here — the caller (handler) must
        // set the session name first, then call notify_opened() to broadcast
        // LIST_CHANGED. This ensures mobile sees the correct name in the list.

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
        // Forward terminal:opened to the GUI so the frontend can auto-create a tab
        // (needed when a terminal is opened by RemoteServer on behalf of a remote desktop)
        forward_terminal_opened(&self.forwarder, &session_id);

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

    /// Update a session's display name (used by remote terminal list).
    pub async fn set_session_name(&self, session_id: &str, name: &str) {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.name = name.to_string();
        }
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
            // Notify RemoteServer to clean up IdMap (if callback set)
            self.notify_closed(session_id);
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
                // Notify RemoteServer to clean up IdMap
                self.notify_closed(&id);
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
                // Notify RemoteServer to clean up IdMap
                self.notify_closed(&id);
            }
        }
    }

    /// Check if there are any active terminal sessions for a given server
    pub async fn has_sessions_for_server(&self, server_id: &str) -> bool {
        let sessions = self.sessions.lock().await;
        sessions.values().any(|s| s.server_id == server_id)
    }

    /// List all session infos for remote LIST_REQUEST.
    /// Per design doc: server_name is left empty (filled by RemoteServer.handle_list
    /// via ConfigManager). preview is computed from ring buffer tail (ANSI stripped).
    pub async fn list_session_infos(&self) -> Vec<SessionInfo> {
        // Phase 1: hold sessions lock, only do lightweight clone
        #[derive(Clone)]
        struct SessionSnapshot {
            sid: String,
            name: String,
            server_id: String,
            is_local: bool,
            tmux_session_name: Option<String>,
            tail_bytes: Vec<u8>,
        }
        let snapshots: Vec<SessionSnapshot> = {
            let sessions = self.sessions.lock().await;
            sessions.iter().map(|(sid, s)| {
                let tail_bytes = {
                    let history = s.history.read().unwrap();
                    let all_bytes: Vec<u8> = history.iter().flatten().cloned().collect();
                    if all_bytes.len() > 2048 {
                        all_bytes[all_bytes.len()-2048..].to_vec()
                    } else {
                        all_bytes
                    }
                };
                let is_local = s.server_id == "__local__";
                SessionSnapshot {
                    sid: sid.clone(),
                    name: s.name.clone(),
                    server_id: s.server_id.clone(),
                    is_local,
                    tmux_session_name: s.tmux_session_name.clone(),
                    tail_bytes,
                }
            }).collect()
        }; // sessions lock released

        // Phase 2: outside lock, do CPU-intensive ANSI stripping
        snapshots.into_iter().map(|snap| {
            let preview = strip_ansi(&String::from_utf8_lossy(&snap.tail_bytes))
                .lines().rev().take(5)
                .collect::<Vec<_>>().iter().rev()
                .copied().collect::<Vec<_>>().join("\n");
            let terminal_type = if snap.is_local { "local" } else { "ssh" };
            SessionInfo {
                session_id: snap.sid,
                name: snap.name,
                server_id: snap.server_id,
                is_local: snap.is_local,
                tmux_session_name: snap.tmux_session_name,
                server_name: String::new(), // filled by RemoteServer.handle_list
                terminal_type: terminal_type.to_string(),
                status: "active".to_string(),
                preview,
            }
        }).collect()
    }

    /// Subscribe a remote client to a terminal session.
    ///
    /// Atomic operation (per design doc): within remote_subscribers lock,
    /// 1. Remove old subscriber with same pairing_id (idempotent SUBSCRIBE)
    /// 2. Read ring buffer snapshot
    /// 3. Push RESIZE + HISTORY frames into subscriber channel (try_send)
    /// 4. Add subscriber to remote_subscribers
    ///
    /// This ensures no OUTPUT is lost between snapshot and subscription —
    /// reader task's broadcast is blocked by the lock, and OUTPUT frames
    /// queued after unlock will follow HISTORY in the mpsc channel (FIFO).
    ///
    /// Returns Ok(()) on success. The subscriber's channel receives
    /// RESIZE + HISTORY frames immediately (within the lock).
    pub async fn subscribe_remote(
        &self,
        session_id: &str,
        subscriber: RemoteSubscriber,
    ) -> Result<(), String> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| format!("session {} not found", session_id))?;

        let terminal_id = subscriber.terminal_id;
        let pairing_id = subscriber.pairing_id.clone();

        // Lock remote_subscribers for atomic operation
        let mut subs = session.remote_subscribers.lock().unwrap();

        // 1. Idempotent: remove old subscriber with same pairing_id
        subs.retain(|s| s.pairing_id != pairing_id);

        // 2. Read ring buffer snapshot (under subs lock, nested history read lock)
        let history_bytes: Vec<u8> = {
            let hb = session.history.read().unwrap();
            hb.iter().flatten().cloned().collect()
        };

        // 3. RESIZE frame is NOT pushed here — the mobile client determines
        // its own dimensions based on screen size and sends RESIZE to us.
        // Pushing our PTY size would overwrite the mobile's chosen dimensions
        // and cause a resize loop.

        // 4. Push HISTORY frames (chunked by MAX_HISTORY_DATA) into subscriber channel
        if !history_bytes.is_empty() {
            let hist_chunks: Vec<&[u8]> =
                history_bytes.chunks(crate::remote_frame::MAX_HISTORY_DATA).collect();
            let total = hist_chunks.len();
            for (seq, chunk) in hist_chunks.iter().enumerate() {
                let is_last = seq == total - 1;
                let hist_frame = crate::remote_frame::Frame::history(
                    terminal_id,
                    seq as u32,
                    is_last,
                    chunk,
                );
                let _ = subscriber.sender.try_send(hist_frame);
            }
        }

        // 5. Add subscriber to remote_subscribers
        subs.push(subscriber);

        Ok(())
    }

    /// Unsubscribe a remote client from a terminal session.
    pub async fn unsubscribe_remote(&self, session_id: &str, pairing_id: &str) {
        let sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get(session_id) {
            let mut subs = session.remote_subscribers.lock().unwrap();
            subs.retain(|s| s.pairing_id != pairing_id);
        }
    }

    /// Forward remote input to a terminal session.
    pub async fn remote_input(&self, session_id: &str, data: &[u8]) -> Result<(), String> {
        self.input(session_id, data).await
    }

    /// Broadcast a frame to all remote subscribers of a terminal session.
    /// Used by RemoteServer.handle_answer to broadcast QUESTION_RESOLVED.
    pub async fn broadcast_to_subscribers(&self, session_id: &str, frame: crate::remote_frame::Frame) {
        let sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get(session_id) {
            let subs = session.remote_subscribers.lock().unwrap();
            for sub in subs.iter() {
                let _ = sub.sender.try_send(frame.clone());
            }
        }
    }

    /// Remove all remote subscribers with the given pairing_id across all sessions.
    /// Used by RemoteServer.revoke_pairing to disconnect a revoked phone.
    pub async fn remove_remote_subscribers(&self, pairing_id: &str) {
        let sessions = self.sessions.lock().await;
        for session in sessions.values() {
            let mut subs = session.remote_subscribers.lock().unwrap();
            subs.retain(|s| s.pairing_id != pairing_id);
        }
    }

    /// Get current PTY size for a session.
    pub async fn get_pty_size(&self, session_id: &str) -> Option<(u16, u16)> {
        let sessions = self.sessions.lock().await;
        sessions.get(session_id).map(|s| *s.pty_size.lock().unwrap())
    }

    /// Update PTY size (called on resize).
    pub async fn update_pty_size(&self, session_id: &str, cols: u16, rows: u16) {
        let sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get(session_id) {
            *session.pty_size.lock().unwrap() = (cols, rows);
        }
    }

    /// Get history snapshot for REDRAW_REQUEST.
    pub async fn get_history(&self, session_id: &str) -> Option<Vec<bytes::Bytes>> {
        let sessions = self.sessions.lock().await;
        sessions.get(session_id).map(|s| {
            let hb = s.history.read().unwrap();
            hb.iter().cloned().collect()
        })
    }
}

// === SECTION 2 END ===

/// Session info for remote LIST_RESPONSE.
/// Per design doc: server_name filled by RemoteServer.handle_list via ConfigManager.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub name: String,
    pub server_id: String,
    pub is_local: bool,
    pub tmux_session_name: Option<String>,
    /// Server display name — filled by RemoteServer.handle_list via ConfigManager.
    /// "__local__" → "桌面端", else → config server name.
    pub server_name: String,
    /// "local" or "ssh" — determines mobile connection mode (relay vs SSH direct).
    pub terminal_type: String,
    /// "active" (PTY running) or "closed" (closing). Sessions in map are always active.
    pub status: String,
    /// Last 5 lines of output (ANSI stripped) for preview in terminal list.
    pub preview: String,
}

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

/// Forward terminal output to the GUI + write to ring buffer + broadcast to remote subscribers.
///
/// This is the unified output path for both SSH (`open()`) and local (`open_local()`) terminals.
/// It replaces `forward_terminal_output` in reader tasks so that remote subscribers (mobile clients)
/// receive output in addition to the desktop GUI.
///
/// Lock order: remote_subscribers → history (nested write/read within subs lock).
/// stderr is forwarded to GUI but NOT written to ring buffer or broadcast (per design).
fn forward_and_broadcast(
    forwarder: &Arc<std::sync::Mutex<Option<BinaryEventForwarder>>>,
    history: &Arc<std::sync::RwLock<crate::remote_frame::RingBuffer>>,
    remote_subscribers: &Arc<std::sync::Mutex<Vec<RemoteSubscriber>>>,
    session_id: &str,
    data: &[u8],
    is_stderr: bool,
) {
    // 1. Forward to desktop GUI (same as forward_terminal_output)
    forward_terminal_output(forwarder, session_id, data, is_stderr);

    if is_stderr {
        return;
    }

    // 2. Write to ring buffer + broadcast to remote subscribers
    let mut subs = match remote_subscribers.lock() {
        Ok(guard) => guard,
        Err(_) => {
            tracing::warn!("forward_and_broadcast: remote_subscribers lock poisoned for session {}", session_id);
            return;
        }
    };

    // Write to ring buffer (under remote_subscribers lock, nested history write lock)
    if let Ok(mut hb) = history.write() {
        hb.push(bytes::Bytes::from(data.to_vec()));
    }

    // Broadcast to subscribers with backpressure handling
    let chunks: Vec<&[u8]> = data.chunks(crate::remote_frame::MAX_OUTPUT_DATA).collect();
    for sub in subs.iter_mut() {
        if sub.lagging {
            // Backpressure recovery: if channel has capacity, send HISTORY snapshot
            if sub.sender.capacity() > 0 {
                let hist_data: Vec<u8> = {
                    if let Ok(hb) = history.read() {
                        hb.iter().flatten().cloned().collect()
                    } else {
                        continue;
                    }
                };
                let hist_chunks: Vec<&[u8]> =
                    hist_data.chunks(crate::remote_frame::MAX_HISTORY_DATA).collect();
                let total = hist_chunks.len();
                for (seq, chunk) in hist_chunks.iter().enumerate() {
                    let is_last = seq == total - 1;
                    let frame = crate::remote_frame::Frame::history(
                        sub.terminal_id,
                        seq as u32,
                        is_last,
                        chunk,
                    );
                    let _ = sub.sender.try_send(frame);
                }
                sub.lagging = false;
            }
            continue;
        }

        // Normal broadcast: send OUTPUT frames (chunked if data > MAX_OUTPUT_DATA)
        let mut failed = false;
        for chunk in &chunks {
            if failed {
                break;
            }
            let frame = crate::remote_frame::Frame::output(sub.terminal_id, chunk);
            match sub.sender.try_send(frame) {
                Ok(()) => {}
                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                    sub.lagging = true;
                    failed = true;
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    failed = true;
                }
            }
        }
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

/// Forward terminal:opened event to the GUI.
/// Used when a terminal is opened by a remote desktop (via RemoteServer),
/// so the local frontend can auto-create a tab for it.
fn forward_terminal_opened(
    forwarder: &Arc<std::sync::Mutex<Option<EventForwarder>>>,
    session_id: &str,
) {
    if let Ok(fwd) = forwarder.lock() {
        if let Some(ref f) = *fwd {
            f(
                "terminal:opened",
                serde_json::json!({ "sessionId": session_id }),
            );
        }
    }
}

/// Strip ANSI escape sequences from a string (for preview in terminal list).
/// Removes CSI sequences (ESC [ ... letter), OSC sequences (ESC ] ... BEL/ST),
/// and simple ESC sequences (ESC + single char).
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            // ESC sequence
            i += 1;
            if i >= bytes.len() { break; }
            if bytes[i] == b'[' {
                // CSI: ESC [ ... 0x40-0x7E
                i += 1;
                while i < bytes.len() && !(bytes[i] >= 0x40 && bytes[i] <= 0x7e) {
                    i += 1;
                }
                if i < bytes.len() { i += 1; } // skip final byte
            } else if bytes[i] == b']' {
                // OSC: ESC ] ... BEL (0x07) or ST (ESC \)
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == 0x07 { i += 1; break; }
                    if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i+1] == b'\\' { i += 2; break; }
                    i += 1;
                }
            } else {
                // Simple: ESC + single char
                i += 1;
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    result
}

#[cfg(test)]
impl TerminalManager {
    /// Test helper: register a mock SSH session (non-local server_id) without
    /// actually opening an SSH connection. Used for FILE_REQUEST SSH tests.
    pub async fn register_mock_ssh_session(
        &self,
        session_id: &str,
        server_id: &str,
        cols: u32,
        rows: u32,
    ) {
        let history: Arc<std::sync::RwLock<crate::remote_frame::RingBuffer>> =
            Arc::new(std::sync::RwLock::new(crate::remote_frame::RingBuffer::new(256 * 1024)));
        let remote_subscribers: Arc<std::sync::Mutex<Vec<RemoteSubscriber>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let pty_size: Arc<std::sync::Mutex<(u16, u16)>> =
            Arc::new(std::sync::Mutex::new((cols as u16, rows as u16)));
        let (cmd_tx, _cmd_rx) = mpsc::unbounded_channel::<TerminalCmd>();
        let session = TerminalSession {
            name: format!("ssh-{}", server_id),
            server_id: server_id.to_string(),
            cmd_tx,
            tasks: Vec::new(),
            history,
            remote_subscribers,
            pty_size,
            kill_child: None,
            tmux_session_name: None,
            ssh_handle: None,
        };
        self.sessions.lock().await.insert(session_id.to_string(), session);
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

    // === forward_and_broadcast tests ===

    /// Helper: create a RemoteSubscriber with a bounded channel
    fn make_subscriber(pairing_id: &str, terminal_id: u32, capacity: usize) -> (RemoteSubscriber, mpsc::Receiver<crate::remote_frame::Frame>) {
        let (tx, rx) = mpsc::channel(capacity);
        (RemoteSubscriber {
            pairing_id: pairing_id.to_string(),
            terminal_id,
            sender: tx,
            lagging: false,
        }, rx)
    }

    /// Test forward_and_broadcast: normal path — stdout writes to ring buffer + broadcasts to subscriber
    #[tokio::test]
    async fn test_forward_and_broadcast_normal() {
        let gui_output: Arc<StdMutex<Vec<u8>>> = Arc::new(StdMutex::new(Vec::new()));
        let gui_clone = gui_output.clone();
        let bin_fwd: BinaryEventForwarder = Box::new(move |_sid, data, _stderr| {
            if let Ok(mut buf) = gui_clone.lock() {
                buf.extend_from_slice(data);
            }
        });
        let forwarder: Arc<StdMutex<Option<BinaryEventForwarder>>> =
            Arc::new(StdMutex::new(Some(bin_fwd)));
        let history = Arc::new(std::sync::RwLock::new(crate::remote_frame::RingBuffer::new(256 * 1024)));
        let remote_subs = Arc::new(StdMutex::new(Vec::new()));

        let (sub, mut rx) = make_subscriber("pair1", 42, 256);
        remote_subs.lock().unwrap().push(sub);

        forward_and_broadcast(&forwarder, &history, &remote_subs, "sid1", b"hello world", false);

        // GUI should receive the data
        assert_eq!(&*gui_output.lock().unwrap(), b"hello world");

        // Ring buffer should have the data
        assert_eq!(history.read().unwrap().total_bytes(), 11);

        // Subscriber should receive an OUTPUT frame
        let frame = rx.recv().await.unwrap();
        assert_eq!(frame.frame_type, crate::remote_frame::OUTPUT);
        assert_eq!(frame.terminal_id, 42);
        assert_eq!(&frame.payload, b"hello world");
    }

    /// Test forward_and_broadcast: stderr forwarded to GUI but NOT to ring buffer or subscribers
    #[tokio::test]
    async fn test_forward_and_broadcast_stderr() {
        let gui_output: Arc<StdMutex<Vec<u8>>> = Arc::new(StdMutex::new(Vec::new()));
        let gui_clone = gui_output.clone();
        let bin_fwd: BinaryEventForwarder = Box::new(move |_sid, data, stderr| {
            if let Ok(mut buf) = gui_clone.lock() {
                if stderr {
                    buf.extend_from_slice(b"[ERR]");
                }
                buf.extend_from_slice(data);
            }
        });
        let forwarder = Arc::new(StdMutex::new(Some(bin_fwd)));
        let history = Arc::new(std::sync::RwLock::new(crate::remote_frame::RingBuffer::new(256 * 1024)));
        let remote_subs = Arc::new(StdMutex::new(Vec::new()));

        let (sub, mut rx) = make_subscriber("pair1", 1, 256);
        remote_subs.lock().unwrap().push(sub);

        forward_and_broadcast(&forwarder, &history, &remote_subs, "sid1", b"error msg", true);

        // GUI should receive stderr data
        assert_eq!(&*gui_output.lock().unwrap(), b"[ERR]error msg");

        // Ring buffer should be empty (stderr not stored)
        assert_eq!(history.read().unwrap().total_bytes(), 0);

        // Subscriber should NOT receive any frame
        let frame = tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv()).await;
        assert!(frame.is_err() || frame.unwrap().is_none(), "stderr should not be broadcast to subscribers");
    }

    /// Test forward_and_broadcast: backpressure — channel full marks lagging, recovery sends HISTORY
    #[tokio::test]
    async fn test_forward_and_broadcast_backpressure_recovery() {
        let bin_fwd: BinaryEventForwarder = Box::new(|_sid, _data, _stderr| {});
        let forwarder = Arc::new(StdMutex::new(Some(bin_fwd)));
        let history = Arc::new(std::sync::RwLock::new(crate::remote_frame::RingBuffer::new(256 * 1024)));
        let remote_subs = Arc::new(StdMutex::new(Vec::new()));

        // Channel capacity = 1, so second send will fail → mark lagging
        let (sub, mut rx) = make_subscriber("pair1", 5, 1);
        remote_subs.lock().unwrap().push(sub);

        // First call: sends one OUTPUT frame (fills channel, capacity now 0)
        forward_and_broadcast(&forwarder, &history, &remote_subs, "sid1", b"first", false);
        // Do NOT drain — channel is full

        // Second call: channel is full → try_send fails → lagging = true, no OUTPUT sent
        forward_and_broadcast(&forwarder, &history, &remote_subs, "sid1", b"second", false);
        // Ring buffer should have both "first" and "second"
        assert_eq!(history.read().unwrap().total_bytes(), 11);
        // Subscriber should be marked lagging
        assert!(remote_subs.lock().unwrap()[0].lagging, "subscriber should be marked lagging");

        // Now drain the channel so capacity > 0 (simulates subscriber consuming)
        let frame1 = rx.recv().await.unwrap();
        assert_eq!(frame1.frame_type, crate::remote_frame::OUTPUT);
        assert_eq!(&frame1.payload, b"first");

        // Third call: lagging recovery — channel has capacity → sends HISTORY snapshot
        forward_and_broadcast(&forwarder, &history, &remote_subs, "sid1", b"third", false);

        // Should receive HISTORY frames (snapshot of ring buffer: "first" + "second" + "third")
        let mut got_history = false;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            match tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await {
                Ok(Some(frame)) => {
                    if frame.frame_type == crate::remote_frame::HISTORY {
                        got_history = true;
                    }
                }
                _ => break,
            }
        }
        assert!(got_history, "should receive HISTORY frame during lagging recovery");
        // After recovery, lagging should be false
        assert!(!remote_subs.lock().unwrap()[0].lagging, "subscriber should not be lagging after recovery");
    }

    /// Test forward_and_broadcast: large data (> MAX_OUTPUT_DATA) is chunked into multiple OUTPUT frames
    #[tokio::test]
    async fn test_forward_and_broadcast_large_data_chunking() {
        let bin_fwd: BinaryEventForwarder = Box::new(|_sid, _data, _stderr| {});
        let forwarder = Arc::new(StdMutex::new(Some(bin_fwd)));
        let history = Arc::new(std::sync::RwLock::new(crate::remote_frame::RingBuffer::new(256 * 1024)));
        let remote_subs = Arc::new(StdMutex::new(Vec::new()));

        let (sub, mut rx) = make_subscriber("pair1", 7, 256);
        remote_subs.lock().unwrap().push(sub);

        // Create data larger than MAX_OUTPUT_DATA (65536)
        let large_data = vec![0xABu8; crate::remote_frame::MAX_OUTPUT_DATA + 1000];

        forward_and_broadcast(&forwarder, &history, &remote_subs, "sid1", &large_data, false);

        // Should receive 2 OUTPUT frames (65536 + 1000)
        let mut total_received = 0usize;
        let mut frame_count = 0u32;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if tokio::time::Instant::now() >= deadline || frame_count >= 2 {
                break;
            }
            match tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await {
                Ok(Some(frame)) => {
                    assert_eq!(frame.frame_type, crate::remote_frame::OUTPUT);
                    assert_eq!(frame.terminal_id, 7);
                    total_received += frame.payload.len();
                    frame_count += 1;
                }
                _ => break,
            }
        }
        assert_eq!(frame_count, 2, "should receive exactly 2 chunked OUTPUT frames");
        assert_eq!(total_received, large_data.len());

        // Ring buffer should have the full data
        assert_eq!(history.read().unwrap().total_bytes(), large_data.len());
    }

    /// Test forward_and_broadcast: no subscribers — data still goes to GUI and ring buffer
    #[tokio::test]
    async fn test_forward_and_broadcast_no_subscribers() {
        let gui_output: Arc<StdMutex<Vec<u8>>> = Arc::new(StdMutex::new(Vec::new()));
        let gui_clone = gui_output.clone();
        let bin_fwd: BinaryEventForwarder = Box::new(move |_sid, data, _stderr| {
            if let Ok(mut buf) = gui_clone.lock() {
                buf.extend_from_slice(data);
            }
        });
        let forwarder = Arc::new(StdMutex::new(Some(bin_fwd)));
        let history = Arc::new(std::sync::RwLock::new(crate::remote_frame::RingBuffer::new(256 * 1024)));
        let remote_subs = Arc::new(StdMutex::new(Vec::new()));

        forward_and_broadcast(&forwarder, &history, &remote_subs, "sid1", b"no subs", false);

        // GUI should receive data
        assert_eq!(&*gui_output.lock().unwrap(), b"no subs");
        // Ring buffer should have data
        assert_eq!(history.read().unwrap().total_bytes(), 7);
    }

    /// Test forward_and_broadcast: multiple subscribers all receive OUTPUT
    #[tokio::test]
    async fn test_forward_and_broadcast_multiple_subscribers() {
        let bin_fwd: BinaryEventForwarder = Box::new(|_sid, _data, _stderr| {});
        let forwarder = Arc::new(StdMutex::new(Some(bin_fwd)));
        let history = Arc::new(std::sync::RwLock::new(crate::remote_frame::RingBuffer::new(256 * 1024)));
        let remote_subs = Arc::new(StdMutex::new(Vec::new()));

        let (sub1, mut rx1) = make_subscriber("pair1", 1, 256);
        let (sub2, mut rx2) = make_subscriber("pair2", 2, 256);
        remote_subs.lock().unwrap().push(sub1);
        remote_subs.lock().unwrap().push(sub2);

        forward_and_broadcast(&forwarder, &history, &remote_subs, "sid1", b"broadcast", false);

        let f1 = rx1.recv().await.unwrap();
        assert_eq!(f1.frame_type, crate::remote_frame::OUTPUT);
        assert_eq!(f1.terminal_id, 1);
        assert_eq!(&f1.payload, b"broadcast");

        let f2 = rx2.recv().await.unwrap();
        assert_eq!(f2.frame_type, crate::remote_frame::OUTPUT);
        assert_eq!(f2.terminal_id, 2);
        assert_eq!(&f2.payload, b"broadcast");
    }

    /// Test forward_and_broadcast: channel closed (subscriber disconnected) — no panic, data still to GUI/ring buffer
    #[tokio::test]
    async fn test_forward_and_broadcast_channel_closed() {
        let gui_output: Arc<StdMutex<Vec<u8>>> = Arc::new(StdMutex::new(Vec::new()));
        let gui_clone = gui_output.clone();
        let bin_fwd: BinaryEventForwarder = Box::new(move |_sid, data, _stderr| {
            if let Ok(mut buf) = gui_clone.lock() {
                buf.extend_from_slice(data);
            }
        });
        let forwarder = Arc::new(StdMutex::new(Some(bin_fwd)));
        let history = Arc::new(std::sync::RwLock::new(crate::remote_frame::RingBuffer::new(256 * 1024)));
        let remote_subs = Arc::new(StdMutex::new(Vec::new()));

        let (sub, rx) = make_subscriber("pair1", 9, 256);
        remote_subs.lock().unwrap().push(sub);
        // Drop receiver → sender will get Closed error on try_send
        drop(rx);

        // Should not panic
        forward_and_broadcast(&forwarder, &history, &remote_subs, "sid1", b"closed chan", false);

        // GUI and ring buffer should still have data
        assert_eq!(&*gui_output.lock().unwrap(), b"closed chan");
        assert_eq!(history.read().unwrap().total_bytes(), 11); // "closed chan" = 11 bytes
    }

    /// Test forward_and_broadcast: HISTORY snapshot content during lagging recovery
    #[tokio::test]
    async fn test_forward_and_broadcast_history_snapshot_content() {
        let bin_fwd: BinaryEventForwarder = Box::new(|_sid, _data, _stderr| {});
        let forwarder = Arc::new(StdMutex::new(Some(bin_fwd)));
        let history = Arc::new(std::sync::RwLock::new(crate::remote_frame::RingBuffer::new(256 * 1024)));
        let remote_subs = Arc::new(StdMutex::new(Vec::new()));

        let (sub, mut rx) = make_subscriber("pair1", 3, 1);
        remote_subs.lock().unwrap().push(sub);

        // Fill ring buffer with known data, force lagging, then recover
        forward_and_broadcast(&forwarder, &history, &remote_subs, "sid1", b"AAA", false);
        // Don't drain — channel full
        forward_and_broadcast(&forwarder, &history, &remote_subs, "sid1", b"BBB", false);
        // Now lagging, ring buffer has "AAA"+"BBB" = 6 bytes
        assert!(remote_subs.lock().unwrap()[0].lagging);

        // Drain channel
        let _ = rx.recv().await.unwrap();

        // Recover — should send HISTORY with content "AAABBB"
        forward_and_broadcast(&forwarder, &history, &remote_subs, "sid1", b"CCC", false);

        // Collect all HISTORY frames and verify content
        let mut history_data = Vec::new();
        let mut last_seq = None;
        let mut got_is_last = false;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            match tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await {
                Ok(Some(frame)) => {
                    if frame.frame_type == crate::remote_frame::HISTORY {
                        // payload = [seq:4][is_last:1][data]
                        assert!(frame.payload.len() >= 5, "HISTORY payload too short");
                        let seq = u32::from_be_bytes([
                            frame.payload[0], frame.payload[1], frame.payload[2], frame.payload[3],
                        ]);
                        let is_last = frame.payload[4];
                        // Verify seq increments
                        if let Some(prev) = last_seq {
                            assert_eq!(seq, prev + 1, "HISTORY seq should increment");
                        } else {
                            assert_eq!(seq, 0, "first HISTORY seq should be 0");
                        }
                        last_seq = Some(seq);
                        if is_last == 1 {
                            got_is_last = true;
                        }
                        history_data.extend_from_slice(&frame.payload[5..]);
                    }
                }
                _ => break,
            }
        }
        assert!(got_is_last, "should receive HISTORY with is_last=1");
        // Ring buffer had "AAA"+"BBB"+"CCC" = 9 bytes
        assert_eq!(history_data, b"AAABBBCCC", "HISTORY snapshot should contain all ring buffer data");
        assert!(!remote_subs.lock().unwrap()[0].lagging, "should not be lagging after recovery");
    }
}
