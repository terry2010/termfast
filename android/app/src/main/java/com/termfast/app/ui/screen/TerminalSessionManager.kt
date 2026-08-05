package com.termfast.app.ui.screen

import android.util.Base64
import androidx.compose.ui.graphics.Color
import com.termfast.app.data.RustEvent
import com.termfast.app.data.RustRepository
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.GlobalScope
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.connectbot.terminal.TerminalEmulator
import org.connectbot.terminal.TerminalEmulatorFactory
import java.util.UUID

/**
 * Manages terminal sessions per server.
 * Each server can have multiple active sessions.
 * Sessions are reused across screen recompositions — when the user navigates
 * away from the terminal screen and comes back, the same session is restored
 * with its output history.
 */
object TerminalSessionManager {
    private val sessions = mutableMapOf<String, SessionState>()
    private var collectorStarted = false
    // Tunnel managers registered by pairingId — used for UNSUBSCRIBE on disconnect
    private val tunnelManagers = mutableMapOf<String, com.termfast.app.data.RemoteTunnelManager>()

    // Regex to strip ANSI escape codes:
    // - CSI: \x1b[?...letter (colors, cursor movement, private modes like ?2004h)
    // - OSC: \x1b]...BEL or \x1b]...\x1b\\ (window title: ]0;root)
    // - Other ESC sequences: \x1b + single char
    private val ansiRegex = Regex(
        "\u001B\\[[0-9;?]*[a-zA-Z]" +          // CSI sequences (incl. private ?2004h etc.)
        "|\u001B\\][^\u0007\u001B]*(\u0007|\u001B\\\\)" + // OSC sequences (terminated by BEL or ST)
        "|\u001B[()][0-9A-Za-z]" +            // Charset designation
        "|\u001B[=>]" +                        // Keypad mode
        "|\u001B[@-Z\\-_]"                     // Other 2-char ESC sequences
    )

    fun stripAnsi(text: String): String {
        return ansiRegex.replace(text, "")
    }

    /**
     * Process raw terminal data into display lines.
     * \r (carriage return) is ignored — in real terminals it just moves cursor
     * to start of line, and \r\n is the normal line ending. Stripping \r
     * makes \r\n behave as a simple newline.
     */
    fun processToLines(raw: String): List<String> {
        val clean = stripAnsi(raw).replace("\r", "")
        return clean.split("\n")
    }

    data class SessionState(
        val sessionId: String,
        val serverId: String,
        val emulator: TerminalEmulator? = null,
        val connected: Boolean = false,
        val createdAt: Long = System.currentTimeMillis(),
        val name: String = "",
        // Preview cache for TerminalsScreen card — approximate plain-text
        // extracted from raw PTY data via stripAnsi(). Not used for rendering.
        val previewCache: String = "",
        // tmux session name if this terminal is attached to a tmux session
        val tmuxSessionName: String? = null,
        // --- Remote terminal fields ---
        // Non-null when this is a remote terminal session (data from relay tunnel).
        val remotePairingId: String? = null,
        // u32 terminal_id from desktop protocol server (valid when remotePairingId != null)
        val remoteTerminalId: Int = 0,
    )

    /** Whether this session is a remote terminal (vs. local SSH). */
    fun isRemoteSession(sessionId: String): Boolean {
        return sessions[sessionId]?.remotePairingId != null
    }

    @Synchronized
    fun getOrCreateSession(serverId: String): String {
        val sessionId = UUID.randomUUID().toString()
        sessions[sessionId] = SessionState(
            sessionId = sessionId,
            serverId = serverId,
            emulator = createEmulator(sessionId),
        )
        return sessionId
    }

    @Synchronized
    fun getOrCreateSessionById(serverId: String, sessionId: String): String {
        val existing = sessions[sessionId]
        if (existing != null) return sessionId
        sessions[sessionId] = SessionState(
            sessionId = sessionId,
            serverId = serverId,
            emulator = createEmulator(sessionId),
        )
        return sessionId
    }

    /**
     * Create or get a remote terminal session.
     *
     * Remote sessions are identified by pairingId + terminalId (u32 from desktop
     * protocol server). The sessionId is a synthetic UUID used as a key in the
     * sessions map. Input from the emulator is sent via RemoteTunnelManager
     * (WebSocket + Rust FFI) instead of the local SSH PTY path.
     *
     * @param pairingId Pairing ID for the tunnel
     * @param terminalId u32 terminal_id from LIST_RESPONSE
     * @param tunnelManager Tunnel manager for sending input/resize frames
     * @param name Display name (from LIST_RESPONSE)
     * @return sessionId (synthetic UUID)
     */
    @Synchronized
    fun getOrCreateRemoteSession(
        pairingId: String,
        terminalId: Int,
        tunnelManager: com.termfast.app.data.RemoteTunnelManager,
        name: String,
    ): String {
        // Register tunnel manager for this pairing (used by disconnectSession)
        tunnelManagers[pairingId] = tunnelManager
        // Reuse existing session if same pairingId + terminalId
        val existing = sessions.values.firstOrNull {
            it.remotePairingId == pairingId && it.remoteTerminalId == terminalId
        }
        if (existing != null) return existing.sessionId

        val sessionId = UUID.randomUUID().toString()
        sessions[sessionId] = SessionState(
            sessionId = sessionId,
            serverId = "remote:$pairingId",  // synthetic serverId for remote
            emulator = createRemoteEmulator(sessionId, terminalId, tunnelManager),
            connected = true,
            name = name,
            remotePairingId = pairingId,
            remoteTerminalId = terminalId,
        )
        return sessionId
    }

    /** Register a tunnel manager for a pairing (for UNSUBSCRIBE on disconnect). */
    @Synchronized
    fun registerTunnelManager(pairingId: String, manager: com.termfast.app.data.RemoteTunnelManager) {
        tunnelManagers[pairingId] = manager
    }

    /** Get or create a shared RemoteTunnelManager for a pairing_id. */
    @Synchronized
    fun getOrCreateTunnelManager(
        pairingId: String,
        pairingKey: ByteArray,
        relayUrl: String,
        pairingJwt: String,
    ): com.termfast.app.data.RemoteTunnelManager {
        val existing = tunnelManagers[pairingId]
        if (existing != null) return existing
        val manager = com.termfast.app.data.RemoteTunnelManager(
            pairingId, pairingKey, relayUrl, pairingJwt
        )
        tunnelManagers[pairingId] = manager
        return manager
    }

    /** Unregister a tunnel manager for a pairing. */
    @Synchronized
    fun unregisterTunnelManager(pairingId: String) {
        tunnelManagers.remove(pairingId)
    }

    /**
     * Test-only: create a remote session with a pre-built emulator (for testing
     * event routing without a real RemoteTunnelManager).
     */
    @Synchronized
    internal fun createRemoteSessionForTest(
        pairingId: String,
        terminalId: Int,
        emulator: TerminalEmulator?,
        sessionId: String = UUID.randomUUID().toString(),
        name: String = "test-remote",
    ): String {
        sessions[sessionId] = SessionState(
            sessionId = sessionId,
            serverId = "remote:$pairingId",
            emulator = emulator,
            connected = true,
            name = name,
            remotePairingId = pairingId,
            remoteTerminalId = terminalId,
        )
        return sessionId
    }

    /**
     * Find an existing remote session by pairingId + terminalId.
     * Used by getOrCreateRemoteSession for reuse logic; exposed for testing.
     */
    @Synchronized
    internal fun findRemoteSession(pairingId: String, terminalId: Int): String? {
        return sessions.values.firstOrNull {
            it.remotePairingId == pairingId && it.remoteTerminalId == terminalId
        }?.sessionId
    }

    /**
     * Create a termlib emulator for a remote terminal session.
     * Keyboard input and resize events are sent via RemoteTunnelManager
     * (encrypted INPUT/RESIZE frames through the WebSocket tunnel).
     */
    private fun createRemoteEmulator(
        sessionId: String,
        terminalId: Int,
        tunnelManager: com.termfast.app.data.RemoteTunnelManager,
    ): TerminalEmulator {
        return TerminalEmulatorFactory.create(
            initialRows = 24,
            initialCols = 80,
            defaultForeground = Color(0xFFCDD6F4),
            defaultBackground = Color(0xFF1E1E2E),
            onKeyboardInput = { bytes ->
                tunnelManager.sendInput(terminalId, bytes)
            },
            onResize = { dims ->
                android.util.Log.d("termfast", "REMOTE onResize callback: ${dims.columns}x${dims.rows} for session=$sessionId")
                tunnelManager.sendResize(terminalId, dims.columns, dims.rows)
            },
        )
    }

    /**
     * Create a termlib TerminalEmulator wired to send keyboard input to the
     * Rust PTY via RustRepository.writeTerminalBytes().
     *
     * ⚠️ Reentrancy: termlib docs warn "Callbacks must not call back into
     *   Terminal methods (causes deadlock)." We only call RustRepository
     *   (async JNI path) here, never emulator.writeInput(), so it's safe.
     */
    private fun createEmulator(sessionId: String): TerminalEmulator {
        return TerminalEmulatorFactory.create(
            initialRows = 24,
            initialCols = 80,
            defaultForeground = Color(0xFFCDD6F4),
            defaultBackground = Color(0xFF1E1E2E),
            onKeyboardInput = { bytes ->
                RustRepository.writeTerminalBytes(sessionId, bytes)
            },
            onResize = { dims ->
                android.util.Log.d("termfast", "onResize callback: ${dims.columns}x${dims.rows} for session=$sessionId")
                RustRepository.resizeTerminal(sessionId, dims.columns, dims.rows)
            },
        )
    }

    // Legacy stubs — kept for compilation during migration. Will be removed
    // after TerminalScreen.kt is migrated to termlib <Terminal> composable.
    @Synchronized
    fun getOutputBySession(sessionId: String): List<String> = emptyList()

    @Synchronized
    fun getCursorColBySession(sessionId: String): Int = 0

    @Synchronized
    fun updateOutputBySession(sessionId: String, output: List<String>) { }

    /** Get the termlib emulator for a session (null if not found). */
    @Synchronized
    fun getEmulatorBySession(sessionId: String): TerminalEmulator? =
        sessions[sessionId]?.emulator

    /** Get the preview text for TerminalsScreen card. */
    @Synchronized
    fun getPreviewBySession(sessionId: String): String =
        sessions[sessionId]?.previewCache ?: ""

    /** Apply a color scheme to all active terminal emulators. */
    @Synchronized
    fun applyThemeToAll(theme: com.termfast.app.ui.TerminalTheme) {
        sessions.values.forEach { state ->
            state.emulator?.applyColorScheme(
                theme.ansiColors,
                theme.foreground,
                theme.background,
            )
        }
    }

    // === Legacy ANSI parser — commented out, replaced by termlib libvterm ===
    // Kept for reference and for stripAnsi() used by preview cache.
    /*
    @Synchronized
    fun appendTerminalData(sessionId: String, raw: String) { ... }
    private sealed class CursorOp { ... }
    private fun parseAnsiCursor(raw: String, i: Int): Pair<Int, CursorOp?> { ... }
    */
    // === End legacy ANSI parser ===



    @Synchronized
    fun isConnectedBySession(sessionId: String): Boolean {
        return sessions[sessionId]?.connected ?: false
    }

    @Synchronized
    fun setConnectedBySession(sessionId: String, connected: Boolean) {
        val existing = sessions[sessionId] ?: return
        sessions[sessionId] = existing.copy(connected = connected)
    }

    @Synchronized
    fun setTmuxSessionName(sessionId: String, tmuxSessionName: String?) {
        val existing = sessions[sessionId] ?: return
        sessions[sessionId] = existing.copy(tmuxSessionName = tmuxSessionName)
    }

    /** Find an existing connected session attached to the given tmux session name. */
    @Synchronized
    fun findSessionByTmuxName(serverId: String, tmuxSessionName: String): SessionState? {
        return sessions.values.firstOrNull {
            it.serverId == serverId &&
            it.tmuxSessionName == tmuxSessionName &&
            it.connected
        }
    }

    @Synchronized
    fun renameSession(sessionId: String, name: String) {
        val existing = sessions[sessionId] ?: return
        sessions[sessionId] = existing.copy(name = name)
    }

    @Synchronized
    fun closeSessionBySessionId(sessionId: String) {
        sessions.remove(sessionId)
    }

    @Synchronized
    fun getSessions(serverId: String): List<SessionState> {
        return sessions.values.filter { it.serverId == serverId }.sortedBy { it.createdAt }
    }

    @Synchronized
    fun getAllSessions(): List<SessionState> {
        return sessions.values.sortedBy { it.createdAt }
    }

    @Synchronized
    fun hasSessions(serverId: String): Boolean {
        return sessions.values.any { it.serverId == serverId }
    }

    fun disconnectSession(sessionId: String) {
        val session = sessions[sessionId]
        if (session != null && session.remotePairingId != null) {
            // Remote session: send UNSUBSCRIBE via registered tunnel manager
            val tunnelManager = tunnelManagers[session.remotePairingId]
            tunnelManager?.sendUnsubscribe(session.remoteTerminalId)
            setConnectedBySession(sessionId, false)
        } else {
            // Local SSH session
            RustRepository.closeTerminal(sessionId)
            setConnectedBySession(sessionId, false)
        }
    }

    /**
     * Write keyboard input to a session (SSH or remote).
     * For SSH: writes to the PTY via RustRepository.
     * For remote: sends INPUT frame via RemoteTunnelManager.
     */
    fun writeToSession(sessionId: String, data: String) {
        val session = sessions[sessionId] ?: return
        if (session.remotePairingId != null) {
            // Remote: send via tunnel manager
            val tunnelManager = tunnelManagers[session.remotePairingId]
            tunnelManager?.sendInput(session.remoteTerminalId, data.toByteArray())
        } else {
            // SSH: write to PTY
            RustRepository.writeTerminal(sessionId, data)
        }
    }

    fun reconnectSession(serverId: String, sessionId: String, onResult: (Boolean) -> Unit) {
        RustRepository.closeTerminal(sessionId)
        setConnectedBySession(sessionId, false)
        GlobalScope.launch(Dispatchers.IO) {
            val status = RustRepository.getServerStatus(serverId)
            if (status.status != "connected") {
                val ok = RustRepository.connectServer(serverId)
                if (!ok) {
                    withContext(Dispatchers.Main) { onResult(false) }
                    return@launch
                }
            }
            val ok = RustRepository.openTerminal(serverId, sessionId, 80, 24)
            if (ok) setConnectedBySession(sessionId, true)
            withContext(Dispatchers.Main) { onResult(ok) }
        }
    }

    /**
     * Start a global event collector that keeps session state in sync
     * even when TerminalScreen is not visible. Call once at app startup.
     */
    fun startGlobalCollector() {
        if (collectorStarted) return
        collectorStarted = true
        GlobalScope.launch {
            RustRepository.events.collect { event ->
                handleEvent(event)
            }
        }
    }

    /**
     * Process a single RustEvent — handles both local and remote terminal events.
     * Extracted from startGlobalCollector for testability.
     */
    @Synchronized
    internal fun handleEvent(event: RustEvent) {
        when (event) {
            is RustEvent.TerminalData -> {
                val bytes = if (event.encoding == "base64") {
                    Base64.decode(event.data, Base64.DEFAULT)
                } else {
                    event.data.toByteArray()
                }
                val session = sessions[event.session_id]
                if (session != null) {
                    // Feed raw bytes to termlib — libvterm handles
                    // UTF-8 + ANSI parsing internally.
                    session.emulator?.writeInput(bytes)
                    // Update preview cache for TerminalsScreen card
                    val rawText = String(bytes, Charsets.UTF_8)
                    val previewText = stripAnsi(rawText).replace("\r", "").trim()
                    if (previewText.isNotBlank()) {
                        sessions[event.session_id] = session.copy(
                            previewCache = (session.previewCache + "\n" + previewText)
                                .lines()
                                .filter { it.isNotBlank() }
                                .takeLast(5)
                                .joinToString("\n")
                        )
                    }
                }
            }
            is RustEvent.TerminalClosed -> {
                setConnectedBySession(event.session_id, false)
            }
            is RustEvent.TerminalError -> {
                setConnectedBySession(event.session_id, false)
            }
            // --- Remote terminal events ---
            is RustEvent.RemoteTerminalOutput -> {
                handleRemoteOutput(event)
            }
            is RustEvent.RemoteTerminalHistory -> {
                handleRemoteHistory(event)
            }
            is RustEvent.RemoteTerminalResize -> {
                handleRemoteResize(event)
            }
            is RustEvent.RemoteTerminalError -> {
                handleRemoteError(event)
            }
            is RustEvent.RemoteTunnelReady -> {
                // Tunnel ready — handled by RemoteTunnelManager, nothing to do here
            }
            is RustEvent.RemoteTerminalList -> {
                // LIST_RESPONSE — handled by RemoteTerminalListScreen
            }
            else -> {}
        }
    }

    /**
     * Decode base64 string to bytes. Uses java.util.Base64 (works in both
     * Android runtime and unit tests; android.util.Base64 is stubbed in tests).
     */
    private fun decodeBase64(data: String): ByteArray =
        java.util.Base64.getMimeDecoder().decode(data)

    /**
     * Handle RemoteTerminalOutput: decode base64 data and write to emulator.
     */
    private fun handleRemoteOutput(event: RustEvent.RemoteTerminalOutput) {
        val session = sessions.values.firstOrNull {
            it.remotePairingId == event.pairing_id &&
            it.remoteTerminalId == event.terminal_id.toInt()
        }
        if (session != null && event.encoding == "base64") {
            val bytes = decodeBase64(event.data)
            session.emulator?.writeInput(bytes)
        }
    }

    /**
     * Handle RemoteTerminalHistory: on seq=0, clear screen; write data to emulator.
     */
    private fun handleRemoteHistory(event: RustEvent.RemoteTerminalHistory) {
        val session = sessions.values.firstOrNull {
            it.remotePairingId == event.pairing_id &&
            it.remoteTerminalId == event.terminal_id.toInt()
        }
        if (session != null && event.encoding == "base64") {
            val bytes = decodeBase64(event.data)
            // On first HISTORY chunk (seq=0), clear the emulator
            // to prepare for a fresh snapshot (e.g. after reconnect)
            if (event.seq == 0L) {
                session.emulator?.clearScreen()
            }
            session.emulator?.writeInput(bytes, 0, bytes.size)
        }
    }

    /**
     * Handle RemoteTerminalResize: resize emulator to desktop PTY dimensions.
     */
    private fun handleRemoteResize(event: RustEvent.RemoteTerminalResize) {
        android.util.Log.d("termfast", "handleRemoteResize: ${event.cols}x${event.rows} pairing=${event.pairing_id}")
        val session = sessions.values.firstOrNull {
            it.remotePairingId == event.pairing_id &&
            it.remoteTerminalId == event.terminal_id.toInt()
        }
        if (session != null) {
            session.emulator?.resize(event.cols, event.rows)
        }
    }

    /**
     * Handle RemoteTerminalError: mark all remote sessions for this pairing
     * as disconnected.
     */
    private fun handleRemoteError(event: RustEvent.RemoteTerminalError) {
        sessions.values.filter {
            it.remotePairingId == event.pairing_id
        }.forEach {
            sessions[it.sessionId] = it.copy(connected = false)
        }
    }
}
