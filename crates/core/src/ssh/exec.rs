//! SSH exec — FP-2.3
//!
//! Remote command execution with timeout and IP detection.

use crate::error::{Error, ErrorCode, IpcError, Result};
use russh::client;
use std::time::Duration;
use tokio::time::timeout;

/// Result of a remote command execution
#[derive(Debug, Clone)]
pub struct ExecResult {
    pub exit_code: u32,
    pub stdout: String,
    pub stderr: String,
}

impl ExecResult {
    pub fn success(stdout: impl Into<String>) -> Self {
        Self {
            exit_code: 0,
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    pub fn failure(exit_code: u32, stdout: impl Into<String>, stderr: impl Into<String>) -> Self {
        Self {
            exit_code,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    pub fn is_success(&self) -> bool {
        self.exit_code == 0
    }
}

/// Execute a command on the remote server with a timeout.
/// On timeout, the channel is killed (§17.1).
pub async fn exec(
    handle: &client::Handle<super::client::SshHandler>,
    command: &str,
    timeout_secs: u64,
) -> Result<ExecResult> {
    let mut channel = handle
        .channel_open_session()
        .await
        .map_err(|e| Error::Ssh(format!("failed to open session channel: {}", e)))?;

    channel
        .exec(true, command)
        .await
        .map_err(|e| Error::Ssh(format!("failed to exec command: {}", e)))?;

    // Read output with timeout
    let read_future = async {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_code = 0u32;

        while let Some(msg) = channel.wait().await {
            match msg {
                russh::ChannelMsg::Data { ref data } => {
                    stdout.extend_from_slice(data);
                }
                russh::ChannelMsg::ExtendedData { ref data, .. } => {
                    stderr.extend_from_slice(data);
                }
                russh::ChannelMsg::ExitStatus { exit_status } => {
                    exit_code = exit_status;
                }
                russh::ChannelMsg::Eof => {
                    break;
                }
                russh::ChannelMsg::Close => {
                    break;
                }
                _ => {}
            }
        }

        ExecResult {
            exit_code,
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        }
    };

    let result = timeout(Duration::from_secs(timeout_secs), read_future)
        .await
        .map_err(|_| {
            // Timeout — kill the channel
            drop(channel);
            Error::Ipc(IpcError::new(
                ErrorCode::TriggerCommandFailed,
                format!("command timed out after {}s: {}", timeout_secs, command),
            ))
        })?;

    Ok(result)
}

/// Detect the client's public IP via SSH_CONNECTION (§5.2)
/// Fallback chain: $SSH_CONNECTION → $SSH_CLIENT → who -m → ss -tnp
pub async fn detect_client_ip(
    handle: &client::Handle<super::client::SshHandler>,
) -> Result<String> {
    // Try $SSH_CONNECTION first
    let result = exec(handle, "echo $SSH_CONNECTION", 10).await?;
    if let Some(ip) = parse_ssh_connection_ip(&result.stdout) {
        return Ok(ip);
    }

    // Fallback: $SSH_CLIENT
    let result = exec(handle, "echo $SSH_CLIENT", 10).await?;
    if let Some(ip) = parse_ssh_connection_ip(&result.stdout) {
        return Ok(ip);
    }

    // Fallback: who -m
    let result = exec(handle, "who -m", 10).await?;
    if let Some(ip) = parse_who_ip(&result.stdout) {
        return Ok(ip);
    }

    // Fallback: ss -tnp | grep ssh (§5.2)
    let result = exec(handle, "ss -tnp 2>/dev/null | grep ssh | head -1", 10).await?;
    if let Some(ip) = parse_ss_ip(&result.stdout) {
        return Ok(ip);
    }

    Err(Error::Ipc(IpcError::new(
        ErrorCode::Internal,
        "failed to detect client IP from SSH connection",
    )))
}

/// Parse client IP from `ss -tnp | grep ssh` output
/// Format: "ESTAB 0 0 peer_ip:peer_port local_ip:local_port users:..."
fn parse_ss_ip(output: &str) -> Option<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return None;
    }
    // ss output format: "ESTAB 0 0 1.2.3.4:12345 5.6.7.8:22 ..."
    // The peer IP is the 4th column (index 3), before the colon
    let fields: Vec<&str> = trimmed.split_whitespace().collect();
    if fields.len() >= 4 {
        let peer = fields[3];
        if let Some(colon_pos) = peer.rfind(':') {
            return Some(peer[..colon_pos].to_string());
        }
        return Some(peer.to_string());
    }
    None
}

/// Parse the first field (client IP) from $SSH_CONNECTION output
/// Format: "client_ip client_port server_ip server_port"
fn parse_ssh_connection_ip(output: &str) -> Option<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }
    let ip = parts[0];
    if is_valid_ip(ip) {
        Some(ip.to_string())
    } else {
        None
    }
}

/// Parse IP from `who -m` output
/// Format: "username pts/0 2024-01-15 14:32 (1.2.3.4)"
fn parse_who_ip(output: &str) -> Option<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Find the IP in parentheses
    if let Some(start) = trimmed.rfind('(') {
        if let Some(end) = trimmed.rfind(')') {
            if end > start {
                let ip = &trimmed[start + 1..end];
                if is_valid_ip(ip) {
                    return Some(ip.to_string());
                }
            }
        }
    }
    None
}

/// Check if a string is a valid IPv4 or IPv6 address
fn is_valid_ip(s: &str) -> bool {
    s.parse::<std::net::IpAddr>().is_ok()
}

/// Determine IP family (ipv4 or ipv6) from an IP address string
pub fn ip_family(ip: &str) -> &'static str {
    if ip.contains(':') {
        "ipv6"
    } else {
        "ipv4"
    }
}

/// Detect client IP using SshClientHandle (convenience method)
pub async fn detect_client_ip_via_exec(ssh: &super::client::SshClientHandle) -> Result<String> {
    // Try $SSH_CONNECTION first
    let result = ssh.exec("echo $SSH_CONNECTION", 10).await?;
    if let Some(ip) = parse_ssh_connection_ip(&result.stdout) {
        return Ok(ip);
    }

    // Fallback: $SSH_CLIENT
    let result = ssh.exec("echo $SSH_CLIENT", 10).await?;
    if let Some(ip) = parse_ssh_connection_ip(&result.stdout) {
        return Ok(ip);
    }

    // Fallback: who -m
    let result = ssh.exec("who -m", 10).await?;
    if let Some(ip) = parse_who_ip(&result.stdout) {
        return Ok(ip);
    }

    Err(Error::Ipc(IpcError::new(
        ErrorCode::Internal,
        "failed to detect client IP from SSH connection",
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ssh_connection_ipv4() {
        let output = "1.2.3.4 12345 5.6.7.8 22\n";
        let ip = parse_ssh_connection_ip(output);
        assert_eq!(ip, Some("1.2.3.4".to_string()));
    }

    #[test]
    fn test_parse_ssh_connection_ipv6() {
        let output = "2001:db8::1 12345 2001:db8::2 22\n";
        let ip = parse_ssh_connection_ip(output);
        assert_eq!(ip, Some("2001:db8::1".to_string()));
    }

    #[test]
    fn test_parse_ssh_connection_empty() {
        assert_eq!(parse_ssh_connection_ip(""), None);
        assert_eq!(parse_ssh_connection_ip("\n"), None);
    }

    #[test]
    fn test_parse_ssh_connection_invalid() {
        assert_eq!(parse_ssh_connection_ip("not_an_ip 12345 5.6.7.8 22"), None);
    }

    #[test]
    fn test_parse_who_ip() {
        let output = "root     pts/0        2024-01-15 14:32 (1.2.3.4)\n";
        let ip = parse_who_ip(output);
        assert_eq!(ip, Some("1.2.3.4".to_string()));
    }

    #[test]
    fn test_parse_who_ip_empty() {
        assert_eq!(parse_who_ip(""), None);
    }

    #[test]
    fn test_ip_family_ipv4() {
        assert_eq!(ip_family("1.2.3.4"), "ipv4");
    }

    #[test]
    fn test_ip_family_ipv6() {
        assert_eq!(ip_family("2001:db8::1"), "ipv6");
    }

    #[test]
    fn test_exec_result_success() {
        let r = ExecResult::success("hello\n");
        assert!(r.is_success());
        assert_eq!(r.stdout, "hello\n");
    }

    #[test]
    fn test_exec_result_failure() {
        let r = ExecResult::failure(1, "", "error");
        assert!(!r.is_success());
        assert_eq!(r.exit_code, 1);
        assert_eq!(r.stderr, "error");
    }

    #[test]
    fn test_is_valid_ip() {
        assert!(is_valid_ip("1.2.3.4"));
        assert!(is_valid_ip("::1"));
        assert!(is_valid_ip("2001:db8::1"));
        assert!(!is_valid_ip("not_an_ip"));
        assert!(!is_valid_ip("256.256.256.256"));
    }
}
