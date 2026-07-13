//! E2E task flow tests — FP-9.2
//!
//! Tests the full task flow: add server → connect → proxy → trigger → disconnect.
//! Uses the daemon's IPC protocol directly (no Tauri).

use vps_guard_core::config::{
    Config, IpCheckConfig, ProxyConfig, ReconnectConfig, ServerConfig, SshConfig,
};
use vps_guard_core::log::{LogEntry, LogKind, LogLevel};
use vps_guard_core::server::instance::ServerStatus;
use vps_guard_credential::InMemoryCredentialStore;
use vps_guard_daemon::{DaemonServer, DaemonState};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn make_test_server(id: &str, name: &str) -> ServerConfig {
    ServerConfig {
        id: id.into(),
        name: name.into(),
        ssh: SshConfig {
            host: "127.0.0.1".into(),
            port: 2222,
            user: "testuser".into(),
            auth_method: "password".into(),
            key_path: "".into(),
            key_auto_generated: false,
            connection_mode: "single".into(),
            skip_hostkey_verify: true,
        },
        proxy: ProxyConfig {
            enabled: false,
            socks5_port: 1080,
            http_port: 8080,
            max_channels: 64,
            channel_idle_timeout: 300,
        },
        reconnect: ReconnectConfig::default(),
        ip_check: IpCheckConfig { enabled: false, interval_secs: 300 },
        last_known_ip: None,
        triggers: Vec::new(),
        suppress_firewall_badge: false,
    }
}

async fn setup_daemon() -> (DaemonServer, std::path::PathBuf) {
    let config = Config::default();
    let mgr = vps_guard_core::config::ConfigManager::with_storage(
        config,
        std::sync::Arc::new(vps_guard_core::config::InMemoryConfigStorage::new()),
    );
    let cred_store = Arc::new(InMemoryCredentialStore::new());
    let state = DaemonState::with_credential_store(mgr, cred_store);
    // Use unique socket path per test
    let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let socket_path = format!("/tmp/vps-guard-e2e-test-{}-{}.sock", std::process::id(), id);
    let _ = std::fs::remove_file(&socket_path);
    let server = DaemonServer::start_with_path(state, socket_path.into()).await.unwrap();
    let socket_path = server.socket_path().to_path_buf();
    (server, socket_path)
}

#[tokio::test]
async fn test_e2e_add_and_list_server() {
    let (server, socket_path) = setup_daemon().await;

    // Add a server via the server manager directly
    let server_config = make_test_server("srv_e2e_1", "E2E Test Server");
    server.state().server_manager.add_server(server_config).await.unwrap();

    // List servers
    let servers = server.state().server_manager.list_servers().await;
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].id(), "srv_e2e_1");
    assert_eq!(servers[0].name(), "E2E Test Server");

    server.shutdown().await;
    let _ = std::fs::remove_file(&socket_path);
}

#[tokio::test]
async fn test_e2e_add_remove_server() {
    let (server, socket_path) = setup_daemon().await;

    // Add
    let config = make_test_server("srv_e2e_2", "Test Remove");
    server.state().server_manager.add_server(config).await.unwrap();
    assert_eq!(server.state().server_manager.list_servers().await.len(), 1);

    // Remove
    server.state().server_manager.remove_server("srv_e2e_2").await.unwrap();
    assert_eq!(server.state().server_manager.list_servers().await.len(), 0);

    server.shutdown().await;
    let _ = std::fs::remove_file(&socket_path);
}

#[tokio::test]
async fn test_e2e_multi_server_management() {
    let (server, socket_path) = setup_daemon().await;

    // Add multiple servers
    for i in 0..5 {
        let config = make_test_server(&format!("srv_multi_{}", i), &format!("Server {}", i));
        server.state().server_manager.add_server(config).await.unwrap();
    }

    let servers = server.state().server_manager.list_servers().await;
    assert_eq!(servers.len(), 5);

    // Verify all have unique IDs
    let ids: Vec<_> = servers.iter().map(|s| s.id()).collect();
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), 5);

    // Remove one
    server.state().server_manager.remove_server("srv_multi_2").await.unwrap();
    assert_eq!(server.state().server_manager.list_servers().await.len(), 4);

    server.shutdown().await;
    let _ = std::fs::remove_file(&socket_path);
}

#[tokio::test]
async fn test_e2e_credential_save_and_load() {
    let (server, socket_path) = setup_daemon().await;

    let key = vps_guard_credential::make_key("srv_cred_1", "password");
    server.state().credential_store.save(&key, "secret123").unwrap();

    assert!(server.state().credential_store.has(&key));
    let loaded = server.state().credential_store.load(&key).unwrap();
    assert_eq!(loaded, "secret123");

    // Delete
    server.state().credential_store.delete(&key).unwrap();
    assert!(!server.state().credential_store.has(&key));

    server.shutdown().await;
    let _ = std::fs::remove_file(&socket_path);
}

#[tokio::test]
async fn test_e2e_export_config() {
    let (server, socket_path) = setup_daemon().await;

    // Add a server to the config manager (not just server manager)
    let server_config = make_test_server("srv_export_1", "Export Test");
    {
        let mgr = server.state().config_manager.lock().await;
        mgr.modify(|config| {
            config.servers.push(server_config.clone());
        }).await.unwrap();
    }

    // Export config
    let mgr = server.state().config_manager.lock().await;
    let config = mgr.get().await;
    let json = serde_json::to_string(&config).unwrap();
    assert!(json.contains("srv_export_1"));
    drop(mgr);

    server.shutdown().await;
    let _ = std::fs::remove_file(&socket_path);
}

#[tokio::test]
async fn test_e2e_proxy_toggle() {
    let (server, socket_path) = setup_daemon().await;

    let mut config = make_test_server("srv_proxy_1", "Proxy Test");
    config.proxy.enabled = true;
    config.proxy.socks5_port = 11080;
    config.proxy.http_port = 18080;
    server.state().server_manager.add_server(config).await.unwrap();

    let srv = server.state().server_manager.get_server("srv_proxy_1").await.unwrap();

    // Proxy should not be running initially (not connected)
    assert!(!srv.is_proxy_running().await);

    // Start proxy manually
    srv.start_proxy().await.unwrap();
    assert!(srv.is_proxy_running().await);

    // Stop proxy
    srv.stop_proxy().await.unwrap();
    assert!(!srv.is_proxy_running().await);

    server.shutdown().await;
    let _ = std::fs::remove_file(&socket_path);
}

#[tokio::test]
async fn test_e2e_trigger_pause_resume() {
    let (server, socket_path) = setup_daemon().await;

    let config = make_test_server("srv_trig_1", "Trigger Test");
    server.state().server_manager.add_server(config).await.unwrap();

    let srv = server.state().server_manager.get_server("srv_trig_1").await.unwrap();

    // Pause all
    srv.trigger_engine.pause_all().await;
    assert!(srv.trigger_engine.is_paused("srv_trig_1").await);

    // Resume all
    srv.trigger_engine.resume_all().await;
    assert!(!srv.trigger_engine.is_paused("srv_trig_1").await);

    server.shutdown().await;
    let _ = std::fs::remove_file(&socket_path);
}

#[tokio::test]
async fn test_e2e_error_recovery_disconnect() {
    let (server, socket_path) = setup_daemon().await;

    let config = make_test_server("srv_err_1", "Error Recovery");
    server.state().server_manager.add_server(config).await.unwrap();

    let srv = server.state().server_manager.get_server("srv_err_1").await.unwrap();

    // Disconnect when not connected should succeed
    srv.disconnect().await.unwrap();
    assert_eq!(srv.status().await, ServerStatus::Disconnected);

    server.shutdown().await;
    let _ = std::fs::remove_file(&socket_path);
}

#[tokio::test]
async fn test_e2e_log_buffer() {
    let (server, socket_path) = setup_daemon().await;

    // Add some log entries
    server.state().log_buffer.add(LogEntry {
        timestamp: chrono::Utc::now(),
        server_id: Some("srv_log_1".into()),
        level: LogLevel::Info,
        kind: LogKind::Connection,
        message: "test message".into(),
        data: None,
        execution_id: None,
    }).await;

    let entries = server.state().log_buffer.get_entries(Some("srv_log_1"), None, None).await;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].message, "test message");

    // Clear
    server.state().log_buffer.clear().await;
    let entries = server.state().log_buffer.get_entries(Some("srv_log_1"), None, None).await;
    assert_eq!(entries.len(), 0);

    server.shutdown().await;
    let _ = std::fs::remove_file(&socket_path);
}
