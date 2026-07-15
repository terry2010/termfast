//! SSH module — FP-2.1 to FP-2.4
//!
//! SSH protocol layer: client, auth, exec, channel_opener.

pub mod auth;
pub mod channel_opener;
pub mod client;
pub mod exec;
pub mod protector;
pub mod pty;

pub use auth::{generate_keypair, generate_keypair_at, push_public_key, AuthMethod};
pub use channel_opener::{ChannelOpener, SshChannelOpener};
pub use client::{ConnectionState, SshClientConfig, SshClientHandle};
pub use exec::{detect_client_ip, exec, ExecResult};
pub use protector::{NoOpSocketProtector, SocketProtector};
pub use pty::{open_pty_shell, resize_pty};
