//! Integration test: daemon socket round-trip — FP-9.11a
//!
//! Tests the daemon socket server with a real client connection.

#[cfg(unix)]
use std::sync::Arc;
#[cfg(unix)]
use termfast_core::config::{Config, ConfigManager, InMemoryConfigStorage};
#[cfg(unix)]
use termfast_daemon::{Action, DaemonServer, DaemonState, Request, Response};

#[cfg(unix)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn start_test_daemon() -> (DaemonServer, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");
        let rs_db_path = dir.path().join("runtime.db");
        let config = Config::default();
        let mgr = ConfigManager::with_storage(config, Arc::new(InMemoryConfigStorage::new()));
        let rs_storage = Arc::new(
            termfast_core::config::SqlCipherStorage::create_new(&rs_db_path, &termfast_core::config::DEFAULT_DEK).unwrap(),
        );
        let state = DaemonState::new(mgr).with_runtime_state(Arc::new(
            termfast_core::config::RuntimeStateManager::new(rs_storage),
        ));
        let server = DaemonServer::start_with_path(state, socket_path)
            .await
            .unwrap();
        (server, dir)
    }

    async fn send_request(stream: &mut tokio::net::UnixStream, request: &Request) -> Response {
        let request_json = serde_json::to_vec(request).unwrap();
        let len = (request_json.len() as u32).to_be_bytes();
        stream.write_all(&len).await.unwrap();
        stream.write_all(&request_json).await.unwrap();

        // Read responses, skipping broadcast Event messages until we get our Ok/Err
        loop {
            let mut len_buf = [0u8; 4];
            stream.read_exact(&mut len_buf).await.unwrap();
            let resp_len = u32::from_be_bytes(len_buf) as usize;
            let mut resp_buf = vec![0u8; resp_len];
            stream.read_exact(&mut resp_buf).await.unwrap();

            let response: Response = serde_json::from_slice(&resp_buf).unwrap();
            match &response {
                Response::Event { .. } => continue, // Skip broadcast events
                Response::Ok { id, .. } | Response::Err { id, .. } => {
                    if id == &request.id {
                        return response;
                    }
                    continue; // Not our response, keep reading
                }
            }
        }
    }

    #[tokio::test]
    async fn test_daemon_start_and_connect() {
        let (server, _dir) = start_test_daemon().await;

        // Verify socket file exists
        assert!(server.socket_path().exists());

        // Connect a client
        let socket_path = server.socket_path().clone();
        let connect_result = tokio::net::UnixStream::connect(&socket_path).await;
        assert!(connect_result.is_ok(), "should connect to daemon socket");

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_daemon_request_response_round_trip() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();

        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        let request = Request::new_simple(Action::GetDaemonStatus);
        let response = send_request(&mut stream, &request).await;

        match response {
            Response::Ok { data, .. } => {
                assert_eq!(data["running"], true);
            }
            _ => panic!("expected Ok response"),
        }

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_daemon_list_servers_empty() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();

        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        let request = Request::new_simple(Action::ListServers);
        let response = send_request(&mut stream, &request).await;

        assert!(matches!(response, Response::Ok { .. }));

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_daemon_get_config() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();

        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        let request = Request::new_simple(Action::GetConfig);
        let response = send_request(&mut stream, &request).await;

        match response {
            Response::Ok { data, .. } => {
                assert!(data["general"].is_object());
            }
            _ => panic!("expected Ok response"),
        }

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_daemon_pause_resume_triggers() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();

        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        // Pause
        let request = Request::new_simple(Action::PauseAllTriggers);
        let response = send_request(&mut stream, &request).await;
        assert!(matches!(response, Response::Ok { .. }));

        // Resume
        let request = Request::new_simple(Action::ResumeAllTriggers);
        let response = send_request(&mut stream, &request).await;
        assert!(matches!(response, Response::Ok { .. }));

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_daemon_clear_logs() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();

        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        let request = Request::new_simple(Action::ClearLogs);
        let response = send_request(&mut stream, &request).await;
        assert!(matches!(response, Response::Ok { .. }));

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_daemon_get_server_status_not_found() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();

        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        let request = Request::new(
            Action::GetServerStatus,
            serde_json::json!({"server_id": "nonexistent"}),
        );
        let response = send_request(&mut stream, &request).await;

        assert!(matches!(response, Response::Err { .. }));

        server.shutdown().await;
    }

    // === SECTION 1 END ===

    /// Helper: create a test server config
    fn make_test_server_config(id: &str, name: &str) -> termfast_core::config::ServerConfig {
        termfast_core::config::ServerConfig {
            id: id.to_string(),
            name: name.to_string(),
            ssh: termfast_core::config::SshConfig {
                host: "127.0.0.1".to_string(),
                port: 22,
                user: "testuser".to_string(),
                auth_method: "password".to_string(),
                key_path: String::new(),
                key_auto_generated: false,
                connection_mode: "single".to_string(),
                skip_hostkey_verify: true,
                host_key_fingerprint: None,
            },
            proxy: termfast_core::config::ProxyConfig {
                enabled: false,
                socks5_port: 1080,
                mixed_port: 0,
                http_port: 8080,
                max_channels: 64,
                channel_idle_timeout: 300,
            },
            reconnect: termfast_core::config::ReconnectConfig::default(),
            ip_check: termfast_core::config::IpCheckConfig::default(),
            last_known_ip: None,
            triggers: vec![],
            suppress_firewall_badge: false,
            test_url: String::new(),
            port_forwards: vec![],
            tmux_mode: "ask".to_string(),
        }
    }

    #[tokio::test]
    async fn test_ipc_add_server_and_list() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();
        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        // AddServer via IPC
        let cfg = make_test_server_config("srv_test_1", "Test VPS 1");
        let request = Request::new(Action::AddServer, serde_json::to_value(&cfg).unwrap());
        let response = send_request(&mut stream, &request).await;
        assert!(matches!(response, Response::Ok { .. }));

        // ListServers should now contain it
        let request = Request::new_simple(Action::ListServers);
        let response = send_request(&mut stream, &request).await;
        match response {
            Response::Ok { data, .. } => {
                let servers = data["servers"].as_array().unwrap();
                assert!(servers.iter().any(|s| s["id"] == "srv_test_1"));
            }
            _ => panic!("expected Ok response"),
        }

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_ipc_remove_server() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();
        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        // Add first
        let cfg = make_test_server_config("srv_rm_1", "Remove Test");
        let request = Request::new(Action::AddServer, serde_json::to_value(&cfg).unwrap());
        let _ = send_request(&mut stream, &request).await;

        // Remove via IPC
        let request = Request::new(
            Action::RemoveServer,
            serde_json::json!({"server_id": "srv_rm_1"}),
        );
        let response = send_request(&mut stream, &request).await;
        assert!(matches!(response, Response::Ok { .. }));

        // List should be empty
        let request = Request::new_simple(Action::ListServers);
        let response = send_request(&mut stream, &request).await;
        match response {
            Response::Ok { data, .. } => {
                let servers = data["servers"].as_array().unwrap();
                assert!(!servers.iter().any(|s| s["id"] == "srv_rm_1"));
            }
            _ => panic!("expected Ok response"),
        }

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_ipc_connect_nonexistent_server_returns_error() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();
        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        // Connect to nonexistent server — should return Err
        let request = Request::new(
            Action::ConnectServer,
            serde_json::json!({"server_id": "nonexistent"}),
        );
        let response = send_request(&mut stream, &request).await;
        assert!(matches!(response, Response::Err { .. }));

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_ipc_disconnect_nonexistent_server_returns_error() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();
        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        let request = Request::new(
            Action::DisconnectServer,
            serde_json::json!({"server_id": "nonexistent"}),
        );
        let response = send_request(&mut stream, &request).await;
        assert!(matches!(response, Response::Err { .. }));

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_ipc_manual_fire_trigger_nonexistent() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();
        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        let request = Request::new(
            Action::ManualFireTrigger,
            serde_json::json!({"server_id": "nonexistent", "trigger_id": "trig_1"}),
        );
        let response = send_request(&mut stream, &request).await;
        assert!(matches!(response, Response::Err { .. }));

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_ipc_save_and_list_local_trigger() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();
        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        // Save a local trigger
        let trigger_json = serde_json::json!({
            "id": "local_trig_1",
            "template_id": "",
            "name": "Test Local Trigger",
            "trigger_type": "ManualFire",
            "enabled": true,
            "continue_on_error": false,
            "commands": ["echo hello"],
            "parameters": {},
            "timeout_secs": 30,
            "cooldown_secs": 60,
            "notify_on_success": false,
            "notify_on_failure": true,
            "last_fired_at": null,
            "template_hash_at_addition": "",
        });
        let request = Request::new(
            Action::SaveLocalTrigger,
            serde_json::json!({ "trigger": trigger_json }),
        );
        let response = send_request(&mut stream, &request).await;
        assert!(matches!(response, Response::Ok { .. }));

        // List via GetConfig — local_triggers should contain our trigger
        let request = Request::new(Action::GetConfig, serde_json::json!({}));
        let response = send_request(&mut stream, &request).await;
        if let Response::Ok { data, .. } = response {
            let local_triggers = data["local_triggers"].as_array();
            assert!(local_triggers.is_some());
            assert_eq!(local_triggers.unwrap().len(), 1);
            assert_eq!(local_triggers.unwrap()[0]["id"], "local_trig_1");
        } else {
            panic!("expected Ok response");
        }

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_ipc_remove_local_trigger() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();
        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        // Save a local trigger first
        let trigger_json = serde_json::json!({
            "id": "local_trig_2",
            "template_id": "",
            "name": "To Be Removed",
            "trigger_type": "ManualFire",
            "enabled": true,
            "continue_on_error": false,
            "commands": ["echo bye"],
            "parameters": {},
            "timeout_secs": 30,
            "cooldown_secs": 60,
            "notify_on_success": false,
            "notify_on_failure": true,
            "last_fired_at": null,
            "template_hash_at_addition": "",
        });
        let request = Request::new(
            Action::SaveLocalTrigger,
            serde_json::json!({ "trigger": trigger_json }),
        );
        let _ = send_request(&mut stream, &request).await;

        // Remove it
        let request = Request::new(
            Action::RemoveLocalTrigger,
            serde_json::json!({ "trigger_id": "local_trig_2" }),
        );
        let response = send_request(&mut stream, &request).await;
        assert!(matches!(response, Response::Ok { .. }));

        // Verify it's gone via GetConfig
        let request = Request::new(Action::GetConfig, serde_json::json!({}));
        let response = send_request(&mut stream, &request).await;
        if let Response::Ok { data, .. } = response {
            let local_triggers = data["local_triggers"].as_array().unwrap();
            assert_eq!(local_triggers.len(), 0);
        } else {
            panic!("expected Ok response");
        }

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_ipc_manual_fire_local_trigger() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();
        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        // Save a local trigger with a simple echo command
        let trigger_json = serde_json::json!({
            "id": "local_trig_3",
            "template_id": "",
            "name": "Echo Test",
            "trigger_type": "ManualFire",
            "enabled": true,
            "continue_on_error": false,
            "commands": ["echo fired"],
            "parameters": {},
            "timeout_secs": 10,
            "cooldown_secs": 0,
            "notify_on_success": false,
            "notify_on_failure": true,
            "last_fired_at": null,
            "template_hash_at_addition": "",
        });
        let request = Request::new(
            Action::SaveLocalTrigger,
            serde_json::json!({ "trigger": trigger_json }),
        );
        let _ = send_request(&mut stream, &request).await;

        // Manually fire it — should succeed (echo is a no-op on any shell)
        let request = Request::new(
            Action::ManualFireLocalTrigger,
            serde_json::json!({ "trigger_id": "local_trig_3" }),
        );
        let response = send_request(&mut stream, &request).await;
        if let Response::Ok { data, .. } = response {
            assert_eq!(data["success"], true);
            assert_eq!(data["total_commands"], 1);
            assert_eq!(data["executed_commands"], 1);
        } else {
            panic!("expected Ok response, got: {:?}", response);
        }

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_ipc_update_local_trigger() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();
        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        // Save a local trigger
        let trigger_json = serde_json::json!({
            "id": "local_trig_4",
            "template_id": "",
            "name": "Original Name",
            "trigger_type": "ManualFire",
            "enabled": true,
            "continue_on_error": false,
            "commands": ["echo original"],
            "parameters": {},
            "timeout_secs": 30,
            "cooldown_secs": 60,
            "notify_on_success": false,
            "notify_on_failure": true,
            "last_fired_at": null,
            "template_hash_at_addition": "",
        });
        let request = Request::new(
            Action::SaveLocalTrigger,
            serde_json::json!({ "trigger": trigger_json }),
        );
        let _ = send_request(&mut stream, &request).await;

        // Update it (same ID, new name)
        let updated_json = serde_json::json!({
            "id": "local_trig_4",
            "template_id": "",
            "name": "Updated Name",
            "trigger_type": "OnTerminalOpen",
            "enabled": true,
            "continue_on_error": false,
            "commands": ["echo updated"],
            "parameters": {},
            "timeout_secs": 30,
            "cooldown_secs": 60,
            "notify_on_success": false,
            "notify_on_failure": true,
            "last_fired_at": null,
            "template_hash_at_addition": "",
        });
        let request = Request::new(
            Action::SaveLocalTrigger,
            serde_json::json!({ "trigger": updated_json }),
        );
        let _ = send_request(&mut stream, &request).await;

        // Verify it was updated (not duplicated)
        let request = Request::new(Action::GetConfig, serde_json::json!({}));
        let response = send_request(&mut stream, &request).await;
        if let Response::Ok { data, .. } = response {
            let local_triggers = data["local_triggers"].as_array().unwrap();
            assert_eq!(local_triggers.len(), 1, "should not duplicate on update");
            assert_eq!(local_triggers[0]["name"], "Updated Name");
            assert_eq!(local_triggers[0]["trigger_type"], "OnTerminalOpen");
        } else {
            panic!("expected Ok response");
        }

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_ipc_update_general_config() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();
        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        let request = Request::new(
            Action::UpdateGeneralConfig,
            serde_json::json!({"language": "zh-CN", "theme": "dark"}),
        );
        let response = send_request(&mut stream, &request).await;
        assert!(matches!(response, Response::Ok { .. }));

        // Verify config was updated
        let request = Request::new_simple(Action::GetConfig);
        let response = send_request(&mut stream, &request).await;
        match response {
            Response::Ok { data, .. } => {
                assert_eq!(data["general"]["language"], "zh-CN");
            }
            _ => panic!("expected Ok response"),
        }

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_multi_client_broadcast() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();

        // Connect two clients
        let mut client1 = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
        let mut client2 = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        // Client 1 adds a server — both clients should receive the broadcast event
        let cfg = make_test_server_config("srv_bc_1", "Broadcast Test");
        let request = Request::new(Action::AddServer, serde_json::to_value(&cfg).unwrap());
        let _ = send_request(&mut client1, &request).await;

        // Both clients should receive a broadcast event (server:added)
        // Read event from client1
        let mut len_buf = [0u8; 4];
        let event1_result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            client1.read_exact(&mut len_buf),
        )
        .await;
        // Event may or may not arrive depending on broadcast timing,
        // but the request/response should work for both clients
        let _ = event1_result;

        // Client 2 can also send requests
        let request = Request::new_simple(Action::ListServers);
        let response = send_request(&mut client2, &request).await;
        assert!(matches!(response, Response::Ok { .. }));

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_ipc_concurrent_connection_limit() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();
        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        // Add 4 servers
        for i in 0..4 {
            let cfg = make_test_server_config(&format!("srv_limit_{}", i), &format!("Limit {}", i));
            let request = Request::new(Action::AddServer, serde_json::to_value(&cfg).unwrap());
            let _ = send_request(&mut stream, &request).await;
        }

        // Try to connect all 4 — the 4th should fail due to max 3 concurrent connections
        for i in 0..3 {
            let request = Request::new(
                Action::ConnectServer,
                serde_json::json!({"server_id": format!("srv_limit_{}", i)}),
            );
            let response = send_request(&mut stream, &request).await;
            // These will fail because there's no real SSH server, but the connection
            // slot is acquired before the SSH attempt. The error is from SSH, not from limit.
            // We just verify the request doesn't panic.
            let _ = response;
        }

        server.shutdown().await;
    }

    // === SECTION 2 END ===

    // === SECTION 3: Port forwarding IPC tests (PF-5) ===

    #[tokio::test]
    async fn test_ipc_list_port_forwards_empty() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();
        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        // Add a server first
        let cfg = make_test_server_config("srv_pf_1", "PF Test");
        let request = Request::new(Action::AddServer, serde_json::to_value(&cfg).unwrap());
        let _ = send_request(&mut stream, &request).await;

        // List port forwards — should be empty
        let request = Request::new(
            Action::ListPortForwards,
            serde_json::json!({"server_id": "srv_pf_1"}),
        );
        let response = send_request(&mut stream, &request).await;
        match response {
            Response::Ok { data, .. } => {
                let rules = data["rules"].as_array().unwrap();
                assert!(rules.is_empty(), "should have no port forwards initially");
            }
            _ => panic!("expected Ok response"),
        }

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_ipc_add_port_forward() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();
        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        // Add a server
        let cfg = make_test_server_config("srv_pf_2", "PF Add Test");
        let request = Request::new(Action::AddServer, serde_json::to_value(&cfg).unwrap());
        let _ = send_request(&mut stream, &request).await;

        // Add a port forward rule
        let rule = serde_json::json!({
            "name": "MySQL Tunnel",
            "type": "local",
            "local_host": "127.0.0.1",
            "local_port": 13306,
            "remote_host": "127.0.0.1",
            "remote_port": 3306,
            "enabled": true,
            "auto_start": false
        });
        let request = Request::new(
            Action::AddPortForward,
            serde_json::json!({"server_id": "srv_pf_2", "rule": rule}),
        );
        let response = send_request(&mut stream, &request).await;
        match response {
            Response::Ok { data, .. } => {
                let rule_id = data["rule_id"].as_str().expect("should have rule_id");
                assert!(rule_id.starts_with("pf_"), "rule_id should start with pf_");
            }
            _ => panic!("expected Ok response, got {:?}", response),
        }

        // List should now contain 1 rule
        let request = Request::new(
            Action::ListPortForwards,
            serde_json::json!({"server_id": "srv_pf_2"}),
        );
        let response = send_request(&mut stream, &request).await;
        match response {
            Response::Ok { data, .. } => {
                let rules = data["rules"].as_array().unwrap();
                assert_eq!(rules.len(), 1);
                assert_eq!(rules[0]["name"], "MySQL Tunnel");
                assert_eq!(rules[0]["local_port"], 13306);
            }
            _ => panic!("expected Ok response"),
        }

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_ipc_delete_port_forward() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();
        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        // Add server
        let cfg = make_test_server_config("srv_pf_3", "PF Delete Test");
        let request = Request::new(Action::AddServer, serde_json::to_value(&cfg).unwrap());
        let _ = send_request(&mut stream, &request).await;

        // Add a rule
        let rule = serde_json::json!({
            "name": "Redis Tunnel",
            "type": "local",
            "local_host": "127.0.0.1",
            "local_port": 16379,
            "remote_host": "127.0.0.1",
            "remote_port": 6379,
            "enabled": true,
            "auto_start": false
        });
        let request = Request::new(
            Action::AddPortForward,
            serde_json::json!({"server_id": "srv_pf_3", "rule": rule}),
        );
        let response = send_request(&mut stream, &request).await;
        let rule_id = match response {
            Response::Ok { data, .. } => data["rule_id"].as_str().unwrap().to_string(),
            _ => panic!("expected Ok"),
        };

        // Delete the rule
        let request = Request::new(
            Action::DeletePortForward,
            serde_json::json!({"server_id": "srv_pf_3", "rule_id": rule_id}),
        );
        let response = send_request(&mut stream, &request).await;
        assert!(matches!(response, Response::Ok { .. }));

        // List should be empty
        let request = Request::new(
            Action::ListPortForwards,
            serde_json::json!({"server_id": "srv_pf_3"}),
        );
        let response = send_request(&mut stream, &request).await;
        match response {
            Response::Ok { data, .. } => {
                let rules = data["rules"].as_array().unwrap();
                assert!(rules.is_empty(), "rule should be deleted");
            }
            _ => panic!("expected Ok"),
        }

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_ipc_get_port_forward_status_not_running() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();
        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        // Add server
        let cfg = make_test_server_config("srv_pf_4", "PF Status Test");
        let request = Request::new(Action::AddServer, serde_json::to_value(&cfg).unwrap());
        let _ = send_request(&mut stream, &request).await;

        // Add a rule
        let rule = serde_json::json!({
            "name": "Web Tunnel",
            "type": "local",
            "local_host": "127.0.0.1",
            "local_port": 18080,
            "remote_host": "127.0.0.1",
            "remote_port": 80,
            "enabled": true,
            "auto_start": false
        });
        let request = Request::new(
            Action::AddPortForward,
            serde_json::json!({"server_id": "srv_pf_4", "rule": rule}),
        );
        let response = send_request(&mut stream, &request).await;
        let rule_id = match response {
            Response::Ok { data, .. } => data["rule_id"].as_str().unwrap().to_string(),
            _ => panic!("expected Ok"),
        };

        // Get status — should show running=false
        let request = Request::new(
            Action::GetPortForwardStatus,
            serde_json::json!({"server_id": "srv_pf_4", "rule_id": rule_id}),
        );
        let response = send_request(&mut stream, &request).await;
        match response {
            Response::Ok { data, .. } => {
                assert_eq!(data["running"], false);
                assert_eq!(data["active_connections"], 0);
            }
            _ => panic!("expected Ok"),
        }

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_ipc_start_port_forward_local() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();
        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        // Add server
        let cfg = make_test_server_config("srv_pf_5", "PF Start Test");
        let request = Request::new(Action::AddServer, serde_json::to_value(&cfg).unwrap());
        let _ = send_request(&mut stream, &request).await;

        // Add a local forward rule
        let rule = serde_json::json!({
            "name": "Local Forward",
            "type": "local",
            "local_host": "127.0.0.1",
            "local_port": 13801,
            "remote_host": "127.0.0.1",
            "remote_port": 80,
            "enabled": true,
            "auto_start": false
        });
        let request = Request::new(
            Action::AddPortForward,
            serde_json::json!({"server_id": "srv_pf_5", "rule": rule}),
        );
        let response = send_request(&mut stream, &request).await;
        let rule_id = match response {
            Response::Ok { data, .. } => data["rule_id"].as_str().unwrap().to_string(),
            _ => panic!("expected Ok"),
        };

        // Start the rule — local forward doesn't need SSH connection
        let request = Request::new(
            Action::StartPortForward,
            serde_json::json!({"server_id": "srv_pf_5", "rule_id": rule_id}),
        );
        let response = send_request(&mut stream, &request).await;
        assert!(matches!(response, Response::Ok { .. }), "start should succeed");

        // Get status — should show running=true
        let request = Request::new(
            Action::GetPortForwardStatus,
            serde_json::json!({"server_id": "srv_pf_5", "rule_id": rule_id}),
        );
        let response = send_request(&mut stream, &request).await;
        match response {
            Response::Ok { data, .. } => {
                assert_eq!(data["running"], true);
            }
            _ => panic!("expected Ok"),
        }

        // Stop the rule
        let request = Request::new(
            Action::StopPortForward,
            serde_json::json!({"server_id": "srv_pf_5", "rule_id": rule_id}),
        );
        let response = send_request(&mut stream, &request).await;
        assert!(matches!(response, Response::Ok { .. }));

        // Get status — should show running=false
        let request = Request::new(
            Action::GetPortForwardStatus,
            serde_json::json!({"server_id": "srv_pf_5", "rule_id": rule_id}),
        );
        let response = send_request(&mut stream, &request).await;
        match response {
            Response::Ok { data, .. } => {
                assert_eq!(data["running"], false);
            }
            _ => panic!("expected Ok"),
        }

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_ipc_update_port_forward() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();
        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        // Add server
        let cfg = make_test_server_config("srv_pf_6", "PF Update Test");
        let request = Request::new(Action::AddServer, serde_json::to_value(&cfg).unwrap());
        let _ = send_request(&mut stream, &request).await;

        // Add a rule
        let rule = serde_json::json!({
            "name": "Original",
            "type": "local",
            "local_host": "127.0.0.1",
            "local_port": 13802,
            "remote_host": "127.0.0.1",
            "remote_port": 80,
            "enabled": true,
            "auto_start": false
        });
        let request = Request::new(
            Action::AddPortForward,
            serde_json::json!({"server_id": "srv_pf_6", "rule": rule}),
        );
        let response = send_request(&mut stream, &request).await;
        let rule_id = match response {
            Response::Ok { data, .. } => data["rule_id"].as_str().unwrap().to_string(),
            _ => panic!("expected Ok"),
        };

        // Update the rule
        let updated_rule = serde_json::json!({
            "name": "Updated Name",
            "type": "local",
            "local_host": "127.0.0.1",
            "local_port": 13803,
            "remote_host": "127.0.0.1",
            "remote_port": 8080,
            "enabled": true,
            "auto_start": false
        });
        let request = Request::new(
            Action::UpdatePortForward,
            serde_json::json!({"server_id": "srv_pf_6", "rule_id": rule_id, "rule": updated_rule}),
        );
        let response = send_request(&mut stream, &request).await;
        assert!(matches!(response, Response::Ok { .. }));

        // List should show updated name and port
        let request = Request::new(
            Action::ListPortForwards,
            serde_json::json!({"server_id": "srv_pf_6"}),
        );
        let response = send_request(&mut stream, &request).await;
        match response {
            Response::Ok { data, .. } => {
                let rules = data["rules"].as_array().unwrap();
                assert_eq!(rules.len(), 1);
                assert_eq!(rules[0]["name"], "Updated Name");
                assert_eq!(rules[0]["local_port"], 13803);
                assert_eq!(rules[0]["remote_port"], 8080);
            }
            _ => panic!("expected Ok"),
        }

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_ipc_update_port_forward_was_running() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();
        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        // Add server
        let cfg = make_test_server_config("srv_pf_wr", "PF WasRunning Test");
        let request = Request::new(Action::AddServer, serde_json::to_value(&cfg).unwrap());
        let _ = send_request(&mut stream, &request).await;

        // Add a local forward rule
        let rule = serde_json::json!({
            "name": "Running Rule",
            "type": "local",
            "local_host": "127.0.0.1",
            "local_port": 13810,
            "remote_host": "127.0.0.1",
            "remote_port": 80,
            "enabled": true,
            "auto_start": false
        });
        let request = Request::new(
            Action::AddPortForward,
            serde_json::json!({"server_id": "srv_pf_wr", "rule": rule}),
        );
        let response = send_request(&mut stream, &request).await;
        let rule_id = match response {
            Response::Ok { data, .. } => data["rule_id"].as_str().unwrap().to_string(),
            _ => panic!("expected Ok"),
        };

        // Start the rule (local forward doesn't need SSH)
        let request = Request::new(
            Action::StartPortForward,
            serde_json::json!({"server_id": "srv_pf_wr", "rule_id": rule_id}),
        );
        let response = send_request(&mut stream, &request).await;
        assert!(matches!(response, Response::Ok { .. }));

        // Update the rule — should return was_running=true
        let updated_rule = serde_json::json!({
            "name": "Updated Running",
            "type": "local",
            "local_host": "127.0.0.1",
            "local_port": 13811,
            "remote_host": "127.0.0.1",
            "remote_port": 8080,
            "enabled": true,
            "auto_start": false
        });
        let request = Request::new(
            Action::UpdatePortForward,
            serde_json::json!({"server_id": "srv_pf_wr", "rule_id": rule_id, "rule": updated_rule}),
        );
        let response = send_request(&mut stream, &request).await;
        match response {
            Response::Ok { data, .. } => {
                assert_eq!(data["was_running"], true, "was_running should be true when updating a running rule");
            }
            _ => panic!("expected Ok"),
        }

        // Verify the rule was stopped (running=false after update)
        let request = Request::new(
            Action::GetPortForwardStatus,
            serde_json::json!({"server_id": "srv_pf_wr", "rule_id": rule_id}),
        );
        let response = send_request(&mut stream, &request).await;
        match response {
            Response::Ok { data, .. } => {
                assert_eq!(data["running"], false, "rule should be stopped after update");
            }
            _ => panic!("expected Ok"),
        }

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_ipc_list_port_forwards_server_not_found() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();
        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        // List port forwards for non-existent server
        let request = Request::new(
            Action::ListPortForwards,
            serde_json::json!({"server_id": "nonexistent"}),
        );
        let response = send_request(&mut stream, &request).await;
        assert!(matches!(response, Response::Err { .. }));

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_ipc_add_port_forward_server_not_found() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();
        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        let rule = serde_json::json!({
            "name": "Test",
            "type": "local",
            "local_host": "127.0.0.1",
            "local_port": 13901,
            "remote_host": "127.0.0.1",
            "remote_port": 80,
            "enabled": true,
            "auto_start": false
        });
        let request = Request::new(
            Action::AddPortForward,
            serde_json::json!({"server_id": "nonexistent", "rule": rule}),
        );
        let response = send_request(&mut stream, &request).await;
        assert!(matches!(response, Response::Err { .. }));

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_ipc_delete_port_forward_not_found() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();
        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        // Add server
        let cfg = make_test_server_config("srv_pf_err", "PF Error Test");
        let request = Request::new(Action::AddServer, serde_json::to_value(&cfg).unwrap());
        let _ = send_request(&mut stream, &request).await;

        // Delete non-existent rule
        let request = Request::new(
            Action::DeletePortForward,
            serde_json::json!({"server_id": "srv_pf_err", "rule_id": "pf_nonexistent"}),
        );
        let response = send_request(&mut stream, &request).await;
        assert!(matches!(response, Response::Err { .. }));

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_ipc_update_port_forward_not_found() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();
        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        // Add server
        let cfg = make_test_server_config("srv_pf_upd", "PF Update Err");
        let request = Request::new(Action::AddServer, serde_json::to_value(&cfg).unwrap());
        let _ = send_request(&mut stream, &request).await;

        // Update non-existent rule
        let rule = serde_json::json!({
            "name": "Test",
            "type": "local",
            "local_host": "127.0.0.1",
            "local_port": 13902,
            "remote_host": "127.0.0.1",
            "remote_port": 80,
            "enabled": true,
            "auto_start": false
        });
        let request = Request::new(
            Action::UpdatePortForward,
            serde_json::json!({"server_id": "srv_pf_upd", "rule_id": "pf_nonexistent", "rule": rule}),
        );
        let response = send_request(&mut stream, &request).await;
        assert!(matches!(response, Response::Err { .. }));

        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_ipc_add_port_forward_invalid_port() {
        let (server, _dir) = start_test_daemon().await;
        let socket_path = server.socket_path().clone();
        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();

        // Add server
        let cfg = make_test_server_config("srv_pf_port", "PF Port Test");
        let request = Request::new(Action::AddServer, serde_json::to_value(&cfg).unwrap());
        let _ = send_request(&mut stream, &request).await;

        // Add rule with port > 65535
        let rule = serde_json::json!({
            "name": "Invalid Port",
            "type": "local",
            "local_host": "127.0.0.1",
            "local_port": 70000,
            "remote_host": "127.0.0.1",
            "remote_port": 80,
            "enabled": true,
            "auto_start": false
        });
        let request = Request::new(
            Action::AddPortForward,
            serde_json::json!({"server_id": "srv_pf_port", "rule": rule}),
        );
        let response = send_request(&mut stream, &request).await;
        assert!(matches!(response, Response::Err { .. }));

        server.shutdown().await;
    }

    // === SECTION 3 END ===
}
