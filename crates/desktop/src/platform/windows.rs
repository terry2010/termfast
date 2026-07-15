//! Windows platform adapter — FP-6.6
//!
//! System proxy via Windows registry.
//! Writes to HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings

use super::*;
use anyhow::Result;

/// Windows platform adapter
pub struct WindowsAdapter;

/// Registry path for Internet Settings
#[allow(dead_code)]
const INTERNET_SETTINGS_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

/// Set a registry DWORD value
#[cfg(windows)]
fn set_registry_dword(key: &str, value_name: &str, value: u32) -> Result<()> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (subkey, _) = hkcu.create_subkey(key)?;
    subkey.set_value(value_name, &value)?;
    Ok(())
}

/// Set a registry string value
#[cfg(windows)]
fn set_registry_string(key: &str, value_name: &str, value: &str) -> Result<()> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (subkey, _) = hkcu.create_subkey(key)?;
    subkey.set_value(value_name, &value)?;
    Ok(())
}

/// Get a registry DWORD value
#[cfg(windows)]
fn get_registry_dword(key: &str, value_name: &str) -> Result<u32> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let subkey = hkcu.open_subkey(key)?;
    let value: u32 = subkey.get_value(value_name)?;
    Ok(value)
}

/// Get a registry string value
#[cfg(windows)]
fn get_registry_string(key: &str, value_name: &str) -> Result<String> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let subkey = hkcu.open_subkey(key)?;
    let value: String = subkey.get_value(value_name)?;
    Ok(value)
}

/// Notify Windows that proxy settings changed (broadcast WM_SETTINGCHANGE)
#[cfg(windows)]
fn notify_proxy_change() {
    // This would call SendMessageTimeout with WM_SETTINGCHANGE
    // For now, the change takes effect on next IE/Edge restart
    // A full implementation would use winapi to broadcast the message
}

#[async_trait::async_trait]
impl PlatformAdapter for WindowsAdapter {
    #[allow(unused_variables)]
    async fn set_system_proxy(&self, config: &SystemProxyConfig) -> Result<SetProxyResult> {
        #[cfg(windows)]
        {
            // Build proxy server string: socks=127.0.0.1:1080;http=127.0.0.1:8080
            let proxy_server = format!(
                "socks=127.0.0.1:{};http=127.0.0.1:{}",
                config.socks5_port, config.http_port
            );

            let result = (|| -> Result<()> {
                set_registry_dword(INTERNET_SETTINGS_KEY, "ProxyEnable", 1)?;
                set_registry_string(INTERNET_SETTINGS_KEY, "ProxyServer", &proxy_server)?;
                set_registry_string(INTERNET_SETTINGS_KEY, "ProxyOverride", "<local>")?;
                notify_proxy_change();
                Ok(())
            })();

            match result {
                Ok(()) => Ok(SetProxyResult {
                    needs_privilege: false,
                    success: true,
                    message: format!(
                        "system proxy set: SOCKS5={}, HTTP={}",
                        config.socks5_port, config.http_port
                    ),
                }),
                Err(e) => Ok(SetProxyResult {
                    needs_privilege: false,
                    success: false,
                    message: format!("registry write failed: {}", e),
                }),
            }
        }
        #[cfg(not(windows))]
        {
            Ok(SetProxyResult {
                needs_privilege: false,
                success: false,
                message: "Windows system proxy only available on Windows".into(),
            })
        }
    }

    async fn clear_system_proxy(&self) -> Result<SetProxyResult> {
        #[cfg(windows)]
        {
            let result = (|| -> Result<()> {
                set_registry_dword(INTERNET_SETTINGS_KEY, "ProxyEnable", 0)?;
                notify_proxy_change();
                Ok(())
            })();

            match result {
                Ok(()) => Ok(SetProxyResult {
                    needs_privilege: false,
                    success: true,
                    message: "system proxy cleared".into(),
                }),
                Err(e) => Ok(SetProxyResult {
                    needs_privilege: false,
                    success: false,
                    message: format!("registry write failed: {}", e),
                }),
            }
        }
        #[cfg(not(windows))]
        {
            Ok(SetProxyResult {
                needs_privilege: false,
                success: false,
                message: "Windows system proxy only available on Windows".into(),
            })
        }
    }

    async fn get_system_proxy(&self) -> Result<Option<SystemProxyConfig>> {
        #[cfg(windows)]
        {
            let enabled = get_registry_dword(INTERNET_SETTINGS_KEY, "ProxyEnable").unwrap_or(0);
            if enabled == 0 {
                return Ok(None);
            }
            let proxy_server =
                get_registry_string(INTERNET_SETTINGS_KEY, "ProxyServer").unwrap_or_default();
            // Parse "socks=127.0.0.1:1080;http=127.0.0.1:8080"
            let mut socks5_port = 0u16;
            let mut http_port = 0u16;
            for part in proxy_server.split(';') {
                if let Some(rest) = part.strip_prefix("socks=") {
                    if let Some(port_str) = rest.split(':').nth(1) {
                        socks5_port = port_str.parse().unwrap_or(0);
                    }
                }
                if let Some(rest) = part.strip_prefix("http=") {
                    if let Some(port_str) = rest.split(':').nth(1) {
                        http_port = port_str.parse().unwrap_or(0);
                    }
                }
            }
            if socks5_port > 0 || http_port > 0 {
                return Ok(Some(SystemProxyConfig {
                    server_id: String::new(),
                    socks5_port,
                    http_port,
                }));
            }
            Ok(None)
        }
        #[cfg(not(windows))]
        Ok(None)
    }

    #[allow(unused_variables)]
    fn apply_window_effect(&self, window: &tauri::WebviewWindow) -> Result<()> {
        #[cfg(windows)]
        {
            use window_vibrancy::apply_mica;
            apply_mica(window, true).map_err(|e| anyhow::anyhow!("failed to apply mica: {}", e))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_path() {
        assert!(INTERNET_SETTINGS_KEY.contains("Internet Settings"));
    }
}
