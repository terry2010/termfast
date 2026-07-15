//! Server management module — Phase 5
pub mod instance;
pub mod lifecycle;
pub mod manager;

pub use instance::ServerInstance;
pub use lifecycle::ConnectionStateMachine;
pub use manager::ServerManager;
