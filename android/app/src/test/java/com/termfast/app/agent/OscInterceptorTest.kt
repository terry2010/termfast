package com.termfast.app.agent

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

class OscInterceptorTest {

    @Test
    fun testParseOsc0OpenCode() {
        val signal = OscInterceptor.parseOsc(0, "OpenCode")
        assertIs<AgentSignal.Title>(signal)
        assertEquals(CliType.OPENCODE, signal.cli)
        assertEquals("OpenCode", signal.title)
    }

    @Test
    fun testParseOsc0OpenCodeWithPrompt() {
        val signal = OscInterceptor.parseOsc(0, "OC | working on task")
        assertIs<AgentSignal.Title>(signal)
        assertEquals(CliType.OPENCODE, signal.cli)
    }

    @Test
    fun testParseOsc0Codex() {
        val signal = OscInterceptor.parseOsc(0, "Action Required")
        assertIs<AgentSignal.Title>(signal)
        assertEquals(CliType.CODEX, signal.cli)
    }

    @Test
    fun testParseOsc0ClaudeCode() {
        val signal = OscInterceptor.parseOsc(0, "claude code - session 1")
        assertIs<AgentSignal.Title>(signal)
        assertEquals(CliType.CLAUDE_CODE, signal.cli)
    }

    @Test
    fun testParseOsc0Devin() {
        val signal = OscInterceptor.parseOsc(0, "Devin - session abc")
        assertIs<AgentSignal.Title>(signal)
        assertEquals(CliType.DEVIN, signal.cli)
    }

    @Test
    fun testParseOsc0NonCliTitle() {
        val signal = OscInterceptor.parseOsc(0, "bash - terminal")
        assertNull(signal)
    }

    @Test
    fun testParseOsc9DevinDone() {
        val signal = OscInterceptor.parseOsc(9, "Devin finished")
        assertIs<AgentSignal.Notify>(signal)
        assertEquals(CliType.DEVIN, signal.cli)
        assertTrue(signal.done)
    }

    @Test
    fun testParseOsc9DevinBlocked() {
        val signal = OscInterceptor.parseOsc(9, "Devin needs input")
        assertIs<AgentSignal.Notify>(signal)
        assertEquals(CliType.DEVIN, signal.cli)
        assertTrue(!signal.done)
    }

    @Test
    fun testParseOsc9NonDevin() {
        val signal = OscInterceptor.parseOsc(9, "Build complete")
        assertNull(signal)
    }

    @Test
    fun testParseOsc777DevinNotify() {
        val signal = OscInterceptor.parseOsc(777, "notify;Devin;Devin needs input")
        assertIs<AgentSignal.Notify>(signal)
        assertEquals(CliType.DEVIN, signal.cli)
        assertTrue(!signal.done)
    }

    @Test
    fun testParseOscUnknown() {
        val signal = OscInterceptor.parseOsc(52, "clipboard data")
        assertNull(signal)
    }

    @Test
    fun testScanStringCompleteOsc() {
        // ESC ] 0 ; OpenCode BEL
        val text = "\u001B]0;OpenCode\u0007"
        val signal = OscInterceptor.scanString(text)
        assertIs<AgentSignal.Title>(signal)
        assertEquals(CliType.OPENCODE, signal.cli)
    }

    @Test
    fun testScanStringWithStTerminator() {
        // ESC ] 0 ; Claude Code ESC \
        val text = "\u001B]0;claude code\u001B\\"
        val signal = OscInterceptor.scanString(text)
        assertIs<AgentSignal.Title>(signal)
        assertEquals(CliType.CLAUDE_CODE, signal.cli)
    }

    @Test
    fun testScanStringNoOsc() {
        val text = "just some terminal output"
        val signal = OscInterceptor.scanString(text)
        assertNull(signal)
    }

    @Test
    fun testScanStringMultipleOscReturnsFirst() {
        val text = "\u001B]0;OpenCode\u0007some output\u001B]9;Devin finished\u0007"
        val signal = OscInterceptor.scanString(text)
        assertIs<AgentSignal.Title>(signal)
        assertEquals(CliType.OPENCODE, signal.cli)
    }

    // === Cross-chunk buffer tests (FP4-3) ===

    @Test
    fun testScanCrossChunkOscCompletes() {
        val sessionId = "test-cross-chunk-1"
        OscInterceptor.clearBuffer(sessionId)
        // First chunk: ESC ] 0 ; Ope (incomplete — no terminator)
        val chunk1 = "\u001B]0;Ope".toByteArray(Charsets.UTF_8)
        val signal1 = OscInterceptor.scan(sessionId, chunk1)
        // Should return null (incomplete sequence buffered)
        assertNull(signal1, "First chunk should not produce signal (incomplete OSC)")

        // Second chunk: nCode BEL (completes the sequence)
        val chunk2 = "nCode\u0007".toByteArray(Charsets.UTF_8)
        val signal2 = OscInterceptor.scan(sessionId, chunk2)
        assertIs<AgentSignal.Title>(signal2)
        assertEquals(CliType.OPENCODE, signal2.cli)
        assertEquals("OpenCode", signal2.title)
        OscInterceptor.clearBuffer(sessionId)
    }

    @Test
    fun testScanCrossChunkStTerminator() {
        val sessionId = "test-cross-chunk-2"
        OscInterceptor.clearBuffer(sessionId)
        // First chunk: ESC ] 9 ; Devin fin
        val chunk1 = "\u001B]9;Devin fin".toByteArray(Charsets.UTF_8)
        val signal1 = OscInterceptor.scan(sessionId, chunk1)
        assertNull(signal1)

        // Second chunk: ished BEL
        val chunk2 = "ished\u0007".toByteArray(Charsets.UTF_8)
        val signal2 = OscInterceptor.scan(sessionId, chunk2)
        assertIs<AgentSignal.Notify>(signal2)
        assertEquals(CliType.DEVIN, signal2.cli)
        assertTrue(signal2.done)
        OscInterceptor.clearBuffer(sessionId)
    }

    @Test
    fun testScanNoBufferForCompleteSequence() {
        val sessionId = "test-no-buffer"
        OscInterceptor.clearBuffer(sessionId)
        // Complete sequence in one chunk
        val chunk = "\u001B]0;OpenCode\u0007".toByteArray(Charsets.UTF_8)
        val signal = OscInterceptor.scan(sessionId, chunk)
        assertIs<AgentSignal.Title>(signal)
        assertEquals(CliType.OPENCODE, signal.cli)

        // Next chunk should NOT have leftover buffer
        val chunk2 = "normal output".toByteArray(Charsets.UTF_8)
        val signal2 = OscInterceptor.scan(sessionId, chunk2)
        assertNull(signal2)
        OscInterceptor.clearBuffer(sessionId)
    }

    @Test
    fun testScanNonOscAfterIncompleteOsc() {
        val sessionId = "test-no-osc-after-incomplete"
        OscInterceptor.clearBuffer(sessionId)
        // First chunk has ESC ] but no terminator
        val chunk1 = "\u001B]0;OpenCode".toByteArray(Charsets.UTF_8)
        val signal1 = OscInterceptor.scan(sessionId, chunk1)
        assertNull(signal1)

        // Second chunk has no terminator either — should buffer again
        val chunk2 = " more text".toByteArray(Charsets.UTF_8)
        val signal2 = OscInterceptor.scan(sessionId, chunk2)
        assertNull(signal2)
        OscInterceptor.clearBuffer(sessionId)
    }
}
