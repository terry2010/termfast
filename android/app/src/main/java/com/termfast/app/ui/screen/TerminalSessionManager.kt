package com.termfast.app.ui.screen

import java.util.UUID

/**
 * Manages terminal sessions per server.
 * Sessions are reused across screen recompositions — when the user navigates
 * away from the terminal screen and comes back, the same session is restored
 * with its output history.
 */
object TerminalSessionManager {
    private val sessions = mutableMapOf<String, SessionState>()

    private data class SessionState(
        val sessionId: String,
        val output: List<String> = emptyList(),
        val connected: Boolean = false,
    )

    @Synchronized
    fun getOrCreateSession(serverId: String): String {
        val existing = sessions[serverId]
        if (existing != null && existing.connected) {
            return existing.sessionId
        }
        // Create new session (old one was closed or never existed)
        val sessionId = UUID.randomUUID().toString()
        sessions[serverId] = SessionState(sessionId = sessionId)
        return sessionId
    }

    @Synchronized
    fun getOutput(serverId: String): List<String> {
        return sessions[serverId]?.output ?: emptyList()
    }

    @Synchronized
    fun updateOutput(serverId: String, output: List<String>) {
        val existing = sessions[serverId] ?: return
        sessions[serverId] = existing.copy(output = output)
    }

    @Synchronized
    fun isConnected(serverId: String): Boolean {
        return sessions[serverId]?.connected ?: false
    }

    @Synchronized
    fun setConnected(serverId: String, connected: Boolean) {
        val existing = sessions[serverId] ?: return
        sessions[serverId] = existing.copy(connected = connected)
    }

    @Synchronized
    fun closeSession(serverId: String) {
        sessions.remove(serverId)
    }
}
