package com.termfast.app.agent

/**
 * Answer submitter — per-CLI strategies for submitting answers to the PTY.
 *
 * Ported from desktop `src/hooks/answerSubmitter.ts` (675 lines).
 * Each CLI has a different interaction model for submitting answers.
 */

/** Delay (ms) between sending navigate keystrokes and type keystrokes. */
const val TEXT_ANSWER_DELAY_MS = 300L

/** Delay (ms) between sending the text and the submit (Enter) keystroke. */
const val TEXT_ANSWER_SUBMIT_DELAY_MS = 300L

/** Parts of a text answer submission (navigate → type → submit). */
data class TextAnswerParts(
    val navigate: String,
    val type: String,
    val submit: String? = null,
    val clear: String? = null,
)

object AnswerSubmitter {

    /**
     * Generate the keystrokes to submit an answer for a given CLI.
     * @param cli the CLI type
     * @param option the selected option string (e.g. "1. Yes")
     * @param index 0-based index of the selected option
     * @param optionCount total number of options (optional)
     * @param isMultiQuestion true if in multi-question mode
     * @return keystroke string to send to the PTY
     */
    fun submitAnswer(
        cli: CliType,
        option: String,
        index: Int,
        optionCount: Int? = null,
        isMultiQuestion: Boolean = false,
    ): String = when (cli) {
        CliType.DEVIN -> submitDevin(option, index, isMultiQuestion)
        CliType.OPENCODE -> submitOpenCode(option, index, optionCount, isMultiQuestion)
        CliType.CLAUDE_CODE -> submitClaudeCode(option, index)
        CliType.CODEX -> submitCodex(option, index)
        else -> option + "\r"
    }

    // ── Devin ──────────────────────────────────────────────────────────────

    /**
     * Toggle a single option in multi-select mode for Devin.
     * Uses relative arrow navigation from current cursor position.
     */
    fun toggleDevinOption(targetIndex: Int, currentPos: Int = 0): String {
        return when {
            targetIndex > currentPos -> "\u001B[B".repeat(targetIndex - currentPos) + " "
            targetIndex < currentPos -> "\u001B[A".repeat(currentPos - targetIndex) + " "
            else -> " "
        }
    }

    /** Submit multi-select for Devin (Enter to submit). */
    fun submitDevinMultiSelect(): String = "\r"

    /**
     * Navigate to the Confirm tab in Devin's multi-question dialog.
     * → arrows (confirmIndex - activeIndex) mod totalTabs times + Enter.
     */
    fun submitDevinConfirm(
        hasOptions: Boolean,
        activeIndex: Int,
        totalTabs: Int,
        isMultiSelect: Boolean = false,
        hasAnswers: Boolean = false,
    ): String {
        if (!hasOptions) return "\r"
        if (totalTabs <= 0) return "\r"
        val confirmIndex = totalTabs - 1
        val currentTab = if (activeIndex >= 0) activeIndex else 0
        val arrowsNeeded = (confirmIndex - currentTab + totalTabs) % totalTabs
        // Single-select multi-question on last tab with no answers → Esc to skip
        if (arrowsNeeded == 0 && !isMultiSelect && !hasAnswers) {
            return "\u001B"
        }
        return "\u001B[C".repeat(arrowsNeeded) + "\r"
    }

    /**
     * Submit "Type your own answer" for Devin.
     * Number key navigates, then 'e' enters text mode, then type + Enter.
     */
    fun submitDevinTextAnswer(
        option: String,
        text: String,
        index: Int? = null,
        currentPos: Int? = null,
    ): TextAnswerParts {
        val numMatch = Regex("^(\\d+)").find(option)
        if (numMatch != null) {
            return TextAnswerParts(
                navigate = numMatch.value + "e",
                type = "\u0015" + text + "\r",  // Ctrl+U clears, then type + Enter
            )
        }
        // No number (e.g. "Other"): use relative arrow navigation
        val idx = index ?: 0
        val cur = currentPos ?: 0
        val nav = when {
            idx > cur -> "\u001B[B".repeat(idx - cur) + "e"
            idx < cur -> "\u001B[A".repeat(cur - idx) + "e"
            else -> "e"
        }
        return TextAnswerParts(navigate = nav, type = "\u0015" + text + "\r")
    }

    private fun submitDevin(option: String, index: Int, isMultiQuestion: Boolean): String {
        val match = Regex("^(\\d+)").find(option)
        if (match != null) {
            // Multi-question: number key selects + auto-advances (no Enter)
            return if (isMultiQuestion) match.value else match.value + "\r"
        }
        if (Regex("other", RegexOption.IGNORE_CASE).containsMatchIn(option)) return "e"
        return if (isMultiQuestion) "${index + 1}" else "${index + 1}\r"
    }

    // ── OpenCode ───────────────────────────────────────────────────────────

    /** Toggle a single option in multi-select mode (number key). */
    fun toggleOpenCodeOption(option: String): String {
        val numMatch = Regex("^(\\d+)").find(option)
        if (numMatch != null) return numMatch.value
        return " "
    }

    /** Submit multi-select: Tab to Confirm + Enter. */
    fun submitOpenCodeMultiSelect(): String = "\t\r"

    /** Navigate to Confirm tab using → arrows + Enter. */
    fun submitOpenCodeConfirm(hasOptions: Boolean, activeIndex: Int, totalTabs: Int): String {
        if (!hasOptions) return "\r"
        if (totalTabs <= 0) return "\t\r"
        val confirmIndex = totalTabs - 1
        val currentTab = if (activeIndex >= 0) activeIndex else 0
        val arrowsNeeded = (confirmIndex - currentTab + totalTabs) % totalTabs
        return "\u001B[C".repeat(arrowsNeeded) + "\r"
    }

    /**
     * Submit "Type your own answer" for OpenCode.
     * Number key enters text mode, then type + Enter.
     * Ctrl+C (\x03) clears existing text (NOT Ctrl+U — OpenCode uses Ctrl+C).
     */
    fun submitOpenCodeTextAnswer(
        option: String,
        text: String,
        hasExistingText: Boolean = false,
    ): TextAnswerParts {
        val numMatch = Regex("^(\\d+)").find(option)
        val clear = if (hasExistingText) "\u0003" else null
        if (numMatch != null) {
            return TextAnswerParts(
                navigate = numMatch.value,
                type = text + "\r",
                clear = clear,
            )
        }
        return TextAnswerParts(navigate = "", type = text + "\r", clear = clear)
    }

    private fun submitOpenCode(
        option: String,
        index: Int,
        optionCount: Int?,
        isMultiQuestion: Boolean,
    ): String {
        val normalized = option.lowercase().trim()
        // Permission dialog buttons: "l" to navigate + Enter
        if (normalized == "allow once" || normalized == "allow always" || normalized == "reject") {
            return "l".repeat(index) + "\r"
        }
        // Numbered options
        val numMatch = Regex("^(\\d+)").find(option)
        if (numMatch != null) {
            return if (isMultiQuestion) numMatch.value else numMatch.value + "\r"
        }
        return "\t".repeat(index) + "\r"
    }

    // ── Claude Code ────────────────────────────────────────────────────────

    /** Toggle a single option in Claude Code multi-select (number key). */
    fun toggleClaudeCodeOption(option: String): String {
        val numMatch = Regex("^(\\d+)").find(option)
        return numMatch?.value ?: ""
    }

    /** Submit multi-select: Tab to Submit + Enter. */
    fun submitClaudeCodeMultiSelect(): String = "\t\r"

    /** Check if an option is a Claude Code Plan Mode option. */
    fun isClaudeCodePlanModeOption(option: String): Boolean =
        Regex("yes, and use auto mode", RegexOption.IGNORE_CASE).containsMatchIn(option) ||
        Regex("yes, manually approve edits", RegexOption.IGNORE_CASE).containsMatchIn(option) ||
        Regex("tell\\s+\\S+\\s+what\\s+to\\s+change", RegexOption.IGNORE_CASE).containsMatchIn(option)

    /** Build keystrokes for Claude Code Plan Mode navigation (Down*index). */
    fun buildClaudeCodePlanModeNavigate(index: Int): String = "\u001B[B".repeat(index)

    /** Navigate to Submit tab using → arrows + Enter. */
    fun submitClaudeCodeConfirm(hasOptions: Boolean, activeIndex: Int, totalTabs: Int): String {
        if (!hasOptions) return "\r"
        if (totalTabs <= 0) return "\r"
        val confirmIndex = totalTabs - 1
        val currentTab = if (activeIndex >= 0) activeIndex else 0
        val arrowsNeeded = (confirmIndex - currentTab + totalTabs) % totalTabs
        return "\u001B[C".repeat(arrowsNeeded) + "\r"
    }

    /**
     * Submit "Type your own answer" for Claude Code.
     * Single-select: number key → text mode → type + Enter.
     * Multi-select: Down arrows → text mode → type + Tab+Enter.
     */
    fun submitClaudeCodeTextAnswer(
        option: String,
        text: String,
        index: Int? = null,
        isMultiSelect: Boolean = false,
        hasExistingText: Boolean = false,
    ): TextAnswerParts {
        val numMatch = Regex("^(\\d+)").find(option)
        val numKey = numMatch?.value ?: "5"
        val isPlanModeFeedback = Regex("tell\\s+\\S+\\s+what\\s+to\\s+change", RegexOption.IGNORE_CASE)
            .containsMatchIn(option)
        // Multi-select: need Down arrows to navigate (number key only toggles)
        if (isMultiSelect && index != null && index > 0) {
            return TextAnswerParts(
                navigate = "\u001B[B".repeat(index),
                type = "\u0015" + text,  // Ctrl+U clears old text
                submit = "\t\r",
            )
        }
        if (isMultiSelect && index == 0) {
            return TextAnswerParts(
                navigate = "",
                type = "\u0015" + text,
                submit = "\t\r",
            )
        }
        // Single-select with existing text: use Down arrows (number key auto-submits)
        if (hasExistingText && index != null && index > 0) {
            return TextAnswerParts(
                navigate = "\u001B[B".repeat(index),
                type = "\u0015" + text,
                submit = "\r",
            )
        }
        if (hasExistingText && index == 0) {
            return TextAnswerParts(navigate = "", type = "\u0015" + text, submit = "\r")
        }
        return TextAnswerParts(
            navigate = numKey,
            type = "\u0015" + text,
            submit = if (isPlanModeFeedback) "\r\r" else "\r",
        )
    }

    private fun submitClaudeCode(option: String, index: Int): String {
        val normalized = option.lowercase().trim()
        // Yes/No prompts
        if (normalized.startsWith("yes")) return "\r"
        if (normalized.startsWith("no")) return "\u001B[B\r"
        // Plan Mode: Down arrows + Enter
        if (isClaudeCodePlanModeOption(option)) {
            return "\u001B[B".repeat(index) + "\r"
        }
        // Multi-question: number key auto-advances
        val numMatch = Regex("^(\\d+)").find(option)
        if (numMatch != null) return numMatch.value
        // Fallback: Down arrows + Enter
        return "\u001B[B".repeat(index) + "\r"
    }

    // ── Codex ──────────────────────────────────────────────────────────────

    private fun submitCodex(option: String, index: Int): String {
        val normalized = option.lowercase().trim()
        val hasNumberPrefix = Regex("^\\d+\\.\\s").containsMatchIn(option)
        // New TUI selection list: extract shortcut from "(y)" / "(esc)" etc.
        if (hasNumberPrefix) {
            val shortcutMatch = Regex("\\(([a-z]{1,3})\\)\\s*$", RegexOption.IGNORE_CASE).find(option)
            if (shortcutMatch != null) {
                val key = shortcutMatch.groupValues[1].lowercase()
                if (key == "esc") return "\u001B"
                return key
            }
            // No shortcut — use number key
            val numMatch = Regex("^(\\d+)\\.").find(option)
            if (numMatch != null) return numMatch.groupValues[1]
        }
        // Legacy trust prompt
        if (normalized.contains("trust") || normalized.contains("allow")) return "\r"
        // Legacy y/n text prompt
        if (normalized.startsWith("yes") || normalized == "y" || normalized == "yes (y)") return "y\r"
        if (normalized.startsWith("no") || normalized == "n" || normalized == "no (n)") return "n\r"
        return if (index == 0) "y\r" else "n\r"
    }

    // ── Navigation (all CLIs) ──────────────────────────────────────────────

    /** Navigate to previous question tab (Left arrow). */
    fun navigatePrevQuestion(): String = "\u001B[D"

    /** Navigate to next question tab (Right arrow). */
    fun navigateNextQuestion(): String = "\u001B[C"
}
