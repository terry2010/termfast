//! Health checker — FP-4.4
//!
//! Process/port alive checking with periodic scheduling.
//! Fires OnProcessDead / OnPortClosed events when checks fail.

use crate::config::TriggerType;
use crate::error::Result;
use crate::ssh::client::SshClientHandle;
use crate::trigger::engine::{TriggerEngine, TriggerEvent};
use std::sync::Arc;
use tokio::task::JoinHandle;

/// Health check result
#[derive(Debug, Clone)]
pub struct HealthCheckResult {
    pub alive: bool,
    pub detail: String,
}

/// Health check configuration for a single check
#[derive(Debug, Clone)]
pub struct HealthCheckConfig {
    pub check_type: HealthCheckType,
    pub interval_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthCheckType {
    Process(String),
    Port(u16),
}

/// Health checker for process/port monitoring
pub struct HealthChecker;

/// Validate a process name for safe inclusion in a shell command.
/// Only alphanumeric, dash, underscore, dot, and slash are allowed —
/// matches typical process names (e.g. `nginx`, `sshd`, `my-daemon`).
/// Rejects shell metacharacters that could enable command injection.
fn validate_process_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(crate::error::Error::Other(
            "process name must not be empty".into(),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
    {
        return Err(crate::error::Error::Other(format!(
            "process name contains illegal characters: {}",
            name
        )));
    }
    Ok(())
}

/// Shell-escape a value by wrapping in single quotes and escaping
/// embedded single quotes. Used for values that pass through format!
/// into a remote shell command.
fn shell_escape(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

impl HealthChecker {
    /// Check if a process is running on the remote server
    pub async fn check_process(
        ssh: &SshClientHandle,
        process_name: &str,
    ) -> Result<HealthCheckResult> {
        validate_process_name(process_name)?;
        let command = format!("pgrep -x {}", shell_escape(process_name));
        let result = ssh.exec(&command, 10).await?;
        let alive = result.is_success() && !result.stdout.trim().is_empty();
        Ok(HealthCheckResult {
            alive,
            detail: if alive {
                format!(
                    "process {} is running (pid: {})",
                    process_name,
                    result.stdout.trim()
                )
            } else {
                format!("process {} is not running", process_name)
            },
        })
    }

    /// Check if a port is open on the remote server
    pub async fn check_port(ssh: &SshClientHandle, port: u16) -> Result<HealthCheckResult> {
        // port is u16, so it's always a safe numeric value — no injection risk
        let command = format!("ss -tln | grep ':{} ' | grep -v grep", port);
        let result = ssh.exec(&command, 10).await?;
        let alive = result.is_success() && !result.stdout.trim().is_empty();
        Ok(HealthCheckResult {
            alive,
            detail: if alive {
                format!("port {} is open", port)
            } else {
                format!("port {} is closed", port)
            },
        })
    }

    /// Start a periodic health check task that fires trigger events on failure.
    /// Returns a JoinHandle so the caller can abort the task when the server disconnects.
    pub fn start_periodic_check(
        ssh: Arc<SshClientHandle>,
        trigger_engine: Arc<TriggerEngine>,
        server_id: String,
        server_name: String,
        config: HealthCheckConfig,
        triggers: Vec<crate::config::TriggerInstance>,
        templates: Vec<crate::config::TriggerTemplate>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let interval = std::time::Duration::from_secs(config.interval_secs.max(10));
            loop {
                tokio::time::sleep(interval).await;

                // Check if SSH is still connected
                if !ssh.is_connected().await {
                    tracing::debug!("health check stopped: SSH disconnected for {}", server_id);
                    break;
                }

                // Perform the check
                let result = match &config.check_type {
                    HealthCheckType::Process(name) => Self::check_process(&ssh, name).await,
                    HealthCheckType::Port(port) => Self::check_port(&ssh, *port).await,
                };

                match result {
                    Ok(check) => {
                        if !check.alive {
                            // Fire the appropriate trigger event
                            let trigger_type = match &config.check_type {
                                HealthCheckType::Process(_) => TriggerType::OnProcessDead,
                                HealthCheckType::Port(_) => TriggerType::OnPortClosed,
                            };
                            tracing::warn!(
                                "health check failed for {}: {}",
                                server_id,
                                check.detail
                            );
                            let event = TriggerEvent {
                                trigger_type,
                                server_id: server_id.clone(),
                                server_name: server_name.clone(),
                                new_ip: None,
                                old_ip: None,
                                new_ips: None,
                                old_ips: None,
                                session_id: None,
                            };
                            if let Err(e) = trigger_engine
                                .fire_event(Some(&ssh), &triggers, &templates, &event)
                                .await
                            {
                                tracing::error!("failed to fire health event: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::debug!("health check error for {}: {}", server_id, e);
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_check_type_equality() {
        assert_eq!(
            HealthCheckType::Process("nginx".into()),
            HealthCheckType::Process("nginx".into())
        );
        assert_eq!(HealthCheckType::Port(80), HealthCheckType::Port(80));
        assert_ne!(HealthCheckType::Port(80), HealthCheckType::Port(443));
    }

    #[test]
    fn test_health_check_config() {
        let config = HealthCheckConfig {
            check_type: HealthCheckType::Process("nginx".into()),
            interval_secs: 30,
        };
        assert_eq!(config.interval_secs, 30);
        assert!(matches!(config.check_type, HealthCheckType::Process(_)));
    }

    #[test]
    fn test_validate_process_name_accepts_valid() {
        assert!(validate_process_name("nginx").is_ok());
        assert!(validate_process_name("my-daemon").is_ok());
        assert!(validate_process_name("sshd").is_ok());
        assert!(validate_process_name("my_app").is_ok());
        assert!(validate_process_name("app.worker").is_ok());
        assert!(validate_process_name("/usr/sbin/nginx").is_ok());
    }

    #[test]
    fn test_validate_process_name_rejects_injection() {
        assert!(validate_process_name("").is_err());
        assert!(validate_process_name("nginx; rm -rf /").is_err());
        assert!(validate_process_name("nginx`whoami`").is_err());
        assert!(validate_process_name("nginx$(whoami)").is_err());
        assert!(validate_process_name("nginx|cat /etc/passwd").is_err());
        assert!(validate_process_name("nginx&").is_err());
        assert!(validate_process_name("nginx\nwhoami").is_err());
        assert!(validate_process_name("nginx' or '1'='1").is_err());
        assert!(validate_process_name("nginx\"").is_err());
        // Space is not in the allowed set
        assert!(validate_process_name("nginx worker").is_err());
    }

    #[test]
    fn test_shell_escape_health() {
        assert_eq!(shell_escape("nginx"), "'nginx'");
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
    }
}
