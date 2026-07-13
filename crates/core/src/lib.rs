//! VPS Guard Core — platform-agnostic core library.
//!
//! Contains all business logic: config, SSH, proxy, trigger engine.
//! Does NOT depend on tauri or daemon — keeps mobile cross-compilation possible.

pub mod error;
pub mod config;
pub mod migration;
pub mod ssh;
pub mod proxy;
pub mod trigger;
pub mod server;
pub mod log;
pub mod platform;

pub use error::{Error, ErrorCode, IpcError, Result};
