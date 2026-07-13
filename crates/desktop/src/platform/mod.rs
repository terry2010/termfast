//! Platform adapter module — FP-6.6 / FP-6.10
//!
//! Platform-specific functionality: system proxy, window effects.

pub mod macos;
pub mod windows;
#[cfg(target_os = "linux")]
pub mod linux;

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

/// Platform adapter trait — abstracts platform-specific operations
#[async_trait::async_trait]
pub trait PlatformAdapter: Send + Sync {
    /// Set system proxy
    async fn set_system_proxy(&self, config: &SystemProxyConfig) -> Result<SetProxyResult>;

    /// Clear system proxy
    async fn clear_system_proxy(&self) -> Result<SetProxyResult>;

    /// Get current system proxy
    async fn get_system_proxy(&self) -> Result<Option<SystemProxyConfig>>;

    /// Apply window effect (vibrancy/mica) to the given window
    fn apply_window_effect(&self, window: &tauri::WebviewWindow) -> Result<()>;
}

/// Get the platform adapter for the current OS
pub fn get_platform_adapter() -> Box<dyn PlatformAdapter> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacOSAdapter)
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsAdapter)
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxAdapter)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Box::new(UnsupportedAdapter)
    }
}

/// Unsupported platform adapter (Linux etc.)
pub struct UnsupportedAdapter;

#[async_trait::async_trait]
impl PlatformAdapter for UnsupportedAdapter {
    async fn set_system_proxy(&self, _config: &SystemProxyConfig) -> Result<SetProxyResult> {
        Ok(SetProxyResult {
            needs_privilege: false,
            success: false,
            message: "system proxy not supported on this platform".into(),
        })
    }

    async fn clear_system_proxy(&self) -> Result<SetProxyResult> {
        Ok(SetProxyResult {
            needs_privilege: false,
            success: false,
            message: "system proxy not supported on this platform".into(),
        })
    }

    async fn get_system_proxy(&self) -> Result<Option<SystemProxyConfig>> {
        Ok(None)
    }

    fn apply_window_effect(&self, _window: &tauri::WebviewWindow) -> Result<()> {
        Ok(())
    }
}
