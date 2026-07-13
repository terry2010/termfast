//! Linux platform adapter — FP-6.6
//!
//! System proxy via gsettings (GNOME) or KDE config (KDE).
//! Primary target: GNOME (gsettings org.gnome.system.proxy).

use super::*;
use anyhow::{bail, Result};

/// Linux platform adapter (GNOME-focused, KDE best-effort)
pub struct LinuxAdapter;

/// Check if gsettings is available (GNOME)
fn has_gsettings() -> bool {
    std::process::Command::new("gsettings")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run a gsettings command
fn gsettings_set(schema: &str, key: &str, value: &str) -> Result<()> {
    let output = std::process::Command::new("gsettings")
        .args(["set", schema, key, value])
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("gsettings set {} {} failed: {}", schema, key, stderr);
    }
    Ok(())
}

/// Get a gsettings value
fn gsettings_get(schema: &str, key: &str) -> Result<String> {
    let output = std::process::Command::new("gsettings")
        .args(["get", schema, key])
        .output()?;
    if !output.status.success() {
        bail!("gsettings get {} {} failed", schema, key);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

const PROXY_SCHEMA: &str = "org.gnome.system.proxy";

#[async_trait::async_trait]
impl PlatformAdapter for LinuxAdapter {
    async fn set_system_proxy(&self, config: &SystemProxyConfig) -> Result<SetProxyResult> {
        if !has_gsettings() {
            return Ok(SetProxyResult {
                needs_privilege: false,
                success: false,
                message: "gsettings not found (GNOME required for system proxy on Linux)".into(),
            });
        }

        let socks_port = config.socks5_port.to_string();
        let http_port = config.http_port.to_string();
        let mut errors = Vec::new();

        // Set mode to 'manual' (manual proxy configuration)
        if let Err(e) = gsettings_set(PROXY_SCHEMA, "mode", "'manual'") {
            errors.push(e.to_string());
        }

        // SOCKS proxy
        if let Err(e) = gsettings_set(&format!("{}.socks", PROXY_SCHEMA), "host", "'127.0.0.1'") {
            errors.push(e.to_string());
        }
        if let Err(e) = gsettings_set(&format!("{}.socks", PROXY_SCHEMA), "port", &socks_port) {
            errors.push(e.to_string());
        }

        // HTTP proxy
        if let Err(e) = gsettings_set(&format!("{}.http", PROXY_SCHEMA), "host", "'127.0.0.1'") {
            errors.push(e.to_string());
        }
        if let Err(e) = gsettings_set(&format!("{}.http", PROXY_SCHEMA), "port", &http_port) {
            errors.push(e.to_string());
        }

        // HTTPS proxy (use same as HTTP)
        if let Err(e) = gsettings_set(&format!("{}.https", PROXY_SCHEMA), "host", "'127.0.0.1'") {
            errors.push(e.to_string());
        }
        if let Err(e) = gsettings_set(&format!("{}.https", PROXY_SCHEMA), "port", &http_port) {
            errors.push(e.to_string());
        }

        let success = errors.is_empty();
        Ok(SetProxyResult {
            needs_privilege: false,
            success,
            message: if success {
                "system proxy set via gsettings".into()
            } else {
                format!("partial failure: {}", errors.join("; "))
            },
        })
    }

    async fn clear_system_proxy(&self) -> Result<SetProxyResult> {
        if !has_gsettings() {
            return Ok(SetProxyResult {
                needs_privilege: false,
                success: false,
                message: "gsettings not found".into(),
            });
        }

        // Set mode to 'none' (no proxy)
        match gsettings_set(PROXY_SCHEMA, "mode", "'none'") {
            Ok(()) => Ok(SetProxyResult {
                needs_privilege: false,
                success: true,
                message: "system proxy cleared".into(),
            }),
            Err(e) => Ok(SetProxyResult {
                needs_privilege: false,
                success: false,
                message: format!("failed to clear proxy: {}", e),
            }),
        }
    }

    async fn get_system_proxy(&self) -> Result<Option<SystemProxyConfig>> {
        if !has_gsettings() {
            return Ok(None);
        }

        let mode = gsettings_get(PROXY_SCHEMA, "mode").unwrap_or_default();
        if mode != "'manual'" {
            return Ok(None);
        }

        let socks_host = gsettings_get(&format!("{}.socks", PROXY_SCHEMA), "host")
            .unwrap_or_default()
            .trim_matches('\'')
            .to_string();
        let socks_port = gsettings_get(&format!("{}.socks", PROXY_SCHEMA), "port")
            .unwrap_or_default()
            .parse::<u16>()
            .unwrap_or(0);

        if !socks_host.is_empty() && socks_port > 0 {
            Ok(Some(SystemProxyConfig {
                server_id: socks_host,
                socks5_port: socks_port,
                http_port: 0,
            }))
        } else {
            Ok(None)
        }
    }

    fn apply_window_effect(&self, _window: &tauri::WebviewWindow) -> Result<()> {
        // No native window effects on Linux (compositor-dependent)
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_gsettings_does_not_panic() {
        let _ = has_gsettings();
    }
}
