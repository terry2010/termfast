package com.termfast.app.data

import kotlinx.serialization.Serializable
import kotlinx.serialization.SerialName
import kotlinx.serialization.json.JsonElement

@Serializable
sealed class RustEvent {
    @Serializable
    @SerialName("log:entry")
    data class LogEntry(val entry: JsonElement) : RustEvent()

    @Serializable
    @SerialName("server:status_changed")
    data class ServerStatusChanged(
        val server_id: String,
        val status: String,
        val exit_ip: String? = null,
        val latency_ms: Long? = null,
        val error_code: String? = null,
        val error_detail: String? = null,
    ) : RustEvent()

    @Serializable
    @SerialName("proxy:status_changed")
    data class ProxyStatusChanged(
        val server_id: String,
        val proxy_running: Boolean,
        val active_channels: Long = 0,
    ) : RustEvent()

    @Serializable
    @SerialName("vpn:status_changed")
    data class VpnStatusChanged(
        val server_id: String,
        val vpn_running: Boolean,
    ) : RustEvent()

    @Serializable
    @SerialName("ip:changed")
    data class IpChanged(
        val server_id: String,
        val server_name: String,
        val old_ip: String? = null,
        val new_ip: String,
    ) : RustEvent()

    @Serializable
    @SerialName("TerminalData")
    data class TerminalData(
        val session_id: String,
        val data: String,
        val encoding: String? = null,
        val is_stderr: Boolean? = null,
    ) : RustEvent()

    @Serializable
    @SerialName("TerminalClosed")
    data class TerminalClosed(
        val session_id: String,
    ) : RustEvent()

    @Serializable
    @SerialName("TerminalError")
    data class TerminalError(
        val session_id: String,
        val error: String,
    ) : RustEvent()

    // --- Remote Terminal events (from Rust FFI frame processing) ---

    @Serializable
    @SerialName("RemoteTunnelReady")
    data class RemoteTunnelReady(
        val pairing_id: String,
    ) : RustEvent()

    @Serializable
    @SerialName("RemoteTerminalList")
    data class RemoteTerminalList(
        val pairing_id: String,
        val terminals: String,
    ) : RustEvent()

    @Serializable
    @SerialName("RemoteTerminalOutput")
    data class RemoteTerminalOutput(
        val pairing_id: String,
        val terminal_id: Long,
        val data: String,
        val encoding: String,
    ) : RustEvent()

    @Serializable
    @SerialName("RemoteTerminalHistory")
    data class RemoteTerminalHistory(
        val pairing_id: String,
        val terminal_id: Long,
        val seq: Long,
        val is_last: Boolean,
        val data: String,
        val encoding: String,
    ) : RustEvent()

    @Serializable
    @SerialName("RemoteTerminalResize")
    data class RemoteTerminalResize(
        val pairing_id: String,
        val terminal_id: Long,
        val cols: Int,
        val rows: Int,
    ) : RustEvent()

    @Serializable
    @SerialName("RemoteTerminalError")
    data class RemoteTerminalError(
        val pairing_id: String,
        val error: String,
    ) : RustEvent()
}
