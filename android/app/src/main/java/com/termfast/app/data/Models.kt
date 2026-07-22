package com.termfast.app.data

import kotlinx.serialization.Serializable

@Serializable
data class Config(
    val version: Int = 2,
    val general: GeneralConfig = GeneralConfig(),
    val trigger_templates: List<TriggerTemplate> = emptyList(),
    val servers: List<ServerConfig> = emptyList(),
)

@Serializable
data class GeneralConfig(
    val auto_start: Boolean = false,
    val theme: String = "system",
    val language: String = "system",
    val log_level: String = "info",
    val max_log_entries: Int = 1000,
    val log_to_file: Boolean = false,
    val log_dir: String = "",
    val log_max_days: Int = 30,
    val log_max_size_mb: Int = 50,
    val proxy_test_url: String = "https://api.ipify.org",
    val crash_reporting: Boolean = false,
    val notify_connect_success: Boolean = false,
    val notify_disconnect: Boolean = true,
    val notify_reconnect_success: Boolean = false,
    val notify_auth_fail: Boolean = true,
    val notify_proxy_toggle: Boolean = false,
    val notify_proxy_port_conflict: Boolean = true,
    val notify_trigger_fail: Boolean = true,
    val notify_trigger_success: Boolean = false,
    val notify_ip_change: Boolean = false,
    val default_socks5_port: Int = 1080,
    val default_http_port: Int = 8080,
    val default_trigger_timeout_secs: Long = 30,
    val default_ip_check_interval_secs: Long = 300,
    val cloud_sync_provider: String = "",
    val http_proxy_mode: String = "auto",
    val http_proxy_url: String = "",
)

@Serializable
data class ServerConfig(
    val id: String = "",
    val name: String = "",
    val ssh: SshConfig = SshConfig(),
    val proxy: ProxyConfig = ProxyConfig(),
    val reconnect: ReconnectConfig = ReconnectConfig(),
    val ip_check: IpCheckConfig = IpCheckConfig(),
    val triggers: List<TriggerInstance> = emptyList(),
    val suppress_firewall_badge: Boolean = false,
    val test_url: String = "https://google.com",
)

@Serializable
data class SshConfig(
    val host: String = "",
    val port: Int = 22,
    val user: String = "root",
    val auth_method: String = "password",
    val key_path: String = "",
    val key_auto_generated: Boolean = false,
    val skip_hostkey_verify: Boolean = false,
)

@Serializable
data class ProxyConfig(
    val enabled: Boolean = false,
    val socks5_port: Int = 1080,
    val http_port: Int = 0,
    val mixed_port: Int = 0,
    val max_channels: Int = 64,
    val channel_idle_timeout: Long = 300,
)

@Serializable
data class ReconnectConfig(
    val auto_reconnect: Boolean = true,
    val heartbeat_interval: Long = 10,
    val max_attempts: Int = 999,
    val reconnect_timeout_secs: Long = 0,
    val initial_backoff_secs: Long = 1,
    val max_backoff_secs: Long = 30,
)

@Serializable
data class IpCheckConfig(
    val enabled: Boolean = true,
    val interval_secs: Long = 300,
)

@Serializable
data class TriggerInstance(
    val id: String = "",
    val template_id: String = "",
    val trigger_type: String = "ManualFire",
    val name: String = "",
    val enabled: Boolean = true,
    val continue_on_error: Boolean = false,
    val timeout_secs: Long = 30,
    val notify_on_success: Boolean = false,
    val notify_on_failure: Boolean = true,
    val parameters: Map<String, String> = emptyMap(),
    val commands: List<String> = emptyList(),
    val template_hash_at_addition: String = "",
    val cooldown_secs: Long = 0,
    val last_fired_at: String? = null,
)

@Serializable
data class TriggerTemplate(
    val id: String = "",
    val name: String = "",
    val trigger_type: String = "ManualFire",
    val description: String = "",
    val built_in: Boolean = false,
    val template_version: Int = 1,
    val parameters_schema: List<ParameterSchema> = emptyList(),
    val commands: List<String> = emptyList(),
    val check_target: String = "",
    val check_interval: Long = 60,
    val timeout_secs: Long = 30,
)

@Serializable
data class ParameterSchema(
    val name: String = "",
    val label: String = "",
    val param_type: String = "string",
    val required: Boolean = true,
    val default: String = "",
    val validation: String = "",
)

@Serializable
data class TriggerResult(
    val success: Boolean = false,
    val trigger_id: String = "",
    val trigger_name: String = "",
    val executed_commands: Int = 0,
    val total_commands: Int = 0,
    val results: List<CommandResult> = emptyList(),
    val error: String? = null,
)

@Serializable
data class CommandResult(
    val command: String = "",
    val exit_code: Int = 0,
    val stdout: String = "",
    val stderr: String = "",
    val success: Boolean = false,
)

@Serializable
data class GeneratedKeyPair(
    val private_key: String = "",
    val public_key: String = "",
    val passphrase: String = "",
)

@Serializable
data class ServerStatus(
    val server_id: String = "",
    val status: String = "disconnected",
    val exit_ip: String? = null,
    val latency_ms: Long? = null,
    val proxy_running: Boolean = false,
    val vpn_running: Boolean = false,
    val active_channels: Long = 0,
)

@Serializable
data class AppSettings(
    val theme: String = "system",
    val language: String = "zh",
    val logLevel: String = "info",
    val vpnMtu: Int = 1400,
    val ipv6Enabled: Boolean = true,
    val routeUla: Boolean = false,
    val dnsStrategy: String = "over-tcp",
    val killSwitchEnabled: Boolean = true,
    val perAppMode: String = "blacklist",
    val perAppPackages: List<String> = emptyList(),
    val notify_connect_success: Boolean = false,
    val notify_disconnect: Boolean = true,
    val notify_reconnect_success: Boolean = false,
    val notify_auth_fail: Boolean = true,
    val notify_proxy_toggle: Boolean = false,
    val notify_proxy_port_conflict: Boolean = true,
    val notify_trigger_fail: Boolean = true,
    val notify_trigger_success: Boolean = false,
    val notify_ip_change: Boolean = false,
)
