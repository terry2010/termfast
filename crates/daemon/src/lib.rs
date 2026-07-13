//! VPS Guard Daemon — socket server + core runtime holder
//!
//! Can be embedded in Tauri (GUI mode) or run standalone (--daemon mode).

pub mod proto;
pub mod lock;
pub mod frame;
pub mod server;
pub mod handler;

pub use proto::{Action, EventType, Request, Response, IpcError};
pub use lock::{DaemonLock, find_daemon_socket};
pub use server::{DaemonServer, DaemonState, ClientHandle};
pub use handler::handle_request;
