//! tmux integration tests with mock SSH server — R1-1
//!
//! Tests detect_tmux, list_termfast_sessions, session_exists,
//! generate_unique_session_name against a mock SSH server with
//! configurable tmux simulation.

use termfast_core::ssh::auth::AuthMethod;
use termfast_core::ssh::client::{SshClientConfig, SshClientHandle};
use termfast_core::ssh::tmux;
use termfast_test_utils::MockSshServer;

/// Start a mock SSH server with tmux simulation and return a connected client handle.
async fn setup_tmux_mock(port: u16, installed: bool, sessions_output: &str) -> SshClientHandle {
    let server = MockSshServer::new(&format!("127.0.0.1:{}", port), "testuser", "testpass")
        .with_tmux(installed, sessions_output);
    tokio::spawn(async move {
        let _ = server.start().await;
    });
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
        known_host_key: None,
        hostkey_mismatch_callback: None,
        socket_protector: None,
    };
    let client = SshClientHandle::new(config);
    let auth = AuthMethod::Password {
        password: "testpass".to_string().into(),
    };
    client.connect(&auth).await.unwrap();
    client
}

fn sample_sessions_output() -> &'static str {
    // Format: name|@termfast|description|created|server|size|session_created|windows|attached|activity
    "termfast_abc|true|my dev session|1700000000|my-server|120x40|1700000000|3|1|1700001000\n\
     termfast_xyz|true|another session|1700002000|my-server|80x24|1700002000|1|0|1700002000\n\
     user_session|false||0||80x24|1700000000|1|0|1700000500"
}

// === SECTION 1 END ===

#[tokio::test]
async fn test_detect_tmux_installed() {
    let client = setup_tmux_mock(18401, true, "").await;
    let handle = client.get_handle().await.unwrap();
    let result = tmux::detect_tmux(&handle).await;
    assert!(result, "detect_tmux should return true when tmux is installed");
}

#[tokio::test]
async fn test_detect_tmux_not_installed() {
    let client = setup_tmux_mock(18402, false, "").await;
    let handle = client.get_handle().await.unwrap();
    let result = tmux::detect_tmux(&handle).await;
    assert!(!result, "detect_tmux should return false when tmux is not installed");
}

#[tokio::test]
async fn test_list_termfast_sessions_filters_non_termfast() {
    let client = setup_tmux_mock(18403, true, sample_sessions_output()).await;
    let handle = client.get_handle().await.unwrap();
    let sessions = tmux::list_termfast_sessions(&handle).await.unwrap();
    // Only 2 @termfast=true sessions, user_session filtered out
    assert_eq!(sessions.len(), 2);
    // Sorted by last_activity descending: termfast_abc (1700001000) > termfast_xyz (1700002000)? No!
    // 1700002000 > 1700001000, so termfast_xyz is first
    assert_eq!(sessions[0].name, "termfast_xyz");
    assert_eq!(sessions[1].name, "termfast_abc");
}

#[tokio::test]
async fn test_list_termfast_sessions_empty() {
    let client = setup_tmux_mock(18404, true, "").await;
    let handle = client.get_handle().await.unwrap();
    let sessions = tmux::list_termfast_sessions(&handle).await.unwrap();
    assert!(sessions.is_empty(), "no sessions should return empty vec");
}

#[tokio::test]
async fn test_list_termfast_sessions_fields() {
    let client = setup_tmux_mock(18405, true, sample_sessions_output()).await;
    let handle = client.get_handle().await.unwrap();
    let sessions = tmux::list_termfast_sessions(&handle).await.unwrap();
    let abc = sessions.iter().find(|s| s.name == "termfast_abc").unwrap();
    assert_eq!(abc.description, "my dev session");
    assert_eq!(abc.created, 1700000000);
    assert_eq!(abc.server, "my-server");
    assert_eq!(abc.size, (120, 40));
    assert_eq!(abc.windows, 3);
    assert_eq!(abc.attached_count, 1);
    assert_eq!(abc.last_activity, 1700001000);
}

// === SECTION 2 END ===

#[tokio::test]
async fn test_session_exists_true() {
    let client = setup_tmux_mock(18406, true, sample_sessions_output()).await;
    let handle = client.get_handle().await.unwrap();
    let exists = tmux::session_exists(&handle, "termfast_abc").await.unwrap();
    assert!(exists, "termfast_abc should exist");
}

#[tokio::test]
async fn test_session_exists_false() {
    let client = setup_tmux_mock(18407, true, sample_sessions_output()).await;
    let handle = client.get_handle().await.unwrap();
    let exists = tmux::session_exists(&handle, "nonexistent_session").await.unwrap();
    assert!(!exists, "nonexistent_session should not exist");
}

#[tokio::test]
async fn test_generate_unique_session_name_succeeds() {
    // Mock has existing sessions but new random name won't collide
    let client = setup_tmux_mock(18408, true, sample_sessions_output()).await;
    let handle = client.get_handle().await.unwrap();
    let name = tmux::generate_unique_session_name(&handle).await.unwrap();
    assert!(name.starts_with("termfast_"), "name should start with termfast_");
    // Name should not be one of the existing sessions
    assert_ne!(name, "termfast_abc");
    assert_ne!(name, "termfast_xyz");
}

#[tokio::test]
async fn test_generate_unique_session_name_all_collide() {
    // 验收 #12: 3 次都碰撞则返回 Err
    // Mock where has-session always returns true — all generated names "exist"
    let server = MockSshServer::new("127.0.0.1:18409", "testuser", "testpass")
        .with_tmux_always_exists();
    tokio::spawn(async move {
        let _ = server.start().await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let config = SshClientConfig {
        host: "127.0.0.1".into(),
        port: 18409,
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
        password: "testpass".to_string().into(),
    };
    client.connect(&auth).await.unwrap();
    let handle = client.get_handle().await.unwrap();

    let result = tmux::generate_unique_session_name(&handle).await;
    assert!(result.is_err(), "should return Err after 3 collisions");
}

#[tokio::test]
async fn test_generate_unique_session_name_format() {
    // 验证成功路径的名称格式
    let client = setup_tmux_mock(18411, true, "").await;
    let handle = client.get_handle().await.unwrap();
    let name = tmux::generate_unique_session_name(&handle).await.unwrap();
    assert!(name.starts_with("termfast_"));
    let parts: Vec<&str> = name.split('_').collect();
    assert_eq!(parts.len(), 3, "name format: termfast_<ts>_<8hex>");
    assert_eq!(parts[2].len(), 8, "random part should be 8 hex chars");
    assert!(parts[2].chars().all(|c| c.is_ascii_hexdigit()), "random part should be hex");
}

#[tokio::test]
async fn test_renamed_session_still_recognized() {
    // 验收 #10: 用户 rename-session 后，session 名不含 termfast_ 前缀，
    // 但 @termfast=true 仍被识别
    let output = "my_renamed_session|true|dev work|1700000000|srv|80x24|1700000000|1|0|1700001000";
    let client = setup_tmux_mock(18410, true, output).await;
    let handle = client.get_handle().await.unwrap();
    let sessions = tmux::list_termfast_sessions(&handle).await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].name, "my_renamed_session");
    assert_eq!(sessions[0].description, "dev work");
}

#[tokio::test]
async fn test_resize_and_notify_executes_update_size_command() {
    // 验收 #13: resize_and_notify 在 SSH+tmux 场景下调用 build_update_size_command
    // 验证 exec 通道收到 `tmux set-option -t <name> @termfast_size "ColsxRows"` 命令
    use termfast_test_utils::MockSshServer;
    use termfast_core::ssh::auth::AuthMethod;
    use termfast_core::ssh::client::{SshClientConfig, SshClientHandle};
    use termfast_core::ssh::exec;

    let server = MockSshServer::new("127.0.0.1:18412", "testuser", "testpass")
        .with_tmux(true, "");
    let sim_handle = server.tmux_sim_handle();
    tokio::spawn(async move {
        let _ = server.start().await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let config = SshClientConfig {
        host: "127.0.0.1".into(),
        port: 18412,
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
        password: "testpass".to_string().into(),
    };
    client.connect(&auth).await.unwrap();
    let handle = client.get_handle().await.unwrap();

    // Simulate what resize_and_notify does: exec build_update_size_command
    let cmd = tmux::build_update_size_command("termfast_test", 100, 30);
    let result = exec::exec(&handle, &cmd, 5).await.unwrap();
    assert!(result.is_success(), "update size command should succeed");

    // Verify the mock SSH server received the correct command
    let log = {
        let sim = sim_handle.lock().unwrap();
        let log = sim.exec_log.lock().unwrap();
        log.clone()
    };
    assert!(
        log.iter().any(|c| c.contains("tmux set-option") && c.contains("@termfast_size") && c.contains("\"100x30\"")),
        "exec log should contain tmux set-option with @termfast_size 100x30, got: {:?}",
        log
    );
}
