package com.termfast.app.data

import android.util.Base64
import android.util.Log
import com.termfast.app.RustBridge
import com.termfast.app.RustEventListener
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.serialization.json.Json
import kotlinx.serialization.builtins.ListSerializer

/**
 * Single source of truth for all Rust-core interactions.
 * Wraps RustBridge JNI calls and exposes async-friendly Flows.
 * Singleton — all screens share the same instance and event flow.
 */
object RustRepository {
    private val json = Json { ignoreUnknownKeys = true; encodeDefaults = true }

    private val _events = MutableSharedFlow<RustEvent>(extraBufferCapacity = 64)
    val events: SharedFlow<RustEvent> = _events.asSharedFlow()

    /** In-memory log buffer, retained across screen navigations. */
    private val _logBuffer = MutableStateFlow<List<RustEvent.LogEntry>>(emptyList())
    val logBuffer: MutableStateFlow<List<RustEvent.LogEntry>> = _logBuffer

    private val listener = object : RustEventListener {
        override fun onEvent(eventJson: String) {
            try {
                val parsed = json.decodeFromString<RustEvent>(eventJson)
                _events.tryEmit(parsed)
                if (parsed is RustEvent.LogEntry) {
                    val current = _logBuffer.value
                    _logBuffer.value = (listOf(parsed) + current).take(500)
                }
            } catch (e: Exception) {
                Log.w("RustRepository", "Failed to parse event: $eventJson", e)
            }
        }
    }

    fun init(dataDir: String) {
        RustBridge.ensureLoaded()
        RustBridge.nativeSetDataDir(dataDir)
        RustBridge.nativeSetEventListener(listener)
    }

    fun ping(): Int = RustBridge.nativePing()

    // --- Config ---
    fun getConfig(): Config? {
        val raw = RustBridge.nativeGetConfigJson()
        return if (raw.isBlank()) null else json.decodeFromString(raw)
    }

    fun saveConfig(config: Config): Boolean {
        val jsonStr = json.encodeToString(Config.serializer(), config)
        return RustBridge.nativeSaveConfigJson(jsonStr)
    }

    // --- Servers ---
    fun listServers(): List<ServerConfig> {
        val raw = RustBridge.nativeListServers()
        return if (raw.isBlank()) emptyList()
        else json.decodeFromString(ListSerializer(ServerConfig.serializer()), raw)
    }

    fun addServer(config: ServerConfig): String {
        val jsonStr = json.encodeToString(ServerConfig.serializer(), config)
        return RustBridge.nativeAddServer(jsonStr)
    }

    fun saveServer(config: ServerConfig): Boolean {
        val jsonStr = json.encodeToString(ServerConfig.serializer(), config)
        return RustBridge.nativeUpdateServer(jsonStr)
    }

    fun removeServer(id: String): Boolean = RustBridge.nativeRemoveServer(id)

    fun connectServer(id: String): Boolean = RustBridge.nativeConnectServer(id)
    fun disconnectServer(id: String): Boolean = RustBridge.nativeDisconnectServer(id)

    fun getServerStatus(id: String): ServerStatus {
        val raw = RustBridge.nativeGetServerStatus(id)
        return if (raw.isBlank()) ServerStatus(server_id = id)
        else json.decodeFromString<ServerStatus>(raw)
    }

    // --- Proxy ---
    fun startProxy(id: String, socks5: Int, http: Int, mixed: Int): Boolean =
        RustBridge.nativeStartProxy(id, socks5, http, mixed)
    fun stopProxy(id: String): Boolean = RustBridge.nativeStopProxy(id)
    fun isProxyRunning(id: String): Boolean = RustBridge.nativeIsProxyRunning(id)

    // --- VPN ---
    fun startVpn(id: String, tunFd: Int, mtu: Int, socks5: Int, dnsStrategy: String, ipv6: Boolean): Boolean =
        RustBridge.nativeStartVpn(id, tunFd, mtu, socks5, dnsStrategy, ipv6)
    fun stopVpn(id: String): Boolean = RustBridge.nativeStopVpn(id)

    // --- Triggers ---
    fun listTriggers(serverId: String): List<TriggerInstance> {
        val raw = RustBridge.nativeListTriggers(serverId)
        return if (raw.isBlank()) emptyList()
        else json.decodeFromString(ListSerializer(TriggerInstance.serializer()), raw)
    }

    fun listTriggerTemplates(): List<TriggerTemplate> {
        val raw = RustBridge.nativeListTriggerTemplates()
        return if (raw.isBlank()) emptyList()
        else json.decodeFromString(ListSerializer(TriggerTemplate.serializer()), raw)
    }

    fun setServerTriggers(serverId: String, triggers: List<TriggerInstance>): Boolean {
        val jsonStr = json.encodeToString(ListSerializer(TriggerInstance.serializer()), triggers)
        return RustBridge.nativeSetServerTriggers(serverId, jsonStr)
    }

    fun runTrigger(serverId: String, triggerId: String): TriggerResult {
        val raw = RustBridge.nativeRunTrigger(serverId, triggerId)
        return if (raw.isBlank()) TriggerResult(error = "empty result")
        else json.decodeFromString<TriggerResult>(raw)
    }

    fun generateKeypair(serverId: String): GeneratedKeyPair {
        val raw = RustBridge.nativeGenerateKeypair(serverId)
        return if (raw.isBlank()) GeneratedKeyPair()
        else json.decodeFromString<GeneratedKeyPair>(raw)
    }

    // --- Credentials ---
    fun saveCredential(serverId: String, type: String, value: String): Boolean =
        RustBridge.nativeSaveCredential(serverId, type, value)
    fun loadCredential(serverId: String, type: String): String? =
        RustBridge.nativeLoadCredential(serverId, type)
    fun deleteCredential(serverId: String, type: String): Boolean =
        RustBridge.nativeDeleteCredential(serverId, type)

    // --- SSH Terminal (PTY) ---
    fun openTerminal(serverId: String, sessionId: String, cols: Int, rows: Int): Boolean =
        RustBridge.nativeOpenTerminal(serverId, sessionId, cols, rows)
    fun writeTerminal(sessionId: String, data: String): Boolean {
        // Encode to base64 so binary data (null bytes, non-UTF-8, etc.) can
        // be transported via JNI JString without corruption.
        val encoded = Base64.encodeToString(data.toByteArray(Charsets.UTF_8), Base64.NO_WRAP)
        return RustBridge.nativeWriteTerminal(sessionId, encoded)
    }
    fun closeTerminal(sessionId: String): Boolean =
        RustBridge.nativeCloseTerminal(sessionId)
    fun resizeTerminal(sessionId: String, cols: Int, rows: Int): Boolean =
        RustBridge.nativeResizeTerminal(sessionId, cols, rows)
}
