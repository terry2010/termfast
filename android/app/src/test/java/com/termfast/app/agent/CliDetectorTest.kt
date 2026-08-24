package com.termfast.app.agent

import kotlin.test.Test
import kotlin.test.assertEquals

class CliDetectorTest {

    // === detectCliFromTitle tests ===

    @Test
    fun testDetectCliFromTitleOpenCode() {
        assertEquals(CliType.OPENCODE, CliDetector.detectCliFromTitle("OpenCode"))
    }

    @Test
    fun testDetectCliFromTitleOpenCodeWithPrompt() {
        assertEquals(CliType.OPENCODE, CliDetector.detectCliFromTitle("OC | working on task"))
    }

    @Test
    fun testDetectCliFromTitleCodex() {
        assertEquals(CliType.CODEX, CliDetector.detectCliFromTitle("Action Required"))
    }

    @Test
    fun testDetectCliFromTitleCodexLowercase() {
        assertEquals(CliType.CODEX, CliDetector.detectCliFromTitle("codex - session 1"))
    }

    @Test
    fun testDetectCliFromTitleClaudeCode() {
        assertEquals(CliType.CLAUDE_CODE, CliDetector.detectCliFromTitle("claude code"))
    }

    @Test
    fun testDetectCliFromTitleDevin() {
        assertEquals(CliType.DEVIN, CliDetector.detectCliFromTitle("Devin - session abc"))
    }

    @Test
    fun testDetectCliFromTitleEmpty() {
        assertEquals(CliType.UNKNOWN, CliDetector.detectCliFromTitle(""))
    }

    @Test
    fun testDetectCliFromTitleNonCli() {
        assertEquals(CliType.UNKNOWN, CliDetector.detectCliFromTitle("bash - terminal"))
    }

    // === detectCliFromScreen tests ===

    @Test
    fun testDetectCliFromScreenOpenCode() {
        // Matches "esc interrupt" + "ctrl+p commands" footer
        val screen = "esc interrupt  ctrl+p commands"
        assertEquals(CliType.OPENCODE, CliDetector.detectCliFromScreen(screen))
    }

    @Test
    fun testDetectCliFromScreenClaudeCode() {
        // Matches "Would you like to proceed?" pattern
        val screen = "Would you like to proceed?"
        assertEquals(CliType.CLAUDE_CODE, CliDetector.detectCliFromScreen(screen))
    }

    @Test
    fun testDetectCliFromScreenCodex() {
        // Matches "codex>" prompt
        val screen = "\ncodex>\n"
        assertEquals(CliType.CODEX, CliDetector.detectCliFromScreen(screen))
    }

    @Test
    fun testDetectCliFromScreenDevin() {
        // Matches "Devin CLI" startup banner
        val screen = "Welcome to Devin CLI v1.0"
        assertEquals(CliType.DEVIN, CliDetector.detectCliFromScreen(screen))
    }

    @Test
    fun testDetectCliFromScreenNonCli() {
        val screen = "user@host:~$ ls -la"
        assertEquals(CliType.UNKNOWN, CliDetector.detectCliFromScreen(screen))
    }

    @Test
    fun testDetectCliFromScreenEmpty() {
        assertEquals(CliType.UNKNOWN, CliDetector.detectCliFromScreen(""))
    }

    // === detectCli (combined) tests ===

    @Test
    fun testDetectCliTitleTakesPriority() {
        val title = "OpenCode"
        val screen = "Welcome to Devin CLI"  // would match Devin if title was empty
        assertEquals(CliType.OPENCODE, CliDetector.detectCli(title, screen))
    }

    @Test
    fun testDetectCliFallsBackToScreen() {
        val title = ""  // no title
        val screen = "Would you like to proceed?"
        assertEquals(CliType.CLAUDE_CODE, CliDetector.detectCli(title, screen))
    }

    @Test
    fun testDetectCliBothEmpty() {
        assertEquals(CliType.UNKNOWN, CliDetector.detectCli("", ""))
    }

    // === OSC 9 pollution tests (FP2-3) ===

    @Test
    fun testDetectCliOsc9PollutionPrefersScreen() {
        // Title is a Devin OSC 9 notify payload (pollution), screen shows OpenCode footer
        val title = "Devin needs input"
        val screen = "esc interrupt  ctrl+p commands"
        // Should prefer screen content (OpenCode) over polluted title (which would match Devin)
        assertEquals(CliType.OPENCODE, CliDetector.detectCli(title, screen))
    }

    @Test
    fun testDetectCliOsc9PollutionFallsBackToTitle() {
        // Title is a Devin OSC 9 notify payload, screen has no CLI patterns
        val title = "Devin needs input"
        val screen = "user@host:~$ "
        // Screen detection fails, fallback to title → matches "devin" → DEVIN
        assertEquals(CliType.DEVIN, CliDetector.detectCli(title, screen))
    }

    @Test
    fun testDetectCliNonPollutedDevinTitle() {
        // Normal Devin title (not OSC 9 pollution)
        val title = "Devin - session abc"
        val screen = "user@host:~$ "
        assertEquals(CliType.DEVIN, CliDetector.detectCli(title, screen))
    }
}
