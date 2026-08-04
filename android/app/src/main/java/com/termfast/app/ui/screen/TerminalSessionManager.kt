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
    )

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
        RustRepository.closeTerminal(sessionId)
        setConnectedBySession(sessionId, false)
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
                    else -> {}
                }
            }
        }
    }
}
