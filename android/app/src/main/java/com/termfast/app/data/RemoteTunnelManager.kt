package com.termfast.app.data

import com.termfast.app.ui.screen.TerminalSessionManager
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/**
 * FFI backend interface for remote tunnel frame operations.
 * Default implementation delegates to RustRepository (JNI → Rust).
 * Tests can provide a mock implementation to verify state transitions
 * without loading the native library.
 */
interface RemoteTunnelFfi {
    /** Initialize tunnel: generate + encrypt HELLO. Returns ciphertext bytes. */
    fun init(pairingId: String, pairingKey: ByteArray): ByteArray

    /** Process inbound binary frame (decrypt + dispatch events). */
    fun onBinary(pairingId: String, data: ByteArray)

    /** Create + encrypt LIST_REQUEST. Returns ciphertext or null on error. */
    fun sendListRequest(pairingId: String): ByteArray?

    /** Create + encrypt SUBSCRIBE. Returns ciphertext or null on error. */
    fun subscribe(pairingId: String, terminalId: Int): ByteArray?

    /** Create + encrypt UNSUBSCRIBE. Returns ciphertext or null on error. */
    fun unsubscribe(pairingId: String, terminalId: Int): ByteArray?

    /** Create + encrypt INPUT. Returns ciphertext or null on error. */
    fun sendInput(pairingId: String, terminalId: Int, data: ByteArray): ByteArray?

    /** Create + encrypt RESIZE. Returns ciphertext or null on error. */
    fun sendResize(pairingId: String, terminalId: Int, cols: Int, rows: Int): ByteArray?

    /** Create + encrypt DESKTOP_PAIR frame. Returns ciphertext or null on error. */
    fun sendDesktopPair(pairingId: String, payloadJson: String): ByteArray?

    /** Create + encrypt NEW_TERMINAL frame. Returns ciphertext or null on error.
     *  serverId: empty = local terminal, otherwise SSH terminal on that server. */
    fun sendNewTerminal(pairingId: String, shell: String, name: String, serverId: String): ByteArray?

    /** Create + encrypt CLOSE_TERMINAL frame. Returns ciphertext or null on error. */
    fun sendCloseTerminal(pairingId: String, terminalId: Int): ByteArray?

    /** Create + encrypt TRIGGER_LIST_REQUEST. Returns ciphertext or null on error. */
    fun sendTriggerListRequest(pairingId: String): ByteArray?

    /** Create + encrypt TRIGGER_EXEC. Returns ciphertext or null on error. */
    fun sendTriggerExec(pairingId: String, triggerJson: String): ByteArray?

    /** Create + encrypt TRIGGER_ADD. Returns ciphertext or null on error. */
    fun sendTriggerAdd(pairingId: String, triggerJson: String): ByteArray?

    /** Create + encrypt TRIGGER_UPDATE. Returns ciphertext or null on error. */
    fun sendTriggerUpdate(pairingId: String, triggerJson: String): ByteArray?

    /** Create + encrypt TRIGGER_REMOVE. Returns ciphertext or null on error. */
    fun sendTriggerRemove(pairingId: String, triggerJson: String): ByteArray?

    /** Close tunnel: send GOODBYE + remove session. Returns GOODBYE ciphertext or null. */
    fun close(pairingId: String): ByteArray?
}

/**
 * Default FFI backend: delegates to RustRepository (JNI → Rust).
 */
object DefaultRemoteTunnelFfi : RemoteTunnelFfi {
    override fun init(pairingId: String, pairingKey: ByteArray): ByteArray =
        RustRepository.remoteTunnelInit(pairingId, pairingKey)

    override fun onBinary(pairingId: String, data: ByteArray) {
        RustRepository.remoteTunnelOnBinary(pairingId, data)
    }

    override fun sendListRequest(pairingId: String): ByteArray? =
        RustRepository.remoteTunnelSendListRequest(pairingId)

    override fun subscribe(pairingId: String, terminalId: Int): ByteArray? =
        RustRepository.remoteTunnelSubscribe(pairingId, terminalId)

    override fun unsubscribe(pairingId: String, terminalId: Int): ByteArray? =
        RustRepository.remoteTunnelUnsubscribe(pairingId, terminalId)

    override fun sendInput(pairingId: String, terminalId: Int, data: ByteArray): ByteArray? =
        RustRepository.remoteTunnelSendInput(pairingId, terminalId, data)

    override fun sendResize(pairingId: String, terminalId: Int, cols: Int, rows: Int): ByteArray? =
        RustRepository.remoteTunnelSendResize(pairingId, terminalId, cols, rows)

    override fun sendDesktopPair(pairingId: String, payloadJson: String): ByteArray? =
        RustRepository.remoteTunnelSendDesktopPair(pairingId, payloadJson)

    override fun sendNewTerminal(pairingId: String, shell: String, name: String, serverId: String): ByteArray? =
        RustRepository.remoteTunnelSendNewTerminal(pairingId, shell, name, serverId)

    override fun sendCloseTerminal(pairingId: String, terminalId: Int): ByteArray? =
        RustRepository.remoteTunnelSendCloseTerminal(pairingId, terminalId)

    override fun sendTriggerListRequest(pairingId: String): ByteArray? =
        RustRepository.remoteTunnelSendTriggerListRequest(pairingId)

    override fun sendTriggerExec(pairingId: String, triggerJson: String): ByteArray? =
        RustRepository.remoteTunnelSendTriggerExec(pairingId, triggerJson)

    override fun sendTriggerAdd(pairingId: String, triggerJson: String): ByteArray? =
        RustRepository.remoteTunnelSendTriggerAdd(pairingId, triggerJson)

    override fun sendTriggerUpdate(pairingId: String, triggerJson: String): ByteArray? =
        RustRepository.remoteTunnelSendTriggerUpdate(pairingId, triggerJson)

    override fun sendTriggerRemove(pairingId: String, triggerJson: String): ByteArray? =
        RustRepository.remoteTunnelSendTriggerRemove(pairingId, triggerJson)

    override fun close(pairingId: String): ByteArray? =
        RustRepository.remoteTunnelClose(pairingId)
}

// === SECTION 1 END ===

/**
 * Manages a remote terminal tunnel: WebSocket transport (TunnelClient) +
 * frame crypto/protocol (Rust FFI remote_terminal module).
 *
 * Lifecycle:
 * 1. start() → TunnelClient connects WebSocket, waits for peer_connected
 * 2. onPeerConnected → Rust FFI init_tunnel → send encrypted HELLO via WebSocket
 * 3. onBinaryFrame → Rust FFI process_binary → events dispatched via RustRepository.events
 * 4. RemoteTunnelReady event → session key established, can send LIST/SUBSCRIBE/INPUT
 * 5. stop() → send GOODBYE via Rust FFI → close WebSocket
 *
 * Reconnection: TunnelClient auto-reconnects on disconnect. After peer_connected,
 * init_tunnel is called again to generate a new HELLO with fresh client_random.
 *
 * @param ffi FFI backend (default: DefaultRemoteTunnelFfi → RustRepository).
 *   Tests inject a mock to verify state transitions without JNI.
 */
class RemoteTunnelManager(
    private val pairingId: String,
    private val pairingKey: ByteArray,
    private val relayUrl: String,
    private val pairingJwt: String,
    private val scope: CoroutineScope = CoroutineScope(Dispatchers.IO + SupervisorJob()),
    private val ffi: RemoteTunnelFfi = DefaultRemoteTunnelFfi,
    private val pairingRefreshToken: String = "",
) {
    private val tunnelManager = TunnelManager(scope)

    /** Tunnel transport state (WebSocket + peer connection). */
    private val _transportState = MutableStateFlow<TunnelState>(TunnelState.Disconnected)
    val transportState: StateFlow<TunnelState> = _transportState.asStateFlow()

    /** Protocol state (HELLO exchange complete, session key established). */
    private val _protocolReady = MutableStateFlow(false)
    val protocolReady: StateFlow<Boolean> = _protocolReady.asStateFlow()

    /** Exposed for tests: callbacks object to simulate tunnel events. */
    internal val testCallbacks: TunnelCallbacks
        get() = callbacks

    private val callbacks = object : TunnelCallbacks {
        override fun onPeerConnected() {
            android.util.Log.i("RemoteTunnel", "onPeerConnected, sending HELLO")
            _transportState.value = TunnelState.Connected
            // Peer connected → init tunnel (generate + send encrypted HELLO)
            sendHello()
        }

        override fun onPeerDisconnected() {
            android.util.Log.i("RemoteTunnel", "onPeerDisconnected")
            _transportState.value = TunnelState.Disconnected
            _protocolReady.value = false
            // Mark all remote sessions for this pairing as disconnected,
            // so the terminal list shows them as offline.
            TerminalSessionManager.markRemoteSessionsDisconnected(pairingId)
        }

        override fun onPeerTimeout() {
            _transportState.value = TunnelState.PeerTimeout
            _protocolReady.value = false
        }

        override fun onBinaryFrame(data: ByteArray) {
            // Binary frame from relay → Rust FFI decrypts + dispatches events
            try {
                ffi.onBinary(pairingId, data)
            } catch (e: Exception) {
                android.util.Log.e("RemoteTunnel", "onBinaryFrame: FFI error", e)
            }
        }

        override fun onError(message: String) {
            android.util.Log.e("RemoteTunnel", "onError: $message")
            // "closed: 1000" is a normal disconnect (e.g. desktop went offline,
            // relay closed the pipe). Don't show as error — auto-reconnect
            // will retry silently. Only set Error state for real failures.
            if (message.startsWith("closed: 1000")) {
                _transportState.value = TunnelState.Disconnected
            } else if (message.contains("pairing revoked") || message.contains("pairing not found")) {
                // Pairing was revoked by the desktop — stop reconnecting and
                // set a special error so UI can remove it from PairingStore.
                android.util.Log.i("RemoteTunnel", "pairing revoked, stopping reconnection")
                _transportState.value = TunnelState.Error("pairing_revoked")
            } else if (message.contains("desktop_offline")) {
                _transportState.value = TunnelState.Error(message)
            } else {
                // Don't let follow-up socket errors (EOFException, etc.) overwrite
                // a desktop_offline error — they're a side effect of the relay
                // closing the connection after sending desktop_offline.
                val current = _transportState.value
                if (current is TunnelState.Error && current.message.contains("desktop_offline")) {
                    android.util.Log.i("RemoteTunnel", "ignoring follow-up error: $message")
                } else {
                    _transportState.value = TunnelState.Error(message)
                }
            }
            _protocolReady.value = false
        }
    }

    /**
     * Start the tunnel: connect WebSocket and wait for peer_connected.
     *
     * If the tunnel is already connected (Connected/WaitingForPeer/Connecting),
     * the existing connection is reused — forceConnect() is NOT called to
     * avoid creating a new WebSocket that kicks the desktop's peer.
     * If the protocol is already ready (HELLO exchange done), a fresh
     * LIST_REQUEST is sent so the caller gets the terminal list.
     * If the protocol is not ready but the transport is connected, a fresh
     * HELLO is sent to re-establish the encrypted session.
     */
    fun start() {
        val config = TunnelConfig(
            relayUrl = relayUrl,
            pairingJwt = pairingJwt,
            pairingId = pairingId,
        )
        // JWT refresher: called by TunnelConnection on 401 to auto-refresh
        // the pairing JWT via /auth/refresh-pairing. On success, the new JWT
        // is persisted to PairingStore so future connections use it directly.
        val refresher: (suspend () -> String?)? = if (pairingRefreshToken.isNotEmpty()) {
            {
                val newJwt = PairingApi.refreshPairingJwt(pairingRefreshToken)
                if (newJwt != null) {
                    PairingStore.updatePairingJwt(pairingId, newJwt)
                }
                newJwt
            }
        } else null
        val conn = tunnelManager.getOrCreate(config, callbacks, refresher)
        val currentState = conn.state
        if (currentState is TunnelState.Connected) {
            // Transport connected — reuse it.
            if (_protocolReady.value) {
                // Protocol already ready — just request a fresh list.
                sendListRequest()
            } else {
                // Protocol not ready (e.g. HELLO was lost) — re-send HELLO
                // to re-establish the encrypted session.
                sendHello()
            }
            return
        }
        if (currentState is TunnelState.WaitingForPeer ||
            currentState is TunnelState.Connecting
        ) {
            // Connection in progress — wait for it.
            return
        }
        // Disconnected or Error — clean up stale FFI state before reconnecting.
        // Without this, the Rust-side tunnel session retains old encryption
        // keys and the HELLO exchange silently fails after reconnection.
        try {
            ffi.close(pairingId)
        } catch (_: Exception) {}
        _transportState.value = TunnelState.Connecting
        _protocolReady.value = false
        conn.forceConnect()
    }

    /**
     * Stop the tunnel: send GOODBYE and close WebSocket.
     * The pairing key is NOT zeroized here — the manager instance may be
     * reused (it stays in TerminalSessionManager's map). Key zeroization
     * happens in [stopAndDestroy] when the manager is permanently removed.
     */
    fun stop() {
        // Send GOODBYE via FFI (best-effort, ignore errors)
        try {
            val goodbyeCt = ffi.close(pairingId)
            goodbyeCt?.let { sendRaw(it) }
        } catch (_: Exception) {
        }
        try {
            tunnelManager.close(pairingId)
        } catch (_: Exception) {
        }
        _protocolReady.value = false
        _transportState.value = TunnelState.Disconnected
    }

    /**
     * Stop the tunnel AND zeroize the pairing key.
     * Called when the manager is permanently removed from the registry
     * (no more remote sessions for this pairing).
     */
    fun stopAndDestroy() {
        stop()
        // Zeroize the pairing key to prevent residual secret in memory
        java.util.Arrays.fill(pairingKey, 0)
    }

    /**
     * Send a LIST_REQUEST frame. Only valid after protocolReady == true.
     * Returns true if the frame was sent successfully.
     */
    fun sendListRequest(): Boolean {
        if (!_protocolReady.value) return false
        val ct = ffi.sendListRequest(pairingId) ?: return false
        return sendRaw(ct)
    }

    /**
     * Send a SUBSCRIBE frame for a terminal. Only valid after protocolReady.
     */
    fun sendSubscribe(terminalId: Int): Boolean {
        if (!_protocolReady.value) return false
        val ct = ffi.subscribe(pairingId, terminalId) ?: return false
        return sendRaw(ct)
    }

    /**
     * Send an UNSUBSCRIBE frame for a terminal.
     */
    fun sendUnsubscribe(terminalId: Int): Boolean {
        if (!_protocolReady.value) return false
        val ct = ffi.unsubscribe(pairingId, terminalId) ?: return false
        return sendRaw(ct)
    }

    /**
     * Send user input (keystrokes) to a terminal.
     */
    fun sendInput(terminalId: Int, data: ByteArray): Boolean {
        if (!_protocolReady.value) return false
        val ct = ffi.sendInput(pairingId, terminalId, data) ?: return false
        return sendRaw(ct)
    }

    /**
     * Send a RESIZE frame (notify desktop of mobile terminal size).
     */
    fun sendResize(terminalId: Int, cols: Int, rows: Int): Boolean {
        if (!_protocolReady.value) return false
        val ct = ffi.sendResize(pairingId, terminalId, cols, rows) ?: return false
        return sendRaw(ct)
    }

    /**
     * Send a DESKTOP_PAIR frame to instruct this desktop to start a
     * desktop-to-desktop pairing. Only valid after protocolReady == true.
     * `payloadJson` is the JSON-encoded DesktopPairMessage.
     */
    fun sendDesktopPair(payloadJson: String): Boolean {
        if (!_protocolReady.value) return false
        val ct = ffi.sendDesktopPair(pairingId, payloadJson) ?: return false
        return sendRaw(ct)
    }

    /**
     * Send a NEW_TERMINAL frame to ask the desktop to open a new terminal.
     * Only valid after protocolReady == true.
     * shell/name are optional (empty = desktop default).
     * serverId: empty = local terminal, otherwise SSH terminal on that server.
     */
    fun sendNewTerminal(shell: String = "", name: String = "", serverId: String = ""): Boolean {
        if (!_protocolReady.value) return false
        val ct = ffi.sendNewTerminal(pairingId, shell, name, serverId) ?: return false
        return sendRaw(ct)
    }

    /**
     * Send a CLOSE_TERMINAL frame to ask the desktop to close (kill) a terminal.
     * Only valid after protocolReady == true.
     */
    fun sendCloseTerminal(terminalId: Int): Boolean {
        if (!_protocolReady.value) return false
        val ct = ffi.sendCloseTerminal(pairingId, terminalId) ?: return false
        return sendRaw(ct)
    }

    // === Remote trigger management ===

    fun sendTriggerListRequest(): Boolean {
        if (!_protocolReady.value) return false
        val ct = ffi.sendTriggerListRequest(pairingId) ?: return false
        return sendRaw(ct)
    }

    fun sendTriggerExec(triggerJson: String): Boolean {
        if (!_protocolReady.value) return false
        val ct = ffi.sendTriggerExec(pairingId, triggerJson) ?: return false
        return sendRaw(ct)
    }

    fun sendTriggerAdd(triggerJson: String): Boolean {
        if (!_protocolReady.value) return false
        val ct = ffi.sendTriggerAdd(pairingId, triggerJson) ?: return false
        return sendRaw(ct)
    }

    fun sendTriggerUpdate(triggerJson: String): Boolean {
        if (!_protocolReady.value) return false
        val ct = ffi.sendTriggerUpdate(pairingId, triggerJson) ?: return false
        return sendRaw(ct)
    }

    fun sendTriggerRemove(triggerJson: String): Boolean {
        if (!_protocolReady.value) return false
        val ct = ffi.sendTriggerRemove(pairingId, triggerJson) ?: return false
        return sendRaw(ct)
    }

    /**
     * Called when RemoteTunnelReady event is received from Rust FFI.
     * Marks protocol as ready and triggers LIST_REQUEST.
     */
    fun onProtocolReady() {
        android.util.Log.i("RemoteTunnel", "onProtocolReady: pairingId=$pairingId")
        _protocolReady.value = true
        // Mark all remote sessions for this pairing as connected again
        TerminalSessionManager.markRemoteSessionsConnected(pairingId)
    }

    /**
     * Called when RemoteTerminalError event is received.
     * Resets protocol state (e.g. invalid_terminal_id error).
     */
    fun onProtocolError() {
        // Keep protocol ready for error frames that don't invalidate the session
        // (e.g. "invalid_terminal_id" means the SUBSCRIBE failed, not the tunnel)
    }

    // === Internal helpers ===

    private fun sendHello() {
        try {
            android.util.Log.i("RemoteTunnel", "sendHello: init FFI, pairingId=$pairingId, keyLen=${pairingKey.size}")
            val helloCt = ffi.init(pairingId, pairingKey)
            android.util.Log.i("RemoteTunnel", "sendHello: got ciphertext ${helloCt.size} bytes, sending")
            val sent = sendRaw(helloCt)
            android.util.Log.i("RemoteTunnel", "sendHello: sendRaw result=$sent")
        } catch (e: Exception) {
            android.util.Log.e("RemoteTunnel", "sendHello: failed", e)
            _transportState.value = TunnelState.Error("HELLO init failed: ${e.message}")
        }
    }

    /** Exposed for tests: send raw bytes via tunnel (returns false if no connection). */
    internal fun sendRawInternal(data: ByteArray): Boolean = sendRaw(data)

    private fun sendRaw(data: ByteArray): Boolean {
        val conn = tunnelManager.getConnection(pairingId) ?: return false
        return conn.sendBinary(data)
    }
}

// === SECTION 2 END ===
