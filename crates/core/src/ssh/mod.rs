//! SSH module — FP-2.1 to FP-2.4
//!
//! SSH protocol layer: client, auth, exec, channel_opener.

pub mod auth;
pub mod channel_opener;
pub mod client;
pub mod exec;
pub mod forwarded_dispatch;
pub mod protector;
pub mod pty;
pub mod tmux;

pub use auth::{generate_keypair, generate_keypair_at, push_public_key, AuthMethod};
pub use channel_opener::{ChannelOpener, SshChannelOpener};
pub use client::{ConnectionState, SshClientConfig, SshClientHandle};
pub use exec::{detect_client_ip, exec, ExecResult};
pub use forwarded_dispatch::{ForwardKey, ForwardedDispatch};
pub use protector::{NoOpSocketProtector, SocketProtector};
pub use pty::{open_pty_shell, resize_pty};
pub use tmux::{
    detect_tmux, generate_unique_session_name, list_termfast_sessions,
    session_exists, TmuxChoice, TmuxMode, TmuxSession,
    build_attach_command, build_new_session_exec_command,
    build_update_size_command, build_kill_session_command,
};
