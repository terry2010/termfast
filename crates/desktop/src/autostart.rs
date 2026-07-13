//! Autostart interface — FP-6.5
//!
//! Wraps tauri-plugin-autostart for enabling/disabling auto-start.

use anyhow::Result;

/// Autostart manager interface
pub trait AutostartManager: Send + Sync {
    /// Enable auto-start
    fn enable(&self) -> Result<()>;

    /// Disable auto-start
    fn disable(&self) -> Result<()>;

    /// Check if auto-start is enabled
    fn is_enabled(&self) -> Result<bool>;
}

/// Stub autostart manager (real implementation uses tauri-plugin-autostart)
pub struct StubAutostartManager {
    enabled: std::sync::Mutex<bool>,
}

impl StubAutostartManager {
    pub fn new() -> Self {
        Self {
            enabled: std::sync::Mutex::new(false),
        }
    }
}

impl Default for StubAutostartManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AutostartManager for StubAutostartManager {
    fn enable(&self) -> Result<()> {
        *self.enabled.lock().unwrap() = true;
        Ok(())
    }

    fn disable(&self) -> Result<()> {
        *self.enabled.lock().unwrap() = false;
        Ok(())
    }

    fn is_enabled(&self) -> Result<bool> {
        Ok(*self.enabled.lock().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stub_autostart() {
        let mgr = StubAutostartManager::new();
        assert!(!mgr.is_enabled().unwrap());
        mgr.enable().unwrap();
        assert!(mgr.is_enabled().unwrap());
        mgr.disable().unwrap();
        assert!(!mgr.is_enabled().unwrap());
    }
}
