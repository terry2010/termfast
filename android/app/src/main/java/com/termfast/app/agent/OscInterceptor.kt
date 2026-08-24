package com.termfast.app.agent

/**
 * OSC interceptor — extract AI CLI status signals from raw PTY bytes.
 *
 * Ported from desktop `src/hooks/oscParser.ts` (179 lines).
 *
 * Scans a byte buffer for OSC sequences (ESC ] <ident> ; <data> BEL/ST) and
 * parses recognized ones into AgentSignal. This is a read-only interception:
 * the bytes are NOT consumed — TerminalSessionManager.feedEmulator() feeds
 * the complete bytes to termlib after this function returns.
 *
 * Supported OSC sequences:
 *   ESC ] 0 ; <title> BEL          → CliType detection (all CLIs)
 *   ESC ] 9 ; <message> BEL        → Devin notification (iTerm2-style)
 *   ESC ] 777 ; notify;Devin;<msg> BEL → Devin notification (rxvt-unicode)
 *   ESC ] 1337 ; devin-idle=true BEL   → Devin done signal (custom)
 *
 * Note: OSC 52 (clipboard) is used by OpenCode but not parsed here — it's
 * not a status signal. termlib handles clipboard operations natively.
 */
object OscInterceptor {

    /** Messages that indicate Devin finished (done state). */
    private val DEVIN_DONE_MESSAGES = listOf(
        "Devin finished",
        "Devin encountered an error",
        "Devin response truncated",
    )

    /** Messages that indicate Devin needs user input (blocked state). */
    private val DEVIN_BLOCKED_MESSAGES = listOf(
        "Devin needs input",
        "Tool approval pending",
        "Question pending",
        "Network permission pending",
        "Input needed",
        "Devin needs authentication",
    )

    /** Max buffer size for incomplete OSC sequences (prevents unbounded growth). */
    private const val MAX_BUFFER_SIZE = 4096

    /** Per-session incomplete OSC buffer (for cross-chunk sequence assembly). */
    private val sessionBuffers = java.util.concurrent.ConcurrentHashMap<String, StringBuilder>()

    /** OSC start marker: ESC ] */
    private const val OSC_START = "\u001B]"

    /** OSC terminators: BEL (0x07) or ST (ESC \) */
    private val oscPattern = Regex("\u001B\\](\\d+);([^\u0007\u001B]*)(?:\u0007|\u001B\\\\)")

    /**
     * Scan a byte buffer for OSC sequences and return the first recognized
     * AgentSignal, or null if none found.
     *
     * Maintains a per-session buffer for incomplete OSC sequences that span
     * multiple byte chunks. If an OSC start marker (ESC ]) is found but no
     * terminator (BEL or ST) follows, the partial sequence is buffered and
     * prepended to the next chunk.
     *
     * @param sessionId the session ID (used for cross-chunk buffer)
     * @param bytes raw PTY bytes (may contain multiple OSC sequences)
     * @return first recognized AgentSignal, or null
     */
    fun scan(sessionId: String, bytes: ByteArray): AgentSignal? {
        val chunkText = String(bytes, Charsets.UTF_8)
        val buffer = sessionBuffers[sessionId]
        val fullText = if (buffer != null && buffer.isNotEmpty()) {
            // Prepend leftover from previous chunk
            val combined = buffer.toString() + chunkText
            buffer.clear()
            sessionBuffers.remove(sessionId)
            combined
        } else {
            chunkText
        }
        return scanStringWithBuffer(sessionId, fullText)
    }

    /**
     * Scan a string for OSC sequences, buffering incomplete ones.
     */
    private fun scanStringWithBuffer(sessionId: String, text: String): AgentSignal? {
        var firstSignal: AgentSignal? = null
        var lastMatchEnd = 0

        for (match in oscPattern.findAll(text)) {
            val ident = match.groupValues[1].toIntOrNull() ?: continue
            val data = match.groupValues[2]
            val signal = parseOsc(ident, data) ?: continue
            if (firstSignal == null) firstSignal = signal
            lastMatchEnd = match.range.last + 1
        }

        // Check for incomplete OSC sequence after the last complete match
        val remaining = text.substring(lastMatchEnd)
        val oscStartIdx = remaining.indexOf(OSC_START)
        if (oscStartIdx >= 0 && oscStartIdx < MAX_BUFFER_SIZE) {
            val incomplete = remaining.substring(oscStartIdx)
            if (incomplete.length < MAX_BUFFER_SIZE) {
                sessionBuffers[sessionId] = StringBuilder(incomplete)
            }
        }

        return firstSignal
    }

    /**
     * Scan a string for OSC sequences. Exposed for unit testing (no buffer).
     */
    fun scanString(text: String): AgentSignal? {
        for (match in oscPattern.findAll(text)) {
            val ident = match.groupValues[1].toIntOrNull() ?: continue
            val data = match.groupValues[2]
            val signal = parseOsc(ident, data) ?: continue
            return signal
        }
        return null
    }

    /** Clear the buffer for a session (called on session close). */
    fun clearBuffer(sessionId: String) {
        sessionBuffers.remove(sessionId)
    }

    /**
     * Dispatch an OSC payload to the right parser by ident number.
     */
    fun parseOsc(ident: Int, data: String): AgentSignal? = when (ident) {
        0 -> parseOsc0(data)
        9 -> parseOsc9(data)
        777 -> parseOsc777(data)
        1337 -> parseOsc1337(data)
        else -> null
    }

    /**
     * Parse OSC 0 (set window title).
     * Returns a Title signal with detected CLI type, or null for non-CLI titles.
     */
    private fun parseOsc0(data: String): AgentSignal? {
        val title = data.trim()
        if (title.isEmpty()) return null
        // OpenCode
        if (title == "OpenCode" || title.startsWith("OC |") || title.startsWith("OC  |")) {
            return AgentSignal.Title(CliType.OPENCODE, title)
        }
        // Codex
        if (title == "Action Required" || title.lowercase().contains("codex")) {
            return AgentSignal.Title(CliType.CODEX, title)
        }
        // Claude Code
        if (title.lowercase().contains("claude")) {
            return AgentSignal.Title(CliType.CLAUDE_CODE, title)
        }
        // Devin
        if (title.lowercase().contains("devin")) {
            return AgentSignal.Title(CliType.DEVIN, title)
        }
        return null
    }

    /**
     * Parse OSC 9 (iTerm2-style system notification).
     * Devin emits: ESC]9;Devin finishedBEL / ESC]9;Devin needs inputBEL
     */
    private fun parseOsc9(data: String): AgentSignal? {
        val message = data.trim()
        if (message.isEmpty()) return null
        if (!message.startsWith("Devin ")) return null
        val done = isDevinDoneMessage(message)
        return AgentSignal.Notify(CliType.DEVIN, message, done)
    }

    /**
     * Parse OSC 777 (rxvt-unicode notification extension).
     * Format: notify;Devin;<message>
     */
    private fun parseOsc777(data: String): AgentSignal? {
        val parts = data.split(";")
        if (parts.size < 3) return null
        val cmd = parts[0]
        val title = parts[1]
        val body = parts.drop(2).joinToString(";")
        if (cmd != "notify") return null
        if (title == "Devin") {
            val done = isDevinDoneMessage(body)
            return AgentSignal.Notify(CliType.DEVIN, body, done)
        }
        return null
    }

    /**
     * Parse OSC 1337 (Devin custom extension).
     * Known: devin-idle=true → done signal.
     */
    private fun parseOsc1337(data: String): AgentSignal? {
        val trimmed = data.trim()
        if (trimmed == "devin-idle=true") {
            return AgentSignal.Done(CliType.DEVIN)
        }
        return null
    }

    /**
     * Classify a Devin notification message as done or blocked.
     * @return true if done, false if blocked.
     */
    private fun isDevinDoneMessage(message: String): Boolean {
        val lower = message.lowercase()
        for (done in DEVIN_DONE_MESSAGES) {
            if (lower.startsWith(done.lowercase())) return true
        }
        for (blocked in DEVIN_BLOCKED_MESSAGES) {
            if (lower.startsWith(blocked.lowercase())) return false
        }
        return false  // unknown → default to blocked (safer)
    }
}
