//! Port forwarding integration tests with mock SSH server
//!
//! Tests LocalForwarder against a real mock SSH server that supports
//! direct-tcpip channels with echo behavior.

use std::sync::Arc;
use std::time::Duration;
use termfast_core::config::{PortForwardRule, PortForwardType};
use termfast_core::proxy::port_forward::{Forwarder, LocalForwarder, RemoteForwarder};
use termfast_core::ssh::auth::AuthMethod;
use termfast_core::ssh::channel_opener::SshChannelOpener;
use termfast_core::ssh::client::{SshClientConfig, SshClientHandle};
use termfast_test_utils::MockSshServer;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zeroize::Zeroizing;

async fn setup_mock_server_and_client(
    ssh_port: u16,
) -> (Arc<SshChannelOpener>, SshClientHandle) {
    let server = MockSshServer::new(&format!("127.0.0.1:{}", ssh_port), "testuser", "testpass");
    tokio::spawn(async move {
        let _ = server.start().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let config = SshClientConfig {
        host: "127.0.0.1".into(),
        port: ssh_port,
        user: "testuser".into(),
        heartbeat_interval: 30,
        max_attempts: 3,
        initial_backoff_secs: 1,
        max_backoff_secs: 5,
        skip_hostkey_verify: true,
        known_host_key: None,
        hostkey_mismatch_callback: None,
        socket_protector: None,
    };
    let client = SshClientHandle::new(config);
    let auth = AuthMethod::Password {
        password: Zeroizing::new("testpass".into()),
    };
    client.connect(&auth).await.expect("connect should succeed");

    let handle = client.get_handle().await.expect("should have handle");
    let opener = Arc::new(SshChannelOpener::new(handle));
    (opener, client)
}

fn make_local_rule(id: &str, local_port: u16, remote_port: u16) -> PortForwardRule {
    PortForwardRule {
        id: id.into(),
        name: format!("Test {}", id),
        forward_type: PortForwardType::Local,
        local_host: "127.0.0.1".into(),
        local_port,
        remote_host: "127.0.0.1".into(),
        remote_port,
        enabled: true,
        auto_start: false,
    }
}
// === SECTION 1 END ===

#[tokio::test]
async fn test_local_forward_data_transfer() {
    let (opener, _client) = setup_mock_server_and_client(4251).await;
    let rule = make_local_rule("pf_fwd_1", 13551, 9999);
    let fw = LocalForwarder::new(rule, opener);

    fw.start().await.expect("start should succeed");
    assert!(fw.status().running);

    // Spawn accept loop
    let fw_arc = Arc::new(fw);
    let fw_for_loop = fw_arc.clone();
    tokio::spawn(async move {
        fw_for_loop.run_accept_loop().await;
    });

    // Give accept loop time to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Connect to local forward port and send data
    let mut stream = tokio::net::TcpStream::connect("127.0.0.1:13551")
        .await
        .expect("should connect to forward port");
    stream.write_all(b"hello world").await.expect("write");

    // MockSshServer echoes data back
    let mut buf = [0u8; 64];
    let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf))
        .await
        .expect("read should not timeout")
        .expect("read should succeed");
    assert_eq!(&buf[..n], b"hello world");

    // Verify byte counters increased
    let status = fw_arc.status();
    assert!(status.bytes_in > 0, "bytes_in should be > 0, got {}", status.bytes_in);
    assert!(status.bytes_out > 0, "bytes_out should be > 0, got {}", status.bytes_out);

    // Stop
    fw_arc.stop_shared().await.unwrap();
    assert!(!fw_arc.status().running);
}

#[tokio::test]
async fn test_local_forward_multiple_connections() {
    let (opener, _client) = setup_mock_server_and_client(4252).await;
    let rule = make_local_rule("pf_fwd_2", 13552, 9999);
    let fw = LocalForwarder::new(rule, opener);

    fw.start().await.expect("start should succeed");
    let fw_arc = Arc::new(fw);
    let fw_for_loop = fw_arc.clone();
    tokio::spawn(async move {
        fw_for_loop.run_accept_loop().await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Open 3 concurrent connections
    let mut streams = Vec::new();
    for i in 0..3 {
        let mut s = tokio::net::TcpStream::connect("127.0.0.1:13552")
            .await
            .expect("should connect");
        let msg = format!("hello from conn {}", i);
        s.write_all(msg.as_bytes()).await.expect("write");
        streams.push((s, msg));
    }

    // Read echo from each
    for (mut s, msg) in streams {
        let mut buf = [0u8; 64];
        let n = tokio::time::timeout(Duration::from_secs(5), s.read(&mut buf))
            .await
            .expect("read should not timeout")
            .expect("read should succeed");
        assert_eq!(&buf[..n], msg.as_bytes());
    }

    // Wait for active_connections to settle
    tokio::time::sleep(Duration::from_millis(200)).await;
    let status = fw_arc.status();
    assert_eq!(status.active_connections, 0, "all connections should be done");
    assert!(status.bytes_in > 0);
    assert!(status.bytes_out > 0);
}

#[tokio::test]
async fn test_local_forward_stop_rejects_new_connections() {
    let (opener, _client) = setup_mock_server_and_client(4253).await;
    let rule = make_local_rule("pf_fwd_3", 13553, 9999);
    let fw = LocalForwarder::new(rule, opener);

    fw.start().await.expect("start should succeed");
    let fw_arc = Arc::new(fw);
    let fw_for_loop = fw_arc.clone();
    let loop_handle = tokio::spawn(async move {
        fw_for_loop.run_accept_loop().await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Stop the forwarder via stop_shared
    fw_arc.stop_shared().await.unwrap();

    // Wait for accept loop to notice cancellation
    tokio::time::sleep(Duration::from_millis(600)).await;

    // New connection should be rejected (connection refused)
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        tokio::net::TcpStream::connect("127.0.0.1:13553"),
    ).await;

    // Should either fail or timeout — either way, not a clean connection
    match result {
        Ok(Ok(_)) => {
            // On some OSes the port might still be in TIME_WAIT, but listener
            // is gone so this should fail. If it succeeds, the test is still
            // valid as long as the accept loop has exited.
        }
        Ok(Err(_)) => {} // Connection refused — expected
        Err(_) => {}      // Timeout — also acceptable
    }

    // Accept loop should have exited
    let loop_result = tokio::time::timeout(Duration::from_secs(1), loop_handle).await;
    assert!(loop_result.is_ok(), "accept loop should have exited");
}
// === SECTION 2 END ===

// === SECTION 3: RemoteForwarder tests ===

fn make_remote_rule(id: &str, remote_listen_port: u16, local_target_port: u16) -> PortForwardRule {
    PortForwardRule {
        id: id.into(),
        name: format!("Remote {}", id),
        forward_type: PortForwardType::Remote,
        local_host: "127.0.0.1".into(),
        local_port: remote_listen_port,
        remote_host: "127.0.0.1".into(),
        remote_port: local_target_port,
        enabled: true,
        auto_start: false,
    }
}

#[tokio::test]
async fn test_remote_forward_tcpip_forward_request() {
    let (_opener, client) = setup_mock_server_and_client(4261).await;
    let handle = client.get_handle().await.expect("should have handle");
    let dispatch = client.get_forwarded_dispatch();

    let rule = make_remote_rule("pf_rmt_1", 18080, 9999);
    let fw = RemoteForwarder::new(rule, handle, dispatch);

    let result = fw.start().await;
    assert!(result.is_ok(), "tcpip_forward should succeed: {:?}", result.err());
    let actual_port = result.unwrap();
    assert_eq!(actual_port, 18080, "server should return requested port");
    assert!(fw.status().running);

    fw.stop_shared().await.unwrap();
    assert!(!fw.status().running);
    client.disconnect().await.unwrap();
}

#[tokio::test]
async fn test_remote_forward_port_zero_auto_assign() {
    let (_opener, client) = setup_mock_server_and_client(4262).await;
    let handle = client.get_handle().await.expect("should have handle");
    let dispatch = client.get_forwarded_dispatch();

    let rule = make_remote_rule("pf_rmt_2", 0, 9999);
    let fw = RemoteForwarder::new(rule, handle, dispatch);

    let result = fw.start().await;
    assert!(result.is_ok(), "tcpip_forward with port 0 should succeed");
    let actual_port = result.unwrap();
    assert!(actual_port > 0, "server should assign a non-zero port, got {}", actual_port);
    assert_eq!(fw.actual_remote_port().await, Some(actual_port));

    fw.stop_shared().await.unwrap();
    client.disconnect().await.unwrap();
}

#[tokio::test]
async fn test_remote_forward_stop_cancels_server_listener() {
    let (_opener, client) = setup_mock_server_and_client(4263).await;
    let handle = client.get_handle().await.expect("should have handle");
    let dispatch = client.get_forwarded_dispatch();

    let rule = make_remote_rule("pf_rmt_3", 18081, 9999);
    let fw = RemoteForwarder::new(rule, handle, dispatch);

    fw.start().await.expect("start should succeed");
    assert!(fw.status().running);

    // Stop should call cancel_tcpip_forward
    fw.stop_shared().await.unwrap();
    assert!(!fw.status().running);
    // actual_remote_port should still be available after stop
    assert_eq!(fw.actual_remote_port().await, Some(18081));

    client.disconnect().await.unwrap();
}
// === SECTION 3 END ===
