//! Server manager — FP-5.2
//!
//! Multi-server container. Manages multiple ServerInstance objects.

use crate::config::ServerConfig;
use crate::error::{Error, ErrorCode, IpcError, Result};
use crate::server::instance::{ServerInstance, ServerStatus};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Manages multiple server instances
pub struct ServerManager {
    servers: Mutex<HashMap<String, Arc<ServerInstance>>>,
    /// Concurrent SSH connection counter (§1.2: max 3)
    connect_count: Mutex<usize>,
}

/// Maximum concurrent SSH connections (§1.2)
const MAX_CONCURRENT_CONNECTIONS: usize = 3;

impl ServerManager {
    pub fn new() -> Self {
        Self {
            servers: Mutex::new(HashMap::new()),
            connect_count: Mutex::new(0),
        }
    }

    /// Add a server
    pub async fn add_server(&self, config: ServerConfig) -> Result<String> {
        let mut servers = self.servers.lock().await;
        if servers.contains_key(&config.id) {
            return Err(Error::Ipc(IpcError::new(
                ErrorCode::Internal,
                format!("server {} already exists", config.id),
            )));
        }
        let id = config.id.clone();
        let instance = Arc::new(ServerInstance::new(config));
        servers.insert(id.clone(), instance);
        Ok(id)
    }

    /// Remove a server
    pub async fn remove_server(&self, server_id: &str) -> Result<()> {
        let mut servers = self.servers.lock().await;
        if let Some(instance) = servers.remove(server_id) {
            // Disconnect if connected
            let _ = instance.disconnect().await;
        }
        Ok(())
    }

    /// Get a server instance
    pub async fn get_server(&self, server_id: &str) -> Result<Arc<ServerInstance>> {
        let servers = self.servers.lock().await;
        servers.get(server_id).cloned().ok_or_else(|| {
            Error::Ipc(IpcError::new(
                ErrorCode::ServerNotFound,
                format!("server {} not found", server_id),
            ))
        })
    }

    /// List all server IDs
    pub async fn list_server_ids(&self) -> Vec<String> {
        let servers = self.servers.lock().await;
        servers.keys().cloned().collect()
    }

    /// Get all server instances
    pub async fn list_servers(&self) -> Vec<Arc<ServerInstance>> {
        let servers = self.servers.lock().await;
        servers.values().cloned().collect()
    }

    /// Reload a server's config from the config manager (updates in-place)
    pub async fn reload_server_config(&self, server_id: &str, new_config: ServerConfig) -> Result<()> {
        let mut servers = self.servers.lock().await;
        if let Some(old) = servers.get(server_id) {
            // Disconnect if currently connected
            let status = old.status.lock().await.clone();
            if status == ServerStatus::Connected || status == ServerStatus::Connecting {
                let _ = old.disconnect().await;
            }
            // Stop proxy if running
            let _ = old.stop_proxy().await;
        }
        // Replace with new instance
        let instance = Arc::new(ServerInstance::new(new_config));
        servers.insert(server_id.to_string(), instance);
        Ok(())
    }

    /// Try to acquire a connection slot (§1.2: max 3 concurrent SSH connections)
    /// Returns Ok(()) if a slot was acquired, Err if the limit is reached.
    pub async fn try_acquire_connection(&self) -> Result<()> {
        let mut count = self.connect_count.lock().await;
        if *count >= MAX_CONCURRENT_CONNECTIONS {
            return Err(Error::Ipc(IpcError::new(
                ErrorCode::Internal,
                format!(
                    "max {} concurrent SSH connections reached",
                    MAX_CONCURRENT_CONNECTIONS
                ),
            )));
        }
        *count += 1;
        Ok(())
    }

    /// Release a connection slot (call on disconnect)
    pub async fn release_connection(&self) {
        let mut count = self.connect_count.lock().await;
        if *count > 0 {
            *count -= 1;
        }
    }

    /// Get current concurrent connection count
    pub async fn concurrent_connection_count(&self) -> usize {
        *self.connect_count.lock().await
    }

    /// Check if a SOCKS5 port is in use by any server
    pub async fn is_socks5_port_in_use(&self, port: u16, exclude: Option<&str>) -> bool {
        let servers = self.servers.lock().await;
        servers
            .iter()
            .filter(|(id, _)| Some(id.as_str()) != exclude)
            .any(|(_, s)| s.config.proxy.socks5_port == port)
    }

    /// Check if an HTTP port is in use by any server
    pub async fn is_http_port_in_use(&self, port: u16, exclude: Option<&str>) -> bool {
        let servers = self.servers.lock().await;
        servers
            .iter()
            .filter(|(id, _)| Some(id.as_str()) != exclude)
            .any(|(_, s)| s.config.proxy.http_port == port)
    }
}

impl Default for ServerManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;

    fn test_config(id: &str, socks5_port: u16) -> ServerConfig {
        ServerConfig {
            id: id.into(),
            name: format!("Server {}", id),
            ssh: SshConfig {
                host: "1.2.3.4".into(),
                port: 22,
                user: "root".into(),
                auth_method: "key".into(),
                key_path: "".into(),
                key_auto_generated: false,
                connection_mode: "single".into(),
                skip_hostkey_verify: false,
            },
            proxy: ProxyConfig {
                enabled: true,
                socks5_port,
                http_port: socks5_port + 1000,
                mixed_port: 0,
                max_channels: 64,
                channel_idle_timeout: 300,
            },
            reconnect: ReconnectConfig::default(),
            ip_check: IpCheckConfig::default(),
            last_known_ip: None,
            triggers: Vec::new(),
            suppress_firewall_badge: false,
        }
    }

    #[tokio::test]
    async fn test_add_and_get_server() {
        let mgr = ServerManager::new();
        let id = mgr.add_server(test_config("srv_1", 1080)).await.unwrap();
        assert_eq!(id, "srv_1");
        let server = mgr.get_server("srv_1").await.unwrap();
        assert_eq!(server.id(), "srv_1");
    }

    #[tokio::test]
    async fn test_remove_server() {
        let mgr = ServerManager::new();
        mgr.add_server(test_config("srv_1", 1080)).await.unwrap();
        mgr.remove_server("srv_1").await.unwrap();
        assert!(mgr.get_server("srv_1").await.is_err());
    }

    #[tokio::test]
    async fn test_list_servers() {
        let mgr = ServerManager::new();
        mgr.add_server(test_config("srv_1", 1080)).await.unwrap();
        mgr.add_server(test_config("srv_2", 1081)).await.unwrap();
        let ids = mgr.list_server_ids().await;
        assert_eq!(ids.len(), 2);
    }

    #[tokio::test]
    async fn test_port_in_use() {
        let mgr = ServerManager::new();
        mgr.add_server(test_config("srv_1", 1080)).await.unwrap();
        assert!(mgr.is_socks5_port_in_use(1080, None).await);
        assert!(!mgr.is_socks5_port_in_use(1080, Some("srv_1")).await);
        assert!(!mgr.is_socks5_port_in_use(1081, None).await);
    }

    #[tokio::test]
    async fn test_get_nonexistent_server() {
        let mgr = ServerManager::new();
        let result = mgr.get_server("nonexistent").await;
        assert!(result.is_err());
    }
}
