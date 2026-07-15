//! TermFast Daemon — socket server + core runtime holder
//!
//! Can be embedded in Tauri (GUI mode) or run standalone (--daemon mode).

pub mod frame;
pub mod handler;
pub mod lock;
pub mod proto;
pub mod server;
pub mod terminal;

pub use handler::handle_request;
pub use lock::{find_daemon_socket, DaemonLock};
pub use proto::{Action, EventType, IpcError, Request, Response};
pub use server::{ClientHandle, DaemonServer, DaemonState};
pub use terminal::TerminalManager;
