package com.termfast.app.ui.screen

import android.util.Base64
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
        val cursorCol: Int = 0,
        val pendingCr: Boolean = false,
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
    fun getCursorColBySession(sessionId: String): Int {
        return sessions[sessionId]?.cursorCol ?: 0
    }

    @Synchronized
    fun updateOutputBySession(sessionId: String, output: List<String>) {
        val existing = sessions[sessionId] ?: return
        sessions[sessionId] = existing.copy(output = output)
    }

    /**
     * Append raw terminal data, correctly merging partial lines across chunks.
     * Terminal data arrives in arbitrary chunks. We process it with a
     *   simple terminal emulator that handles:
     *   - \n  : newline (start a new line)
     *   - \r  : carriage return (move cursor to start of current line)
     *   - \b  : backspace (move cursor left one column)
     *   - \u007f (DEL): ignored in output stream
     *   - ANSI cursor movement: \u001b[D (left), \u001b[C (right),
     *     \u001b[<n>G (go to column n, 1-based), \u001b[<n>D/C (move n)
     *   - ANSI line erase: \u001b[K (erase to end of line)
     *   - Other ANSI escape codes: stripped
     */
    @Synchronized
    fun appendTerminalData(sessionId: String, raw: String) {
        val existing = sessions[sessionId] ?: return
        if (raw.isEmpty()) return

        // Work on a mutable copy of the existing output lines. The "current
        //   line" is the last element; we track a cursor column within it.
        val lines = existing.output.toMutableList()
        var cursorCol = if (!existing.lastLineComplete && lines.isNotEmpty()) {
            // Resume from saved cursor position (may be mid-line after \b moves).
            existing.cursorCol
        } else {
            if (lines.isEmpty() || existing.lastLineComplete) lines.add("")
            0
        }
        var pendingCr = existing.pendingCr

        // Parse the raw string, handling ANSI cursor sequences inline and
        //   stripping other ANSI codes. We scan char by char; when we hit
        //   \u001b, we parse the full CSI sequence and apply cursor ops.
        var i = 0
        while (i < raw.length) {
            val ch = raw[i]
            when {
                ch == '\u001b' -> {
                    // ANSI escape sequence — parse and handle cursor movement.
                    val (consumed, op) = parseAnsiCursor(raw, i)
                    when (op) {
                        is CursorOp.Left -> {
                            cursorCol = maxOf(0, cursorCol - op.n)
                            pendingCr = false
                        }
                        is CursorOp.Right -> {
                            val lineLen = lines.lastOrNull()?.length ?: 0
                            cursorCol = minOf(lineLen, cursorCol + op.n)
                            pendingCr = false
                        }
                        is CursorOp.ToCol -> {
                            cursorCol = maxOf(0, op.col)
                            pendingCr = false
                        }
                        is CursorOp.EraseToEnd -> {
                            // Erase from cursor to end of line.
                            if (lines.isNotEmpty() && cursorCol < lines.last().length) {
                                lines[lines.lastIndex] = lines.last().substring(0, cursorCol)
                            }
                            pendingCr = false
                        }
                        null -> { /* stripped, no-op */ }
                    }
                    i += consumed
                }
                ch == '\n' -> {
                    cursorCol = 0
                    pendingCr = false
                    lines.add("")
                    i++
                }
                ch == '\r' -> {
                    cursorCol = 0
                    pendingCr = true
                    i++
                }
                ch == '\u0008' -> {
                    if (cursorCol > 0) cursorCol--
                    pendingCr = false
                    i++
                }
                ch == '\u007f' -> {
                    // DEL — ignored in output stream.
                    i++
                }
                else -> {
                    // Printable character.
                    if (pendingCr && cursorCol == 0 && lines.isNotEmpty()) {
                        lines[lines.lastIndex] = ""
                        pendingCr = false
                    }
                    val cur = lines.lastOrNull() ?: run { lines.add(""); "" }
                    if (cursorCol < cur.length) {
                        lines[lines.lastIndex] = cur.substring(0, cursorCol) + ch + cur.substring(cursorCol + 1)
                    } else {
                        lines[lines.lastIndex] = cur + ch
                    }
                    cursorCol++
                    i++
                }
            }
        }

        val endsWithNl = raw.endsWith("\n")
        sessions[sessionId] = existing.copy(
            output = lines,
            lastLineComplete = endsWithNl,
            cursorCol = cursorCol,
            pendingCr = pendingCr,
        )
    }

    /** Result of parsing an ANSI escape sequence at position [i]. */
    private sealed class CursorOp {
        data class Left(val n: Int) : CursorOp()
        data class Right(val n: Int) : CursorOp()
        data class ToCol(val col: Int) : CursorOp()
        object EraseToEnd : CursorOp()
    }

    /**
     * Parse an ANSI escape sequence starting at [i] in [raw].
     * Returns (charsConsumed, CursorOp?) — op is null for non-cursor sequences
     *   (colors, OSC, etc.) which should just be stripped.
     */
    private fun parseAnsiCursor(raw: String, i: Int): Pair<Int, CursorOp?> {
        // Need at least \u001b + one more char.
        if (i + 1 >= raw.length) return Pair(1, null)
        val next = raw[i + 1]
        if (next == '[') {
            // CSI sequence: \u001b[<params><letter>
            var j = i + 2
            val paramStart = j
            while (j < raw.length && (raw[j].isDigit() || raw[j] == ';' || raw[j] == '?')) j++
            if (j >= raw.length) return Pair(raw.length - i, null)
            val paramStr = raw.substring(paramStart, j)
            val letter = raw[j]
            val consumed = j - i + 1
            val n = paramStr.toIntOrNull() ?: 1
            return when (letter) {
                'D' -> Pair(consumed, CursorOp.Left(n))      // cursor left
                'C' -> Pair(consumed, CursorOp.Right(n))     // cursor right
                'G' -> Pair(consumed, CursorOp.ToCol(n - 1)) // cursor to column (1-based)
                'K' -> {
                    // Erase in line: 0K = cursor to end, 1K = start to cursor, 2K = whole line
                    val mode = paramStr.toIntOrNull() ?: 0
                    if (mode == 0) Pair(consumed, CursorOp.EraseToEnd) else Pair(consumed, null)
                }
                else -> Pair(consumed, null) // other CSI: strip
            }
        }
        // OSC or other 2-char ESC — strip (simplified: skip to BEL or ST or just 2 chars)
        if (next == ']') {
            var j = i + 2
            while (j < raw.length && raw[j] != '\u0007' && raw[j] != '\u001b') j++
            if (j < raw.length && raw[j] == '\u0007') return Pair(j - i + 1, null)
            if (j < raw.length && raw[j] == '\u001b' && j + 1 < raw.length && raw[j + 1] == '\\') return Pair(j - i + 2, null)
            return Pair(raw.length - i, null)
        }
        // Other 2-char ESC sequence
        return Pair(2, null)
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
                        // Decode base64 to raw bytes, then convert to UTF-8 string.
                        // The backend sends base64 to preserve binary data; we
                        // decode it here so the terminal renderer gets the original
                        // bytes (not lossy-converted by the Rust side).
                        val raw = if (event.encoding == "base64") {
                            try {
                                String(Base64.decode(event.data, Base64.DEFAULT), Charsets.UTF_8)
                            } catch (e: Exception) {
                                event.data  // fallback: treat as plain string
                            }
                        } else {
                            event.data
                        }
                        appendTerminalData(event.session_id, raw)
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
