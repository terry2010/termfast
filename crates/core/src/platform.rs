//! Platform adapter trait — FP-6.6
//!
//! Abstracts platform-specific system proxy operations.
//! The daemon uses this trait to set/clear system proxy.
//! Desktop crate provides the actual implementation (macOS/Windows).

use anyhow::Result;

/// System proxy configuration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SystemProxyConfig {
    pub server_id: String,
    pub socks5_port: u16,
    pub http_port: u16,
}

/// Result of setting system proxy
#[derive(Debug, Clone)]
pub struct SetProxyResult {
    pub needs_privilege: bool,
    pub success: bool,
    pub message: String,
}

/// Platform adapter trait — abstracts platform-specific system proxy operations.
/// Implemented by the desktop crate (macOS/Windows) and injected into the daemon.
#[async_trait::async_trait]
pub trait SystemProxyAdapter: Send + Sync {
    /// Set system proxy to the given SOCKS5/HTTP ports
    async fn set_system_proxy(&self, config: &SystemProxyConfig) -> Result<SetProxyResult>;

    /// Clear system proxy settings
    async fn clear_system_proxy(&self) -> Result<SetProxyResult>;

    /// Get current system proxy configuration
    async fn get_system_proxy(&self) -> Result<Option<SystemProxyConfig>>;
}

/// No-op adapter (used when no platform adapter is available, e.g., in tests/headless)
pub struct NoopSystemProxyAdapter;

#[async_trait::async_trait]
impl SystemProxyAdapter for NoopSystemProxyAdapter {
    async fn set_system_proxy(&self, _config: &SystemProxyConfig) -> Result<SetProxyResult> {
        Ok(SetProxyResult {
            needs_privilege: false,
            success: false,
            message: "no platform adapter available".into(),
        })
    }

    async fn clear_system_proxy(&self) -> Result<SetProxyResult> {
        Ok(SetProxyResult {
            needs_privilege: false,
            success: false,
            message: "no platform adapter available".into(),
        })
    }

    async fn get_system_proxy(&self) -> Result<Option<SystemProxyConfig>> {
        Ok(None)
    }
}

// === SECTION 1 END ===
