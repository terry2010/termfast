package com.termfast.app.agent

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class AgentStateMachineTest {

    @Test
    fun testCreateAgentState() {
        val state = AgentStateMachine.createAgentState(1000L)
        assertEquals(AgentStatus.UNKNOWN, state.status)
        assertEquals(CliType.UNKNOWN, state.cli)
        assertEquals(1000L, state.lastOutputAt)
        assertEquals(1000L, state.lastStatusChangeAt)
        assertNull(state.pendingStatus)
    }

    // === applySignal ===

    @Test
    fun testApplySignalBlocked() {
        val state = AgentStateMachine.createAgentState(1000L)
        state.cli = CliType.DEVIN
        val signal = AgentSignal.Blocked(CliType.DEVIN, "needs input")
        AgentStateMachine.applySignal(state, signal, 2000L)
        assertEquals(AgentStatus.BLOCKED, state.status)
        assertEquals("needs input", state.blockedMessage)
        assertTrue(state.blockedFromOsc)
    }

    @Test
    fun testApplySignalDone() {
        val state = AgentStateMachine.createAgentState(1000L)
        state.cli = CliType.DEVIN
        state.status = AgentStatus.WORKING
        val signal = AgentSignal.Done(CliType.DEVIN)
        AgentStateMachine.applySignal(state, signal, 2000L)
        assertEquals(AgentStatus.DONE, state.status)
    }

    @Test
    fun testApplySignalNotifyDone() {
        val state = AgentStateMachine.createAgentState(1000L)
        state.cli = CliType.DEVIN
        state.status = AgentStatus.WORKING
        val signal = AgentSignal.Notify(CliType.DEVIN, "Devin finished", true)
        AgentStateMachine.applySignal(state, signal, 2000L)
        assertEquals(AgentStatus.DONE, state.status)
    }

    @Test
    fun testApplySignalNotifyBlocked() {
        val state = AgentStateMachine.createAgentState(1000L)
        state.cli = CliType.DEVIN
        state.status = AgentStatus.WORKING
        val signal = AgentSignal.Notify(CliType.DEVIN, "Devin needs input", false)
        AgentStateMachine.applySignal(state, signal, 2000L)
        assertEquals(AgentStatus.BLOCKED, state.status)
    }

    @Test
    fun testApplySignalTitleUpdatesCli() {
        val state = AgentStateMachine.createAgentState(1000L)
        val signal = AgentSignal.Title(CliType.OPENCODE, "OpenCode")
        AgentStateMachine.applySignal(state, signal, 2000L)
        assertEquals(CliType.OPENCODE, state.cli)
    }

    @Test
    fun testApplySignalNullReturnsCurrentStatus() {
        val state = AgentStateMachine.createAgentState(1000L)
        state.status = AgentStatus.WORKING
        val status = AgentStateMachine.applySignal(state, null, 2000L)
        assertEquals(AgentStatus.WORKING, status)
    }

    // === applyScreenStatus ===

    @Test
    fun testApplyScreenStatusBlocked() {
        val state = AgentStateMachine.createAgentState(1000L)
        AgentStateMachine.applyScreenStatus(state, AgentStatus.BLOCKED, "question?", 2000L)
        assertEquals(AgentStatus.BLOCKED, state.status)
        assertEquals("question?", state.blockedMessage)
        assertFalse(state.blockedFromOsc)
    }

    @Test
    fun testApplyScreenStatusWorking() {
        val state = AgentStateMachine.createAgentState(1000L)
        state.status = AgentStatus.IDLE
        AgentStateMachine.applyScreenStatus(state, AgentStatus.WORKING, null, 2000L)
        assertEquals(AgentStatus.WORKING, state.status)
    }

    @Test
    fun testApplyScreenStatusIdleDoesNotOverrideBlocked() {
        val state = AgentStateMachine.createAgentState(1000L)
        state.status = AgentStatus.BLOCKED
        AgentStateMachine.applyScreenStatus(state, AgentStatus.IDLE, null, 2000L)
        assertEquals(AgentStatus.BLOCKED, state.status)
    }

    @Test
    fun testApplyScreenStatusDone() {
        val state = AgentStateMachine.createAgentState(1000L)
        state.status = AgentStatus.WORKING
        AgentStateMachine.applyScreenStatus(state, AgentStatus.DONE, null, 2000L)
        assertEquals(AgentStatus.DONE, state.status)
        assertNull(state.blockedMessage)
    }

    // === clearScreenBlocked ===

    @Test
    fun testClearScreenBlocked() {
        val state = AgentStateMachine.createAgentState(1000L)
        state.status = AgentStatus.BLOCKED
        state.blockedMessage = "msg"
        state.blockedMissCount = 3
        AgentStateMachine.clearScreenBlocked(state, AgentStatus.WORKING, 2000L)
        assertEquals(AgentStatus.WORKING, state.status)
        assertNull(state.blockedMessage)
        assertEquals(0, state.blockedMissCount)
    }

    @Test
    fun testClearScreenBlockedNoOpIfNotBlocked() {
        val state = AgentStateMachine.createAgentState(1000L)
        state.status = AgentStatus.WORKING
        AgentStateMachine.clearScreenBlocked(state, AgentStatus.IDLE, 2000L)
        assertEquals(AgentStatus.WORKING, state.status)
    }

    // === notifyOutput ===

    @Test
    fun testNotifyOutputWhenIdleSchedulesWorking() {
        val state = AgentStateMachine.createAgentState(1000L)
        state.cli = CliType.DEVIN
        state.status = AgentStatus.IDLE
        AgentStateMachine.notifyOutput(state, 2000L)
        assertEquals(AgentStatus.WORKING, state.pendingStatus)
    }

    @Test
    fun testNotifyOutputWhenBlockedStaysBlocked() {
        val state = AgentStateMachine.createAgentState(1000L)
        state.cli = CliType.DEVIN
        state.status = AgentStatus.BLOCKED
        AgentStateMachine.notifyOutput(state, 2000L)
        assertEquals(AgentStatus.BLOCKED, state.status)
        assertEquals(2000L, state.lastOutputAt)
    }

    @Test
    fun testNotifyOutputWhenCliUnknownNoChange() {
        val state = AgentStateMachine.createAgentState(1000L)
        state.status = AgentStatus.IDLE
        AgentStateMachine.notifyOutput(state, 2000L)
        assertEquals(AgentStatus.IDLE, state.status)
    }

    // === tick (debounce + done→idle decay) ===

    @Test
    fun testTickFiresPendingTransition() {
        val state = AgentStateMachine.createAgentState(1000L)
        state.cli = CliType.DEVIN
        state.status = AgentStatus.IDLE
        // Schedule a debounced transition to WORKING
        AgentStateMachine.notifyOutput(state, 1000L)
        assertEquals(AgentStatus.WORKING, state.pendingStatus)
        // Tick before debounce window → no transition yet
        AgentStateMachine.tick(state, 1200L)
        assertEquals(AgentStatus.IDLE, state.status)
        // Tick after debounce window (500ms) → transition fires
        AgentStateMachine.tick(state, 1600L)
        assertEquals(AgentStatus.WORKING, state.status)
        assertNull(state.pendingStatus)
    }

    @Test
    fun testTickDoneToIdleDecay() {
        val state = AgentStateMachine.createAgentState(1000L)
        state.status = AgentStatus.DONE
        state.lastOutputAt = 1000L
        // Tick before decay timeout (5s) → stays DONE
        AgentStateMachine.tick(state, 4000L)
        assertEquals(AgentStatus.DONE, state.status)
        // Tick after decay timeout → transitions to IDLE
        AgentStateMachine.tick(state, 7000L)
        assertEquals(AgentStatus.IDLE, state.status)
    }

    // === setCliType ===

    @Test
    fun testSetCliType() {
        val state = AgentStateMachine.createAgentState(1000L)
        AgentStateMachine.setCliType(state, CliType.OPENCODE)
        assertEquals(CliType.OPENCODE, state.cli)
    }

    // === resetAgentState ===

    @Test
    fun testResetAgentState() {
        val state = AgentStateMachine.createAgentState(1000L)
        state.status = AgentStatus.WORKING
        state.cli = CliType.DEVIN
        state.blockedMessage = "msg"
        AgentStateMachine.resetAgentState(state, 5000L)
        assertEquals(AgentStatus.UNKNOWN, state.status)
        assertEquals(CliType.UNKNOWN, state.cli)
        assertNull(state.blockedMessage)
    }
}
