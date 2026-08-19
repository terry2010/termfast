// src-tauri/src/tunnel_manager.rs — Desktop tunnel manager
//
// Manages TunnelClient instances for paired phones. When a phone completes
// pairing, the frontend calls ipc_tunnel_start to begin a WebSocket tunnel
// to the relay. The tunnel bridges encrypted frame I/O between the phone
// (via relay) and the desktop's RemoteServer (which has access to
// TerminalManager for local terminal sharing).
//
// One tunnel per paired phone. Tunnels auto-reconnect on disconnect.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use termfast_daemon::remote_server::RemoteServer;
use termfast_daemon::tunnel_client::{TunnelClient, TunnelConfig};
use termfast_daemon::TerminalManager;

/// Manages tunnel clients for all paired phones.
pub struct DesktopTunnelManager {
    /// Shared RemoteServer instance (has access to TerminalManager)
    remote_server: Arc<RemoteServer>,
    /// Active tunnel tasks: pairing_id → JoinHandle
    tunnels: Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
}

impl DesktopTunnelManager {
    /// Create a new tunnel manager with the given TerminalManager and ConfigManager.
    pub fn new(
        terminal_manager: Arc<TerminalManager>,
        config_manager: Arc<tokio::sync::Mutex<termfast_core::config::ConfigManager>>,
    ) -> Self {
        Self {
            remote_server: Arc::new(RemoteServer::new(terminal_manager, config_manager)),
            tunnels: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Start a tunnel for a paired phone.
    ///
    /// # Arguments
    /// * `pairing_id` - Pairing ID for this phone
    /// * `pairing_key` - 32-byte ECDH-derived pairing key K
    /// * `relay_url` - Relay WebSocket URL (e.g. "wss://termfast.xisj.com/tunnel")
    /// * `jwt` - Desktop user JWT (for relay authentication)
    ///
    /// If a tunnel for this pairing_id already exists, it is stopped first.
    pub async fn start_tunnel(
        &self,
        pairing_id: String,
        pairing_key: [u8; 32],
        relay_url: String,
        jwt: String,
    ) -> Result<(), String> {
        // Stop existing tunnel for THIS pairing_id only (e.g. reconnect).
        // Do NOT stop_all — multiple phones can be paired simultaneously.
        self.stop_tunnel(&pairing_id).await;

        // Register pairing key in RemoteServer (so handle_tunnel can look it up)
        self.remote_server.add_pairing(pairing_id.clone(), pairing_key);

        let config = TunnelConfig {
            relay_url,
            jwt,
            pairing_id: pairing_id.clone(),
            pairing_key,
        };

        let remote_server = self.remote_server.clone();
        let pairing_id_clone = pairing_id.clone();
        let tunnels = self.tunnels.clone();

        let handle = tokio::spawn(async move {
            let client = TunnelClient::new(config, remote_server);
            tracing::info!("Starting tunnel client for pairing {}", pairing_id_clone);
            client.run().await;
            // Remove from map when tunnel exits
            tunnels.lock().await.remove(&pairing_id_clone);
            tracing::info!("Tunnel client exited for pairing {}", pairing_id_clone);
        });

        self.tunnels.lock().await.insert(pairing_id, handle);
        Ok(())
    }

    /// Stop the tunnel for a specific pairing_id.
    pub async fn stop_tunnel(&self, pairing_id: &str) {
        if let Some(handle) = self.tunnels.lock().await.remove(pairing_id) {
            handle.abort();
            tracing::info!("Stopped tunnel for pairing {}", pairing_id);
        }
    }

    /// Get the shared RemoteServer instance (for revoke_pairing etc.)
    pub fn remote_server(&self) -> &Arc<RemoteServer> {
        &self.remote_server
    }

    /// Check if a tunnel task is active for the given pairing_id.
    /// Returns true if the tunnel task exists (connected or reconnecting to relay).
    /// Used by ipc_list_desktop_pairings to report online status for server-role
    /// desktop pairings (this desktop is the server, peer is the client).
    pub async fn is_tunnel_active(&self, pairing_id: &str) -> bool {
        self.tunnels.lock().await.contains_key(pairing_id)
    }

    /// Stop all tunnels (on app shutdown).
    pub async fn stop_all(&self) {
        let mut tunnels = self.tunnels.lock().await;
        for (pairing_id, handle) in tunnels.drain() {
            handle.abort();
            tracing::info!("Stopped tunnel for pairing {} (shutdown)", pairing_id);
        }
    }
}

// === SECTION 1 END ===
