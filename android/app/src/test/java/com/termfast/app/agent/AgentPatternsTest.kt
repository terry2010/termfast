package com.termfast.app.agent

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class AgentPatternsTest {

    // === getPatterns ===

    @Test
    fun testGetPatternsDevin() {
        val patterns = getPatterns(CliType.DEVIN)
        assertNotNull(patterns)
        assertTrue(patterns.statusPatterns.isNotEmpty())
    }

    @Test
    fun testGetPatternsOpenCode() {
        val patterns = getPatterns(CliType.OPENCODE)
        assertNotNull(patterns)
        assertTrue(patterns.statusPatterns.isNotEmpty())
    }

    @Test
    fun testGetPatternsClaudeCode() {
        val patterns = getPatterns(CliType.CLAUDE_CODE)
        assertNotNull(patterns)
        assertTrue(patterns.statusPatterns.isNotEmpty())
    }

    @Test
    fun testGetPatternsCodex() {
        val patterns = getPatterns(CliType.CODEX)
        assertNotNull(patterns)
        assertTrue(patterns.statusPatterns.isNotEmpty())
    }

    @Test
    fun testGetPatternsUnknown() {
        val patterns = getPatterns(CliType.UNKNOWN)
        assertNull(patterns)
    }

    // === detectStatusFromScreen ===

    @Test
    fun testDetectStatusDevinBlocked() {
        val screen = "Do you want to approve this tool call?\n1 Yes (Approve)"
        val status = detectStatusFromScreen(CliType.DEVIN, screen)
        assertEquals(AgentStatus.BLOCKED, status)
    }

    @Test
    fun testDetectStatusDevinIdle() {
        val screen = "❭ Ask Devin to do something"
        val status = detectStatusFromScreen(CliType.DEVIN, screen)
        assertEquals(AgentStatus.IDLE, status)
    }

    @Test
    fun testDetectStatusUnknownCli() {
        val status = detectStatusFromScreen(CliType.UNKNOWN, "some text")
        assertNull(status)
    }

    @Test
    fun testDetectStatusNoMatch() {
        val status = detectStatusFromScreen(CliType.DEVIN, "just some random output")
        assertNull(status)
    }

    // === stripAnsi ===

    @Test
    fun testStripAnsiSgr() {
        val input = "\u001B[31mRed Text\u001B[0m"
        assertEquals("Red Text", stripAnsi(input))
    }

    @Test
    fun testStripAnsiCsi() {
        val input = "\u001B[2JHello"
        assertEquals("Hello", stripAnsi(input))
    }

    @Test
    fun testStripAnsiOsc() {
        val input = "\u001B]0;Title\u0007Hello"
        assertEquals("Hello", stripAnsi(input))
    }

    @Test
    fun testStripAnsiNoEscape() {
        val input = "plain text"
        assertEquals("plain text", stripAnsi(input))
    }

    @Test
    fun testStripAnsiMixed() {
        val input = "\u001B[1mBold\u001B[0m \u001B]0;Title\u0007Text"
        assertEquals("Bold Text", stripAnsi(input))
    }

    // === extractQuestion / extractOptions ===

    @Test
    fun testExtractQuestionUnknownCli() {
        assertNull(extractQuestion(CliType.UNKNOWN, "text"))
    }

    @Test
    fun testExtractOptionsUnknownCli() {
        assertNull(extractOptions(CliType.UNKNOWN, "text"))
    }

    // === detectMultiSelect / detectMultiQuestion ===

    @Test
    fun testDetectMultiSelectUnknownCli() {
        assertFalse(detectMultiSelect(CliType.UNKNOWN, "text"))
    }

    @Test
    fun testDetectMultiQuestionUnknownCli() {
        assertFalse(detectMultiQuestion(CliType.UNKNOWN, "text"))
    }

    // === Positive extraction assertions ===

    @Test
    fun testCodexQuestionAndOptions() {
        val screen = """
            Question 1/2 (1 unanswered)
            你想测试哪类单选问题？
            › 1. 功能偏好 (Recommended)  询问功能偏好
              2. 技术选择  询问某个技术方案
            tab to add notes | enter to submit answer | ←/→ to navigate questions | esc to interrupt
        """.trimIndent()
        assertEquals("你想测试哪类单选问题？", extractQuestion(CliType.CODEX, screen))
        val options = extractOptions(CliType.CODEX, screen)
        assertNotNull(options)
        assertEquals(listOf("1. 功能偏好 (Recommended)", "2. 技术选择"), options)
        assertEquals(AgentStatus.BLOCKED, detectStatusFromScreen(CliType.CODEX, screen))
        assertTrue(detectMultiQuestion(CliType.CODEX, screen))
    }

    @Test
    fun testCodexMultiSelectDetection() {
        val screen = """
            Which features do you want?
            › [x] Option one
              [ ] Option two
              [ ] Option three
            space to toggle | enter to confirm
        """.trimIndent()
        assertTrue(detectMultiSelect(CliType.CODEX, screen))
        val options = extractOptions(CliType.CODEX, screen)
        assertNotNull(options)
        assertEquals(listOf("Option one", "Option two", "Option three"), options)
    }

    @Test
    fun testCodexMultiSelectUncheckedOnly() {
        // Two unchecked rows still count as multi-select
        val screen = "› [ ] Option one\n  [ ] Option two"
        assertTrue(detectMultiSelect(CliType.CODEX, screen))
    }

    @Test
    fun testCodexMultiSelectLoneCheckboxNotEnough() {
        // A single "[ ]" line in normal output must not flip multi-select on
        val screen = "  [ ] buy milk\nsome other output"
        assertFalse(detectMultiSelect(CliType.CODEX, screen))
    }

    @Test
    fun testCodexMultiSelectNumberedCheckbox() {
        val screen = "› 1. [x] Alpha\n  2. [ ] Beta"
        assertTrue(detectMultiSelect(CliType.CODEX, screen))
        val options = extractOptions(CliType.CODEX, screen)
        assertNotNull(options)
        assertEquals(listOf("1. Alpha", "2. Beta"), options)
    }

    @Test
    fun testCodexCursorIndex() {
        val screen = "Question 1/1\nPick one\n› 1. First\n  2. Second\n  3. Third"
        assertEquals(0, extractCursorIndex(CliType.CODEX, screen))
        val screen2 = "Question 1/1\nPick one\n  1. First\n› 2. Second\n  3. Third"
        assertEquals(1, extractCursorIndex(CliType.CODEX, screen2))
    }

    @Test
    fun testCodexCursorIndexCheckboxRows() {
        val screen = "Pick many\n  [x] A\n› [ ] B\n  [ ] C"
        assertEquals(1, extractCursorIndex(CliType.CODEX, screen))
    }

    @Test
    fun testCodexWorkingSpinner() {
        assertEquals(AgentStatus.WORKING, detectStatusFromScreen(CliType.CODEX, "• Working (3s • esc to interrupt)"))
        assertEquals(AgentStatus.WORKING, detectStatusFromScreen(CliType.CODEX, "• Working"))
        assertEquals(AgentStatus.WORKING, detectStatusFromScreen(CliType.CODEX, "output\n• Thinking\nmore"))
    }

    @Test
    fun testCodexIdlePrompt() {
        assertEquals(AgentStatus.IDLE, detectStatusFromScreen(CliType.CODEX, "› Ask Codex to do anything"))
        assertEquals(AgentStatus.IDLE, detectStatusFromScreen(CliType.CODEX, "❯"))
    }

    @Test
    fun testClaudePermissionShortFooter() {
        // Write/Create file dialog has no "ctrl+e to explain"
        val screen = "Do you want to create /tmp/foo.txt?\n❯ 1. Yes\n  2. No\nEsc to cancel · Tab to amend"
        assertEquals(AgentStatus.BLOCKED, detectStatusFromScreen(CliType.CLAUDE_CODE, screen))
        assertEquals("Do you want to create /tmp/foo.txt?", extractQuestion(CliType.CLAUDE_CODE, screen))
        val options = extractOptions(CliType.CLAUDE_CODE, screen)
        assertNotNull(options)
        assertEquals(listOf("1. Yes", "2. No"), options)
    }

    @Test
    fun testClaudePermissionFullFooter() {
        val screen = "Do you want to proceed?\n❯ 1. Yes\n  2. No\nEsc to cancel · Tab to amend · ctrl+e to explain"
        assertEquals(AgentStatus.BLOCKED, detectStatusFromScreen(CliType.CLAUDE_CODE, screen))
    }

    @Test
    fun testDevinActionFallbackQuestion() {
        // Permission dialog without "Running command" — extract from "⏺ <action>"
        val screen = "⏺ Writing /tmp/test.txt\n↑↓ select · ↵ confirm · esc cancel"
        assertEquals("Approve: Writing /tmp/test.txt?", extractQuestion(CliType.DEVIN, screen))
    }

    @Test
    fun testDevinSelectorQuestionAndOptions() {
        val screen = """
            ── Q1 · Q2 ──
            Which language do you prefer?
            ❭ 1 Rust
            · 2 Go
            ────────────
            ↑↓ navigate · ↵ select · esc cancel
        """.trimIndent()
        assertEquals("Which language do you prefer?", extractQuestion(CliType.DEVIN, screen))
        val options = extractOptions(CliType.DEVIN, screen)
        assertNotNull(options)
        assertEquals(listOf("1. Rust", "2. Go"), options)
    }
}
