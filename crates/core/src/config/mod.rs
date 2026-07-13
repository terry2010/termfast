//! Configuration module — FP-1.2 / FP-1.3 / FP-1.3b / FP-1.4

#[allow(clippy::module_inception)]
pub mod config;
pub mod storage;
pub mod runtime_state;
pub mod migration;
pub mod builtin_templates;
pub mod manager;

pub use config::*;
pub use storage::{ConfigStorage, FileConfigStorage, InMemoryConfigStorage};
pub use runtime_state::{RuntimeStateManager, RuntimeState};
pub use manager::ConfigManager;
