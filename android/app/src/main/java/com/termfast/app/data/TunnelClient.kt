package com.termfast.app.data

import kotlinx.coroutines.*
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
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
    data class Error(val message: String, val code: Int = 0) : ControlMessage()
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
                message = json["message"]?.jsonPrimitive?.content ?: "unknown error",
                code = json["code"]?.jsonPrimitive?.intOrNull ?: 0,
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
    var pairingJwt: String,
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
    private val jwtRefresher: (suspend () -> String?)? = null,
) {
    @Volatile
    private var webSocket: WebSocket? = null
    @Volatile
    private var _state: TunnelState = TunnelState.Disconnected
    val state: TunnelState get() = _state

    private var reconnectJob: Job? = null
    @Volatile
    private var backoffMs: Long = 1000
    @Volatile
    private var manuallyClosed = false
    @Volatile
    private var connecting = false
    // Generation counter: each forceConnect/connectOnce increments this.
    // Callbacks from old connections check their generation and bail out
    // if it's stale, preventing concurrent reconnect loops.
    @Volatile
    private var generation: Int = 0

    /**
     * Start connecting. Auto-reconnects on disconnect with exponential backoff.
     * Safe to call multiple times — duplicate calls are ignored.
     */
    fun connect() {
        if (connecting) return
        scope.launch {
            if (connecting) return@launch
            connecting = true
            manuallyClosed = false
            backoffMs = 1000
            connectOnce()
        }
    }

    /**
     * Force reconnect: close any existing connection and start fresh.
     * Used when the tunnel manager needs to retry after an error state.
     */
    fun forceConnect() {
        scope.launch {
            // Increment generation to invalidate all callbacks from old connections
            generation++
            manuallyClosed = true
            connecting = false
            reconnectJob?.cancel()
            webSocket?.close(1000, "force reconnect")
            webSocket = null
            // Small delay to let old callbacks fire and be suppressed by stale generation
            kotlinx.coroutines.delay(100)
            // Now reset for fresh connection
            manuallyClosed = false
            backoffMs = 1000
            connecting = true
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
            connecting = false
            reconnectJob?.cancel()
            webSocket?.close(1000, "client closing")
            webSocket = null
            updateState(TunnelState.Disconnected)
        }
    }

    private suspend fun connectOnce() {
        // Capture generation so callbacks from this connection can detect
        // if a newer forceConnect() has superseded them.
        val myGen = generation
        updateState(TunnelState.Connecting)

        val wsUrl = if (config.relayUrl.startsWith("ws://") || config.relayUrl.startsWith("wss://")) {
            // Already a WebSocket URL — ensure it has the /tunnel path
            if (config.relayUrl.contains("/tunnel")) {
                config.relayUrl
            } else {
                config.relayUrl + "/tunnel"
            }
        } else {
            config.relayUrl.replace("http", "ws") + "/tunnel"
        }
        val request = Request.Builder()
            .url(wsUrl)
            .header("Authorization", "Bearer ${config.pairingJwt}")
            .build()

        val connected = CompletableDeferred<Boolean>()

        val ws = client.newWebSocket(request, object : WebSocketListener() {
            override fun onOpen(webSocket: WebSocket, response: Response) {
                if (myGen != generation) return
                // Send connect control message.
                // Security: build JSON with kotlinx.serialization to ensure
                // pairingId is properly escaped (defense-in-depth; pairingId
                // is normally a UUID, but be safe against injection).
                // Note: avoid org.json.JSONObject — its put() returns null in
                // unit tests (stub), causing NPE on chained calls.
                val connectMsg = buildJsonObject {
                    put("type", "connect")
                    put("pairing_id", config.pairingId)
                    put("role", "mobile")
                }.toString()
                webSocket.send(connectMsg)
                updateState(TunnelState.WaitingForPeer)
                connected.complete(true)
            }

            override fun onMessage(webSocket: WebSocket, text: String) {
                if (myGen != generation) return
                handleControlMessage(text, myGen)
            }

            override fun onMessage(webSocket: WebSocket, bytes: ByteString) {
                if (myGen != generation) return
                callbacks.onBinaryFrame(bytes.toByteArray())
            }

            override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
                if (myGen != generation) return
                handleDisconnect("closed: $code $reason", myGen)
            }

            override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                if (myGen != generation) return
                val code = response?.code
                android.util.Log.e("TunnelClient", "onFailure: code=$code, throwable=${t.javaClass.name}: ${t.message}, response=${response?.message}")
                if (code == 401) {
                    // JWT invalid/expired — try to refresh before giving up
                    if (jwtRefresher != null) {
                        scope.launch {
                            if (myGen != generation) return@launch
                            android.util.Log.i("TunnelClient", "401 received, attempting JWT refresh")
                            val newJwt = try { jwtRefresher() } catch (_: Exception) { null }
                            if (newJwt != null && myGen == generation) {
                                android.util.Log.i("TunnelClient", "JWT refresh succeeded, reconnecting")
                                config.pairingJwt = newJwt
                                connecting = false
                                backoffMs = 1000
                                scheduleReconnect(myGen)
                            } else if (myGen == generation) {
                                android.util.Log.w("TunnelClient", "JWT refresh failed, giving up")
                                manuallyClosed = true
                                connecting = false
                                updateState(TunnelState.Error("authentication failed (401)"))
                                callbacks.onError("authentication failed (401)")
                            }
                        }
                        return
                    }
                    // No refresher — stop reconnecting
                    manuallyClosed = true
                    connecting = false
                    updateState(TunnelState.Error("authentication failed (401)"))
                    callbacks.onError("authentication failed (401)")
                    return
                }
                if (code == 429) {
                    // Rate limited — use longer backoff before retrying
                    callbacks.onError("too many connections (429), retrying in 60s")
                    updateState(TunnelState.Error("too many connections (429)"))
                    backoffMs = 60_000
                    connecting = false
                    scheduleReconnect(myGen)
                    return
                }
                // DNS resolution failure (common right after screen unlock —
                // network stack not fully ready). Use short backoff and don't
                // show error state; just silently retry.
                val isDnsError = t is java.net.UnknownHostException ||
                    t.message?.contains("Unable to resolve host") == true
                if (isDnsError) {
                    android.util.Log.i("TunnelClient", "DNS not ready, retrying in 500ms")
                    backoffMs = 500
                    connecting = false
                    // Keep state as Connecting (not Error) so UI doesn't show error
                    updateState(TunnelState.Connecting)
                    scheduleReconnect(myGen)
                    return
                }
                val errMsg = "${t.javaClass.simpleName}: ${t.message ?: "unknown"} (code=$code)"
                handleDisconnect("failure: $errMsg", myGen)
            }
        })

        webSocket = ws

        // Wait for connection to establish
        val success = withTimeoutOrNull(10_000) { connected.await() }
        if (success == null) {
            if (myGen != generation) return
            handleDisconnect("connection timeout", myGen)
        }
    }

// === SECTION 2 END ===

    private fun handleControlMessage(text: String, myGen: Int) {
        if (myGen != generation) return
        when (val msg = parseControlMessage(text)) {
            is ControlMessage.PeerConnected -> {
                updateState(TunnelState.Connected)
                backoffMs = 1000 // reset backoff on successful peer connection
                callbacks.onPeerConnected()
            }
            is ControlMessage.PeerDisconnected -> {
                callbacks.onPeerDisconnected()
                updateState(TunnelState.Disconnected)
                connecting = false
                // Desktop went offline — proactively close WS and reconnect
                webSocket?.close(1000, "peer_disconnected")
                webSocket = null
                scheduleReconnect(myGen)
            }
            is ControlMessage.PeerTimeout -> {
                updateState(TunnelState.PeerTimeout)
                callbacks.onPeerTimeout()
                connecting = false
                // peer_timeout means desktop offline > 5 min, don't auto-reconnect
                manuallyClosed = true
            }
            is ControlMessage.Error -> {
                callbacks.onError(msg.message)
                updateState(TunnelState.Error(msg.message))
                connecting = false
                // desktop_offline (code 4030): desktop is not connected to relay,
                // no point retrying immediately — stop auto-reconnect.
                if (msg.code == 4030 || msg.message.contains("desktop_offline")) {
                    manuallyClosed = true
                }
                // pairing revoked: desktop revoked the pairing, stop reconnecting.
                if (msg.message.contains("pairing revoked") || msg.message.contains("pairing not found")) {
                    manuallyClosed = true
                }
            }
            is ControlMessage.Unknown -> {
                // Ignore unknown control messages
            }
        }
    }

    private fun handleDisconnect(reason: String, myGen: Int) {
        if (myGen != generation) return
        webSocket = null
        connecting = false
        if (manuallyClosed) {
            updateState(TunnelState.Disconnected)
            return
        }
        updateState(TunnelState.Disconnected)
        callbacks.onError(reason)
        // Auto-reconnect with exponential backoff (1s → 30s cap)
        scheduleReconnect(myGen)
    }

    private fun scheduleReconnect(myGen: Int) {
        if (manuallyClosed) return
        if (myGen != generation) return
        // Cancel any pending reconnect to avoid duplicate concurrent reconnects
        // (e.g. peer_disconnected closes WS → onClosed also fires → scheduleReconnect twice)
        reconnectJob?.cancel()
        reconnectJob = scope.launch {
            delay(backoffMs)
            backoffMs = (backoffMs * 2).coerceAtMost(30_000)
            // Bail out if a newer connection has superseded us, or we were closed
            if (manuallyClosed || myGen != generation) return@launch
            connecting = true
            connectOnce()
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
    fun getOrCreate(config: TunnelConfig, callbacks: TunnelCallbacks, jwtRefresher: (suspend () -> String?)? = null): TunnelConnection {
        return connections.computeIfAbsent(config.pairingId) {
            TunnelConnection(config, callbacks, client, scope, jwtRefresher)
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


