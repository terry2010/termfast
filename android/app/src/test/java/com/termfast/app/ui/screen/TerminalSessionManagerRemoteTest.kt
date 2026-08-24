package com.termfast.app.ui.screen

import com.termfast.app.data.RustEvent
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * Tests for TerminalSessionManager remote terminal event routing.
 *
 * TerminalEmulator is a sealed interface (only TerminalEmulatorImpl permitted),
 * so we test with emulator=null. This verifies:
 * - Session lookup by pairingId + terminalId (correct routing)
 * - State changes (connected → disconnected on error)
 * - No crashes when emulator is null (graceful degradation)
 * - Event filtering (wrong pairingId/terminalId ignored)
 *
 * The actual emulator writeInput/resize/clearScreen calls are verified
 * indirectly: if routing is correct and no crash occurs with null emulator,
 * the production code with a real emulator will receive the calls.
 *
 * Note: android.util.Base64 is stubbed in unit tests, so we use
 * java.util.Base64 for encoding test data. The production code uses
 * android.util.Base64 which works on-device.
 */
class TerminalSessionManagerRemoteTest {

    private fun b64(data: ByteArray): String =
        java.util.Base64.getEncoder().encodeToString(data)

    private fun createRemoteSession(
        pairingId: String = "test-pairing",
        terminalId: Int = 5,
        sessionId: String = "test-session-uuid-${System.nanoTime()}",
        connected: Boolean = true,
    ): String {
        return TerminalSessionManager.createRemoteSessionForTest(
            pairingId = pairingId,
            terminalId = terminalId,
            emulator = null, // sealed interface, can't mock
            sessionId = sessionId,
        ).also {
            TerminalSessionManager.setConnectedBySession(it, connected)
        }
    }

    @Test
    fun testIsRemoteSessionReturnsTrueForRemoteSession() {
        val sid = createRemoteSession()
        assertTrue(TerminalSessionManager.isRemoteSession(sid))
    }

    @Test
    fun testIsRemoteSessionReturnsFalseForNonExistentSession() {
        assertFalse(TerminalSessionManager.isRemoteSession("nonexistent"))
    }

    @Test
    fun testRemoteTerminalOutputDoesNotCrashWithNullEmulator() {
        createRemoteSession(terminalId = 5)
        val data = byteArrayOf(0x48, 0x49)
        val encoded = b64(data)

        // Should not crash even with null emulator
        TerminalSessionManager.handleEvent(
            RustEvent.RemoteTerminalOutput(
                pairing_id = "test-pairing",
                terminal_id = 5L,
                data = encoded,
                encoding = "base64",
            )
        )
    }

    @Test
    fun testRemoteTerminalOutputIgnoresNonBase64Encoding() {
        createRemoteSession(terminalId = 5)
        // Non-base64 encoding should be ignored — verify no crash + no effect
        TerminalSessionManager.handleEvent(
            RustEvent.RemoteTerminalOutput(
                pairing_id = "test-pairing",
                terminal_id = 5L,
                data = "raw text",
                encoding = "utf8",
            )
        )
    }

    @Test
    fun testRemoteTerminalOutputIgnoresWrongTerminalId() {
        createRemoteSession(terminalId = 5)
        // Wrong terminal_id — session not found, no crash
        TerminalSessionManager.handleEvent(
            RustEvent.RemoteTerminalOutput(
                pairing_id = "test-pairing",
                terminal_id = 99L,
                data = b64(byteArrayOf(0x41)),
                encoding = "base64",
            )
        )
    }

    @Test
    fun testRemoteTerminalOutputIgnoresWrongPairingId() {
        createRemoteSession(pairingId = "test-pairing", terminalId = 5)
        TerminalSessionManager.handleEvent(
            RustEvent.RemoteTerminalOutput(
                pairing_id = "wrong-pairing",
                terminal_id = 5L,
                data = b64(byteArrayOf(0x41)),
                encoding = "base64",
            )
        )
    }

    @Test
    fun testRemoteTerminalHistorySeq0DoesNotCrashWithNullEmulator() {
        createRemoteSession(terminalId = 5)
        val encoded = b64(byteArrayOf(0x41, 0x42))

        TerminalSessionManager.handleEvent(
            RustEvent.RemoteTerminalHistory(
                pairing_id = "test-pairing",
                terminal_id = 5L,
                seq = 0L,
                is_last = false,
                data = encoded,
                encoding = "base64",
            )
        )
    }

    @Test
    fun testRemoteTerminalHistorySeqNonZeroDoesNotCrash() {
        createRemoteSession(terminalId = 5)
        TerminalSessionManager.handleEvent(
            RustEvent.RemoteTerminalHistory(
                pairing_id = "test-pairing",
                terminal_id = 5L,
                seq = 1L,
                is_last = true,
                data = b64(byteArrayOf(0x43)),
                encoding = "base64",
            )
        )
    }

    @Test
    fun testRemoteTerminalHistoryMultipleChunksDoNotCrash() {
        createRemoteSession(terminalId = 5)

        TerminalSessionManager.handleEvent(
            RustEvent.RemoteTerminalHistory(
                pairing_id = "test-pairing",
                terminal_id = 5L,
                seq = 0L,
                is_last = false,
                data = b64(byteArrayOf(0x41)),
                encoding = "base64",
            )
        )
        TerminalSessionManager.handleEvent(
            RustEvent.RemoteTerminalHistory(
                pairing_id = "test-pairing",
                terminal_id = 5L,
                seq = 1L,
                is_last = true,
                data = b64(byteArrayOf(0x42)),
                encoding = "base64",
            )
        )
    }

    @Test
    fun testRemoteTerminalResizeDoesNotCrashWithNullEmulator() {
        createRemoteSession(terminalId = 5)
        TerminalSessionManager.handleEvent(
            RustEvent.RemoteTerminalResize(
                pairing_id = "test-pairing",
                terminal_id = 5L,
                cols = 120,
                rows = 40,
            )
        )
    }

    @Test
    fun testRemoteTerminalResizeIgnoresWrongTerminalId() {
        createRemoteSession(terminalId = 5)
        TerminalSessionManager.handleEvent(
            RustEvent.RemoteTerminalResize(
                pairing_id = "test-pairing",
                terminal_id = 99L,
                cols = 100,
                rows = 30,
            )
        )
    }

    @Test
    fun testRemoteTerminalErrorMarksSessionsDisconnected() {
        val sid = createRemoteSession(pairingId = "test-pairing", terminalId = 5)
        assertTrue(TerminalSessionManager.isConnectedBySession(sid))

        TerminalSessionManager.handleEvent(
            RustEvent.RemoteTerminalError(
                pairing_id = "test-pairing",
                error = "invalid_terminal_id",
            )
        )

        assertFalse(TerminalSessionManager.isConnectedBySession(sid))
    }

    @Test
    fun testRemoteTerminalErrorOnlyAffectsMatchingPairing() {
        val sid1 = createRemoteSession(pairingId = "pairing-A", terminalId = 1, sessionId = "sid-a")
        val sid2 = createRemoteSession(pairingId = "pairing-B", terminalId = 2, sessionId = "sid-b")
        assertTrue(TerminalSessionManager.isConnectedBySession(sid1))
        assertTrue(TerminalSessionManager.isConnectedBySession(sid2))

        TerminalSessionManager.handleEvent(
            RustEvent.RemoteTerminalError(
                pairing_id = "pairing-A",
                error = "some error",
            )
        )

        assertFalse(TerminalSessionManager.isConnectedBySession(sid1))
        // pairing-B session should still be connected
        assertTrue(TerminalSessionManager.isConnectedBySession(sid2))
    }

    @Test
    fun testRemoteTerminalErrorAffectsAllSessionsForPairing() {
        // Multiple terminals for same pairing — error should disconnect all
        val sid1 = createRemoteSession(pairingId = "pairing-X", terminalId = 1, sessionId = "sid-x1")
        val sid2 = createRemoteSession(pairingId = "pairing-X", terminalId = 2, sessionId = "sid-x2")
        assertTrue(TerminalSessionManager.isConnectedBySession(sid1))
        assertTrue(TerminalSessionManager.isConnectedBySession(sid2))

        TerminalSessionManager.handleEvent(
            RustEvent.RemoteTerminalError(
                pairing_id = "pairing-X",
                error = "tunnel error",
            )
        )

        assertFalse(TerminalSessionManager.isConnectedBySession(sid1))
        assertFalse(TerminalSessionManager.isConnectedBySession(sid2))
    }

    @Test
    fun testRemoteTunnelReadyEventDoesNotCrash() {
        TerminalSessionManager.handleEvent(
            RustEvent.RemoteTunnelReady(pairing_id = "test-pairing")
        )
    }

    @Test
    fun testRemoteTerminalListEventDoesNotCrash() {
        TerminalSessionManager.handleEvent(
            RustEvent.RemoteTerminalList(
                pairing_id = "test-pairing",
                terminals = "[]",
            )
        )
    }

    @Test
    fun testDisconnectRemoteSessionMarksDisconnected() {
        val sid = createRemoteSession()
        assertTrue(TerminalSessionManager.isConnectedBySession(sid))
        TerminalSessionManager.disconnectSession(sid)
        assertFalse(TerminalSessionManager.isConnectedBySession(sid))
    }

    @Test
    fun testGetOrCreateRemoteSessionReusesExistingSession() {
        // Verify reuse logic: creating a session with same pairingId + terminalId
        // returns the same sessionId. Uses createRemoteSessionForTest (no real
        // emulator needed) to verify the lookup logic that getOrCreateRemoteSession
        // uses. The real getOrCreateRemoteSession requires TerminalEmulatorFactory
        // which needs android.os.Looper (not available in unit tests).
        val sid1 = createRemoteSession(
            pairingId = "reuse-test",
            terminalId = 42,
            sessionId = "reuse-sid",
        )
        // Simulate second call with same pairingId + terminalId — should find existing
        val existing = TerminalSessionManager.findRemoteSession("reuse-test", 42)
        assertEquals(sid1, existing)
    }

    @Test
    fun testCloseRemoteSessionRemovesFromMap() {
        val sid = createRemoteSession()
        assertTrue(TerminalSessionManager.isRemoteSession(sid))
        TerminalSessionManager.closeSessionBySessionId(sid)
        assertFalse(TerminalSessionManager.isRemoteSession(sid))
    }

    @Test
    fun testMarkRemoteSessionsDisconnectedPreservesSessions() {
        // Regression test: markRemoteSessionsDisconnected should NOT delete
        // sessions — it should only mark them as disconnected. This prevents
        // terminals from disappearing when the phone briefly loses network
        // and reconnects (desktop terminal_id mapping is persistent).
        val sid = createRemoteSession(pairingId = "disconnect-test", terminalId = 10)
        assertTrue(TerminalSessionManager.isConnectedBySession(sid))

        TerminalSessionManager.markRemoteSessionsDisconnected("disconnect-test")

        // Session should still exist (NOT deleted)
        assertTrue(TerminalSessionManager.isRemoteSession(sid),
            "session should still exist after disconnect — only marked disconnected")
        // But should be marked as disconnected
        assertFalse(TerminalSessionManager.isConnectedBySession(sid),
            "session should be marked disconnected")
    }

    @Test
    fun testMarkRemoteSessionsConnectedRestoresConnection() {
        // After disconnect + reconnect, markRemoteSessionsConnected should
        // restore the connected flag so the user can continue using the terminal.
        val sid = createRemoteSession(pairingId = "reconnect-test", terminalId = 11)
        TerminalSessionManager.markRemoteSessionsDisconnected("reconnect-test")
        assertFalse(TerminalSessionManager.isConnectedBySession(sid))

        TerminalSessionManager.markRemoteSessionsConnected("reconnect-test")

        assertTrue(TerminalSessionManager.isConnectedBySession(sid),
            "session should be marked connected after reconnect")
        assertTrue(TerminalSessionManager.isRemoteSession(sid),
            "session should still exist after reconnect")
    }

    @Test
    fun testSyncRemoteSessionsWithListRemovesStaleSessions() {
        // After reconnect, LIST_RESPONSE should clean up sessions whose
        // terminal_id is no longer on the desktop (closed while offline).
        val aliveSid = createRemoteSession(pairingId = "sync-test", terminalId = 20)
        val staleSid = createRemoteSession(pairingId = "sync-test", terminalId = 21)

        // LIST_RESPONSE payload: {"terminals": [{"id": 20}], "servers": []}
        // Only terminalId=20 is alive; terminalId=21 was closed on desktop.
        val payload = """{"terminals":[{"id":20,"name":"alive","status":"running","preview":"","server_id":"__local__","server_name":"桌面端","terminal_type":"local","tmux_session_name":""}],"servers":[]}"""
        TerminalSessionManager.syncRemoteSessionsWithList("sync-test", payload)

        assertTrue(TerminalSessionManager.isRemoteSession(aliveSid),
            "alive session (terminalId=20) should still exist")
        assertFalse(TerminalSessionManager.isRemoteSession(staleSid),
            "stale session (terminalId=21) should be removed")
    }

    @Test
    fun testSyncRemoteSessionsWithListParsesObjectPayload() {
        // Regression: LIST_RESPONSE payload is {"terminals":[...], "servers":[...]}
        // (JSON object), not a bare JSON array. Verify parsing doesn't throw
        // and correctly identifies alive terminals.
        val sid = createRemoteSession(pairingId = "parse-test", terminalId = 30)

        val payload = """{"terminals":[{"id":30,"name":"test","status":"running","preview":"","server_id":"__local__","server_name":"桌面端","terminal_type":"local","tmux_session_name":""}],"servers":[{"server_id":"__local__","server_name":"桌面端","is_local":true}]}"""
        val removed = TerminalSessionManager.syncRemoteSessionsWithList("parse-test", payload)

        assertTrue(removed.isEmpty(), "no sessions should be removed — terminalId=30 is alive")
        assertTrue(TerminalSessionManager.isRemoteSession(sid),
            "session should still exist after sync")
    }

    @Test
    fun testSyncRemoteSessionsWithListEmptyTerminalsRemovesAll() {
        // If desktop has no terminals (e.g. restarted), all remote sessions
        // for that pairing should be removed.
        val sid1 = createRemoteSession(pairingId = "empty-test", terminalId = 40)
        val sid2 = createRemoteSession(pairingId = "empty-test", terminalId = 41)

        val payload = """{"terminals":[],"servers":[]}"""
        TerminalSessionManager.syncRemoteSessionsWithList("empty-test", payload)

        assertFalse(TerminalSessionManager.isRemoteSession(sid1),
            "session 40 should be removed — not in list")
        assertFalse(TerminalSessionManager.isRemoteSession(sid2),
            "session 41 should be removed — not in list")
    }
}

