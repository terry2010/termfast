//! HTTP proxy helper — builds reqwest clients that respect the user's
//! proxy settings (auto / disabled / custom).
//!
//! - `auto`: detect system proxy (macOS System Configuration, Windows
//!   registry Internet Settings, or `HTTPS_PROXY` env var on Linux)
//! - `disabled`: never use a proxy
//! - `custom`: use the user-specified proxy URL

use std::time::Duration;

/// Proxy mode matching `GeneralConfig.http_proxy_mode`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyMode {
    Auto,
    Disabled,
    Custom(String),
}

impl ProxyMode {
    /// Parse from config string values.
    pub fn from_config(mode: &str, url: &str) -> Self {
        match mode {
            "disabled" => ProxyMode::Disabled,
            "custom" if !url.is_empty() => ProxyMode::Custom(url.to_string()),
            _ => ProxyMode::Auto, // default + "auto" + invalid values
        }
    }
}

/// Build a reqwest client with the given proxy mode and timeouts.
pub fn build_client(
    mode: &ProxyMode,
    timeout: Duration,
    connect_timeout: Duration,
) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(connect_timeout);

    match mode {
        ProxyMode::Disabled => {
            // Explicitly no proxy
        }
        ProxyMode::Custom(url) => {
            match reqwest::Proxy::all(url) {
                Ok(p) => builder = builder.proxy(p),
                Err(e) => tracing::warn!("invalid custom proxy '{}': {}", url, e),
            }
        }
        ProxyMode::Auto => {
            if let Some(proxy_url) = detect_system_proxy() {
                tracing::info!("using system proxy: {}", proxy_url);
                match reqwest::Proxy::all(&proxy_url) {
                    Ok(p) => builder = builder.proxy(p),
                    Err(e) => tracing::warn!("failed to apply system proxy '{}': {}", proxy_url, e),
                }
            }
        }
    }

    builder.build().unwrap_or_else(|_| reqwest::Client::new())
}

/// Detect system HTTP/HTTPS proxy URL.
/// Returns `None` if no proxy is configured.
fn detect_system_proxy() -> Option<String> {
    // 1. Check environment variables (all platforms)
    if let Some(url) = detect_env_proxy() {
        return Some(url);
    }
    // 2. Platform-specific system proxy detection
    #[cfg(target_os = "macos")]
    {
        if let Some(url) = detect_macos_proxy() {
            return Some(url);
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(url) = detect_windows_proxy() {
            return Some(url);
        }
    }
    None
}

/// Check `HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY` env vars.
fn detect_env_proxy() -> Option<String> {
    for var in &["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy", "HTTP_PROXY", "http_proxy"] {
        if let Ok(val) = std::env::var(var) {
            if !val.is_empty() {
                return Some(val);
            }
        }
    }
    None
}

// === SECTION proxy END ===

// === macOS system proxy detection ===
#[cfg(target_os = "macos")]
mod macos_proxy {
    use core_foundation::base::{CFRelease, TCFType};
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use std::ffi::c_void;

    // FFI declarations for SystemConfiguration framework
    #[link(name = "SystemConfiguration", kind = "framework")]
    extern "C" {
        fn SCDynamicStoreCopyProxies() -> *mut c_void;
    }

    /// Read macOS system proxy via SCDynamicStoreCopyProxies.
    /// Returns the first enabled HTTP or HTTPS proxy as "http://host:port".
    pub fn detect() -> Option<String> {
        unsafe {
            let dict_ref = SCDynamicStoreCopyProxies();
            if dict_ref.is_null() {
                return None;
            }
            let dict: CFDictionary<CFString, CFString> =
                CFDictionary::wrap_under_create_rule(dict_ref as *mut _);

            // Check HTTPS proxy first
            let enable_key = CFString::new("HTTPSEnable");
            if is_enabled(dict_ref, &enable_key) {
                let host_key = CFString::new("HTTPSProxy");
                let port_key = CFString::new("HTTPSPort");
                if let (Some(host), Some(port)) = (dict.find(&host_key), dict.find(&port_key)) {
                    let url = format!("http://{}:{}", host.to_string(), port.to_string());
                    CFRelease(dict_ref as *const _);
                    return Some(url);
                }
            }

            // Check HTTP proxy
            let enable_key = CFString::new("HTTPEnable");
            if is_enabled(dict_ref, &enable_key) {
                let host_key = CFString::new("HTTPProxy");
                let port_key = CFString::new("HTTPPort");
                if let (Some(host), Some(port)) = (dict.find(&host_key), dict.find(&port_key)) {
                    let url = format!("http://{}:{}", host.to_string(), port.to_string());
                    CFRelease(dict_ref as *const _);
                    return Some(url);
                }
            }

            CFRelease(dict_ref as *const _);
            None
        }
    }

    /// Check if a proxy enable key is set to 1 in the proxy dict.
    unsafe fn is_enabled(dict_ref: *mut c_void, key: &CFString) -> bool {
        let dict: CFDictionary<CFString, CFNumber> =
            CFDictionary::wrap_under_create_rule(dict_ref as *mut _);
        if let Some(num) = dict.find(key) {
            if let Some(val) = num.to_i32() {
                return val == 1;
            }
        }
        false
    }
}

#[cfg(target_os = "macos")]
fn detect_macos_proxy() -> Option<String> {
    macos_proxy::detect()
}

// === Windows system proxy detection ===
#[cfg(target_os = "windows")]
mod windows_proxy {
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExA, RegQueryValueExA, HKEY_CURRENT_USER, KEY_READ, REG_VALUE_TYPE,
    };
    use windows::core::PCSTR;

    /// Read Windows Internet Settings proxy from registry.
    /// Checks `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Internet Settings`.
    pub fn detect() -> Option<String> {
        unsafe {
            let subkey = b"Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings\0";
            let mut hkey = windows::Win32::System::Registry::HKEY::default();
            let result = RegOpenKeyExA(
                HKEY_CURRENT_USER,
                PCSTR(subkey.as_ptr()),
                0,
                KEY_READ,
                &mut hkey,
            );
            if result.is_err() {
                return None;
            }

            // Check ProxyEnable (DWORD)
            let mut enabled: u32 = 0;
            let mut len: u32 = 4;
            let mut reg_type = REG_VALUE_TYPE::default();
            let _ = RegQueryValueExA(
                hkey,
                PCSTR(b"ProxyEnable\0".as_ptr()),
                None,
                Some(&mut reg_type),
                Some(&mut enabled as *mut u32 as *mut u8),
                Some(&mut len),
            );

            let _ = RegCloseKey(hkey);

            if enabled == 0 {
                return None;
            }

            // Read ProxyServer (string)
            let mut buf = [0u8; 256];
            let mut buf_len: u32 = 256;
            let mut reg_type2 = REG_VALUE_TYPE::default();
            let _ = RegQueryValueExA(
                hkey,
                PCSTR(b"ProxyServer\0".as_ptr()),
                None,
                Some(&mut reg_type2),
                Some(buf.as_mut_ptr()),
                Some(&mut buf_len),
            );

            let _ = RegCloseKey(hkey);

            if buf_len == 0 {
                return None;
            }
            let raw = String::from_utf8_lossy(&buf[..buf_len as usize]);
            let raw = raw.trim_end_matches('\0');

            // ProxyServer can be "http=127.0.0.1:7890;https=127.0.0.1:7890"
            // or just "127.0.0.1:7890"
            for part in raw.split(';') {
                if let Some(addr) = part.strip_prefix("https=") {
                    return Some(format!("http://{}", addr));
                }
            }
            for part in raw.split(';') {
                if let Some(addr) = part.strip_prefix("http=") {
                    return Some(format!("http://{}", addr));
                }
            }
            // No protocol prefix — assume HTTP
            if !raw.is_empty() {
                return Some(format!("http://{}", raw));
            }
            None
        }
    }
}

#[cfg(target_os = "windows")]
fn detect_windows_proxy() -> Option<String> {
    windows_proxy::detect()
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn detect_macos_proxy() -> Option<String> {
    None
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn detect_windows_proxy() -> Option<String> {
    None
}
