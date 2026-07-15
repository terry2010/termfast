//! TermFast Core — platform-agnostic core library.
//!
//! Contains all business logic: config, SSH, proxy, trigger engine.
//! Does NOT depend on tauri or daemon — keeps mobile cross-compilation possible.

pub mod config;
pub mod error;
pub mod log;
pub mod migration;
pub mod platform;
pub mod proxy;
pub mod server;
pub mod ssh;
pub mod trigger;

pub use error::{Error, ErrorCode, IpcError, Result};
