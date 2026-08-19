//! Runtime state manager — FP-1.3b
//!
//! Stores high-frequency data (last_known_ip, trigger execution timestamps)
//! in the SQLCipher DB's `runtime_state` table.
//!
//! No in-memory cache — all reads/writes go directly to the DB via
//! `spawn_blocking`. This avoids cache consistency issues when multiple
//! managers share the same DB. Single-row UPSERT/SELECT is < 1ms.

use crate::config::sqlite_storage::SqlCipherStorage;
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

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

/// Manages runtime state in SQLCipher (thin wrapper, no cache).
pub struct RuntimeStateManager {
    storage: Arc<SqlCipherStorage>,
}

impl RuntimeStateManager {
    /// Create new manager backed by SqlCipherStorage.
    pub fn new(storage: Arc<SqlCipherStorage>) -> Self {
        Self { storage }
    }

    /// Get a reference to the underlying SqlCipherStorage.
    /// Used by cloud sync and other subsystems that need DB access.
    pub fn storage(&self) -> &Arc<SqlCipherStorage> {
        &self.storage
    }

    /// Get last known IP for a server
    pub async fn get_last_known_ip(&self, server_id: &str) -> Option<String> {
        let storage = self.storage.clone();
        let server_id = server_id.to_string();
        tokio::task::spawn_blocking(move || {
            let data = storage.get_runtime_state(&server_id).ok().flatten()?;
            let state: ServerRuntimeState = serde_json::from_str(&data).ok()?;
            state.last_known_ip
        })
        .await
        .ok()
        .flatten()
    }

    /// Set last known IP for a server and persist
    pub async fn set_last_known_ip(&self, server_id: &str, ip: &str) -> Result<()> {
        let storage = self.storage.clone();
        let server_id = server_id.to_string();
        let ip = ip.to_string();
        tokio::task::spawn_blocking(move || {
            // Read existing state or create new
            let state = storage
                .get_runtime_state(&server_id)
                .ok()
                .flatten()
                .and_then(|d| serde_json::from_str::<ServerRuntimeState>(&d).ok())
                .unwrap_or_default();
            let new_state = ServerRuntimeState {
                last_known_ip: Some(ip),
                ..state
            };
            let json = serde_json::to_string(&new_state)?;
            storage.upsert_runtime_state(&server_id, &json)
        })
        .await
        .map_err(|e| crate::error::Error::Other(format!("spawn_blocking: {}", e)))?
    }

    /// Get last trigger execution time for a server
    pub async fn get_last_trigger_executed_at(&self, server_id: &str) -> Option<String> {
        let storage = self.storage.clone();
        let server_id = server_id.to_string();
        tokio::task::spawn_blocking(move || {
            let data = storage.get_runtime_state(&server_id).ok().flatten()?;
            let state: ServerRuntimeState = serde_json::from_str(&data).ok()?;
            state.last_trigger_executed_at
        })
        .await
        .ok()
        .flatten()
    }

    /// Set last trigger execution time and persist
    pub async fn set_last_trigger_executed_at(
        &self,
        server_id: &str,
        timestamp: &str,
    ) -> Result<()> {
        let storage = self.storage.clone();
        let server_id = server_id.to_string();
        let timestamp = timestamp.to_string();
        tokio::task::spawn_blocking(move || {
            let state = storage
                .get_runtime_state(&server_id)
                .ok()
                .flatten()
                .and_then(|d| serde_json::from_str::<ServerRuntimeState>(&d).ok())
                .unwrap_or_default();
            let new_state = ServerRuntimeState {
                last_trigger_executed_at: Some(timestamp),
                ..state
            };
            let json = serde_json::to_string(&new_state)?;
            storage.upsert_runtime_state(&server_id, &json)
        })
        .await
        .map_err(|e| crate::error::Error::Other(format!("spawn_blocking: {}", e)))?
    }

    /// Remove all state for a server (on server deletion)
    pub async fn remove_server(&self, server_id: &str) -> Result<()> {
        let storage = self.storage.clone();
        let server_id = server_id.to_string();
        tokio::task::spawn_blocking(move || storage.delete_runtime_state(&server_id))
            .await
            .map_err(|e| crate::error::Error::Other(format!("spawn_blocking: {}", e)))?
    }

    /// Get a snapshot of the full runtime state
    pub async fn snapshot(&self) -> RuntimeState {
        let storage = self.storage.clone();
        match tokio::task::spawn_blocking(move || -> Result<RuntimeState> {
            let entries = storage.list_runtime_state()?;
            let mut servers = HashMap::new();
            for (server_id, data) in entries {
                if let Ok(state) = serde_json::from_str::<ServerRuntimeState>(&data) {
                    servers.insert(server_id, state);
                }
            }
            Ok(RuntimeState { servers })
        })
        .await
        {
            Ok(Ok(state)) => state,
            Ok(Err(e)) => {
                tracing::warn!("snapshot: failed to list runtime state: {}", e);
                RuntimeState::default()
            }
            Err(e) => {
                tracing::warn!("snapshot: spawn_blocking failed: {}", e);
                RuntimeState::default()
            }
        }
    }
}
