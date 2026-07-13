//! Server management module — Phase 5
pub mod instance;
pub mod manager;
pub mod lifecycle;

pub use instance::ServerInstance;
pub use manager::ServerManager;
pub use lifecycle::ConnectionStateMachine;
