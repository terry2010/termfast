package com.termfast.app.agent

/**
 * CLI behavior — per-CLI behavior abstraction for terminal interaction.
 *
 * Ported from desktop `src/hooks/cliBehavior.ts` (546 lines).
 * Replaces if-else chains with a registry-based dispatch.
 */

/** A single keystroke to send, with an optional delay before the next step. */
data class KeystrokeStep(
    val data: String,
    val delayAfter: Long? = null,
)

/** Result of a behavior action: what to send + UI side-effects. */
data class ActionResult(
    val steps: List<KeystrokeStep>,
    val dismiss: Boolean,
    val newCursorPos: Int? = null,
)

/** Context passed to behavior action methods. */
data class BehaviorContext(
    val options: List<String>?,
    val isMultiSelect: Boolean,
    val isMultiQuestion: Boolean,
    val activeTabIndex: Int,
    val totalTabs: Int,
    val cursorPos: Int = 0,
    val otherEditing: Boolean = false,
    val hasExistingText: Boolean = false,
)

/** UI state for overlay rendering decisions. */
data class UiState(
    val isFirstQuestion: Boolean,
    val isLastQuestion: Boolean,
    val isMultiSelect: Boolean,
    val totalTabs: Int,
    val activeTabIndex: Int,
)

/** Per-CLI behavior interface. */
interface CliBehavior {
    fun answer(option: String, index: Int, ctx: BehaviorContext): ActionResult
    fun toggle(option: String, index: Int, ctx: BehaviorContext): ActionResult
    fun submitMultiSelect(ctx: BehaviorContext): ActionResult
    fun textAnswer(option: String, text: String, index: Int, ctx: BehaviorContext): ActionResult
    fun textCancel(ctx: BehaviorContext): ActionResult
    fun prevQuestion(ctx: BehaviorContext): ActionResult
    fun nextQuestion(ctx: BehaviorContext): ActionResult
    fun confirm(hasAnswers: Boolean, ctx: BehaviorContext): ActionResult

    fun lastQuestionIndex(totalTabs: Int): Int
    fun hidePrev(ui: UiState): Boolean
    fun hideNext(ui: UiState): Boolean
    val syncCheckedFromScreen: Boolean
    val hasSkipOnLastQuestion: Boolean
    val cacheOptionsOnOther: Boolean
    fun detectOtherExpanded(screenText: String, isMultiSelect: Boolean, isMultiQuestion: Boolean): Boolean
    fun handleOscTitle(title: String): Pair<AgentStatus, String>?
}

object CliBehaviorRegistry {
    private val behaviors: Map<CliType, CliBehavior> = mapOf(
        CliType.DEVIN to DevinBehavior,
        CliType.OPENCODE to OpenCodeBehavior,
        CliType.CLAUDE_CODE to ClaudeCodeBehavior,
        CliType.CODEX to CodexBehavior,
    )
    private val defaultBehavior = DefaultBehavior

    fun getBehavior(cli: CliType): CliBehavior = behaviors[cli] ?: defaultBehavior
}

// === SECTION 1 END ===

// ── Default behavior (unknown CLI) ───────────────────────────────────────────

object DefaultBehavior : CliBehavior {
    override fun answer(option: String, index: Int, ctx: BehaviorContext) = ActionResult(
        steps = listOf(KeystrokeStep(
            AnswerSubmitter.submitAnswer(CliType.UNKNOWN, option, index, ctx.options?.size, ctx.isMultiQuestion)
        )),
        dismiss = !ctx.isMultiQuestion,
    )
    override fun toggle(option: String, index: Int, ctx: BehaviorContext) = ActionResult(
        steps = listOf(KeystrokeStep(
            AnswerSubmitter.submitAnswer(CliType.UNKNOWN, option, index)
        )),
        dismiss = false,
    )
    override fun submitMultiSelect(ctx: BehaviorContext) = ActionResult(
        steps = listOf(KeystrokeStep("\r")), dismiss = true,
    )
    override fun textAnswer(option: String, text: String, index: Int, ctx: BehaviorContext) = ActionResult(
        steps = listOf(
            KeystrokeStep(option + "\r"),
            KeystrokeStep(text + "\r", TEXT_ANSWER_DELAY_MS),
        ),
        dismiss = false,
    )
    override fun textCancel(ctx: BehaviorContext) = ActionResult(steps = emptyList(), dismiss = false)
    override fun prevQuestion(ctx: BehaviorContext) = ActionResult(
        steps = listOf(KeystrokeStep(AnswerSubmitter.navigatePrevQuestion())),
        dismiss = false, newCursorPos = 0,
    )
    override fun nextQuestion(ctx: BehaviorContext) = ActionResult(
        steps = listOf(KeystrokeStep(AnswerSubmitter.navigateNextQuestion())),
        dismiss = false, newCursorPos = 0,
    )
    override fun confirm(hasAnswers: Boolean, ctx: BehaviorContext) = ActionResult(
        steps = listOf(KeystrokeStep("\r")), dismiss = true,
    )
    override fun lastQuestionIndex(totalTabs: Int) = totalTabs - 2
    override fun hidePrev(ui: UiState) = false
    override fun hideNext(ui: UiState) = false
    override val syncCheckedFromScreen = false
    override val hasSkipOnLastQuestion = false
    override val cacheOptionsOnOther = false
    override fun detectOtherExpanded(screenText: String, isMultiSelect: Boolean, isMultiQuestion: Boolean) = false
    override fun handleOscTitle(title: String): Pair<AgentStatus, String>? = null
}

// === SECTION 2 END ===

// ── Devin behavior ────────────────────────────────────────────────────────────

object DevinBehavior : CliBehavior {
    override fun answer(option: String, index: Int, ctx: BehaviorContext) = ActionResult(
        steps = listOf(KeystrokeStep(
            AnswerSubmitter.submitAnswer(CliType.DEVIN, option, index, ctx.options?.size, ctx.isMultiQuestion)
        )),
        dismiss = !ctx.isMultiQuestion,
    )
    override fun toggle(option: String, index: Int, ctx: BehaviorContext) = ActionResult(
        steps = listOf(KeystrokeStep(AnswerSubmitter.toggleDevinOption(index, ctx.cursorPos))),
        dismiss = false, newCursorPos = index,
    )
    override fun submitMultiSelect(ctx: BehaviorContext) = ActionResult(
        steps = listOf(KeystrokeStep(AnswerSubmitter.submitDevinMultiSelect())),
        dismiss = true,
    )
    override fun textAnswer(option: String, text: String, index: Int, ctx: BehaviorContext): ActionResult {
        val parts = AnswerSubmitter.submitDevinTextAnswer(option, text, index, ctx.cursorPos)
        return ActionResult(
            steps = listOf(
                KeystrokeStep(parts.navigate),
                KeystrokeStep(parts.type, TEXT_ANSWER_DELAY_MS),
            ),
            dismiss = !ctx.isMultiQuestion,
            newCursorPos = 0,
        )
    }
    override fun textCancel(ctx: BehaviorContext) = ActionResult(
        steps = listOf(KeystrokeStep("\u001B")), dismiss = false,
    )
    override fun prevQuestion(ctx: BehaviorContext): ActionResult {
        if (ctx.otherEditing) {
            return ActionResult(
                steps = listOf(
                    KeystrokeStep("\u001B[A", 300),
                    KeystrokeStep("\u001B[D", 300),
                    KeystrokeStep("\u001B[B"),
                ),
                dismiss = false, newCursorPos = 0,
            )
        }
        return ActionResult(
            steps = listOf(KeystrokeStep(AnswerSubmitter.navigatePrevQuestion())),
            dismiss = false, newCursorPos = 0,
        )
    }
    override fun nextQuestion(ctx: BehaviorContext): ActionResult {
        if (ctx.otherEditing) {
            return ActionResult(
                steps = listOf(
                    KeystrokeStep("\u001B[A", 300),
                    KeystrokeStep("\u001B[C", 300),
                    KeystrokeStep("\u001B[B"),
                ),
                dismiss = false, newCursorPos = 0,
            )
        }
        return ActionResult(
            steps = listOf(KeystrokeStep(AnswerSubmitter.navigateNextQuestion())),
            dismiss = false, newCursorPos = 0,
        )
    }
    override fun confirm(hasAnswers: Boolean, ctx: BehaviorContext): ActionResult {
        val hasOptions = ctx.options?.isNotEmpty() == true
        val ks = AnswerSubmitter.submitDevinConfirm(
            hasOptions, ctx.activeTabIndex, ctx.totalTabs, ctx.isMultiSelect, hasAnswers,
        )
        return ActionResult(steps = listOf(KeystrokeStep(ks)), dismiss = true)
    }
    // Devin has NO Confirm tab — all tabs are question tabs.
    override fun lastQuestionIndex(totalTabs: Int) = totalTabs - 1
    override fun hidePrev(ui: UiState) = ui.isFirstQuestion
    override fun hideNext(ui: UiState) = ui.isLastQuestion
    override val syncCheckedFromScreen = false
    override val hasSkipOnLastQuestion = true
    override val cacheOptionsOnOther = true
    override fun detectOtherExpanded(screenText: String, isMultiSelect: Boolean, isMultiQuestion: Boolean): Boolean {
        if (!isMultiQuestion && !isMultiSelect) return false
        return Regex("^\\s*❭\\s+Other\\s*\\(type your own\\)", setOf(RegexOption.IGNORE_CASE, RegexOption.MULTILINE))
            .containsMatchIn(screenText)
    }
    override fun handleOscTitle(title: String): Pair<AgentStatus, String>? = null
}

// === SECTION 3 END ===

// ── OpenCode behavior ──────────────────────────────────────────────────────────

object OpenCodeBehavior : CliBehavior {
    override fun answer(option: String, index: Int, ctx: BehaviorContext): ActionResult {
        val normalized = option.lowercase().trim()
        // "Reject" may enter RejectPrompt sub-dialog — split into two steps
        if (normalized == "reject") {
            return ActionResult(
                steps = listOf(
                    KeystrokeStep("l".repeat(index), TEXT_ANSWER_DELAY_MS),
                    KeystrokeStep("\r"),
                ),
                dismiss = false,
            )
        }
        val ks = AnswerSubmitter.submitAnswer(CliType.OPENCODE, option, index, ctx.options?.size, ctx.isMultiQuestion)
        return ActionResult(steps = listOf(KeystrokeStep(ks)), dismiss = false)
    }
    override fun toggle(option: String, index: Int, ctx: BehaviorContext) = ActionResult(
        steps = listOf(KeystrokeStep(AnswerSubmitter.toggleOpenCodeOption(option))),
        dismiss = false,
    )
    override fun submitMultiSelect(ctx: BehaviorContext) = ActionResult(
        steps = listOf(KeystrokeStep(AnswerSubmitter.submitOpenCodeMultiSelect())),
        dismiss = true,
    )
    override fun textAnswer(option: String, text: String, index: Int, ctx: BehaviorContext): ActionResult {
        val parts = AnswerSubmitter.submitOpenCodeTextAnswer(option, text, ctx.hasExistingText)
        val steps = mutableListOf<KeystrokeStep>()
        steps.add(KeystrokeStep(parts.navigate, TEXT_ANSWER_DELAY_MS))
        // Multi-select with existing text: need second number key to enter editing
        if (ctx.isMultiSelect && ctx.hasExistingText) {
            steps.add(KeystrokeStep(parts.navigate, TEXT_ANSWER_DELAY_MS))
        }
        if (parts.clear != null) {
            steps.add(KeystrokeStep(parts.clear, TEXT_ANSWER_DELAY_MS))
        }
        steps.add(KeystrokeStep(parts.type, TEXT_ANSWER_DELAY_MS))
        // Multi-select multi-question: Tab to advance to next question
        if (ctx.isMultiSelect && ctx.isMultiQuestion) {
            steps.add(KeystrokeStep("\t", TEXT_ANSWER_DELAY_MS))
        }
        return ActionResult(steps = steps, dismiss = !ctx.isMultiQuestion)
    }
    override fun textCancel(ctx: BehaviorContext) = ActionResult(steps = emptyList(), dismiss = false)
    override fun prevQuestion(ctx: BehaviorContext) = ActionResult(
        steps = listOf(KeystrokeStep(AnswerSubmitter.navigatePrevQuestion())),
        dismiss = false, newCursorPos = 0,
    )
    override fun nextQuestion(ctx: BehaviorContext) = ActionResult(
        steps = listOf(KeystrokeStep(AnswerSubmitter.navigateNextQuestion())),
        dismiss = false, newCursorPos = 0,
    )
    override fun confirm(hasAnswers: Boolean, ctx: BehaviorContext): ActionResult {
        val hasOptions = ctx.options?.isNotEmpty() == true
        val ks = AnswerSubmitter.submitOpenCodeConfirm(hasOptions, ctx.activeTabIndex, ctx.totalTabs)
        return ActionResult(steps = listOf(KeystrokeStep(ks)), dismiss = true)
    }
    override fun lastQuestionIndex(totalTabs: Int) = totalTabs - 2
    override fun hidePrev(ui: UiState) = false
    override fun hideNext(ui: UiState) = false
    override val syncCheckedFromScreen = false
    override val hasSkipOnLastQuestion = false
    override val cacheOptionsOnOther = false
    override fun detectOtherExpanded(screenText: String, isMultiSelect: Boolean, isMultiQuestion: Boolean) = false
    override fun handleOscTitle(title: String): Pair<AgentStatus, String>? = null
}

// === SECTION 4 END ===

// ── Claude Code behavior ──────────────────────────────────────────────────────

object ClaudeCodeBehavior : CliBehavior {
    override fun answer(option: String, index: Int, ctx: BehaviorContext): ActionResult {
        // Plan Mode: Down arrows + delayed Enter
        if (AnswerSubmitter.isClaudeCodePlanModeOption(option)) {
            val navKeys = AnswerSubmitter.buildClaudeCodePlanModeNavigate(index)
            return ActionResult(
                steps = listOf(
                    KeystrokeStep(navKeys),
                    KeystrokeStep("\r", TEXT_ANSWER_DELAY_MS),
                ),
                dismiss = true,
            )
        }
        val ks = AnswerSubmitter.submitAnswer(CliType.CLAUDE_CODE, option, index)
        return ActionResult(steps = listOf(KeystrokeStep(ks)), dismiss = !ctx.isMultiQuestion)
    }
    override fun toggle(option: String, index: Int, ctx: BehaviorContext) = ActionResult(
        steps = listOf(KeystrokeStep(AnswerSubmitter.toggleClaudeCodeOption(option))),
        dismiss = false,
    )
    override fun submitMultiSelect(ctx: BehaviorContext) = ActionResult(
        steps = listOf(
            KeystrokeStep("\t"),
            KeystrokeStep("\r", TEXT_ANSWER_SUBMIT_DELAY_MS),
        ),
        dismiss = true,
    )
    override fun textAnswer(option: String, text: String, index: Int, ctx: BehaviorContext): ActionResult {
        val parts = AnswerSubmitter.submitClaudeCodeTextAnswer(
            option, text, index, ctx.isMultiSelect, ctx.hasExistingText,
        )
        val steps = mutableListOf<KeystrokeStep>()
        steps.add(KeystrokeStep(parts.navigate, if (parts.navigate.isNotEmpty()) TEXT_ANSWER_DELAY_MS else null))
        steps.add(KeystrokeStep(parts.type, TEXT_ANSWER_DELAY_MS))
        if (parts.submit != null) {
            for (char in parts.submit) {
                steps.add(KeystrokeStep(char.toString(), TEXT_ANSWER_SUBMIT_DELAY_MS))
            }
        }
        return ActionResult(steps = steps, dismiss = !ctx.isMultiQuestion)
    }
    override fun textCancel(ctx: BehaviorContext) = ActionResult(steps = emptyList(), dismiss = false)
    override fun prevQuestion(ctx: BehaviorContext) = ActionResult(
        steps = listOf(KeystrokeStep(AnswerSubmitter.navigatePrevQuestion())),
        dismiss = false, newCursorPos = 0,
    )
    override fun nextQuestion(ctx: BehaviorContext) = ActionResult(
        steps = listOf(KeystrokeStep(AnswerSubmitter.navigateNextQuestion())),
        dismiss = false, newCursorPos = 0,
    )
    override fun confirm(hasAnswers: Boolean, ctx: BehaviorContext): ActionResult {
        if (ctx.isMultiSelect) {
            return ActionResult(
                steps = listOf(KeystrokeStep(AnswerSubmitter.submitClaudeCodeMultiSelect())),
                dismiss = true,
            )
        }
        val hasOptions = ctx.options?.isNotEmpty() == true
        val ks = AnswerSubmitter.submitClaudeCodeConfirm(hasOptions, ctx.activeTabIndex, ctx.totalTabs)
        // Split arrows and Enter into separate steps (React state flush delay)
        val enterIdx = ks.lastIndexOf("\r")
        if (enterIdx > 0) {
            val arrows = ks.substring(0, enterIdx)
            val enter = ks.substring(enterIdx)
            return ActionResult(
                steps = listOf(
                    KeystrokeStep(arrows, TEXT_ANSWER_DELAY_MS),
                    KeystrokeStep(enter, TEXT_ANSWER_SUBMIT_DELAY_MS),
                ),
                dismiss = true,
            )
        }
        return ActionResult(steps = listOf(KeystrokeStep(ks)), dismiss = true)
    }
    override fun lastQuestionIndex(totalTabs: Int) = totalTabs - 2
    override fun hidePrev(ui: UiState) = ui.isMultiSelect && ui.totalTabs == 2
    override fun hideNext(ui: UiState): Boolean {
        if (ui.isMultiSelect && ui.totalTabs == 2) return true
        if (ui.activeTabIndex == ui.totalTabs - 1) return true
        return false
    }
    override val syncCheckedFromScreen = true
    override val hasSkipOnLastQuestion = false
    override val cacheOptionsOnOther = false
    override fun detectOtherExpanded(screenText: String, isMultiSelect: Boolean, isMultiQuestion: Boolean) = false
    override fun handleOscTitle(title: String): Pair<AgentStatus, String>? = null
}

// === SECTION 5 END ===

// ── Codex behavior ────────────────────────────────────────────────────────────

object CodexBehavior : CliBehavior {
    override fun answer(option: String, index: Int, ctx: BehaviorContext) = ActionResult(
        steps = listOf(KeystrokeStep(
            AnswerSubmitter.submitAnswer(CliType.CODEX, option, index)
        )),
        dismiss = !ctx.isMultiQuestion,
    )
    override fun toggle(option: String, index: Int, ctx: BehaviorContext) = ActionResult(
        steps = listOf(KeystrokeStep(" ")),
        dismiss = false,
    )
    override fun submitMultiSelect(ctx: BehaviorContext) = ActionResult(
        steps = listOf(KeystrokeStep("\r")), dismiss = true,
    )
    override fun textAnswer(option: String, text: String, index: Int, ctx: BehaviorContext) = ActionResult(
        steps = listOf(
            KeystrokeStep(option + "\r"),
            KeystrokeStep(text + "\r", TEXT_ANSWER_DELAY_MS),
        ),
        dismiss = false,
    )
    override fun textCancel(ctx: BehaviorContext) = ActionResult(steps = emptyList(), dismiss = false)
    override fun prevQuestion(ctx: BehaviorContext) = ActionResult(
        steps = listOf(KeystrokeStep(AnswerSubmitter.navigatePrevQuestion())),
        dismiss = false, newCursorPos = 0,
    )
    override fun nextQuestion(ctx: BehaviorContext) = ActionResult(
        steps = listOf(KeystrokeStep(AnswerSubmitter.navigateNextQuestion())),
        dismiss = false, newCursorPos = 0,
    )
    override fun confirm(hasAnswers: Boolean, ctx: BehaviorContext) = ActionResult(
        steps = listOf(KeystrokeStep("\r")), dismiss = true,
    )
    override fun lastQuestionIndex(totalTabs: Int) = totalTabs - 2
    override fun hidePrev(ui: UiState) = false
    override fun hideNext(ui: UiState) = false
    override val syncCheckedFromScreen = false
    override val hasSkipOnLastQuestion = false
    override val cacheOptionsOnOther = false
    override fun detectOtherExpanded(screenText: String, isMultiSelect: Boolean, isMultiQuestion: Boolean) = false
    // Codex emits "Action Required" as OSC 0 title when blocked.
    override fun handleOscTitle(title: String): Pair<AgentStatus, String>? {
        if (title == "Action Required") return AgentStatus.BLOCKED to "Action required"
        return null
    }
}

// === SECTION 6 END ===
