package com.termfast.app.ui.screen

import android.util.Base64
import android.content.Context
import androidx.compose.ui.graphics.Color
import com.termfast.app.data.RustEvent
import com.termfast.app.data.RustRepository
import com.termfast.app.service.RemoteTunnelService
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
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
    // User-defined session order (sessionId → orderIndex). Sessions not in this
    // map fall back to createdAt ordering. Used for drag-to-reorder.
    private val sessionOrder = mutableMapOf<String, Int>()
    // User-defined top-level group order (topKey → orderIndex). Top keys not in
    // this map fall back to natural order (first-seen). Used for drag-to-reorder
    // top-level groups (desktops / SSH servers).
    private val topLevelOrder = mutableMapOf<String, Int>()
    private var collectorStarted = false
    // Tunnel managers registered by pairingId — used for UNSUBSCRIBE on disconnect
    private val tunnelManagers = mutableMapOf<String, com.termfast.app.data.RemoteTunnelManager>()
    // Pending RESIZE frames: "pairingId:terminalId" → (cols, rows).
    // RESIZE frames can arrive before the session is created (e.g. SUBSCRIBE
    // response sends RESIZE immediately, but session is created on user tap).
    // getOrCreateRemoteSession checks this map and applies the pending size.
    private val pendingResize = mutableMapOf<String, Pair<Int, Int>>()
    // App context for starting/stopping RemoteTunnelService
    private var appContext: Context? = null
    // Managed coroutine scope for global event collection and reconnection tasks.
    // Replaces GlobalScope usage to prevent unbounded coroutine leaks.
    private val managerScope = kotlinx.coroutines.CoroutineScope(
        kotlinx.coroutines.SupervisorJob() + kotlinx.coroutines.Dispatchers.Default
    )

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
        // Server ID on the desktop side: "__local__" for desktop's local terminal,
        // or an SSH server ID for desktop's SSH terminal. Used for 3-level grouping.
        val remoteServerId: String = "__local__",
        // Server display name on the desktop side: "桌面端" or SSH server name.
        val remoteServerName: String = "桌面端",
        // Desktop PTY dimensions (cols, rows) — set from RESIZE frame.
        // Used as forcedSize in the Terminal composable so the emulator
        // matches the desktop PTY, with scrolling/scaling to fit the screen.
        val remotePtyCols: Int = 0,
        val remotePtyRows: Int = 0,
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
        remoteServerId: String = "__local__",
        remoteServerName: String = "桌面端",
    ): String {
        android.util.Log.d("termfast", "getOrCreateRemoteSession: pairingId=$pairingId terminalId=$terminalId")
        // Register tunnel manager for this pairing (used by disconnectSession)
        tunnelManagers[pairingId] = tunnelManager
        // Reuse existing session if same pairingId + terminalId
        val existing = sessions.values.firstOrNull {
            it.remotePairingId == pairingId && it.remoteTerminalId == terminalId
        }
        if (existing != null) {
            // Update server info in case it changed
            sessions[existing.sessionId] = existing.copy(
                remoteServerId = remoteServerId,
                remoteServerName = remoteServerName,
            )
            return existing.sessionId
        }

        val sessionId = UUID.randomUUID().toString()
        // Check for pending RESIZE frame (arrived before session was created)
        val pendingKey = "$pairingId:$terminalId"
        val pending = pendingResize.remove(pendingKey)
        if (pending != null) {
            android.util.Log.d("termfast", "getOrCreateRemoteSession: applying pending resize ${pending.first}x${pending.second} for $pendingKey")
        }
        sessions[sessionId] = SessionState(
            sessionId = sessionId,
            serverId = "remote:$pairingId",  // synthetic serverId for remote
            emulator = createRemoteEmulator(sessionId, terminalId, tunnelManager),
            connected = true,
            name = name,
            remotePairingId = pairingId,
            remoteTerminalId = terminalId,
            remoteServerId = remoteServerId,
            remoteServerName = remoteServerName,
            remotePtyCols = pending?.first ?: 0,
            remotePtyRows = pending?.second ?: 0,
        )
        // Apply pending resize to emulator if available
        if (pending != null) {
            sessions[sessionId]?.emulator?.resize(pending.second, pending.first)
        }
        return sessionId
    }

    /** Register a tunnel manager for a pairing (for UNSUBSCRIBE on disconnect). */
    @Synchronized
    fun registerTunnelManager(pairingId: String, manager: com.termfast.app.data.RemoteTunnelManager) {
        tunnelManagers[pairingId] = manager
    }

    /** Initialize with app context for starting/stopping RemoteTunnelService. */
    fun init(context: Context) {
        appContext = context.applicationContext
    }

    /** Whether any tunnel managers are active (for RemoteTunnelService start/stop). */
    @Synchronized
    fun hasActiveTunnels(): Boolean = tunnelManagers.isNotEmpty()

    /** Start or stop RemoteTunnelService based on tunnel count. */
    private fun notifyTunnelServiceStateChanged(hasTunnels: Boolean) {
        val ctx = appContext ?: return
        if (hasTunnels) RemoteTunnelService.start(ctx)
        else RemoteTunnelService.stop(ctx)
    }

    /** Get or create a shared RemoteTunnelManager for a pairing_id. */
    @Synchronized
    fun getOrCreateTunnelManager(
        pairingId: String,
        pairingKey: ByteArray,
        relayUrl: String,
        pairingJwt: String,
        pairingRefreshToken: String = "",
    ): com.termfast.app.data.RemoteTunnelManager {
        val existing = tunnelManagers[pairingId]
        if (existing != null) return existing
        val manager = com.termfast.app.data.RemoteTunnelManager(
            pairingId, pairingKey, relayUrl, pairingJwt,
            pairingRefreshToken = pairingRefreshToken,
        )
        tunnelManagers[pairingId] = manager
        // Start RemoteTunnelService when first tunnel is created
        if (tunnelManagers.size == 1) {
            notifyTunnelServiceStateChanged(true)
        }
        return manager
    }

    /** Unregister a tunnel manager for a pairing. */
    @Synchronized
    fun unregisterTunnelManager(pairingId: String) {
        tunnelManagers.remove(pairingId)
    }

    /** Get an existing tunnel manager for a pairing, or null. */
    @Synchronized
    fun getTunnelManager(pairingId: String): com.termfast.app.data.RemoteTunnelManager? =
        tunnelManagers[pairingId]

    /**
     * Stop and remove tunnel managers whose pairingId is NOT in [activePids].
     * Called by TerminalsScreen when remote sessions are closed, to free
     * WebSocket connections and FFI resources for pairings with no sessions.
     */
    @Synchronized
    fun stopTunnelsNotIn(activePids: Set<String>) {
        val toStop = tunnelManagers.keys.filter { it !in activePids }
        for (pid in toStop) {
            val tm = tunnelManagers.remove(pid)
            tm?.stopAndDestroy()
        }
        // Stop RemoteTunnelService when all tunnels are gone
        if (tunnelManagers.isEmpty()) {
            notifyTunnelServiceStateChanged(false)
        }
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

    @Synchronized
    fun getSessionState(sessionId: String): SessionState? =
        sessions[sessionId]

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
        sessionOrder.remove(sessionId)
    }

    @Synchronized
    fun getSessions(serverId: String): List<SessionState> {
        return sessions.values.filter { it.serverId == serverId }
            .sortedWith(compareBy({ sessionOrder[it.sessionId] ?: Int.MAX_VALUE }, { it.createdAt }))
    }

    @Synchronized
    fun getAllSessions(): List<SessionState> {
        return sessions.values
            .sortedWith(compareBy({ sessionOrder[it.sessionId] ?: Int.MAX_VALUE }, { it.createdAt }))
    }

    /** Reorder sessions within a server group. [orderedSessionIds] is the new order. */
    @Synchronized
    fun reorderSessions(orderedSessionIds: List<String>) {
        orderedSessionIds.forEachIndexed { index, sid ->
            sessionOrder[sid] = index
        }
    }

    /** Reorder top-level groups. [orderedTopKeys] is the new order of top-level keys. */
    @Synchronized
    fun reorderTopLevels(orderedTopKeys: List<String>) {
        orderedTopKeys.forEachIndexed { index, key ->
            topLevelOrder[key] = index
        }
    }

    /** Get the user-defined order index for a top-level key, or null if unset. */
    @Synchronized
    fun getTopLevelOrder(topKey: String): Int? = topLevelOrder[topKey]

    @Synchronized
    fun hasSessions(serverId: String): Boolean {
        return sessions.values.any { it.serverId == serverId }
    }

    /** Count active remote terminal sessions for a given pairingId. */
    @Synchronized
    fun getRemoteSessionCount(pairingId: String): Int {
        return sessions.values.count { it.remotePairingId == pairingId }
    }

    @Synchronized
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
     * Remove a session from the local list (card disappears, badge updates).
     * - Remote: sends UNSUBSCRIBE only (terminal stays alive on desktop,
     *           can be reopened from the terminal picker later).
     * - Local SSH: closes the PTY and removes the session.
     * Unlike [closeTerminalSession], the terminal process is NOT killed on the desktop.
     */
    @Synchronized
    fun removeSession(sessionId: String) {
        val session = sessions[sessionId] ?: return
        if (session.remotePairingId != null) {
            // Remote: unsubscribe only, terminal stays alive on desktop
            val tunnelManager = tunnelManagers[session.remotePairingId]
            tunnelManager?.sendUnsubscribe(session.remoteTerminalId)
        } else {
            // Local SSH: close the PTY
            RustRepository.closeTerminal(sessionId)
        }
        sessions.remove(sessionId)
    }

    /**
     * Close (kill) a terminal session permanently.
     * - Remote: sends CLOSE_TERMINAL to desktop (kills the terminal process),
     *           then removes the local session.
     * - Local SSH: closes the PTY and removes the session.
     * Unlike [disconnectSession], the terminal process is terminated on the desktop.
     */
    @Synchronized
    fun closeTerminalSession(sessionId: String) {
        val session = sessions[sessionId] ?: return
        if (session.remotePairingId != null) {
            // Remote: send CLOSE_TERMINAL to kill the terminal on desktop
            val tunnelManager = tunnelManagers[session.remotePairingId]
            tunnelManager?.sendCloseTerminal(session.remoteTerminalId)
            tunnelManager?.sendUnsubscribe(session.remoteTerminalId)
        } else {
            // Local SSH: close the PTY
            RustRepository.closeTerminal(sessionId)
        }
        sessions.remove(sessionId)
    }

    /**
     * Write keyboard input to a session (SSH or remote).
     * For SSH: writes to the PTY via RustRepository.
     * For remote: sends INPUT frame via RemoteTunnelManager.
     */
    @Synchronized
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
        managerScope.launch(Dispatchers.IO) {
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
        managerScope.launch {
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
                // Tunnel ready — mark protocol ready on the tunnel manager,
                // mark sessions connected, and request terminal list to sync
                // (remove sessions whose terminals no longer exist on desktop).
                val tm = tunnelManagers[event.pairing_id]
                tm?.onProtocolReady()
                markRemoteSessionsConnected(event.pairing_id)
                tm?.sendListRequest()
            }
            is RustEvent.RemoteTerminalList -> {
                // LIST_RESPONSE — check if any remote sessions for this pairing
                // have terminals that no longer exist on desktop, and remove them.
                syncRemoteSessionsWithList(event.pairing_id, event.terminals)
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
    @Synchronized
    private fun handleRemoteOutput(event: RustEvent.RemoteTerminalOutput) {
        val session = sessions.values.firstOrNull {
            it.remotePairingId == event.pairing_id &&
            it.remoteTerminalId == event.terminal_id.toInt()
        }
        if (session != null && event.encoding == "base64") {
            val bytes = decodeBase64(event.data)
            android.util.Log.d("termfast", "handleRemoteOutput: ${bytes.size} bytes session=${session.sessionId} cols=${session.remotePtyCols} rows=${session.remotePtyRows}")
            session.emulator?.writeInput(bytes)
        }
    }

    /**
     * Handle RemoteTerminalHistory: on seq=0, clear screen; write data to emulator.
     */
    @Synchronized
    private fun handleRemoteHistory(event: RustEvent.RemoteTerminalHistory) {
        val session = sessions.values.firstOrNull {
            it.remotePairingId == event.pairing_id &&
            it.remoteTerminalId == event.terminal_id.toInt()
        }
        if (session != null && event.encoding == "base64") {
            val bytes = decodeBase64(event.data)
            android.util.Log.d("termfast", "handleRemoteHistory: ${bytes.size} bytes seq=${event.seq} session=${session.sessionId} cols=${session.remotePtyCols} rows=${session.remotePtyRows}")
            // On first HISTORY chunk (seq=0), clear the emulator
            // to prepare for a fresh snapshot (e.g. after reconnect)
            if (event.seq == 0L) {
                session.emulator?.clearScreen()
            }
            session.emulator?.writeInput(bytes, 0, bytes.size)
        }
    }

    /**
     * Handle RemoteTerminalResize: store desktop PTY dimensions in SessionState.
     * The Terminal composable uses these as forcedSize, so the emulator
     * matches the desktop PTY and the composable doesn't resize it back
     * to the mobile screen size. The emulator itself is also resized here
     * to ensure it matches.
     */
    @Synchronized
    private fun handleRemoteResize(event: RustEvent.RemoteTerminalResize) {
        android.util.Log.d("termfast", "handleRemoteResize: ${event.cols}x${event.rows} pairing=${event.pairing_id} terminal_id=${event.terminal_id}")
        val session = sessions.values.firstOrNull {
            it.remotePairingId == event.pairing_id &&
            it.remoteTerminalId == event.terminal_id.toInt()
        }
        if (session != null) {
            android.util.Log.d("termfast", "handleRemoteResize: found session=${session.sessionId} oldCols=${session.remotePtyCols} oldRows=${session.remotePtyRows}")
            sessions[session.sessionId] = session.copy(
                remotePtyCols = event.cols,
                remotePtyRows = event.rows,
            )
            // termlib's resize(rows, cols) — first param is rows, second is cols
            session.emulator?.resize(event.rows, event.cols)
        } else {
            android.util.Log.d("termfast", "handleRemoteResize: no session found for pairing=${event.pairing_id} terminal_id=${event.terminal_id}, storing as pending")
            // Store as pending — getOrCreateRemoteSession will apply it when
            // the session is created (RESIZE can arrive before session creation)
            pendingResize["${event.pairing_id}:${event.terminal_id}"] = event.cols to event.rows
        }
    }

    /**
     * Handle RemoteTerminalError: mark all remote sessions for this pairing
     * as disconnected.
     */
    @Synchronized
    private fun handleRemoteError(event: RustEvent.RemoteTerminalError) {
        sessions.values.filter {
            it.remotePairingId == event.pairing_id
        }.forEach {
            sessions[it.sessionId] = it.copy(connected = false)
        }
    }

    /**
     * Remove all remote sessions for a pairing.
     * Called when the tunnel peer disconnects (desktop went offline/restarted).
     * Old terminal_ids are stale — the desktop will assign new ones on reconnect.
     * The user can re-open terminals from the picker after reconnection.
     */
    @Synchronized
    fun markRemoteSessionsDisconnected(pairingId: String) {
        val toRemove = sessions.values.filter { it.remotePairingId == pairingId }
        for (s in toRemove) {
            sessions.remove(s.sessionId)
        }
    }

    /**
     * Mark all remote sessions for a pairing as connected.
     * Called when the tunnel protocol becomes ready again (reconnected).
     */
    @Synchronized
    fun markRemoteSessionsConnected(pairingId: String) {
        sessions.values.filter { it.remotePairingId == pairingId }.forEach {
            sessions[it.sessionId] = it.copy(connected = true)
        }
    }

    /**
     * Sync remote sessions with a LIST_RESPONSE from desktop.
     * Removes sessions whose terminalId no longer exists on the desktop
     * (e.g. desktop was restarted, or terminal was closed while offline).
     * Returns the list of removed session names (for Toast notification).
     */
    @Synchronized
    fun syncRemoteSessionsWithList(pairingId: String, terminalsJson: String): List<String> {
        val aliveIds = try {
            val arr = org.json.JSONArray(terminalsJson)
            (0 until arr.length()).mapNotNull { i ->
                arr.getJSONObject(i).optInt("id", -1).takeIf { it >= 0 }
            }.toSet()
        } catch (_: Exception) {
            return emptyList()
        }
        val toRemove = sessions.values.filter { s ->
            s.remotePairingId == pairingId && s.remoteTerminalId !in aliveIds
        }
        val names = toRemove.mapNotNull { it.name.ifBlank { null } }
            .ifEmpty { toRemove.map { "远程终端" } }
        toRemove.forEach { s ->
            sessions.remove(s.sessionId)
        }
        return names
    }
}
