package com.termfast.app.agent

import java.util.concurrent.ConcurrentHashMap

/**
 * Agent status monitor — main scheduler for AI CLI autonomous parsing.
 *
 * Ported from desktop `src/hooks/useAgentStatus.ts` (~700 lines).
 *
 * Responsibilities:
 * 1. Maintain per-session AgentState (state machine)
 * 2. Process OSC signals (from OscInterceptor, via TerminalSessionManager.feedEmulator)
 * 3. Process screen snapshots (from snapshotFlow, collected by TerminalScreen)
 * 4. Detect CLI type (sticky — once detected, don't un-detect)
 * 5. Detect status, question, options, multi-select, multi-question, tabs
 * 6. Output AgentStatusState for the UI (AgentQuestionSheet)
 */
object AgentStatusMonitor {

    /** Per-session agent state (state machine). */
    private val states = ConcurrentHashMap<String, AgentState>()

    /** Per-session last computed status state (cached for UI). */
    private val statusStates = ConcurrentHashMap<String, AgentStatusState>()

    /** Per-session last snapshot processing timestamp (for 200ms throttle). */
    private val lastParseAt = ConcurrentHashMap<String, Long>()

    /** Per-session last seen terminal title (for re-detection on title change). */
    private val lastTitles = ConcurrentHashMap<String, String>()

    /** Minimum interval between snapshot processing (ms). */
    private const val MIN_PARSE_INTERVAL_MS = 200L

    /**
     * Get or create the agent state for a session.
     */
    private fun getOrCreateState(sessionId: String): AgentState {
        return states.computeIfAbsent(sessionId) {
            AgentStateMachine.createAgentState(System.nanoTime() / 1_000_000)
        }
    }

    /**
     * Get the current status state for a session (for UI rendering).
     * Returns a default IDLE state if the session has no monitor state.
     */
    fun getStatusState(sessionId: String): AgentStatusState {
        return statusStates[sessionId] ?: AgentStatusState.IDLE
    }

    /**
     * Handle an OSC signal from OscInterceptor.
     * Called by TerminalSessionManager.feedEmulator().
     */
    fun onOscSignal(sessionId: String, signal: AgentSignal) {
        val state = getOrCreateState(sessionId)
        val now = System.nanoTime() / 1_000_000
        AgentStateMachine.applySignal(state, signal, now)
        // OSC title may carry CLI type detection
        if (signal is AgentSignal.Title) {
            // CLI type is updated inside applySignal (sticky)
            // Also check if this CLI's title indicates blocked (e.g. Codex "Action Required")
            val behavior = CliBehaviorRegistry.getBehavior(state.cli)
            val titleResult = behavior.handleOscTitle(signal.title)
            if (titleResult != null) {
                AgentStateMachine.applyScreenStatus(state, titleResult.first, titleResult.second, now)
            }
        }
        updateStatusState(sessionId, state, null)
    }

    /**
     * Notify PTY output received (for working state detection).
     * Called by TerminalSessionManager when raw bytes arrive.
     */
    fun onOutput(sessionId: String) {
        val state = states[sessionId] ?: return
        val now = System.nanoTime() / 1_000_000
        AgentStateMachine.notifyOutput(state, now)
    }

    /**
     * Process a scraped snapshot — the main screen scraping entry point.
     * Called by TerminalScreen when snapshotFlow emits.
     *
     * @param sessionId the session ID
     * @param snapshot the ScrapedSnapshot (from TermlibAccess.toScrapedSnapshot)
     */
    fun processSnapshot(sessionId: String, snapshot: ScrapedSnapshot) {
        val state = getOrCreateState(sessionId)
        val now = System.nanoTime() / 1_000_000

        // 200ms throttle: skip if last parse was too recent (high-frequency snapshots)
        val lastParse = lastParseAt[sessionId] ?: 0L
        if (now - lastParse < MIN_PARSE_INTERVAL_MS) return
        lastParseAt[sessionId] = now

        // Tick the state machine (handle debounced transitions, done→idle decay)
        AgentStateMachine.tick(state, now)

        // Scrape screen text
        val screenLines = ScreenScraper.scrapeScreen(snapshot)
        val screenText = ScreenScraper.joinLines(screenLines)
        val terminalTitle = snapshot.terminalTitle

        // Detect CLI type (sticky — only if not already detected, OR title changed)
        val lastTitle = lastTitles[sessionId]
        val titleChanged = lastTitle != null && lastTitle != terminalTitle
        lastTitles[sessionId] = terminalTitle
        if (state.cli == CliType.UNKNOWN || titleChanged) {
            val detectedCli = CliDetector.detectCli(terminalTitle, screenText)
            if (detectedCli != CliType.UNKNOWN) {
                AgentStateMachine.setCliType(state, detectedCli)
            }
        }

        // If CLI is still unknown, no status detection possible
        if (state.cli == CliType.UNKNOWN) {
            updateStatusState(sessionId, state, snapshot)
            return
        }

        // Detect status from screen
        val detectedStatus = detectStatusFromScreen(state.cli, screenText)

        // Apply status to state machine
        if (detectedStatus != null) {
            // For blocked status, check miss count (flicker prevention)
            if (detectedStatus == AgentStatus.BLOCKED) {
                state.blockedMissCount = 0
                val message = extractQuestion(state.cli, screenText)
                AgentStateMachine.applyScreenStatus(state, AgentStatus.BLOCKED, message, now)
                state.isMultiSelect = detectMultiSelect(state.cli, screenText)
            } else {
                // Non-blocked status detected while blocked (screen-scrape-set)
                if (state.status == AgentStatus.BLOCKED && !state.blockedFromOsc) {
                    state.blockedMissCount++
                    if (state.blockedMissCount >= AgentStateMachine.BLOCKED_MISS_THRESHOLD) {
                        AgentStateMachine.clearScreenBlocked(state, detectedStatus, now)
                    }
                } else if (!state.blockedFromOsc) {
                    // Only apply screen status if blocked is not OSC-set (authoritative)
                    AgentStateMachine.applyScreenStatus(state, detectedStatus, null, now)
                }
            }
        } else {
            // No status detected — if blocked (screen-scrape-set), increment miss count
            if (state.status == AgentStatus.BLOCKED && !state.blockedFromOsc) {
                state.blockedMissCount++
                if (state.blockedMissCount >= AgentStateMachine.BLOCKED_MISS_THRESHOLD) {
                    AgentStateMachine.clearScreenBlocked(state, AgentStatus.WORKING, now)
                }
            }
        }

        // Update cached status state for UI
        updateStatusState(sessionId, state, snapshot)
    }

    /**
     * Update the cached AgentStatusState from the current AgentState + snapshot.
     * If snapshot is null (e.g. OSC-only update), use the last screen text.
     */
    private fun updateStatusState(sessionId: String, state: AgentState, snapshot: ScrapedSnapshot?) {
        if (state.status != AgentStatus.BLOCKED) {
            statusStates[sessionId] = AgentStatusState(
                status = state.status,
                cli = state.cli,
                blockedMessage = state.blockedMessage,
            )
            return
        }

        // Blocked — extract question details from snapshot if available
        if (snapshot == null) {
            // Keep existing blocked state, just update status/cli
            val existing = statusStates[sessionId]
            statusStates[sessionId] = (existing ?: AgentStatusState()).copy(
                status = state.status,
                cli = state.cli,
                blockedMessage = state.blockedMessage,
            )
            return
        }

        val screenLines = ScreenScraper.scrapeScreen(snapshot)
        val screenText = ScreenScraper.joinLines(screenLines)
        val isMultiSelect = detectMultiSelect(state.cli, screenText)
        val isMultiQuestion = detectMultiQuestion(state.cli, screenText)
        val question = extractQuestion(state.cli, screenText)
        val options = extractOptions(state.cli, screenText)
        val cursorIndex = extractCursorIndex(state.cli, screenText)
        val reviewAnswers = extractReviewAnswers(state.cli, screenText)

        // Extract tab info for multi-question dialogs
        var activeTabIndex = -1
        var totalTabs = 0
        if (isMultiQuestion) {
            val tabInfo = ScreenScraper.extractTabInfo(snapshot)
            if (tabInfo != null) {
                totalTabs = tabInfo.labels.size
                activeTabIndex = tabInfo.activeIndex
            }
        }

        statusStates[sessionId] = AgentStatusState(
            status = state.status,
            cli = state.cli,
            question = question,
            options = options,
            isMultiSelect = isMultiSelect,
            isMultiQuestion = isMultiQuestion,
            activeTabIndex = activeTabIndex,
            totalTabs = totalTabs,
            cursorIndex = cursorIndex,
            reviewAnswers = reviewAnswers,
            blockedMessage = state.blockedMessage,
        )
    }

    /**
     * Reset the monitor state for a session (e.g. when terminal is closed).
     */
    fun resetSession(sessionId: String) {
        states.remove(sessionId)
        statusStates.remove(sessionId)
        lastParseAt.remove(sessionId)
        lastTitles.remove(sessionId)
    }

    /**
     * Execute a behavior action (answer, toggle, confirm, etc.) and return
     * the keystroke steps to send to the PTY.
     *
     * Called by AgentQuestionSheet when the user interacts with the overlay.
     *
     * @param sessionId the session ID
     * @param action the action to execute
     * @return the ActionResult (keystroke steps + dismiss flag)
     */
    fun executeAction(sessionId: String, action: AgentAction): ActionResult {
        val state = states[sessionId] ?: return ActionResult(emptyList(), false)
        val statusState = statusStates[sessionId] ?: return ActionResult(emptyList(), false)
        val behavior = CliBehaviorRegistry.getBehavior(state.cli)
        val ctx = BehaviorContext(
            options = statusState.options,
            isMultiSelect = statusState.isMultiSelect,
            isMultiQuestion = statusState.isMultiQuestion,
            activeTabIndex = statusState.activeTabIndex,
            totalTabs = statusState.totalTabs,
        )
        return when (action) {
            is AgentAction.Answer -> behavior.answer(action.option, action.index, ctx)
            is AgentAction.Toggle -> behavior.toggle(action.option, action.index, ctx)
            is AgentAction.SubmitMultiSelect -> behavior.submitMultiSelect(ctx)
            is AgentAction.TextAnswer -> behavior.textAnswer(action.option, action.text, action.index, ctx)
            is AgentAction.TextCancel -> behavior.textCancel(ctx)
            is AgentAction.PrevQuestion -> behavior.prevQuestion(ctx)
            is AgentAction.NextQuestion -> behavior.nextQuestion(ctx)
            is AgentAction.Confirm -> behavior.confirm(action.hasAnswers, ctx)
        }
    }
}

/** Actions that the UI (AgentQuestionSheet) can trigger. */
sealed class AgentAction {
    data class Answer(val option: String, val index: Int) : AgentAction()
    data class Toggle(val option: String, val index: Int) : AgentAction()
    object SubmitMultiSelect : AgentAction()
    data class TextAnswer(val option: String, val text: String, val index: Int) : AgentAction()
    object TextCancel : AgentAction()
    object PrevQuestion : AgentAction()
    object NextQuestion : AgentAction()
    data class Confirm(val hasAnswers: Boolean) : AgentAction()
}
