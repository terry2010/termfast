package com.termfast.app.agent

/**
 * CLI detector — detect which AI CLI is running in a terminal tab.
 *
 * Ported from desktop `src/hooks/cliDetector.ts` (174 lines).
 *
 * Detection signals (in priority order):
 *   1. OSC 0 title: "OpenCode", "OC | ...", "claude", "codex", "Devin"
 *   2. Screen content patterns (fallback when OSC 0 is not emitted)
 *
 * Once a CLI is detected, the type is sticky — we don't un-detect on title
 * changes (e.g. OpenCode clears its title on idle, but it's still OpenCode).
 */
object CliDetector {

    /**
     * Detect CLI type from an OSC 0 title string.
     * @return detected CLI type, or UNKNOWN if not a recognized AI CLI.
     */
    fun detectCliFromTitle(title: String): CliType {
        val t = title.trim()
        if (t.isEmpty()) return CliType.UNKNOWN

        // OpenCode: "OpenCode" or "OC | <task>"
        if (t == "OpenCode" || t.startsWith("OC |") || t.startsWith("OC  |")) {
            return CliType.OPENCODE
        }

        // Codex: "Action Required" or "codex" in title
        if (t == "Action Required" || t.lowercase().contains("codex")) {
            return CliType.CODEX
        }

        // Claude Code: "claude" in title
        if (t.lowercase().contains("claude")) {
            return CliType.CLAUDE_CODE
        }

        // Devin: "Devin" in title (case-insensitive)
        if (t.lowercase().contains("devin")) {
            return CliType.DEVIN
        }

        return CliType.UNKNOWN
    }

    /**
     * Detect CLI type from screen content (fallback when no OSC 0 title).
     * @param screenText ANSI-stripped screen text
     * @return detected CLI type, or UNKNOWN if no signature matched.
     */
    fun detectCliFromScreen(screenText: String): CliType {
        // Devin: "Devin CLI" text in the startup banner
        if (Regex("Devin\\s+CLI", RegexOption.IGNORE_CASE).containsMatchIn(screenText)) {
            return CliType.DEVIN
        }
        // Devin: Braille art logo pattern
        if (Regex("[⣴⣾⣶⡄⠛⠿⠟⠻⣤⣦⠻⢿⠃].*Devin", RegexOption.IGNORE_CASE).containsMatchIn(screenText)) {
            return CliType.DEVIN
        }

        // OpenCode: "esc interrupt" + "ctrl+p commands" footer
        if (Regex("esc\\s+interrupt").containsMatchIn(screenText) &&
            Regex("ctrl\\+p\\s+commands").containsMatchIn(screenText)) {
            return CliType.OPENCODE
        }
        // OpenCode idle footer
        if (Regex("ctrl\\+p\\s+commands").containsMatchIn(screenText) &&
            Regex("•\\s*OpenCode\\s+\\d").containsMatchIn(screenText)) {
            return CliType.OPENCODE
        }
        // OpenCode logo
        if (Regex("█▀▀█\\s+█▀▀█").containsMatchIn(screenText)) {
            return CliType.OPENCODE
        }
        // OpenCode permission dialog
        if (Regex("△\\s+(Permission required|Always allow)\\b").containsMatchIn(screenText)) {
            return CliType.OPENCODE
        }
        // OpenCode question/selector dialog
        if (Regex("↑↓\\s+select.*enter\\s+\\w+.*esc\\s+dismiss").containsMatchIn(screenText)) {
            return CliType.OPENCODE
        }
        // OpenCode completion marker
        if (Regex("▣\\s+\\S+\\s+·\\s+.+?\\s+·\\s+(?:\\d+m\\s+)?\\d+(?:\\.\\d+)?s").containsMatchIn(screenText)) {
            return CliType.OPENCODE
        }

        // Claude Code: Braille spinner + "❯" prompt
        if (Regex("[✶✢✽✻✳·*][^\\n]*…").containsMatchIn(screenText) &&
            Regex("[>❯][\\s\\u00a0]").containsMatchIn(screenText)) {
            return CliType.CLAUDE_CODE
        }
        // Claude Code: multi-question selection widget footer
        if (Regex("Enter\\s*to\\s*select.*(?:Tab/Arrow|Tab).*Esc\\s*to\\s*cancel", RegexOption.IGNORE_CASE).containsMatchIn(screenText)) {
            return CliType.CLAUDE_CODE
        }
        // Claude Code: permission dialog footer
        if (Regex("Esc\\s*to\\s*cancel.*Tab\\s*to\\s*amend.*ctrl\\+e\\s*to\\s*explain", RegexOption.IGNORE_CASE).containsMatchIn(screenText)) {
            return CliType.CLAUDE_CODE
        }
        // Claude Code: multi-question tab row
        if (Regex("←\\s+[☐☒].*✔\\s*Submit\\s*→").containsMatchIn(screenText)) {
            return CliType.CLAUDE_CODE
        }
        // Claude Code: selection widget footer (older)
        if (Regex("↑/↓\\s+to\\s+navigate").containsMatchIn(screenText)) {
            return CliType.CLAUDE_CODE
        }
        // Claude Code: plan approval / trust dialog
        if (Regex("Would you like to proceed\\?").containsMatchIn(screenText) ||
            Regex("Yes,\\s+I\\s+trust\\s+this\\s+folder").containsMatchIn(screenText)) {
            return CliType.CLAUDE_CODE
        }
        // Claude Code: completion summary
        if (Regex("[✶✢✽✻✳][^\\n…]*\\bfor\\s+\\d+(?:\\.\\d+)?\\s*s\\b").containsMatchIn(screenText)) {
            return CliType.CLAUDE_CODE
        }

        // Codex: progress spinner
        if (Regex("•.*\\(\\d+s\\s*•\\s*esc\\s+to\\s+interrupt\\)").containsMatchIn(screenText)) {
            return CliType.CODEX
        }
        // Codex: "codex>" prompt
        if (Regex("^\\s*codex>\\s*$", RegexOption.MULTILINE).containsMatchIn(screenText)) {
            return CliType.CODEX
        }
        // Codex: Approve/Allow y/n prompt
        if (Regex("^(?:Approve|Allow)\\b.*\\b(?:y/n|yes/no)\\b", setOf(RegexOption.IGNORE_CASE, RegexOption.MULTILINE)).containsMatchIn(screenText)) {
            return CliType.CODEX
        }
        // Codex: trust prompt
        if (Regex("allow\\s+Codex\\s+to\\s+work\\s+in\\s+this\\s+folder", RegexOption.IGNORE_CASE).containsMatchIn(screenText) ||
            Regex("Do you trust the contents of this directory\\?", RegexOption.IGNORE_CASE).containsMatchIn(screenText)) {
            return CliType.CODEX
        }

        return CliType.UNKNOWN
    }

    /** Known OSC 9 notify prefixes that pollute the title (Devin notifications). */
    private val OSC9_POLLUTION_PREFIXES = listOf(
        "notify;Devin;",
        "Devin needs input",
        "Devin finished",
        "Devin encountered",
        "Devin response",
        "Tool approval pending",
        "Question pending",
        "Network permission pending",
        "Input needed",
        "Devin needs authentication",
    )

    /**
     * Check if a title is actually an OSC 9 notify payload (pollution), not a real CLI name.
     * Devin's OSC 9 notifications can set the title to the notification message,
     * which would falsely match the "devin" substring.
     */
    private fun isOsc9Pollution(title: String): Boolean {
        val trimmed = title.trim()
        if (trimmed.isEmpty()) return false
        return OSC9_POLLUTION_PREFIXES.any { trimmed.startsWith(it) || trimmed.contains(it) }
    }

    /**
     * Combined detection: try OSC title first, then screen content.
     * Devin special handling: if title looks like an OSC 9 notify payload (pollution),
     * prefer screen content detection and use title only as secondary confirmation.
     * @param title OSC 0 title (or empty if none)
     * @param screenText ANSI-stripped screen text (or empty if none)
     * @return detected CLI type, or UNKNOWN.
     */
    fun detectCli(title: String, screenText: String): CliType {
        // Devin special handling: OSC 9 notify payloads can pollute the title.
        // If title matches a known notify prefix, skip title-based detection and
        // prefer screen content (per design doc §FP2).
        if (!isOsc9Pollution(title)) {
            val fromTitle = detectCliFromTitle(title)
            if (fromTitle != CliType.UNKNOWN) return fromTitle
        }
        val fromScreen = detectCliFromScreen(screenText)
        if (fromScreen != CliType.UNKNOWN) return fromScreen
        // Fallback: if screen detection failed and title was polluted, still try title
        // (e.g. "Devin needs input" → detectCliFromTitle matches "devin" → DEVIN)
        return detectCliFromTitle(title)
    }
}
