package com.termfast.app.agent

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class AnswerSubmitterTest {

    // === Devin ===

    @Test
    fun testSubmitDevinNumberOption() {
        val result = AnswerSubmitter.submitAnswer(CliType.DEVIN, "1. Yes", 0)
        assertEquals("1\r", result)
    }

    @Test
    fun testSubmitDevinNumberOptionMultiQuestion() {
        val result = AnswerSubmitter.submitAnswer(CliType.DEVIN, "2. Maybe", 1, isMultiQuestion = true)
        assertEquals("2", result)  // no Enter in multi-question mode
    }

    @Test
    fun testSubmitDevinOtherOption() {
        val result = AnswerSubmitter.submitAnswer(CliType.DEVIN, "Other", 2)
        assertEquals("e", result)
    }

    @Test
    fun testSubmitDevinFallback() {
        val result = AnswerSubmitter.submitAnswer(CliType.DEVIN, "custom option", 1)
        assertEquals("2\r", result)  // index+1 + Enter
    }

    @Test
    fun testToggleDevinOptionDown() {
        val result = AnswerSubmitter.toggleDevinOption(2, 0)
        assertEquals("\u001B[B\u001B[B ", result)  // 2 down arrows + space
    }

    @Test
    fun testToggleDevinOptionUp() {
        val result = AnswerSubmitter.toggleDevinOption(0, 2)
        assertEquals("\u001B[A\u001B[A ", result)  // 2 up arrows + space
    }

    @Test
    fun testToggleDevinOptionSamePos() {
        val result = AnswerSubmitter.toggleDevinOption(1, 1)
        assertEquals(" ", result)
    }

    @Test
    fun testSubmitDevinMultiSelect() {
        assertEquals("\r", AnswerSubmitter.submitDevinMultiSelect())
    }

    @Test
    fun testSubmitDevinConfirmNoOptions() {
        assertEquals("\r", AnswerSubmitter.submitDevinConfirm(false, 0, 3))
    }

    @Test
    fun testSubmitDevinConfirmNavigate() {
        // activeIndex=0, totalTabs=3 → confirmIndex=2, arrowsNeeded=2
        val result = AnswerSubmitter.submitDevinConfirm(true, 0, 3)
        assertEquals("\u001B[C\u001B[C\r", result)
    }

    @Test
    fun testSubmitDevinConfirmOnLastTabNoAnswers() {
        // On last tab, single-select, no answers → Esc to skip
        val result = AnswerSubmitter.submitDevinConfirm(true, 2, 3, isMultiSelect = false, hasAnswers = false)
        assertEquals("\u001B", result)
    }

    // === OpenCode ===

    @Test
    fun testSubmitOpenCodeNumberOption() {
        val result = AnswerSubmitter.submitAnswer(CliType.OPENCODE, "1. Yes", 0, 2)
        assertEquals("1\r", result)
    }

    @Test
    fun testToggleOpenCodeOptionNumber() {
        val result = AnswerSubmitter.toggleOpenCodeOption("3. Maybe")
        assertEquals("3", result)
    }

    @Test
    fun testToggleOpenCodeOptionFallback() {
        val result = AnswerSubmitter.toggleOpenCodeOption("custom")
        assertEquals(" ", result)
    }

    @Test
    fun testSubmitOpenCodeMultiSelect() {
        assertEquals("\t\r", AnswerSubmitter.submitOpenCodeMultiSelect())
    }

    // === Claude Code ===

    @Test
    fun testSubmitClaudeCodeYes() {
        // "Yes" prompt → Enter
        val result = AnswerSubmitter.submitAnswer(CliType.CLAUDE_CODE, "Yes", 0)
        assertEquals("\r", result)
    }

    @Test
    fun testSubmitClaudeCodeNumberOption() {
        // "1. Yes" → number key (multi-question auto-advance)
        val result = AnswerSubmitter.submitAnswer(CliType.CLAUDE_CODE, "1. Yes", 0)
        assertEquals("1", result)
    }

    @Test
    fun testToggleClaudeCodeOption() {
        val result = AnswerSubmitter.toggleClaudeCodeOption("2. No")
        assertEquals("2", result)
    }

    // === Codex ===

    @Test
    fun testSubmitCodexYesShortcut() {
        val result = AnswerSubmitter.submitAnswer(CliType.CODEX, "1. Yes (y)", 0)
        assertEquals("y", result)
    }

    @Test
    fun testSubmitCodexNoShortcut() {
        val result = AnswerSubmitter.submitAnswer(CliType.CODEX, "2. No (n)", 1)
        assertEquals("n", result)
    }

    @Test
    fun testSubmitCodexEscShortcut() {
        val result = AnswerSubmitter.submitAnswer(CliType.CODEX, "3. Cancel (esc)", 2)
        assertEquals("\u001B", result)
    }

    @Test
    fun testSubmitCodexNumberPrefix() {
        val result = AnswerSubmitter.submitAnswer(CliType.CODEX, "1. First option", 0)
        assertEquals("1", result)
    }

    @Test
    fun testSubmitCodexLegacyYes() {
        val result = AnswerSubmitter.submitAnswer(CliType.CODEX, "yes (y)", 0)
        assertEquals("y\r", result)
    }

    @Test
    fun testSubmitCodexLegacyNo() {
        val result = AnswerSubmitter.submitAnswer(CliType.CODEX, "no (n)", 1)
        assertEquals("n\r", result)
    }

    @Test
    fun testSubmitCodexLegacyTrust() {
        val result = AnswerSubmitter.submitAnswer(CliType.CODEX, "Trust this project", 0)
        assertEquals("\r", result)
    }

    @Test
    fun testSubmitCodexFallbackFirst() {
        val result = AnswerSubmitter.submitAnswer(CliType.CODEX, "unknown option", 0)
        assertEquals("y\r", result)
    }

    @Test
    fun testSubmitCodexFallbackSecond() {
        val result = AnswerSubmitter.submitAnswer(CliType.CODEX, "unknown option", 1)
        assertEquals("n\r", result)
    }

    // === Navigation ===

    @Test
    fun testNavigatePrevQuestion() {
        assertEquals("\u001B[D", AnswerSubmitter.navigatePrevQuestion())
    }

    @Test
    fun testNavigateNextQuestion() {
        assertEquals("\u001B[C", AnswerSubmitter.navigateNextQuestion())
    }

    // === Unknown CLI fallback ===

    @Test
    fun testSubmitUnknownCli() {
        val result = AnswerSubmitter.submitAnswer(CliType.UNKNOWN, "test", 0)
        assertEquals("test\r", result)
    }
}
