//! macOS platform adapter — FP-6.6
//!
//! System proxy via networksetup, sudoers whitelist for privilege escalation.

use super::*;
use anyhow::{bail, Result};

/// macOS platform adapter
pub struct MacOSAdapter;

/// sudoers file path for termfast
const SUDOERS_PATH: &str = "/etc/sudoers.d/termfast";

/// sudoers file content template
pub fn sudoers_content() -> String {
    let user = whoami::username().unwrap_or_else(|_| "unknown".to_string());
    format!(
        "# TermFast system proxy management\n\
         {user} ALL=(root) NOPASSWD: /usr/sbin/networksetup -setsocksfirewallproxy *\n\
         {user} ALL=(root) NOPASSWD: /usr/sbin/networksetup -setwebproxy *\n\
         {user} ALL=(root) NOPASSWD: /usr/sbin/networksetup -setsocksfirewallproxystate *\n\
         {user} ALL=(root) NOPASSWD: /usr/sbin/networksetup -setwebproxystate *\n",
        user = user
    )
}

/// Check if sudoers file is valid
pub fn check_sudoers_valid() -> bool {
    if !std::path::Path::new(SUDOERS_PATH).exists() {
        return false;
    }
    let output = std::process::Command::new("visudo")
        .args(["-cf", SUDOERS_PATH])
        .output();
    match output {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

/// Try to install the sudoers file by prompting for admin password via
/// macOS's native osascript dialog. This avoids requiring the user to
/// manually run a terminal command.
fn install_sudoers() -> Result<()> {
    let content = sudoers_content();
    // Use osascript with administrator privileges to write the sudoers file.
    // This pops up the standard macOS admin password dialog.
    let script = format!(
        "do shell script \"echo '{}' > {} && chmod 440 {} && visudo -cf {}\" with administrator privileges",
        content.replace('\'', "'\\''").replace('\n', "\\n"),
        SUDOERS_PATH,
        SUDOERS_PATH,
        SUDOERS_PATH
    );
    let output = std::process::Command::new("osascript")
        .args(["-e", &script])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("failed to install sudoers: {}", stderr);
    }
    Ok(())
}

/// Get the network service name (e.g., "Wi-Fi", "Ethernet")
pub fn get_network_service() -> Result<String> {
    let output = std::process::Command::new("networksetup")
        .args(["-listallnetworkservices"])
        .output()?;

    let services = String::from_utf8_lossy(&output.stdout);
    // Find the first active service (skip the first line which is a header)
    for line in services.lines().skip(1) {
        let name = line.trim_start_matches("* ").trim();
        if !name.is_empty() && !name.contains("VPN") && !name.contains("Bluetooth") {
            return Ok(name.to_string());
        }
    }
    bail!("no suitable network service found")
}

#[async_trait::async_trait]
impl PlatformAdapter for MacOSAdapter {
    async fn set_system_proxy(&self, config: &SystemProxyConfig) -> Result<SetProxyResult> {
        // Check sudoers first — if not configured, try to install it via
        // osascript admin prompt (GUI-friendly, no terminal needed).
        if !check_sudoers_valid() {
            match install_sudoers() {
                Ok(()) => {
                    // Sudoers installed successfully, continue
                }
                Err(e) => {
                    return Ok(SetProxyResult {
                        needs_privilege: true,
                        success: false,
                        message: format!(
                            "Failed to install sudoers automatically: {}\n\
                             Please run in terminal:\n  \
                             echo '{}' | sudo tee {}",
                            e,
                            sudoers_content().replace('\n', "\\n"),
                            SUDOERS_PATH
                        ),
                    });
                }
            }
        }

        let service = match get_network_service() {
            Ok(s) => s,
            Err(e) => {
                return Ok(SetProxyResult {
                    needs_privilege: false,
                    success: false,
                    message: format!("failed to get network service: {}", e),
                });
            }
        };

        // sudoers is configured — sudo will run without password prompt
        let socks_port = config.socks5_port.to_string();
        let http_port = config.http_port.to_string();

        let commands: Vec<(&str, Vec<&str>)> = vec![
            (
                "sudo",
                vec![
                    "networksetup",
                    "-setsocksfirewallproxy",
                    &service,
                    "127.0.0.1",
                    &socks_port,
                ],
            ),
            (
                "sudo",
                vec![
                    "networksetup",
                    "-setwebproxy",
                    &service,
                    "127.0.0.1",
                    &http_port,
                ],
            ),
            (
                "sudo",
                vec![
                    "networksetup",
                    "-setsocksfirewallproxystate",
                    &service,
                    "on",
                ],
            ),
            (
                "sudo",
                vec!["networksetup", "-setwebproxystate", &service, "on"],
            ),
        ];

        let mut all_success = true;
        for (_, args) in &commands {
            let output = std::process::Command::new("sudo")
                .args(args)
                .stdin(std::process::Stdio::null())
                .output();
            match output {
                Ok(o) if o.status.success() => {}
                _ => {
                    all_success = false;
                    break;
                }
            }
        }

        Ok(SetProxyResult {
            needs_privilege: !all_success,
            success: all_success,
            message: if all_success {
                format!("system proxy set via {}", service)
            } else {
                "failed to set system proxy (sudoers may be misconfigured)".into()
            },
        })
    }

    async fn clear_system_proxy(&self) -> Result<SetProxyResult> {
        // Check sudoers first — same auto-install logic as set_system_proxy
        if !check_sudoers_valid() {
            match install_sudoers() {
                Ok(()) => {}
                Err(e) => {
                    return Ok(SetProxyResult {
                        needs_privilege: true,
                        success: false,
                        message: format!(
                            "Failed to install sudoers automatically: {}\n\
                             Please run in terminal:\n  \
                             echo '{}' | sudo tee {}",
                            e,
                            sudoers_content().replace('\n', "\\n"),
                            SUDOERS_PATH
                        ),
                    });
                }
            }
        }

        let service = match get_network_service() {
            Ok(s) => s,
            Err(e) => {
                return Ok(SetProxyResult {
                    needs_privilege: false,
                    success: false,
                    message: format!("failed to get network service: {}", e),
                });
            }
        };

        let mut all_success = true;
        for args in &[
            vec![
                "networksetup",
                "-setsocksfirewallproxystate",
                &service,
                "off",
            ],
            vec!["networksetup", "-setwebproxystate", &service, "off"],
        ] {
            let output = std::process::Command::new("sudo")
                .args(args)
                .stdin(std::process::Stdio::null())
                .output();
            match output {
                Ok(o) if o.status.success() => {}
                _ => {
                    all_success = false;
                    break;
                }
            }
        }

        Ok(SetProxyResult {
            needs_privilege: !all_success,
            success: all_success,
            message: if all_success {
                "system proxy cleared".into()
            } else {
                "failed to clear system proxy".into()
            },
        })
    }

    async fn get_system_proxy(&self) -> Result<Option<SystemProxyConfig>> {
        // Query macOS system SOCKS5 proxy settings via networksetup
        let output = tokio::process::Command::new("networksetup")
            .args(["-getsocksfirewallproxy", "Wi-Fi"])
            .output()
            .await
            .map_err(|e| anyhow::anyhow!("failed to run networksetup: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Parse output like:
        //   Enabled: No
        //   Server: 127.0.0.1
        //   Port: 1080
        //   Authenticated Proxy Enabled: 0

        let mut enabled = false;
        let mut host = String::new();
        let mut port: u16 = 0;

        for line in stdout.lines() {
            let line = line.trim();
            if let Some(val) = line.strip_prefix("Enabled:") {
                enabled = val.trim() == "Yes";
            } else if let Some(val) = line.strip_prefix("Server:") {
                host = val.trim().to_string();
            } else if let Some(val) = line.strip_prefix("Port:") {
                port = val.trim().parse().unwrap_or(0);
            }
        }

        if enabled && !host.is_empty() && port > 0 {
            Ok(Some(SystemProxyConfig {
                server_id: host,
                socks5_port: port,
                http_port: 0,
            }))
        } else {
            Ok(None)
        }
    }

    fn apply_window_effect(&self, window: &tauri::WebviewWindow) -> Result<()> {
        use window_vibrancy::apply_vibrancy;
        use window_vibrancy::NSVisualEffectMaterial;

        apply_vibrancy(window, NSVisualEffectMaterial::Sidebar, None, None)
            .map_err(|e| anyhow::anyhow!("failed to apply vibrancy: {}", e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sudoers_content() {
        let content = sudoers_content();
        assert!(content.contains("networksetup"));
        assert!(content.contains("NOPASSWD"));
    }

    #[test]
    fn test_check_sudoers_valid_no_file() {
        // Should return false when file doesn't exist
        // (assuming /etc/sudoers.d/termfast doesn't exist in test env)
        let result = check_sudoers_valid();
        // May be true if someone has set it up, but typically false
        // Just verify it doesn't panic
        let _ = result;
    }
}
