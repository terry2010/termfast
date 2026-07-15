//! Configuration module — FP-1.2 / FP-1.3 / FP-1.3b / FP-1.4

pub mod builtin_templates;
#[allow(clippy::module_inception)]
pub mod config;
pub mod manager;
pub mod migration;
pub mod runtime_state;
pub mod storage;

pub use config::*;
pub use manager::ConfigManager;
pub use runtime_state::{RuntimeState, RuntimeStateManager};
pub use storage::{ConfigStorage, FileConfigStorage, InMemoryConfigStorage};
