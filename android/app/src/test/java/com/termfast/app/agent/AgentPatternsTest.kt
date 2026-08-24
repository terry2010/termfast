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
}
