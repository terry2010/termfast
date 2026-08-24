package com.termfast.app.agent

/**
 * Agent state machine — pure functions for AI CLI status transitions.
 *
 * Ported from desktop `src/hooks/agentStateMachine.ts` (320 lines).
 *
 * Design (TAME — Terminal Agent Management Environment):
 *   - Priority states (blocked) bypass debounce — fire immediately
 *   - Non-priority transitions debounce 500ms to avoid status flicker
 *   - "done" auto-decays to "idle" after 5s of no activity
 *   - Any PTY output while idle/working resets the idle timer
 *
 * The state machine is CLI-agnostic. Different CLIs feed different signals
 * via applySignal(); the transition rules are universal.
 */

/** Internal state — includes timers that drive time-based transitions. */
data class AgentState(
    var status: AgentStatus = AgentStatus.UNKNOWN,
    var cli: CliType = CliType.UNKNOWN,
    /** Monotonic timestamp (ms) of last PTY output. */
    var lastOutputAt: Long = 0L,
    /** Monotonic timestamp (ms) of last status change. */
    var lastStatusChangeAt: Long = 0L,
    /** Pending status from a debounced transition (null = none pending). */
    var pendingStatus: AgentStatus? = null,
    /** Monotonic timestamp (ms) when the pending status should fire. */
    var pendingFireAt: Long = 0L,
    /** Blocked reason message. */
    var blockedMessage: String? = null,
    /** True if blocked status was set by an OSC signal (authoritative). */
    var blockedFromOsc: Boolean = false,
    /** Consecutive screen scrapes that did NOT detect "blocked" while blocked. */
    var blockedMissCount: Int = 0,
    /** True if the current blocked dialog is multi-select. */
    var isMultiSelect: Boolean = false,
)

object AgentStateMachine {
    /** Debounce window for non-priority transitions (ms). */
    private const val DEBOUNCE_MS = 500L

    /** After "done", auto-decay to "idle" after this long with no activity (ms). */
    private const val DONE_TO_IDLE_MS = 5000L

    /** Fallback timeout: working → done after this long with no output (ms). */
    private const val WORKING_IDLE_TIMEOUT_MS = 60000L

    /** When blocked (screen-scrape-set), consecutive misses before clearing. */
    const val BLOCKED_MISS_THRESHOLD = 3

    /** Create an initial agent state. */
    fun createAgentState(now: Long): AgentState = AgentState(
        status = AgentStatus.UNKNOWN,
        cli = CliType.UNKNOWN,
        lastOutputAt = now,
        lastStatusChangeAt = now,
    )

    /**
     * Apply a signal to the state machine.
     * @param state current state (mutated in place)
     * @param signal the signal to apply (or null for a tick)
     * @param now current monotonic timestamp (ms)
     * @return the new status
     */
    fun applySignal(state: AgentState, signal: AgentSignal?, now: Long): AgentStatus {
        if (signal == null) return state.status

        // Update CLI type from signal (sticky)
        if (signal.cli != CliType.UNKNOWN && state.cli == CliType.UNKNOWN) {
            state.cli = signal.cli
        }

        when (signal) {
            is AgentSignal.Blocked -> {
                transitionTo(state, AgentStatus.BLOCKED, now)
                state.blockedMessage = signal.message
                state.blockedFromOsc = true
                state.pendingStatus = null
            }
            is AgentSignal.Done -> {
                transitionTo(state, AgentStatus.DONE, now)
                state.blockedMessage = null
                state.blockedFromOsc = false
                state.pendingStatus = null
            }
            is AgentSignal.Notify -> {
                if (signal.done) {
                    transitionTo(state, AgentStatus.DONE, now)
                    state.blockedMessage = null
                    state.blockedFromOsc = false
                } else {
                    transitionTo(state, AgentStatus.BLOCKED, now)
                    state.blockedMessage = signal.message
                    state.blockedFromOsc = true
                }
                state.pendingStatus = null
            }
            is AgentSignal.Title -> {
                // Title signal updates CLI type but doesn't change status.
                // Status from screen scraping is applied via applyScreenStatus().
            }
        }
        return state.status
    }

    /**
     * Apply a status detected from screen scraping.
     * Used by CLIs that don't emit OSC status signals (OpenCode, Claude Code, Codex).
     */
    fun applyScreenStatus(
        state: AgentState,
        status: AgentStatus,
        message: String?,
        now: Long,
    ) {
        when (status) {
            AgentStatus.BLOCKED -> {
                transitionTo(state, AgentStatus.BLOCKED, now)
                state.blockedMessage = message
                state.blockedFromOsc = false
                state.pendingStatus = null
                state.blockedMissCount = 0
            }
            AgentStatus.DONE -> {
                transitionTo(state, AgentStatus.DONE, now)
                state.blockedMessage = null
                state.blockedFromOsc = false
                state.pendingStatus = null
            }
            AgentStatus.WORKING -> {
                transitionTo(state, AgentStatus.WORKING, now)
                state.pendingStatus = null
            }
            AgentStatus.IDLE -> {
                // Don't override blocked or done with idle from screen.
                if (state.status != AgentStatus.BLOCKED && state.status != AgentStatus.DONE) {
                    transitionTo(state, AgentStatus.IDLE, now)
                    state.pendingStatus = null
                }
            }
            AgentStatus.UNKNOWN -> { /* no-op */ }
        }
    }

    /**
     * Force-clear blocked status set by screen scraping.
     * Used when blockedMissCount reaches threshold.
     */
    fun clearScreenBlocked(state: AgentState, targetStatus: AgentStatus, now: Long) {
        if (state.status != AgentStatus.BLOCKED) return
        transitionTo(state, targetStatus, now)
        state.blockedMessage = null
        state.blockedFromOsc = false
        state.pendingStatus = null
        state.blockedMissCount = 0
        state.isMultiSelect = false
    }

    /**
     * Notify the state machine that PTY output was received.
     * Any output → working (unless already blocked or CLI unknown).
     */
    fun notifyOutput(state: AgentState, now: Long): AgentStatus {
        state.lastOutputAt = now
        if (state.status == AgentStatus.BLOCKED) return state.status
        if (state.cli == CliType.UNKNOWN) return state.status
        if (state.status == AgentStatus.IDLE || state.status == AgentStatus.DONE ||
            state.status == AgentStatus.UNKNOWN) {
            scheduleDebounced(state, AgentStatus.WORKING, now)
        }
        return state.status
    }

    /**
     * Tick the state machine — called on a regular interval (e.g. every 500ms).
     * Handles debounced transitions and time-based decay (done → idle).
     */
    fun tick(state: AgentState, now: Long): AgentStatus {
        // Fire pending debounced transition
        if (state.pendingStatus != null && now >= state.pendingFireAt) {
            transitionTo(state, state.pendingStatus!!, now)
            state.pendingStatus = null
        }
        // done → idle after DONE_TO_IDLE_MS of no activity
        if (state.status == AgentStatus.DONE && now - state.lastOutputAt >= DONE_TO_IDLE_MS) {
            transitionTo(state, AgentStatus.IDLE, now)
        }
        // working → done after WORKING_IDLE_TIMEOUT_MS of no output
        if (state.status == AgentStatus.WORKING && state.cli != CliType.UNKNOWN &&
            now - state.lastOutputAt >= WORKING_IDLE_TIMEOUT_MS) {
            transitionTo(state, AgentStatus.DONE, now)
            state.pendingStatus = null
        }
        return state.status
    }

    /** Transition to a new status, recording the timestamp. */
    private fun transitionTo(state: AgentState, status: AgentStatus, now: Long) {
        if (state.status == status) return
        state.status = status
        state.lastStatusChangeAt = now
    }

    /** Schedule a debounced transition (non-priority states only). */
    private fun scheduleDebounced(state: AgentState, status: AgentStatus, now: Long) {
        // Priority states (blocked) fire immediately
        if (status == AgentStatus.BLOCKED) {
            transitionTo(state, status, now)
            state.pendingStatus = null
        } else {
            state.pendingStatus = status
            state.pendingFireAt = now + DEBOUNCE_MS
        }
    }

    /** Set the CLI type explicitly. Does not change the status. */
    fun setCliType(state: AgentState, cli: CliType) {
        state.cli = cli
    }

    /** Reset the state machine. */
    fun resetAgentState(state: AgentState, now: Long) {
        state.status = AgentStatus.UNKNOWN
        state.cli = CliType.UNKNOWN
        state.lastOutputAt = now
        state.lastStatusChangeAt = now
        state.pendingStatus = null
        state.pendingFireAt = 0L
        state.blockedMessage = null
        state.blockedFromOsc = false
        state.blockedMissCount = 0
        state.isMultiSelect = false
    }
}
