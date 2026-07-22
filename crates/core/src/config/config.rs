//! Configuration structure definitions — FP-1.2
//!
//! Corresponds to design doc §7.2 schema.
//! All structs derive Serialize/Deserialize/Clone/Debug with snake_case rename.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Top-level config structure (§7.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Config file schema version, for future migration
    #[serde(default = "default_version")]
    pub version: u32,
    /// General preferences
    #[serde(default)]
    pub general: GeneralConfig,
    /// Trigger template library (built-in + user)
    #[serde(default)]
    pub trigger_templates: Vec<TriggerTemplate>,
    /// Server configurations
    #[serde(default)]
    pub servers: Vec<ServerConfig>,
}

fn default_version() -> u32 {
    2
}

/// General application preferences (§7.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneralConfig {
    #[serde(default = "default_false")]
    pub auto_start: bool,
    #[serde(default = "default_true")]
    pub minimize_to_tray: bool,
    /// "system" | "light" | "dark"
    #[serde(default = "default_theme")]
    pub theme: String,
    /// "system" | "zh-CN" | "en"
    #[serde(default = "default_language")]
    pub language: String,
    /// "trace" | "debug" | "info" | "warn" | "error"
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_max_log_entries")]
    pub max_log_entries: usize,
    #[serde(default)]
    pub log_to_file: bool,
    #[serde(default)]
    pub log_dir: String,
    #[serde(default = "default_log_max_days")]
    pub log_max_days: u32,
    #[serde(default = "default_log_max_size_mb")]
    pub log_max_size_mb: u32,
    /// Which server's proxy is set as system proxy (null = none)
    #[serde(default)]
    pub system_proxy_server_id: Option<String>,
    /// URL for proxy connectivity test
    #[serde(default = "default_proxy_test_url")]
    pub proxy_test_url: String,
    /// Crash reporting opt-in
    #[serde(default)]
    pub crash_reporting: bool,
    /// Suppress firewall badge for this server
    #[serde(default)]
    pub suppress_firewall_badge: bool,
    // Notification preferences (§9.5)
    #[serde(default = "default_true")]
    pub notify_connect_success: bool,
    #[serde(default = "default_true")]
    pub notify_disconnect: bool,
    #[serde(default = "default_true")]
    pub notify_reconnect_success: bool,
    #[serde(default = "default_true")]
    pub notify_auth_fail: bool,
    #[serde(default)]
    pub notify_proxy_toggle: bool,
    #[serde(default)]
    pub notify_proxy_port_conflict: bool,
    #[serde(default = "default_true")]
    pub notify_trigger_fail: bool,
    #[serde(default)]
    pub notify_trigger_success: bool,
    #[serde(default)]
    pub notify_ip_change: bool,
    // Proxy defaults (used when adding new servers)
    #[serde(default = "default_socks5_port")]
    pub default_socks5_port: u16,
    #[serde(default = "default_http_port")]
    pub default_http_port: u16,
    #[serde(default = "default_queue_timeout")]
    pub default_queue_timeout_secs: u64,
    // Trigger defaults
    #[serde(default = "default_timeout_secs")]
    pub default_trigger_timeout_secs: u64,
    #[serde(default)]
    pub default_continue_on_error: bool,
    #[serde(default = "default_ip_check_interval")]
    pub default_ip_check_interval_secs: u64,
    /// User-defined custom variables for trigger templates (key → value)
    #[serde(default)]
    pub custom_variables: Vec<CustomVariable>,
    // Cloud sync — which provider is active for config sync
    /// "dropbox" | "baidu" | "" (none)
    #[serde(default)]
    pub cloud_sync_provider: String,
    /// Outbound HTTP proxy mode for cloud sync requests.
    /// "auto" — use system proxy if available (default)
    /// "disabled" — never use proxy
    /// "custom" — use proxy_url
    #[serde(default = "default_proxy_mode")]
    pub http_proxy_mode: String,
    /// Custom proxy URL, used when http_proxy_mode == "custom".
    /// e.g. "http://127.0.0.1:7890" or "socks5://127.0.0.1:7891"
    #[serde(default)]
    pub http_proxy_url: String,
}

/// User-defined custom variable for trigger templates
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomVariable {
    pub name: String,
    pub value: String,
}

fn default_false() -> bool {
    false
}
fn default_true() -> bool {
    true
}
fn default_theme() -> String {
    "system".into()
}
fn default_language() -> String {
    "system".into()
}
fn default_log_level() -> String {
    "info".into()
}
fn default_max_log_entries() -> usize {
    1000
}
fn default_log_max_days() -> u32 {
    30
}
fn default_log_max_size_mb() -> u32 {
    50
}
fn default_proxy_test_url() -> String {
    "https://api.ipify.org".into()
}
fn default_proxy_mode() -> String {
    "auto".into()
}
fn default_socks5_port() -> u16 {
    1080
}
fn default_http_port() -> u16 {
    8080
}
fn default_queue_timeout() -> u64 {
    30
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            auto_start: false,
            minimize_to_tray: true,
            theme: default_theme(),
            language: default_language(),
            log_level: default_log_level(),
            max_log_entries: default_max_log_entries(),
            log_to_file: false,
            log_dir: String::new(),
            log_max_days: default_log_max_days(),
            log_max_size_mb: default_log_max_size_mb(),
            system_proxy_server_id: None,
            proxy_test_url: default_proxy_test_url(),
            crash_reporting: false,
            suppress_firewall_badge: false,
            notify_connect_success: false,
            notify_disconnect: true,
            notify_reconnect_success: false,
            notify_auth_fail: true,
            notify_proxy_toggle: false,
            notify_proxy_port_conflict: true,
            notify_trigger_fail: true,
            notify_trigger_success: false,
            notify_ip_change: false,
            default_socks5_port: default_socks5_port(),
            default_http_port: default_http_port(),
            default_queue_timeout_secs: default_queue_timeout(),
            default_trigger_timeout_secs: default_timeout_secs(),
            default_continue_on_error: false,
            default_ip_check_interval_secs: default_ip_check_interval(),
            custom_variables: Vec::new(),
            cloud_sync_provider: String::new(),
            http_proxy_mode: default_proxy_mode(),
            http_proxy_url: String::new(),
        }
    }
}

/// Trigger type enum (§7.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TriggerType {
    OnConnect,
    OnReconnect,
    OnIpChange,
    OnProcessDead,
    OnPortClosed,
    #[default]
    ManualFire,
}

/// Parameter schema definition for trigger templates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParameterSchema {
    pub name: String,
    pub label: String,
    /// "port" | "string" | "token" | "number"
    #[serde(default = "default_param_type")]
    pub param_type: String,
    #[serde(default = "default_true")]
    pub required: bool,
    #[serde(default)]
    pub default: String,
    #[serde(default)]
    pub validation: String,
}

fn default_param_type() -> String {
    "string".into()
}

/// Trigger template definition (§7.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriggerTemplate {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub trigger_type: TriggerType,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub built_in: bool,
    #[serde(default = "default_template_version")]
    pub template_version: u32,
    #[serde(default)]
    pub parameters_schema: Vec<ParameterSchema>,
    /// Commands with placeholder/conditional block syntax
    #[serde(default)]
    pub commands: Vec<String>,
    /// For on_process_dead: process name to check
    #[serde(default)]
    pub check_target: String,
    /// For on_process_dead / on_port_closed: check interval in seconds
    #[serde(default = "default_check_interval")]
    pub check_interval: u64,
    /// Default timeout for commands
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

fn default_template_version() -> u32 {
    1
}
fn default_check_interval() -> u64 {
    60
}
fn default_timeout_secs() -> u64 {
    30
}

// === SECTION 1 END ===

/// SSH connection configuration (§7.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SshConfig {
    pub host: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    pub user: String,
    /// "key" | "password"
    #[serde(default = "default_auth_method")]
    pub auth_method: String,
    /// Path to private key file (for key auth)
    #[serde(default)]
    pub key_path: String,
    /// Whether the key was auto-generated by TermFast
    #[serde(default)]
    pub key_auto_generated: bool,
    /// "single" (default) | "dual" — single connection or dual connection
    #[serde(default = "default_connection_mode")]
    pub connection_mode: String,
    /// Skip hostkey verification (security downgrade, §17.2)
    #[serde(default)]
    pub skip_hostkey_verify: bool,
    /// Known host key fingerprint (SHA256:xxx). Persisted after first connection
    /// for TOFU (Trust On First Use) verification. None on first connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_key_fingerprint: Option<String>,
}

fn default_ssh_port() -> u16 {
    22
}
fn default_auth_method() -> String {
    "password".into()
}
fn default_connection_mode() -> String {
    "single".into()
}

/// Proxy configuration (§7.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProxyConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub socks5_port: u16,
    #[serde(default)]
    pub http_port: u16,
    /// Mixed port (0 = disabled). When non-zero, both SOCKS5 and HTTP share this port.
    #[serde(default)]
    pub mixed_port: u16,
    /// Max concurrent SSH channels for proxy
    #[serde(default = "default_max_channels")]
    pub max_channels: usize,
    /// Idle timeout for proxy channels in seconds
    #[serde(default = "default_channel_idle_timeout")]
    pub channel_idle_timeout: u64,
}

fn default_max_channels() -> usize {
    64
}
fn default_channel_idle_timeout() -> u64 {
    300
}

/// Reconnection configuration (§7.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReconnectConfig {
    /// Whether to automatically reconnect after SSH connection drops.
    /// When true, the server monitors the connection and reconnects on disconnect.
    #[serde(default = "default_auto_reconnect")]
    pub auto_reconnect: bool,
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval: u64,
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    /// Total seconds to keep trying to reconnect before giving up.
    /// 0 = unlimited. Min 3s, max 259200s (3 days).
    #[serde(default = "default_reconnect_timeout")]
    pub reconnect_timeout_secs: u64,
    #[serde(default = "default_initial_backoff")]
    pub initial_backoff_secs: u64,
    #[serde(default = "default_max_backoff")]
    pub max_backoff_secs: u64,
}

fn default_auto_reconnect() -> bool {
    true
}
fn default_heartbeat_interval() -> u64 {
    10
}
fn default_max_attempts() -> u32 {
    999
}
fn default_reconnect_timeout() -> u64 {
    86400 // 24 hours
}
fn default_initial_backoff() -> u64 {
    1
}
fn default_max_backoff() -> u64 {
    60
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            auto_reconnect: default_auto_reconnect(),
            heartbeat_interval: default_heartbeat_interval(),
            max_attempts: default_max_attempts(),
            reconnect_timeout_secs: default_reconnect_timeout(),
            initial_backoff_secs: default_initial_backoff(),
            max_backoff_secs: default_max_backoff(),
        }
    }
}

/// IP check configuration (§7.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IpCheckConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_ip_check_interval")]
    pub interval_secs: u64,
}

fn default_ip_check_interval() -> u64 {
    300
}

impl Default for IpCheckConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_secs: default_ip_check_interval(),
        }
    }
}

/// Trigger instance — a trigger added to a server, copied from template (§7.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriggerInstance {
    /// UUID, auto-generated at add time
    pub id: String,
    /// Reference to template (may be deleted later, instance survives)
    #[serde(default)]
    pub template_id: String,
    /// Trigger type — stored on instance so it works without template lookup
    #[serde(default)]
    pub trigger_type: TriggerType,
    /// Display name (copied from template or user-customized)
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// If false, stop on first command failure
    #[serde(default)]
    pub continue_on_error: bool,
    /// Per-command timeout in seconds
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub notify_on_success: bool,
    #[serde(default = "default_true")]
    pub notify_on_failure: bool,
    /// User-filled parameters (e.g. ProtectedPort, TelegramToken)
    #[serde(default)]
    pub parameters: HashMap<String, String>,
    /// Commands copied from template at add time (snapshot)
    #[serde(default)]
    pub commands: Vec<String>,
    /// SHA256 hash of template commands at addition time
    /// Used to compute `modified_from_template` and `template_has_update`
    #[serde(default)]
    pub template_hash_at_addition: String,
    /// Cooldown in seconds between trigger fires
    #[serde(default)]
    pub cooldown_secs: u64,
    /// Last time this trigger was fired (RFC3339 timestamp)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fired_at: Option<String>,
}

/// Server configuration (§7.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerConfig {
    pub id: String,
    pub name: String,
    pub ssh: SshConfig,
    pub proxy: ProxyConfig,
    #[serde(default)]
    pub reconnect: ReconnectConfig,
    #[serde(default)]
    pub ip_check: IpCheckConfig,
    /// Last known client public IP (stored in runtime_state.json, not config.json)
    #[serde(default)]
    pub last_known_ip: Option<String>,
    #[serde(default)]
    pub triggers: Vec<TriggerInstance>,
    #[serde(default)]
    pub suppress_firewall_badge: bool,
    /// URL used by the test button in the server list / detail screen.
    #[serde(default = "default_server_test_url")]
    pub test_url: String,
}

fn default_server_test_url() -> String {
    "https://google.com".to_string()
}

// === SECTION 2 END ===

impl Default for Config {
    fn default() -> Self {
        Self {
            version: default_version(),
            general: GeneralConfig::default(),
            trigger_templates: crate::config::builtin_templates::all_builtin_templates(),
            servers: Vec::new(),
        }
    }
}

impl Config {
    /// Find a server by ID
    pub fn find_server(&self, server_id: &str) -> Option<&ServerConfig> {
        self.servers.iter().find(|s| s.id == server_id)
    }

    /// Find a mutable server by ID
    pub fn find_server_mut(&mut self, server_id: &str) -> Option<&mut ServerConfig> {
        self.servers.iter_mut().find(|s| s.id == server_id)
    }

    /// Find a template by ID
    pub fn find_template(&self, template_id: &str) -> Option<&TriggerTemplate> {
        self.trigger_templates.iter().find(|t| t.id == template_id)
    }

    /// Find a mutable template by ID
    pub fn find_template_mut(&mut self, template_id: &str) -> Option<&mut TriggerTemplate> {
        self.trigger_templates
            .iter_mut()
            .find(|t| t.id == template_id)
    }

    /// Check if a SOCKS5 port is already used by another server
    pub fn is_socks5_port_in_use(&self, port: u16, exclude_server_id: Option<&str>) -> bool {
        self.servers
            .iter()
            .filter(|s| Some(s.id.as_str()) != exclude_server_id)
            .any(|s| s.proxy.socks5_port == port)
    }

    /// Check if an HTTP port is already used by another server
    pub fn is_http_port_in_use(&self, port: u16, exclude_server_id: Option<&str>) -> bool {
        self.servers
            .iter()
            .filter(|s| Some(s.id.as_str()) != exclude_server_id)
            .any(|s| s.proxy.http_port == port)
    }

    /// Check if a mixed port is already used by another server
    pub fn is_mixed_port_in_use(&self, port: u16, exclude_server_id: Option<&str>) -> bool {
        self.servers
            .iter()
            .filter(|s| Some(s.id.as_str()) != exclude_server_id)
            .any(|s| s.proxy.mixed_port == port)
    }

    /// Generate a new unique server ID
    pub fn generate_server_id() -> String {
        format!("srv_{}", uuid::Uuid::new_v4().simple())
    }

    /// Generate a new unique trigger ID
    pub fn generate_trigger_id() -> String {
        format!("trg_{}", uuid::Uuid::new_v4().simple())
    }
}

impl TriggerInstance {
    /// Check if this instance's commands differ from the template at addition time
    pub fn modified_from_template(&self) -> bool {
        if self.template_hash_at_addition.is_empty() {
            return false;
        }
        let current_hash = crate::config::config::hash_commands(&self.commands);
        current_hash != self.template_hash_at_addition
    }

    /// Check if the template has been updated since this instance was added
    pub fn template_has_update(&self, template: &TriggerTemplate) -> bool {
        if self.template_hash_at_addition.is_empty() {
            return false;
        }
        let template_hash = crate::config::config::hash_commands(&template.commands);
        template_hash != self.template_hash_at_addition
    }
}

/// Compute SHA256 hash of a command list (for template comparison)
pub fn hash_commands(commands: &[String]) -> String {
    use ring::digest::{digest, SHA256};
    let joined = commands.join("\n");
    let hash = digest(&SHA256, joined.as_bytes());
    hex::encode(hash.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default_has_builtin_templates() {
        let config = Config::default();
        assert!(
            config.trigger_templates.len() >= 5,
            "should have 5 built-in templates"
        );
        assert!(config.trigger_templates.iter().all(|t| t.built_in));
    }

    #[test]
    fn test_config_serialization_round_trip() {
        let config = Config::default();
        let json = serde_json::to_string_pretty(&config).unwrap();
        let de: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(de.version, config.version);
        assert_eq!(de.servers.len(), config.servers.len());
        assert_eq!(de.trigger_templates.len(), config.trigger_templates.len());
    }

    #[test]
    fn test_missing_fields_use_defaults() {
        let json = r#"{"version": 1}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.general.theme, "system");
        assert_eq!(config.general.log_level, "info");
        assert_eq!(config.general.max_log_entries, 1000);
        assert!(config.servers.is_empty());
    }

    #[test]
    fn test_port_in_use_check() {
        let mut config = Config::default();
        let server = ServerConfig {
            id: "srv_test".into(),
            name: "Test".into(),
            ssh: SshConfig {
                host: "1.2.3.4".into(),
                port: 22,
                user: "root".into(),
                auth_method: "key".into(),
                key_path: "".into(),
                key_auto_generated: false,
                connection_mode: "single".into(),
                skip_hostkey_verify: false,
                host_key_fingerprint: None,
            },
            proxy: ProxyConfig {
                enabled: true,
                socks5_port: 1080,
                http_port: 8080,
                mixed_port: 0,
                max_channels: 64,
                channel_idle_timeout: 300,
            },
            reconnect: ReconnectConfig::default(),
            ip_check: IpCheckConfig::default(),
            last_known_ip: None,
            triggers: Vec::new(),
            suppress_firewall_badge: false,
            test_url: default_server_test_url(),
        };
        config.servers.push(server);

        assert!(config.is_socks5_port_in_use(1080, None));
        assert!(!config.is_socks5_port_in_use(1080, Some("srv_test")));
        assert!(!config.is_socks5_port_in_use(1081, None));
        assert!(config.is_http_port_in_use(8080, None));
    }

    #[test]
    fn test_trigger_type_serialization() {
        let t = TriggerType::OnIpChange;
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, "\"OnIpChange\"");
        let de: TriggerType = serde_json::from_str(&json).unwrap();
        assert_eq!(de, t);
    }

    #[test]
    fn test_hash_commands_consistency() {
        let cmds = vec!["echo hello".to_string(), "echo world".to_string()];
        let h1 = hash_commands(&cmds);
        let h2 = hash_commands(&cmds);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // SHA256 hex = 64 chars
    }

    #[test]
    fn test_hash_commands_differ_on_change() {
        let cmds1 = vec!["echo hello".to_string()];
        let cmds2 = vec!["echo world".to_string()];
        assert_ne!(hash_commands(&cmds1), hash_commands(&cmds2));
    }

    #[test]
    fn test_server_config_round_trip() {
        let server = ServerConfig {
            id: "srv_test".into(),
            name: "Test Server".into(),
            ssh: SshConfig {
                host: "1.2.3.4".into(),
                port: 22,
                user: "root".into(),
                auth_method: "key".into(),
                key_path: "~/.ssh/key".into(),
                key_auto_generated: true,
                connection_mode: "single".into(),
                skip_hostkey_verify: false,
                host_key_fingerprint: None,
            },
            proxy: ProxyConfig {
                enabled: true,
                socks5_port: 1080,
                http_port: 8080,
                mixed_port: 0,
                max_channels: 64,
                channel_idle_timeout: 300,
            },
            reconnect: ReconnectConfig::default(),
            ip_check: IpCheckConfig::default(),
            last_known_ip: Some("1.2.3.4".into()),
            triggers: Vec::new(),
            suppress_firewall_badge: false,
            test_url: default_server_test_url(),
        };
        let json = serde_json::to_string(&server).unwrap();
        let de: ServerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(de.id, server.id);
        assert_eq!(de.ssh.host, server.ssh.host);
        assert_eq!(de.proxy.socks5_port, server.proxy.socks5_port);
    }
}

// === SECTION 3 END ===
