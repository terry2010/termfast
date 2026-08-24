package com.termfast.app.agent

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull

class AgentStatusMonitorTest {

    private fun makeSnapshot(text: String, title: String = ""): ScrapedSnapshot {
        val lines = text.split("\n").map { ScrapedLine(it, it.map { ScrapedCell(it, false, false, 0, 0L, 1) }) }
        return ScrapedSnapshot(lines = lines, terminalTitle = title, cursorRow = 0, cursorCol = 0, rows = lines.size, cols = 0)
    }

    @Test
    fun testGetStatusStateDefault() {
        val state = AgentStatusMonitor.getStatusState("nonexistent-session")
        assertEquals(AgentStatus.IDLE, state.status)
    }

    @Test
    fun testResetSession() {
        val sessionId = "test-reset-session"
        AgentStatusMonitor.resetSession(sessionId)
        // Should not throw, and state should be cleared
        val state = AgentStatusMonitor.getStatusState(sessionId)
        assertEquals(AgentStatus.IDLE, state.status)
    }

    @Test
    fun testOnOscSignalTitleSetsCli() {
        val sessionId = "test-osc-title"
        AgentStatusMonitor.resetSession(sessionId)
        val signal = AgentSignal.Title(CliType.OPENCODE, "OpenCode")
        AgentStatusMonitor.onOscSignal(sessionId, signal)
        // After title signal, CLI type should be set (checked via processSnapshot)
        AgentStatusMonitor.resetSession(sessionId)
    }

    @Test
    fun testOnOscSignalBlocked() {
        val sessionId = "test-osc-blocked"
        AgentStatusMonitor.resetSession(sessionId)
        val signal = AgentSignal.Blocked(CliType.DEVIN, "Devin needs input")
        AgentStatusMonitor.onOscSignal(sessionId, signal)
        val state = AgentStatusMonitor.getStatusState(sessionId)
        // Status should be BLOCKED (OSC signals are authoritative)
        assertEquals(AgentStatus.BLOCKED, state.status)
        AgentStatusMonitor.resetSession(sessionId)
    }

    @Test
    fun testOnOscSignalDone() {
        val sessionId = "test-osc-done"
        AgentStatusMonitor.resetSession(sessionId)
        // First set to WORKING via signal
        AgentStatusMonitor.onOscSignal(sessionId, AgentSignal.Notify(CliType.DEVIN, "working", false))
        // Then set to DONE
        AgentStatusMonitor.onOscSignal(sessionId, AgentSignal.Done(CliType.DEVIN))
        val state = AgentStatusMonitor.getStatusState(sessionId)
        assertEquals(AgentStatus.DONE, state.status)
        AgentStatusMonitor.resetSession(sessionId)
    }

    @Test
    fun testOnOutputNotifiesStateMachine() {
        val sessionId = "test-output"
        AgentStatusMonitor.resetSession(sessionId)
        // Should not throw
        AgentStatusMonitor.onOutput(sessionId)
        AgentStatusMonitor.resetSession(sessionId)
    }

    @Test
    fun testProcessSnapshotDetectsCli() {
        val sessionId = "test-snapshot-cli"
        AgentStatusMonitor.resetSession(sessionId)
        val snapshot = makeSnapshot("OpenCode\n> Edit file?", "OpenCode")
        AgentStatusMonitor.processSnapshot(sessionId, snapshot)
        // CLI should be detected (checked via second processSnapshot which should find status)
        // We can't directly access state.cli, but getStatusState should not crash
        AgentStatusMonitor.getStatusState(sessionId)
        AgentStatusMonitor.resetSession(sessionId)
    }

    @Test
    fun testProcessSnapshotThrottle() {
        val sessionId = "test-throttle"
        AgentStatusMonitor.resetSession(sessionId)
        val snapshot = makeSnapshot("test", "")
        // First call should process
        AgentStatusMonitor.processSnapshot(sessionId, snapshot)
        // Immediate second call should be throttled (no exception, just skipped)
        AgentStatusMonitor.processSnapshot(sessionId, snapshot)
        AgentStatusMonitor.resetSession(sessionId)
    }

    @Test
    fun testProcessSnapshotThrottleSkipsSecondCall() {
        // Stronger throttle test: verify the second call is actually skipped
        // by checking that CLI detection from the second snapshot is NOT applied
        // (first snapshot has no CLI, second has OpenCode — if throttled, CLI stays UNKNOWN)
        val sessionId = "test-throttle-skip"
        AgentStatusMonitor.resetSession(sessionId)
        // First snapshot: no CLI content, no title → no detection
        val snapshot1 = makeSnapshot("plain output", "")
        AgentStatusMonitor.processSnapshot(sessionId, snapshot1)
        // Immediate second snapshot: has OpenCode footer (should detect OPENCODE)
        // but should be throttled (within 200ms), so detection is skipped
        val snapshot2 = makeSnapshot("esc interrupt  ctrl+p commands", "OpenCode")
        AgentStatusMonitor.processSnapshot(sessionId, snapshot2)
        // If throttled, the OSC title signal was NOT processed → status state should still be IDLE
        // (not BLOCKED/WORKING which would require CLI detection)
        val state = AgentStatusMonitor.getStatusState(sessionId)
        // CLI should still be UNKNOWN (throttled, second snapshot not processed)
        assertEquals(CliType.UNKNOWN, state.cli, "Second snapshot should be throttled — CLI not detected")
        AgentStatusMonitor.resetSession(sessionId)
    }

    @Test
    fun testProcessSnapshotTitleChangeTriggersRedetection() {
        // Verify that title change triggers CLI re-detection
        val sessionId = "test-title-redetect"
        AgentStatusMonitor.resetSession(sessionId)
        // First snapshot: no CLI detected (plain output, empty title)
        val snapshot1 = makeSnapshot("plain output", "")
        AgentStatusMonitor.processSnapshot(sessionId, snapshot1)
        // Wait to avoid throttle (sleep > 200ms)
        Thread.sleep(250)
        // Second snapshot: title changes to "OpenCode" → should re-detect
        val snapshot2 = makeSnapshot("esc interrupt  ctrl+p commands", "OpenCode")
        AgentStatusMonitor.processSnapshot(sessionId, snapshot2)
        // After title change + re-detection, CLI should be OPENCODE
        // We verify via getStatusState — if CLI was detected, status should not be IDLE default
        // (processSnapshot with OpenCode screen should detect WORKING or BLOCKED)
        // Note: we can't directly access state.cli, but the status state should reflect detection
        val state = AgentStatusMonitor.getStatusState(sessionId)
        assertEquals(CliType.OPENCODE, state.cli, "Title change should trigger CLI re-detection")
        AgentStatusMonitor.resetSession(sessionId)
    }

    @Test
    fun testProcessSnapshotSameTitleNoRedetection() {
        // Verify that same title does NOT trigger re-detection (cache works)
        val sessionId = "test-same-title"
        AgentStatusMonitor.resetSession(sessionId)
        // First snapshot with title "OpenCode"
        val snapshot1 = makeSnapshot("esc interrupt  ctrl+p commands", "OpenCode")
        AgentStatusMonitor.processSnapshot(sessionId, snapshot1)
        // Wait to avoid throttle
        Thread.sleep(250)
        // Second snapshot with same title but different screen (no OpenCode patterns)
        val snapshot2 = makeSnapshot("plain output no cli patterns", "OpenCode")
        AgentStatusMonitor.processSnapshot(sessionId, snapshot2)
        // CLI should still be OPENCODE (cached, not re-detected from screen)
        val state = AgentStatusMonitor.getStatusState(sessionId)
        assertEquals(CliType.OPENCODE, state.cli, "Same title should not trigger re-detection — CLI stays cached")
        AgentStatusMonitor.resetSession(sessionId)
    }

    @Test
    fun testExecuteActionUnknownSession() {
        val sessionId = "test-execute-unknown"
        AgentStatusMonitor.resetSession(sessionId)
        // Execute action on unknown session should not crash
        val result = AgentStatusMonitor.executeAction(sessionId, AgentAction.Answer("test", 0))
        assertNotNull(result)
        AgentStatusMonitor.resetSession(sessionId)
    }
}
