//! Android ConfigStorage implementation.
//!
//! On Android, config and runtime state are persisted under the app private
//! directory passed in from Kotlin (`context.getFilesDir()`).

use std::path::PathBuf;
use std::sync::Arc;
use termfast_core::config::{Config, ConfigManager, FileConfigStorage, RuntimeStateManager};

pub fn config_manager_for_dir(dir: PathBuf) -> anyhow::Result<ConfigManager> {
    let config_path = dir.join("config.json");
    let storage = Arc::new(FileConfigStorage::new(config_path));
    let config = Config::default();
    Ok(ConfigManager::with_storage(config, storage))
}

pub fn runtime_state_manager_for_dir(dir: PathBuf) -> RuntimeStateManager {
    RuntimeStateManager::new(dir.join("runtime_state.json"))
}
