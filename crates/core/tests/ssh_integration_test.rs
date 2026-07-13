//! SSH integration tests with mock SSH server — FP-9.1
//!
//! Tests real SSH connection, exec, and direct-tcpip channels
//! against a mock SSH server running on localhost.
//! Each test starts its own mock server on a unique port.

use vps_guard_core::ssh::auth::AuthMethod;
use vps_guard_core::ssh::client::{ConnectionState, SshClientConfig, SshClientHandle};
use vps_guard_core::ssh::exec;
use vps_guard_test_utils::MockSshServer;

/// Start a mock SSH server on a given port and return a client handle
async fn setup_with_mock_server(port: u16) -> SshClientHandle {
    let server = MockSshServer::new(
        &format!("127.0.0.1:{}", port),
        "testuser",
        "testpass",
    );
    tokio::spawn(async move {
        let _ = server.start().await;
    });
    // Give server time to start
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let config = SshClientConfig {
        host: "127.0.0.1".into(),
        port,
        user: "testuser".into(),
        heartbeat_interval: 30,
        max_attempts: 3,
        initial_backoff_secs: 1,
        max_backoff_secs: 5,
        skip_hostkey_verify: true,
        hostkey_mismatch_callback: None,
    };
    SshClientHandle::new(config)
}

// === SECTION 1 END ===

#[tokio::test]
async fn test_ssh_connect_and_disconnect() {
    let client = setup_with_mock_server(3221).await;
    let auth = AuthMethod::Password { password: "testpass".into() };

    client.connect(&auth).await.expect("connect should succeed");
    assert!(client.is_connected().await);
    assert_eq!(client.state().await, ConnectionState::Connected);

    client.disconnect().await.unwrap();
    assert!(!client.is_connected().await);
}

#[tokio::test]
async fn test_ssh_exec_command() {
    let client = setup_with_mock_server(3222).await;
    let auth = AuthMethod::Password { password: "testpass".into() };

    client.connect(&auth).await.expect("connect should succeed");
    let result = client.exec("echo hello", 10).await;
    let exec_result = result.expect("exec should succeed");
    assert!(exec_result.stdout.contains("hello") || exec_result.stdout.contains("mock"));
    client.disconnect().await.unwrap();
}

#[tokio::test]
async fn test_ssh_detect_ip() {
    let client = setup_with_mock_server(3223).await;
    let auth = AuthMethod::Password { password: "testpass".into() };

    client.connect(&auth).await.expect("connect should succeed");
    let handle = client.get_handle().await.expect("should have handle");
    let ip = exec::detect_client_ip(&handle).await.expect("IP detection should succeed");
    eprintln!("detected IP: {}", ip);
    assert!(!ip.is_empty());
    client.disconnect().await.unwrap();
}

#[tokio::test]
async fn test_ssh_auth_failure() {
    let client = setup_with_mock_server(3224).await;
    let auth = AuthMethod::Password { password: "wrongpass".into() };

    let result = client.connect(&auth).await;
    assert!(result.is_err(), "auth with wrong password should fail");
}

#[tokio::test]
async fn test_ssh_open_direct_tcpip() {
    let client = setup_with_mock_server(3225).await;
    let auth = AuthMethod::Password { password: "testpass".into() };

    client.connect(&auth).await.expect("connect should succeed");
    let result = client.open_direct_tcpip("example.com", 80).await;
    assert!(result.is_ok(), "direct-tcpip channel should open: {:?}", result.err());
    client.disconnect().await.unwrap();
}

#[tokio::test]
async fn test_ssh_reconnect() {
    let client = setup_with_mock_server(3226).await;
    let auth = AuthMethod::Password { password: "testpass".into() };

    client.connect(&auth).await.expect("connect should succeed");
    client.disconnect().await.unwrap();

    // Reconnect
    client.connect(&auth).await.expect("reconnect should succeed");
    assert!(client.is_connected().await);
    client.disconnect().await.unwrap();
}

// === SECTION 2 END ===
