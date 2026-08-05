package com.termfast.app.data

import kotlinx.coroutines.*
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonPrimitive
import okhttp3.*
import okio.ByteString
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit

/**
 * Tunnel connection state.
 */
sealed class TunnelState {
    object Disconnected : TunnelState()
    object Connecting : TunnelState()
    object WaitingForPeer : TunnelState()
    object Connected : TunnelState()
    data class Error(val message: String) : TunnelState()
    object PeerTimeout : TunnelState()
}

/**
 * Control message from relay (text frame JSON).
 */
sealed class ControlMessage {
    data class PeerConnected(val raw: String) : ControlMessage()
    data class PeerDisconnected(val raw: String) : ControlMessage()
    data class PeerTimeout(val raw: String) : ControlMessage()
    data class Error(val message: String) : ControlMessage()
    data class Unknown(val raw: String) : ControlMessage()
}

/**
 * Parse a text frame control message from relay.
 */
fun parseControlMessage(text: String): ControlMessage {
    return try {
        val json = Json.parseToJsonElement(text) as JsonObject
        val type = json["type"]?.jsonPrimitive?.content ?: ""
        when (type) {
            "peer_connected" -> ControlMessage.PeerConnected(text)
            "peer_disconnected" -> ControlMessage.PeerDisconnected(text)
            "peer_timeout" -> ControlMessage.PeerTimeout(text)
            "error" -> ControlMessage.Error(
                json["message"]?.jsonPrimitive?.content ?: "unknown error"
            )
            else -> ControlMessage.Unknown(text)
        }
    } catch (e: Exception) {
        ControlMessage.Unknown(text)
    }
}

// === SECTION 1 END ===

/**
 * Configuration for a tunnel connection.
 *
 * @param relayUrl Base URL of relay server (e.g. "wss://termfast.xisj.com")
 * @param pairingJwt Pairing JWT (scope="tunnel") for authentication
 * @param pairingId Pairing ID for connect control message
 */
data class TunnelConfig(
    val relayUrl: String,
    val pairingJwt: String,
    val pairingId: String,
)

/**
 * Callbacks for tunnel connection events.
 */
interface TunnelCallbacks {
    /** Called when peer_connected is received (pipe established). */
    fun onPeerConnected()
    /** Called when peer_disconnected is received. */
    fun onPeerDisconnected()
    /** Called when peer_timeout is received (desktop offline > 5 min). */
    fun onPeerTimeout()
    /** Called when a binary frame is received (encrypted protocol frame). */
    fun onBinaryFrame(data: ByteArray)
    /** Called on tunnel error. */
    fun onError(message: String)
}

/**
 * A single tunnel connection to the relay for one pairing_id.
 *
 * Manages WebSocket lifecycle:
 * 1. Connect with Authorization: Bearer header
 * 2. Send connect control message (text frame)
 * 3. Wait for peer_connected
 * 4. Relay binary frames to/from callbacks
 * 5. Auto-reconnect with exponential backoff (1s → 30s cap)
 *
 * Encryption/decryption of binary frames is handled by Rust FFI (frame_crypto).
 * This class only manages WebSocket transport + control messages.
 */
class TunnelConnection(
    private val config: TunnelConfig,
    private val callbacks: TunnelCallbacks,
    private val client: OkHttpClient,
    private val scope: CoroutineScope,
) {
    private var webSocket: WebSocket? = null
    @Volatile
    private var _state: TunnelState = TunnelState.Disconnected
    val state: TunnelState get() = _state

    private var reconnectJob: Job? = null
    @Volatile
    private var backoffMs: Long = 1000
    @Volatile
    private var manuallyClosed = false

    /**
     * Start connecting. Auto-reconnects on disconnect with exponential backoff.
     */
    fun connect() {
        scope.launch {
            manuallyClosed = false
            backoffMs = 1000
            connectOnce()
        }
    }

    /**
     * Send a binary frame (encrypted protocol frame from Rust FFI).
     *
     * Only allowed when state is Connected (peer_connected received, relay pipe
     * established). Sending binary frames before peer_connected violates the
     * relay pipe protocol — frames would be dropped by the relay.
     */
    fun sendBinary(data: ByteArray): Boolean {
        if (_state !is TunnelState.Connected) return false
        val ws = webSocket ?: return false
        return ws.send(ByteString.of(*data))
    }

    /**
     * Close the tunnel. Does not auto-reconnect.
     */
    fun close() {
        scope.launch {
            manuallyClosed = true
            reconnectJob?.cancel()
            webSocket?.close(1000, "client closing")
            webSocket = null
            updateState(TunnelState.Disconnected)
        }
    }

    private suspend fun connectOnce() {
        updateState(TunnelState.Connecting)

        val wsUrl = config.relayUrl.replace("http", "ws") + "/tunnel"
        val request = Request.Builder()
            .url(wsUrl)
            .header("Authorization", "Bearer ${config.pairingJwt}")
            .build()

        val connected = CompletableDeferred<Boolean>()

        val ws = client.newWebSocket(request, object : WebSocketListener() {
            override fun onOpen(webSocket: WebSocket, response: Response) {
                // Send connect control message
                val connectMsg = """{"type":"connect","pairing_id":"${config.pairingId}","role":"mobile"}"""
                webSocket.send(connectMsg)
                updateState(TunnelState.WaitingForPeer)
                connected.complete(true)
            }

            override fun onMessage(webSocket: WebSocket, text: String) {
                handleControlMessage(text)
            }

            override fun onMessage(webSocket: WebSocket, bytes: ByteString) {
                callbacks.onBinaryFrame(bytes.toByteArray())
            }

            override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
                handleDisconnect("closed: $code $reason")
            }

            override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                // HTTP 401 during WebSocket upgrade = JWT invalid/expired
                // Stop reconnecting and notify app layer
                val code = response?.code
                if (code == 401) {
                    manuallyClosed = true
                    updateState(TunnelState.Error("authentication failed (401)"))
                    callbacks.onError("authentication failed (401)")
                    return
                }
                handleDisconnect("failure: ${t.message}")
            }
        })

        webSocket = ws

        // Wait for connection to establish
        val success = withTimeoutOrNull(10_000) { connected.await() }
        if (success == null) {
            handleDisconnect("connection timeout")
        }
    }

// === SECTION 2 END ===

    private fun handleControlMessage(text: String) {
        when (val msg = parseControlMessage(text)) {
            is ControlMessage.PeerConnected -> {
                updateState(TunnelState.Connected)
                backoffMs = 1000 // reset backoff on successful peer connection
                callbacks.onPeerConnected()
            }
            is ControlMessage.PeerDisconnected -> {
                callbacks.onPeerDisconnected()
                updateState(TunnelState.Disconnected)
                // Desktop went offline — proactively close WS and reconnect
                webSocket?.close(1000, "peer_disconnected")
                webSocket = null
                scheduleReconnect()
            }
            is ControlMessage.PeerTimeout -> {
                updateState(TunnelState.PeerTimeout)
                callbacks.onPeerTimeout()
                // peer_timeout means desktop offline > 5 min, don't auto-reconnect
                manuallyClosed = true
            }
            is ControlMessage.Error -> {
                callbacks.onError(msg.message)
                updateState(TunnelState.Error(msg.message))
            }
            is ControlMessage.Unknown -> {
                // Ignore unknown control messages
            }
        }
    }

    private fun handleDisconnect(reason: String) {
        webSocket = null
        if (manuallyClosed) {
            updateState(TunnelState.Disconnected)
            return
        }
        updateState(TunnelState.Disconnected)
        callbacks.onError(reason)
        // Auto-reconnect with exponential backoff (1s → 30s cap)
        scheduleReconnect()
    }

    private fun scheduleReconnect() {
        if (manuallyClosed) return
        // Cancel any pending reconnect to avoid duplicate concurrent reconnects
        // (e.g. peer_disconnected closes WS → onClosed also fires → scheduleReconnect twice)
        reconnectJob?.cancel()
        reconnectJob = scope.launch {
            delay(backoffMs)
            backoffMs = (backoffMs * 2).coerceAtMost(30_000)
            if (!manuallyClosed) {
                connectOnce()
            }
        }
    }

    private fun updateState(newState: TunnelState) {
        _state = newState
    }
}

// === SECTION 3 END ===

/**
 * Manages multiple tunnel connections (one per pairing_id).
 *
 * Each pairing has its own TunnelConnection with independent WebSocket,
 * encryption, and reconnection. This supports the multi-desktop scenario
 * where a phone is paired with multiple desktops.
 */
class TunnelManager(
    private val scope: CoroutineScope = CoroutineScope(Dispatchers.IO + SupervisorJob()),
) {
    private val client: OkHttpClient = OkHttpClient.Builder()
        .pingInterval(30, TimeUnit.SECONDS)
        .connectTimeout(10, TimeUnit.SECONDS)
        .readTimeout(0, TimeUnit.SECONDS) // long-lived connection
        .build()

    private val connections = ConcurrentHashMap<String, TunnelConnection>()

    /**
     * Get or create a tunnel connection for a pairing_id.
     */
    fun getOrCreate(config: TunnelConfig, callbacks: TunnelCallbacks): TunnelConnection {
        return connections.computeIfAbsent(config.pairingId) {
            TunnelConnection(config, callbacks, client, scope)
        }
    }

    /**
     * Get an existing connection by pairing_id.
     */
    fun getConnection(pairingId: String): TunnelConnection? = connections[pairingId]

    /**
     * Close and remove a connection by pairing_id.
     */
    fun close(pairingId: String) {
        connections.remove(pairingId)?.close()
    }

    /**
     * Close all connections.
     */
    fun closeAll() {
        connections.values.forEach { it.close() }
        connections.clear()
    }
}


