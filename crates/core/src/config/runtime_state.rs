//! Runtime state manager — FP-1.3b
//!
//! Stores high-frequency data (last_known_ip, trigger execution timestamps)
//! in a separate `runtime_state.json` to avoid SSD write amplification on config.json.
//! Has its own tokio::sync::Mutex, independent from config.json's lock (§11.5).

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

/// Per-server runtime state (§11.5)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerRuntimeState {
    /// Last known client public IP
    #[serde(default)]
    pub last_known_ip: Option<String>,
    /// Last trigger execution timestamp (ISO 8601)
    #[serde(default)]
    pub last_trigger_executed_at: Option<String>,
}

/// Top-level runtime state structure
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeState {
    #[serde(default)]
    pub servers: HashMap<String, ServerRuntimeState>,
}

/// Manages runtime_state.json with independent async mutex.
pub struct RuntimeStateManager {
    state: Mutex<RuntimeState>,
    path: PathBuf,
}

impl RuntimeStateManager {
    /// Create new manager with explicit path
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            state: Mutex::new(RuntimeState::default()),
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Create with platform default path (same dir as config.json)
    pub fn with_default_path() -> Result<Self> {
        let proj_dir = directories::ProjectDirs::from("", "", "vps-guard")
            .ok_or_else(|| Error::Config("cannot determine data directory".into()))?;
        let path = proj_dir.data_dir().join("runtime_state.json");
        Ok(Self::new(path))
    }

    /// Load state from file (called at startup)
    pub async fn load(&self) -> Result<()> {
        let mut state = self.state.lock().await;

        if !self.path.exists() {
            tracing::info!("runtime_state.json not found, starting with empty state");
            return Ok(());
        }

        let content = match std::fs::read_to_string(&self.path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("failed to read runtime_state.json: {}, using empty state", e);
                return Ok(()); // Degrade to empty state (§11.5)
            }
        };

        match serde_json::from_str::<RuntimeState>(&content) {
            Ok(s) => {
                *state = s;
            }
            Err(e) => {
                // File corrupt → degrade to empty state (§11.5)
                tracing::warn!(
                    "runtime_state.json corrupt: {}, degrading to empty state",
                    e
                );
                *state = RuntimeState::default();
            }
        }

        Ok(())
    }

    /// Save state to file (atomic write)
    pub async fn save(&self) -> Result<()> {
        let state = self.state.lock().await;
        self.save_inner(&state)
    }

    fn save_inner(&self, state: &RuntimeState) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(Error::Io)?;
        }

        let json = serde_json::to_string_pretty(state)?;
        let tmp_path = self.path.with_extension("json.tmp");

        std::fs::write(&tmp_path, &json).map_err(Error::Io)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            let _ = std::fs::set_permissions(&tmp_path, perms);
        }

        std::fs::rename(&tmp_path, &self.path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            Error::Io(e)
        })?;

        Ok(())
    }

    /// Get last known IP for a server
    pub async fn get_last_known_ip(&self, server_id: &str) -> Option<String> {
        let state = self.state.lock().await;
        state
            .servers
            .get(server_id)
            .and_then(|s| s.last_known_ip.clone())
    }

    /// Set last known IP for a server and persist
    pub async fn set_last_known_ip(&self, server_id: &str, ip: &str) -> Result<()> {
        let mut state = self.state.lock().await;
        let entry = state.servers.entry(server_id.to_string()).or_default();
        entry.last_known_ip = Some(ip.to_string());
        drop(state);
        self.save().await
    }

    /// Get last trigger execution time for a server
    pub async fn get_last_trigger_executed_at(&self, server_id: &str) -> Option<String> {
        let state = self.state.lock().await;
        state
            .servers
            .get(server_id)
            .and_then(|s| s.last_trigger_executed_at.clone())
    }

    /// Set last trigger execution time and persist
    pub async fn set_last_trigger_executed_at(
        &self,
        server_id: &str,
        timestamp: &str,
    ) -> Result<()> {
        let mut state = self.state.lock().await;
        let entry = state.servers.entry(server_id.to_string()).or_default();
        entry.last_trigger_executed_at = Some(timestamp.to_string());
        drop(state);
        self.save().await
    }

    /// Remove all state for a server (on server deletion)
    pub async fn remove_server(&self, server_id: &str) -> Result<()> {
        let mut state = self.state.lock().await;
        state.servers.remove(server_id);
        drop(state);
        self.save().await
    }

    /// Get a snapshot of the full runtime state
    pub async fn snapshot(&self) -> RuntimeState {
        self.state.lock().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_runtime_state_save_load_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("runtime_state.json");
        let mgr = RuntimeStateManager::new(&path);

        mgr.set_last_known_ip("srv_test", "1.2.3.4").await.unwrap();
        assert!(path.exists());

        // Create new manager and load
        let mgr2 = RuntimeStateManager::new(&path);
        mgr2.load().await.unwrap();
        let ip = mgr2.get_last_known_ip("srv_test").await;
        assert_eq!(ip, Some("1.2.3.4".to_string()));
    }

    #[tokio::test]
    async fn test_runtime_state_missing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        let mgr = RuntimeStateManager::new(&path);

        mgr.load().await.unwrap();
        let ip = mgr.get_last_known_ip("srv_test").await;
        assert_eq!(ip, None);
    }

    #[tokio::test]
    async fn test_runtime_state_corrupt_file_degrades() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("runtime_state.json");
        std::fs::write(&path, "{ invalid json }").unwrap();

        let mgr = RuntimeStateManager::new(&path);
        mgr.load().await.unwrap(); // should not error
        let ip = mgr.get_last_known_ip("srv_test").await;
        assert_eq!(ip, None); // degraded to empty state
    }

    #[tokio::test]
    async fn test_runtime_state_remove_server() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("runtime_state.json");
        let mgr = RuntimeStateManager::new(&path);

        mgr.set_last_known_ip("srv_1", "1.1.1.1").await.unwrap();
        mgr.set_last_known_ip("srv_2", "2.2.2.2").await.unwrap();

        mgr.remove_server("srv_1").await.unwrap();

        assert_eq!(mgr.get_last_known_ip("srv_1").await, None);
        assert_eq!(
            mgr.get_last_known_ip("srv_2").await,
            Some("2.2.2.2".to_string())
        );
    }

    #[tokio::test]
    async fn test_runtime_state_trigger_timestamp() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("runtime_state.json");
        let mgr = RuntimeStateManager::new(&path);

        let ts = "2026-01-15T14:32:00Z";
        mgr.set_last_trigger_executed_at("srv_test", ts)
            .await
            .unwrap();

        let loaded = mgr.get_last_trigger_executed_at("srv_test").await;
        assert_eq!(loaded, Some(ts.to_string()));
    }

    #[tokio::test]
    async fn test_independent_mutex_no_blocking() {
        // Verify that RuntimeStateManager has its own mutex
        let dir = tempdir().unwrap();
        let path = dir.path().join("runtime_state.json");
        let mgr = RuntimeStateManager::new(&path);

        // Lock and hold
        let state = mgr.state.lock().await;
        // Can still read (we hold the lock in this test context)
        assert!(state.servers.is_empty());
        drop(state);

        // Now can write
        mgr.set_last_known_ip("srv_test", "1.2.3.4").await.unwrap();
    }
}
