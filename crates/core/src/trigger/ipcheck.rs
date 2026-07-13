//! IP change detection — FP-4.3
//!
//! Detects client IP changes via SSH_CONNECTION.
//! Runs on connect + periodic re-check (default every 5 minutes).

use crate::error::Result;
use crate::ssh::client::SshClientHandle;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::Duration;

/// IP change detector
pub struct IpChangeDetector {
    last_ip: Arc<Mutex<Option<String>>>,
    interval: Duration,
}

impl IpChangeDetector {
    pub fn new(interval_secs: u64) -> Self {
        Self {
            last_ip: Arc::new(Mutex::new(None)),
            interval: Duration::from_secs(interval_secs),
        }
    }

    /// Check for IP change. Returns (new_ip, old_ip, changed)
    pub async fn check(&self, ssh: &SshClientHandle) -> Result<(String, Option<String>, bool)> {
        let new_ip = crate::ssh::exec::detect_client_ip_via_exec(ssh).await?;
        let mut last = self.last_ip.lock().await;
        let old_ip = last.clone();
        let changed = old_ip.as_ref() != Some(&new_ip);
        if changed {
            *last = Some(new_ip.clone());
        }
        Ok((new_ip, old_ip, changed))
    }

    /// Get the last known IP
    pub async fn last_ip(&self) -> Option<String> {
        self.last_ip.lock().await.clone()
    }

    /// Set the last known IP (e.g., from runtime state on startup)
    pub async fn set_last_ip(&self, ip: Option<String>) {
        *self.last_ip.lock().await = ip;
    }

    /// Get the check interval
    pub fn interval(&self) -> Duration {
        self.interval
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ip_detector_creation() {
        let detector = IpChangeDetector::new(300);
        assert_eq!(detector.interval(), Duration::from_secs(300));
        assert_eq!(detector.last_ip().await, None);
    }

    #[tokio::test]
    async fn test_set_last_ip() {
        let detector = IpChangeDetector::new(300);
        detector.set_last_ip(Some("1.2.3.4".into())).await;
        assert_eq!(detector.last_ip().await, Some("1.2.3.4".into()));
    }
}
