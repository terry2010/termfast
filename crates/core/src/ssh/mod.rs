//! SSH module — FP-2.1 to FP-2.4
//!
//! SSH protocol layer: client, auth, exec, channel_opener.

pub mod client;
pub mod auth;
pub mod exec;
pub mod channel_opener;

pub use client::{SshClientHandle, ConnectionState, SshClientConfig};
pub use auth::{AuthMethod, generate_keypair, push_public_key};
pub use exec::{exec, detect_client_ip, ExecResult};
pub use channel_opener::{ChannelOpener, SshChannelOpener};
