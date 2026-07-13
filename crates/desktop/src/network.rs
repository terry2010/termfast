//! Offline/network detection — FP-6.9
//!
//! Detects system offline/online state to pause/resume reconnection.

use std::sync::Arc;
use tokio::sync::Mutex;

/// Network state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkState {
    Online,
    Offline,
}

/// Network state monitor
pub struct NetworkMonitor {
    state: Arc<Mutex<NetworkState>>,
    /// Server IDs that were connected before going offline
    was_connected: Arc<Mutex<Vec<String>>>,
}

impl NetworkMonitor {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(NetworkState::Online)),
            was_connected: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Get current network state
    pub async fn state(&self) -> NetworkState {
        self.state.lock().await.clone()
    }

    /// Check if currently offline
    pub async fn is_offline(&self) -> bool {
        self.state().await == NetworkState::Offline
    }

    /// Mark as offline — record which servers were connected
    /// If connected_servers is non-empty, it replaces the recorded list.
    /// If empty, the existing recorded list is preserved.
    pub async fn go_offline(&self, connected_servers: Vec<String>) {
        *self.state.lock().await = NetworkState::Offline;
        if !connected_servers.is_empty() {
            *self.was_connected.lock().await = connected_servers;
        }
    }

    /// Mark as online — return servers that should reconnect
    pub async fn go_online(&self) -> Vec<String> {
        *self.state.lock().await = NetworkState::Online;
        self.was_connected.lock().await.clone()
    }

    /// Record a server as was-connected (for partial state tracking)
    pub async fn record_connected(&self, server_id: &str) {
        let mut was = self.was_connected.lock().await;
        if !was.contains(&server_id.to_string()) {
            was.push(server_id.to_string());
        }
    }

    /// Remove a server from was-connected list
    pub async fn remove_connected(&self, server_id: &str) {
        let mut was = self.was_connected.lock().await;
        was.retain(|s| s != server_id);
    }

    /// Start a background polling task that checks network connectivity every 5s.
    /// When state changes, calls the provided callback with (new_state, servers_to_reconnect).
    pub fn start_monitoring<F>(
        self: &Arc<Self>,
        check_interval_secs: u64,
        on_state_change: F,
    ) -> tokio::task::JoinHandle<()>
    where
        F: Fn(NetworkState, Vec<String>) + Send + Sync + 'static,
    {
        let monitor = self.clone();
        let callback = Arc::new(on_state_change);
        tokio::spawn(async move {
            loop {
                // Check connectivity by trying to connect to a public DNS server
                let is_online = check_connectivity().await;

                let current = monitor.state().await;
                let new_state = if is_online {
                    NetworkState::Online
                } else {
                    NetworkState::Offline
                };

                if current != new_state {
                    tracing::info!("network state changed: {:?} -> {:?}", current, new_state);
                    let servers = if new_state == NetworkState::Offline {
                        // Going offline: record nothing, just notify
                        monitor.go_offline(vec![]).await;
                        vec![]
                    } else {
                        // Going online: return servers that should reconnect
                        monitor.go_online().await
                    };
                    callback(new_state, servers);
                }

                tokio::time::sleep(std::time::Duration::from_secs(check_interval_secs)).await;
            }
        })
    }
}

/// Check network connectivity by attempting a TCP connection to 1.1.1.1:53
async fn check_connectivity() -> bool {
    use tokio::net::TcpStream;
    use tokio::time::Duration;
    tokio::time::timeout(
        Duration::from_secs(3),
        TcpStream::connect("1.1.1.1:53"),
    )
    .await
    .is_ok_and(|r| r.is_ok())
}

impl Default for NetworkMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_initial_state_online() {
        let monitor = NetworkMonitor::new();
        assert_eq!(monitor.state().await, NetworkState::Online);
        assert!(!monitor.is_offline().await);
    }

    #[tokio::test]
    async fn test_go_offline_online() {
        let monitor = NetworkMonitor::new();
        monitor.go_offline(vec!["srv_1".into(), "srv_2".into()]).await;
        assert!(monitor.is_offline().await);

        let reconnect = monitor.go_online().await;
        assert_eq!(reconnect, vec!["srv_1", "srv_2"]);
        assert!(!monitor.is_offline().await);
    }

    #[tokio::test]
    async fn test_record_remove_connected() {
        let monitor = NetworkMonitor::new();
        monitor.record_connected("srv_1").await;
        monitor.record_connected("srv_2").await;
        monitor.record_connected("srv_1").await; // duplicate

        monitor.go_offline(vec![]).await;
        let reconnect = monitor.go_online().await;
        assert_eq!(reconnect.len(), 2);

        monitor.remove_connected("srv_1").await;
        monitor.go_offline(vec![]).await;
        let reconnect = monitor.go_online().await;
        assert_eq!(reconnect.len(), 1);
        assert_eq!(reconnect[0], "srv_2");
    }
}
