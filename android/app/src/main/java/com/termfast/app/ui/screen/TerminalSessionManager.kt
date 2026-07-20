package com.termfast.app.ui.screen

import com.termfast.app.data.RustEvent
import com.termfast.app.data.RustRepository
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.GlobalScope
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
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
        val output: List<String> = emptyList(),
        val connected: Boolean = false,
        val createdAt: Long = System.currentTimeMillis(),
        val name: String = "",
        val lastLineComplete: Boolean = true,
    )

    @Synchronized
    fun getOrCreateSession(serverId: String): String {
        val sessionId = UUID.randomUUID().toString()
        sessions[sessionId] = SessionState(sessionId = sessionId, serverId = serverId)
        return sessionId
    }

    @Synchronized
    fun getOrCreateSessionById(serverId: String, sessionId: String): String {
        val existing = sessions[sessionId]
        if (existing != null) return sessionId
        sessions[sessionId] = SessionState(sessionId = sessionId, serverId = serverId)
        return sessionId
    }

    @Synchronized
    fun getOutputBySession(sessionId: String): List<String> {
        return sessions[sessionId]?.output ?: emptyList()
    }

    @Synchronized
    fun updateOutputBySession(sessionId: String, output: List<String>) {
        val existing = sessions[sessionId] ?: return
        sessions[sessionId] = existing.copy(output = output)
    }

    /**
     * Append raw terminal data, correctly merging partial lines across chunks.
     * Terminal data arrives in arbitrary chunks — a prompt may come without
     * a trailing newline, then the echoed input arrives separately. We track
     * whether the last line is "complete" (ended with \n) and merge accordingly.
     */
    @Synchronized
    fun appendTerminalData(sessionId: String, raw: String) {
        val existing = sessions[sessionId] ?: return
        val clean = stripAnsi(raw).replace("\r", "")
        if (clean.isEmpty()) return
        val endsWithNl = clean.endsWith("\n")
        val newLines = if (endsWithNl) clean.split("\n").dropLast(1) else clean.split("\n")

        val merged = if (!existing.lastLineComplete && existing.output.isNotEmpty() && newLines.isNotEmpty()) {
            // Previous last line was partial — merge with first new line
            existing.output.dropLast(1) + listOf(existing.output.last() + newLines.first()) + newLines.drop(1)
        } else {
            existing.output + newLines
        }
        sessions[sessionId] = existing.copy(output = merged, lastLineComplete = endsWithNl)
    }

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
                        appendTerminalData(event.session_id, event.data)
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
