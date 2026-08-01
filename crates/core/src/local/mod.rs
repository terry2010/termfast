//! Local terminal support — PTY + shell detection.
//!
//! Provides `open_local_pty()` for spawning a local shell in a pseudo-terminal,
//! plus shell detection and PATH resolution helpers (macOS Dock PATH fix).

pub mod pty;
pub mod shell;

// Re-export portable-pty types so daemon doesn't need a direct portable-pty dependency.
pub use portable_pty::{ChildKiller, PtySize};
