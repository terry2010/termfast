package com.termfast.app.data

import android.util.Base64
import android.util.Log
import android.content.Context
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

    private val _events = MutableSharedFlow<RustEvent>(extraBufferCapacity = 256)
    val events: SharedFlow<RustEvent> = _events.asSharedFlow()

    /** Emit an event from Kotlin side (e.g. tunnel disconnect notification). */
    fun emitEvent(event: RustEvent) {
        if (!_events.tryEmit(event)) {
            Log.w("RustRepository", "Event buffer full, dropped: ${event::class.simpleName}")
        }
    }

    /** In-memory log buffer, retained across screen navigations. */
    private val _logBuffer = MutableStateFlow<List<RustEvent.LogEntry>>(emptyList())
    val logBuffer: MutableStateFlow<List<RustEvent.LogEntry>> = _logBuffer

    private val listener = object : RustEventListener {
        override fun onEvent(eventJson: String) {
            try {
                val parsed = json.decodeFromString<RustEvent>(eventJson)
                if (!_events.tryEmit(parsed)) {
                    Log.w("RustRepository", "Event buffer full, dropped: ${parsed::class.simpleName}")
                }
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

    // --- Server ordering (stored in SharedPreferences, separate from Rust config) ---
    private const val ORDER_PREFS = "server_order"
    private const val ORDER_KEY = "ordered_ids"
    // Unified global ordering: stores mixed item keys (remote_xxx / ssh_id)
    private const val GLOBAL_ORDER_KEY = "global_ordered_keys"
    private var appContext: Context? = null

    fun initOrdering(context: Context) {
        appContext = context.applicationContext
    }

    /** Persist the unified global order (mixed remote_ and ssh keys). */
    fun reorderGlobal(orderedKeys: List<String>) {
        val ctx = appContext ?: return
        val prefs = ctx.getSharedPreferences(ORDER_PREFS, Context.MODE_PRIVATE)
        prefs.edit().putString(GLOBAL_ORDER_KEY, orderedKeys.joinToString(",")).apply()
    }

    /** Get the saved global order (mixed keys). Empty if never saved. */
    fun getGlobalOrder(): List<String> {
        val ctx = appContext ?: return emptyList()
        val prefs = ctx.getSharedPreferences(ORDER_PREFS, Context.MODE_PRIVATE)
        return prefs.getString(GLOBAL_ORDER_KEY, "")?.split(",")?.filter { it.isNotEmpty() } ?: emptyList()
    }

    /** Get servers sorted by user-defined order (unsaved servers appended at end). */
    fun listServersOrdered(): List<ServerConfig> {
        val all = listServers()
        val ctx = appContext ?: return all
        val prefs = ctx.getSharedPreferences(ORDER_PREFS, Context.MODE_PRIVATE)
        val orderedIds = prefs.getString(ORDER_KEY, "")?.split(",")?.filter { it.isNotEmpty() } ?: emptyList()
        val idSet = all.map { it.id }.toSet()
        val ordered = orderedIds.filter { it in idSet }.mapNotNull { id -> all.find { it.id == id } }
        val remaining = all.filter { it.id !in orderedIds }
        return ordered + remaining
    }

    /** Persist the new server order. [orderedIds] is the full list of server IDs in desired order. */
    fun reorderServers(orderedIds: List<String>) {
        val ctx = appContext ?: return
        val prefs = ctx.getSharedPreferences(ORDER_PREFS, Context.MODE_PRIVATE)
        prefs.edit().putString(ORDER_KEY, orderedIds.joinToString(",")).apply()
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

    fun setTriggerTemplates(templates: List<TriggerTemplate>): Boolean {
        val jsonStr = json.encodeToString(ListSerializer(TriggerTemplate.serializer()), templates)
        return RustBridge.nativeSetTriggerTemplates(jsonStr)
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
    fun writeTerminalBytes(sessionId: String, data: ByteArray): Boolean {
        // Same as writeTerminal but accepts raw bytes — avoids String→ByteArray
        // round-trip that would corrupt non-UTF-8 sequences (e.g. from termlib
        // onKeyboardInput callback).
        val encoded = Base64.encodeToString(data, Base64.NO_WRAP)
        return RustBridge.nativeWriteTerminal(sessionId, encoded)
    }
    fun closeTerminal(sessionId: String): Boolean =
        RustBridge.nativeCloseTerminal(sessionId)
    fun resizeTerminal(sessionId: String, cols: Int, rows: Int): Boolean =
        RustBridge.nativeResizeTerminal(sessionId, cols, rows)

    // --- tmux Session Management ---
    fun tmuxListSessions(serverId: String): String =
        RustBridge.nativeTmuxListSessions(serverId)
    fun tmuxNewSession(serverId: String, sessionId: String, description: String, cols: Int, rows: Int): String =
        RustBridge.nativeTmuxNewSession(serverId, sessionId, description, cols, rows)
    fun tmuxAttachSession(serverId: String, sessionId: String, tmuxSessionName: String, cols: Int, rows: Int): String =
        RustBridge.nativeTmuxAttachSession(serverId, sessionId, tmuxSessionName, cols, rows)
    fun tmuxKillSession(serverId: String, tmuxSessionName: String): Boolean =
        RustBridge.nativeTmuxKillSession(serverId, tmuxSessionName)

    // --- Pairing ---
    fun pairingRegister(email: String, password: String): String =
        RustBridge.nativePairingRegister(email, password)
    fun pairingLogin(email: String, password: String): String =
        RustBridge.nativePairingLogin(email, password)
    fun pairingComplete(pairingId: String, phonePubkey: String, deviceId: String): String =
        RustBridge.nativePairingComplete(pairingId, phonePubkey, deviceId)
    fun pairingDownloadConfig(pairingJwt: String): String =
        RustBridge.nativePairingDownloadConfig(pairingJwt)

    // --- Remote Terminal (WebSocket tunnel frame-level API) ---
    // Kotlin TunnelClient manages WebSocket transport; Rust FFI handles
    // frame encryption/decryption and protocol logic (HELLO, LIST, SUBSCRIBE, etc.)

    /** Initialize a tunnel session: generate HELLO, encrypt with pairing key.
     *  Returns encrypted HELLO bytes to send via TunnelClient.sendBinary(). */
    fun remoteTunnelInit(pairingId: String, pairingKey: ByteArray): ByteArray =
        RustBridge.nativeRemoteTunnelInit(pairingId, pairingKey)

    /** Process a binary frame received from the relay.
     *  Rust decrypts and dispatches events (RemoteTerminalOutput, etc.). */
    fun remoteTunnelOnBinary(pairingId: String, data: ByteArray) {
        RustBridge.nativeRemoteTunnelOnBinary(pairingId, data)
    }

    /** Create + encrypt a LIST_REQUEST frame. Returns ciphertext to send via WebSocket. */
    fun remoteTunnelSendListRequest(pairingId: String): ByteArray? =
        RustBridge.nativeRemoteTunnelSendListRequest(pairingId)

    /** Create + encrypt a SUBSCRIBE frame. Returns ciphertext to send via WebSocket. */
    fun remoteTunnelSubscribe(pairingId: String, terminalId: Int): ByteArray? =
        RustBridge.nativeRemoteTunnelSubscribe(pairingId, terminalId)

    /** Create + encrypt an UNSUBSCRIBE frame. Returns ciphertext to send via WebSocket. */
    fun remoteTunnelUnsubscribe(pairingId: String, terminalId: Int): ByteArray? =
        RustBridge.nativeRemoteTunnelUnsubscribe(pairingId, terminalId)

    /** Create + encrypt an INPUT frame with user keystrokes. Returns ciphertext to send. */
    fun remoteTunnelSendInput(pairingId: String, terminalId: Int, data: ByteArray): ByteArray? =
        RustBridge.nativeRemoteTunnelSendInput(pairingId, terminalId, data)

    /** Create + encrypt an INPUT_ANSWER frame (agent popup answer). Returns ciphertext to send.
     *  E2: Includes semantic metadata (cli, option_index, options, etc.) so desktop
     *  frontend can use cliBehavior to generate correct keystrokes. */
    fun remoteTunnelSendInputAnswer(
        pairingId: String,
        terminalId: Int,
        questionId: String,
        answer: String,
        optionIndex: Int,
        cli: String,
        options: Array<String>,
        isMultiSelect: Boolean,
        isMultiQuestion: Boolean,
        activeTabIndex: Int = -1,
        totalTabs: Int = 0,
    ): ByteArray? =
        RustBridge.nativeRemoteTunnelSendInputAnswer(
            pairingId, terminalId, questionId, answer, optionIndex, cli, options, isMultiSelect, isMultiQuestion,
            activeTabIndex, totalTabs,
        )

    /** Create + encrypt a RESIZE frame. Returns ciphertext to send via WebSocket. */
    fun remoteTunnelSendResize(pairingId: String, terminalId: Int, cols: Int, rows: Int): ByteArray? =
        RustBridge.nativeRemoteTunnelSendResize(pairingId, terminalId, cols, rows)

    /** Close tunnel session: send GOODBYE + remove session state. Returns GOODBYE ciphertext. */
    fun remoteTunnelClose(pairingId: String): ByteArray? =
        RustBridge.nativeRemoteTunnelClose(pairingId)

    /** Create + encrypt a DESKTOP_PAIR frame. Used to instruct a desktop to start
     *  a desktop-to-desktop pairing. Returns ciphertext to send via WebSocket. */
    fun remoteTunnelSendDesktopPair(pairingId: String, payloadJson: String): ByteArray? =
        RustBridge.nativeRemoteTunnelSendDesktopPair(pairingId, payloadJson)

    /** Create + encrypt a NEW_TERMINAL frame. Asks the desktop to open a new
     *  terminal. shell/name are optional (empty = desktop default).
     *  serverId: empty = local terminal, otherwise SSH terminal on that server.
     *  Returns ciphertext to send via WebSocket. */
    fun remoteTunnelSendNewTerminal(pairingId: String, shell: String, name: String, serverId: String): ByteArray? =
        RustBridge.nativeRemoteTunnelSendNewTerminal(pairingId, shell, name, serverId)

    /** Create + encrypt a CLOSE_TERMINAL frame. Asks the desktop to close (kill)
     *  a terminal session. Returns ciphertext to send via WebSocket. */
    fun remoteTunnelSendCloseTerminal(pairingId: String, terminalId: Int): ByteArray? =
        RustBridge.nativeRemoteTunnelSendCloseTerminal(pairingId, terminalId)

    // === Remote trigger management ===

    /** Create + encrypt a TRIGGER_LIST_REQUEST frame. Asks the desktop to send
     *  its local trigger list. Returns ciphertext to send via WebSocket. */
    fun remoteTunnelSendTriggerListRequest(pairingId: String): ByteArray? =
        RustBridge.nativeRemoteTunnelSendTriggerListRequest(pairingId)

    /** Create + encrypt a TRIGGER_EXEC frame. Asks the desktop to execute a trigger.
     *  triggerJson = JSON { "trigger_id": "..." }. Returns ciphertext to send via WebSocket. */
    fun remoteTunnelSendTriggerExec(pairingId: String, triggerJson: String): ByteArray? =
        RustBridge.nativeRemoteTunnelSendTriggerExec(pairingId, triggerJson)

    /** Create + encrypt a TRIGGER_ADD frame. Asks the desktop to add a trigger.
     *  triggerJson = JSON trigger object. Returns ciphertext to send via WebSocket. */
    fun remoteTunnelSendTriggerAdd(pairingId: String, triggerJson: String): ByteArray? =
        RustBridge.nativeRemoteTunnelSendTriggerAdd(pairingId, triggerJson)

    /** Create + encrypt a TRIGGER_UPDATE frame. Asks the desktop to update a trigger.
     *  triggerJson = JSON trigger object. Returns ciphertext to send via WebSocket. */
    fun remoteTunnelSendTriggerUpdate(pairingId: String, triggerJson: String): ByteArray? =
        RustBridge.nativeRemoteTunnelSendTriggerUpdate(pairingId, triggerJson)

    /** Create + encrypt a TRIGGER_REMOVE frame. Asks the desktop to remove a trigger.
     *  triggerJson = JSON { "trigger_id": "..." }. Returns ciphertext to send via WebSocket. */
    fun remoteTunnelSendTriggerRemove(pairingId: String, triggerJson: String): ByteArray? =
        RustBridge.nativeRemoteTunnelSendTriggerRemove(pairingId, triggerJson)
}
