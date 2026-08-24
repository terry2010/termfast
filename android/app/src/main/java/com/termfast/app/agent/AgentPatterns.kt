package com.termfast.app.agent

/**
 * Agent patterns — per-CLI regex patterns for status detection + question extraction.
 *
 * Ported from desktop `src/hooks/agentPatterns.ts` (1060 lines).
 * All patterns operate on plain screen text (from ScreenScraper).
 */

/** ANSI stripping — termlib snapshot text is already plain, but keep for safety. */
fun stripAnsi(text: String): String {
    var result = text.replace(Regex("\u001B\\[[0-9;]*m"), "")       // SGR
    result = result.replace(Regex("\u001B\\[[0-9;?]*[a-zA-Z]"), "") // CSI
    result = result.replace(Regex("\u001B\\][^\u0007\u001B]*(\u0007|\u001B\\\\)"), "") // OSC
    result = result.replace(Regex("\u001B[()][0-9a-zA-Z]"), "")     // Charset
    result = result.replace(Regex("\u001B[=>]"), "")                // Keypad
    result = result.replace("\u001B", "")                           // Stray ESC
    return result
}

/** A status pattern with priority for ordered matching. */
data class StatusPattern(
    val status: AgentStatus,
    val pattern: Regex,
    val priority: Int,
)

/** Per-CLI pattern bundle. */
data class CliPatterns(
    val statusPatterns: List<StatusPattern>,
    val questionExtractor: ((String) -> String?)?,
    val optionsExtractor: ((String) -> List<String>?)?,
    val multiSelectDetector: ((String) -> Boolean)?,
    val multiQuestionDetector: ((String) -> Boolean)?,
    val reviewAnswersExtractor: ((String) -> List<String>?)?,
    val cursorIndexExtractor: ((String) -> Int?)?,
)

// === SECTION 1 END ===

// ── Devin patterns ────────────────────────────────────────────────────────────

private val devinPatterns = CliPatterns(
    statusPatterns = listOf(
        StatusPattern(AgentStatus.BLOCKED,
            Regex("\\d+\\s+Yes\\s*\\(Approve|↑↓\\s+select.*↵\\s+confirm", RegexOption.IGNORE_CASE), 10),
        StatusPattern(AgentStatus.BLOCKED,
            Regex("↑↓\\s+navigate.*↵\\s+select.*esc\\s+cancel", RegexOption.IGNORE_CASE), 9),
        StatusPattern(AgentStatus.BLOCKED,
            Regex("Do you want to|Press . to|Would you like to", RegexOption.IGNORE_CASE), 8),
        StatusPattern(AgentStatus.WORKING,
            Regex("^\\s*[\\u2800-\\u28FF]{1,4}\\s+\\w*ing\\b[^\\n]*\\(esc\\s+to\\s+interrupt\\)",
                setOf(RegexOption.IGNORE_CASE, RegexOption.MULTILINE)), 7),
        StatusPattern(AgentStatus.IDLE,
            Regex("❭ Ask Devin to", RegexOption.IGNORE_CASE), 3),
    ),
    questionExtractor = { text ->
        val lines = text.split("\n")
        val selectorIdx = lines.indexOfFirst { Regex("↑↓\\s+navigate.*esc\\s+cancel", RegexOption.IGNORE_CASE).containsMatchIn(it) }
        if (selectorIdx >= 0) {
            var bottomSepIdx = -1
            var i = selectorIdx - 1
            while (i >= 0 && i >= selectorIdx - 5) {
                if (Regex("^[─━]+$").matches(lines[i].trim())) { bottomSepIdx = i; break }
                i--
            }
            var tabRowIdx = -1
            if (bottomSepIdx >= 0) {
                var j = bottomSepIdx - 1
                while (j >= 0) {
                    if (lines[j].contains("──")) { tabRowIdx = j; break }
                    j--
                }
            }
            val startIdx = if (tabRowIdx >= 0) tabRowIdx + 1 else 0
            val endIdx = if (bottomSepIdx >= 0) bottomSepIdx else selectorIdx
            val optionPattern = Regex("^\\s*[❭·□■]\\s*(\\d+)\\s+(.+)")
            val unnumberedPattern = Regex("^\\s*[❭·□■]\\s+(.+)")
            val otherPattern = Regex("^\\s*[❭·□■]\\s+(Other\\s*\\(type your own\\))", RegexOption.IGNORE_CASE)
            var firstOptionIdx = -1
            for (k in startIdx until endIdx) {
                if (optionPattern.matches(lines[k]) || otherPattern.matches(lines[k]) ||
                    unnumberedPattern.matches(lines[k])) {
                    firstOptionIdx = k; break
                }
            }
            if (firstOptionIdx >= 0) {
                var m = firstOptionIdx - 1
                while (m >= startIdx) {
                    val trimmed = lines[m].trim()
                    if (trimmed.isEmpty()) { m--; continue }
                    if (Regex("^[─━]+$").matches(trimmed)) { m--; continue }
                    if (Regex("^[┃│║┌┐└┘├┤┬┴┼─━]+$").matches(trimmed)) { m--; continue }
                    return@CliPatterns trimmed
                }
            }
            return@CliPatterns "Devin is asking a question"
        }
        val hasPermissionFooter = Regex("↑↓\\s+select.*↵\\s+(confirm|insert).*esc\\s+cancel", RegexOption.IGNORE_CASE).containsMatchIn(text)
        if (!hasPermissionFooter) return@CliPatterns null
        for (line in lines) {
            val trimmed = line.trim()
            if (trimmed.endsWith("?") && trimmed.length > 10) return@CliPatterns trimmed
        }
        for (line in lines) {
            if (Regex("Running command", RegexOption.IGNORE_CASE).containsMatchIn(line.trim())) {
                return@CliPatterns "Approve command execution?"
            }
        }
        null
    },
    optionsExtractor = { text ->
        val lines = text.split("\n")
        val selectorIdx = lines.indexOfFirst { Regex("↑↓\\s+navigate.*esc\\s+cancel", RegexOption.IGNORE_CASE).containsMatchIn(it) }
        if (selectorIdx >= 0) {
            val optionPattern = Regex("^\\s*[❭·□■]\\s*(\\d+)\\s+(.+)")
            val otherPattern = Regex("^\\s*[❭·□■]\\s+(Other\\s*\\(type your own\\))", RegexOption.IGNORE_CASE)
            val unnumberedPattern = Regex("^\\s*[❭·□■]\\s+(.+)")
            var bottomSepIdx = -1
            var i = selectorIdx - 1
            while (i >= 0 && i >= selectorIdx - 5) {
                if (Regex("^[─━]+$").matches(lines[i].trim())) { bottomSepIdx = i; break }
                i--
            }
            var tabRowIdx = -1
            if (bottomSepIdx >= 0) {
                var j = bottomSepIdx - 1
                while (j >= 0) {
                    if (lines[j].contains("──")) { tabRowIdx = j; break }
                    j--
                }
            }
            val startIdx = if (tabRowIdx >= 0) tabRowIdx + 1 else 0
            val endIdx = if (bottomSepIdx >= 0) bottomSepIdx else selectorIdx
            val numberedOptions = mutableListOf<String>()
            val numberedIndices = mutableListOf<Int>()
            for (k in startIdx until endIdx) {
                val m = optionPattern.matchEntire(lines[k])
                if (m != null) {
                    numberedOptions.add("${m.groupValues[1]}. ${m.groupValues[2].trim()}")
                    numberedIndices.add(m.groupValues[1].toInt())
                    continue
                }
                val om = otherPattern.matchEntire(lines[k])
                if (om != null) {
                    numberedOptions.add(om.groupValues[1].trim())
                    numberedIndices.add(-1)
                }
            }
            if (numberedOptions.isNotEmpty()) {
                val nums = numberedIndices.filter { it > 0 }.sorted()
                val isConsecutive = nums.isNotEmpty() && nums.mapIndexed { idx, n -> n == idx + 1 }.all { it }
                if (isConsecutive) return@CliPatterns numberedOptions
            }
            val options = mutableListOf<String>()
            for (k in startIdx until endIdx) {
                val om = otherPattern.matchEntire(lines[k])
                if (om != null) { options.add(om.groupValues[1].trim()); continue }
                val m = unnumberedPattern.matchEntire(lines[k])
                if (m != null) options.add(m.groupValues[1].trim())
            }
            if (options.isNotEmpty()) return@CliPatterns options
        }
        val hasPermissionFooter = Regex("↑↓\\s+select.*↵\\s+(confirm|insert).*esc\\s+cancel", RegexOption.IGNORE_CASE).containsMatchIn(text)
        if (!hasPermissionFooter) return@CliPatterns null
        val options = mutableListOf<String>()
        val optionPattern = Regex("^(?:[❭·]\\s*)?(\\d+)[.\\s]+(.+)$")
        for (line in lines) {
            val m = optionPattern.matchEntire(line.trim())
            if (m != null) options.add("${m.groupValues[1]}. ${m.groupValues[2].trim()}")
        }
        if (options.isNotEmpty()) options else null
    },
    multiSelectDetector = { text ->
        Regex("↑↓\\s+navigate.*␣\\s+toggle.*esc\\s+cancel", RegexOption.IGNORE_CASE).containsMatchIn(text)
    },
    multiQuestionDetector = { text ->
        Regex("↑↓\\s+navigate.*←→\\s+switch question.*esc\\s+cancel", RegexOption.IGNORE_CASE).containsMatchIn(text)
    },
    cursorIndexExtractor = { text ->
        val lines = text.split("\n")
        val selectorIdx = lines.indexOfFirst { Regex("↑↓\\s+navigate.*esc\\s+cancel", RegexOption.IGNORE_CASE).containsMatchIn(it) }
        if (selectorIdx < 0) return@CliPatterns null
        var bottomSepIdx = -1
        var i = selectorIdx - 1
        while (i >= 0 && i >= selectorIdx - 5) {
            if (Regex("^[─━]+$").matches(lines[i].trim())) { bottomSepIdx = i; break }
            i--
        }
        var tabRowIdx = -1
        if (bottomSepIdx >= 0) {
            var j = bottomSepIdx - 1
            while (j >= 0) {
                if (lines[j].contains("──")) { tabRowIdx = j; break }
                j--
            }
        }
        val startIdx = if (tabRowIdx >= 0) tabRowIdx + 1 else 0
        val endIdx = if (bottomSepIdx >= 0) bottomSepIdx else selectorIdx
        val optionPattern = Regex("^\\s*[❭·□■]\\s*(\\d+)\\s+(.+)")
        val otherPattern = Regex("^\\s*[❭·□■]\\s+(Other\\s*\\(type your own\\))", RegexOption.IGNORE_CASE)
        val unnumberedPattern = Regex("^\\s*[❭·□■]\\s+(.+)")
        var optionIndex = 0
        for (k in startIdx until endIdx) {
            val line = lines[k]
            val isCursor = Regex("^\\s*❭").containsMatchIn(line)
            if (optionPattern.matches(line)) {
                if (isCursor) return@CliPatterns optionIndex
                optionIndex++
            } else if (otherPattern.matches(line) || unnumberedPattern.matches(line)) {
                if (isCursor) return@CliPatterns optionIndex
                optionIndex++
            }
        }
        null
    },
    reviewAnswersExtractor = null,
)

// === SECTION 2 END ===

// ── OpenCode patterns ──────────────────────────────────────────────────────────

private val opencodePatterns = CliPatterns(
    statusPatterns = listOf(
        StatusPattern(AgentStatus.BLOCKED,
            Regex("△\\s+(?:Permission required|Always allow|Reject permission)\\b"), 10),
        StatusPattern(AgentStatus.BLOCKED,
            Regex("↑↓\\s+select.*enter\\s+\\w+.*esc\\s+dismiss"), 9),
        StatusPattern(AgentStatus.BLOCKED,
            Regex("⇆\\s+tab.*enter\\s+submit.*esc\\s+dismiss"), 9),
        StatusPattern(AgentStatus.DONE,
            Regex("▣\\s+\\S+\\s+·\\s+.+?\\s+·\\s+(?:\\d+m\\s+)?\\d+(?:\\.\\d+)?s"), 8),
        StatusPattern(AgentStatus.WORKING,
            Regex("^\\s+[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]\\s+\\S+", RegexOption.MULTILINE), 7),
        StatusPattern(AgentStatus.WORKING,
            Regex("esc\\s+interrupt"), 6),
        StatusPattern(AgentStatus.IDLE,
            Regex("ctrl\\+p\\s+commands"), 5),
    ),
    questionExtractor = { text ->
        val lines = text.split("\n")
        // Permission dialog
        for (i in lines.indices) {
            if (Regex("△\\s+(?:Permission required|Always allow|Reject permission)").containsMatchIn(lines[i])) {
                val titleMatch = Regex("△\\s+(.+)").find(lines[i])
                val title = titleMatch?.groupValues?.get(1)?.trim() ?: "Permission required"
                for (j in i + 1 until minOf(lines.size, i + 5)) {
                    val trimmed = lines[j].trim()
                    if (trimmed.isNotEmpty() && !Regex("^[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]").containsMatchIn(trimmed)) {
                        return@CliPatterns "$title: $trimmed"
                    }
                }
                return@CliPatterns title
            }
        }
        // Question/selector dialog
        val selectorIdx = lines.indexOfFirst { Regex("↑↓\\s+select.*esc\\s+dismiss").containsMatchIn(it) }
        if (selectorIdx >= 0) {
            var firstOptionIdx = -1
            for (i in selectorIdx - 1 downTo 0) {
                if (Regex("^\\s*[┃│║]?\\s*\\d+\\.\\s+\\S").matches(lines[i])) {
                    firstOptionIdx = i
                } else {
                    val trimmed = lines[i].trim()
                    if (trimmed.isEmpty()) continue
                    if (Regex("^[┃│║┌┐└┘├┤┬┴┼─━]+$").matches(trimmed)) continue
                    if (Regex("^[┃│║]").containsMatchIn(trimmed)) {
                        val content = trimmed.replace(Regex("^[┃│║]\\s*"), "").trim()
                        val isTabRow = Regex("\\bConfirm\\b").containsMatchIn(content) &&
                            Regex("\\s{2,}").containsMatchIn(content)
                        if (isTabRow) break
                        continue
                    }
                    break
                }
            }
            if (firstOptionIdx >= 0) {
                for (i in firstOptionIdx - 1 downTo 0) {
                    val trimmed = lines[i].trim()
                    if (trimmed.isEmpty()) continue
                    if (Regex("^[┃│║┌┐└┘├┤┬┴┼─━]+$").matches(trimmed)) continue
                    val content = trimmed.replace(Regex("^[┃│║]\\s*"), "").trim()
                    if (content.isEmpty()) continue
                    val isTabRow = Regex("\\bConfirm\\b").containsMatchIn(content) &&
                        Regex("\\s{2,}").containsMatchIn(content)
                    if (isTabRow) break
                    return@CliPatterns content
                }
            }
            return@CliPatterns "OpenCode is asking a question"
        }
        null
    },
    optionsExtractor = { text ->
        val lines = text.split("\n")
        // Permission dialog
        for (line in lines) {
            if (Regex("△\\s+Permission required").containsMatchIn(line)) {
                for (fl in lines) {
                    if (Regex("Allow\\s+once.*Allow\\s+always.*Reject").containsMatchIn(fl)) {
                        val buttons = Regex("Allow\\s+once|Allow\\s+always|Reject").findAll(fl).map { it.value }.toList()
                        if (buttons.isNotEmpty()) return@CliPatterns buttons
                    }
                }
                return@CliPatterns listOf("Allow once", "Allow always", "Reject")
            }
        }
        for (line in lines) {
            if (Regex("△\\s+Always allow").containsMatchIn(line)) return@CliPatterns listOf("Confirm", "Cancel")
        }
        for (line in lines) {
            if (Regex("△\\s+Reject permission").containsMatchIn(line)) return@CliPatterns null
        }
        // Question/selector dialog
        val selectorIdx = lines.indexOfFirst { Regex("↑↓\\s+select.*esc\\s+dismiss").containsMatchIn(it) }
        if (selectorIdx >= 0) {
            val options = mutableListOf<String>()
            for (i in selectorIdx - 1 downTo 0) {
                val m = Regex("^\\s*[┃│║]?\\s*(\\d+)\\.\\s+(.+)$").matchEntire(lines[i])
                if (m != null) {
                    val label = m.groupValues[2].replace(Regex("^\\[[\\s✓]\\]\\s*"), "").trim()
                    options.add(0, "${m.groupValues[1]}. $label")
                } else {
                    val trimmed = lines[i].trim()
                    if (trimmed.isEmpty()) continue
                    if (Regex("^[┃│║┌┐└┘├┤┬┴┼─━]+$").matches(trimmed)) continue
                    if (Regex("^[┃│║]").containsMatchIn(trimmed)) {
                        val content = trimmed.replace(Regex("^[┃│║]\\s*"), "").trim()
                        val isTabRow = Regex("\\bConfirm\\b").containsMatchIn(content) &&
                            Regex("\\s{2,}").containsMatchIn(content)
                        if (isTabRow) break
                        continue
                    }
                    break
                }
            }
            if (options.isNotEmpty()) return@CliPatterns options
        }
        null
    },
    multiSelectDetector = { text ->
        Regex("↑↓\\s+select.*enter\\s+toggle.*esc\\s+dismiss").containsMatchIn(text)
    },
    multiQuestionDetector = { text ->
        text.split("\n").any { l ->
            val trimmed = l.replace(Regex("^[┃│║]\\s*"), "").trim()
            Regex("\\bConfirm\\b").containsMatchIn(trimmed) &&
            Regex("\\s{2,}").containsMatchIn(trimmed) &&
            trimmed.split(Regex("\\s{2,}")).size >= 2
        }
    },
    reviewAnswersExtractor = { text ->
        val lines = text.split("\n")
        val reviewIdx = lines.indexOfFirst {
            it.replace(Regex("^\\s*[┃│║]\\s*"), "").trim() == "Review"
        }
        if (reviewIdx < 0) return@CliPatterns null
        val answers = mutableListOf<String>()
        for (i in reviewIdx + 1 until lines.size) {
            val trimmed = lines[i].replace(Regex("^\\s*[┃│║]\\s*"), "").trim()
            if (Regex("⇆\\s+tab|enter\\s+submit|enter\\s+confirm|esc\\s+dismiss").containsMatchIn(trimmed)) break
            if (Regex("\\bConfirm\\b").containsMatchIn(trimmed) && Regex("\\s{2,}").containsMatchIn(trimmed)) break
            if (trimmed.isEmpty()) continue
            if (Regex("^[^:]+:\\s*.+").matches(trimmed)) answers.add(trimmed)
        }
        if (answers.isNotEmpty()) answers else null
    },
    cursorIndexExtractor = null,
)

// === SECTION 3 END ===

// ── Claude Code patterns ──────────────────────────────────────────────────────

private val claudeCodePatterns = CliPatterns(
    statusPatterns = listOf(
        StatusPattern(AgentStatus.BLOCKED,
            Regex("Enter\\s*to\\s*select.*(?:Tab/Arrow|Tab).*Esc\\s*to\\s*cancel", RegexOption.IGNORE_CASE), 11),
        StatusPattern(AgentStatus.BLOCKED,
            Regex("Esc\\s*to\\s*cancel.*Tab\\s*to\\s*amend.*ctrl\\+e\\s*to\\s*explain", RegexOption.IGNORE_CASE), 10),
        StatusPattern(AgentStatus.BLOCKED,
            Regex("←\\s+[☐☒].*✔\\s*Submit\\s*→"), 9),
        StatusPattern(AgentStatus.BLOCKED,
            Regex("↑/↓\\s+to\\s+navigate"), 10),
        StatusPattern(AgentStatus.BLOCKED,
            Regex("Would you like to proceed\\?"), 9),
        StatusPattern(AgentStatus.BLOCKED,
            Regex("Yes,\\s+I\\s+trust\\s+this\\s+folder"), 9),
        StatusPattern(AgentStatus.BLOCKED,
            Regex("Yes,\\s+I\\s+accept"), 9),
        StatusPattern(AgentStatus.WORKING,
            Regex("[✶✢✽✻✳·*][^\\n]*…"), 8),
        StatusPattern(AgentStatus.DONE,
            Regex("[✶✢✽✻✳][^\\n…]*\\bfor\\s+\\d+(?:\\.\\d+)?\\s*s\\b"), 7),
        StatusPattern(AgentStatus.IDLE,
            Regex("[>❯][\\s\\u00a0]"), 5),
    ),
    questionExtractor = { text ->
        val lines = text.split("\n")
        // Plan Mode approval
        val planQIdx = lines.indexOfFirst { Regex("Would you like to proceed\\?").containsMatchIn(it) }
        if (planQIdx >= 0) {
            val hasNumberedOptions = lines.drop(planQIdx + 1).any {
                Regex("^\\s*[❯>]?\\s*\\d+\\.\\s+").containsMatchIn(it)
            }
            if (hasNumberedOptions) return@CliPatterns lines[planQIdx].trim()
            return@CliPatterns "Would you like to proceed?"
        }
        // Permission dialog
        for (line in lines) {
            if (Regex("Do\\s*you\\s*want\\s*to\\s*proceed\\?", RegexOption.IGNORE_CASE).containsMatchIn(line)) {
                return@CliPatterns "Do you want to proceed?"
            }
        }
        // Trust dialog
        for (line in lines) {
            if (Regex("Yes,\\s+I\\s+trust\\s+this\\s+folder").containsMatchIn(line)) {
                return@CliPatterns "Do you trust this folder?"
            }
        }
        // Multi-question selection widget
        val multiQFooterIdx = lines.indexOfFirst {
            Regex("Enter\\s*to\\s*select.*(?:Tab/Arrow|Tab).*Esc\\s*to\\s*cancel", RegexOption.IGNORE_CASE).containsMatchIn(it)
        }
        if (multiQFooterIdx >= 0) {
            val optionPattern = Regex("^\\s*[❯>]?\\s*(\\d+)\\.\\s+(.+)")
            var firstOptionIdx = -1
            for (i in 0 until multiQFooterIdx) {
                if (optionPattern.matches(lines[i])) { firstOptionIdx = i; break }
            }
            if (firstOptionIdx >= 0) {
                for (i in firstOptionIdx - 1 downTo 0) {
                    val trimmed = lines[i].trim()
                    if (trimmed.isEmpty()) continue
                    if (Regex("☐.*✔\\s*Submit").containsMatchIn(trimmed) || Regex("^[←→]").containsMatchIn(trimmed)) continue
                    if (Regex("^[─━]+$").matches(trimmed)) continue
                    return@CliPatterns trimmed
                }
            }
            return@CliPatterns "Claude Code is asking a question"
        }
        // Submit tab
        for (line in lines) {
            if (Regex("Ready\\s+to\\s+submit\\s+your\\s+answers\\?", RegexOption.IGNORE_CASE).containsMatchIn(line)) {
                return@CliPatterns "Ready to submit your answers?"
            }
        }
        // Selection widget
        for (i in lines.indices) {
            if (Regex("↑/↓\\s+to\\s+navigate").containsMatchIn(lines[i])) {
                var firstOptionIdx = -1
                for (j in i - 1 downTo maxOf(0, i - 20)) {
                    if (Regex("^\\s{0,3}[❯>]?\\s*\\d+\\.\\s").containsMatchIn(lines[j])) firstOptionIdx = j
                }
                if (firstOptionIdx >= 0) {
                    for (j in firstOptionIdx - 1 downTo 0) {
                        val trimmed = lines[j].trim()
                        if (trimmed.isEmpty()) continue
                        if (Regex("^[✶✢✽✻✳·*]").containsMatchIn(trimmed)) continue
                        if (Regex("^[>❯]").containsMatchIn(trimmed)) continue
                        if (Regex("^─+$").matches(trimmed)) continue
                        if (Regex("^[←→]").containsMatchIn(trimmed) || Regex("✔\\s*Submit").containsMatchIn(trimmed)) continue
                        return@CliPatterns trimmed
                    }
                }
                return@CliPatterns "Select an option"
            }
        }
        null
    },
    optionsExtractor = { text ->
        val lines = text.split("\n")
        // Plan Mode approval
        val planQIdx = lines.indexOfFirst { Regex("Would you like to proceed\\?").containsMatchIn(it) }
        if (planQIdx >= 0) {
            var footerIdx = lines.size
            for (i in planQIdx + 1 until lines.size) {
                if (Regex("ctrl\\+g\\s+to\\s+edit", RegexOption.IGNORE_CASE).containsMatchIn(lines[i])) {
                    footerIdx = i; break
                }
            }
            val options = mutableListOf<String>()
            val optionPattern = Regex("^\\s*[❯>]?\\s*(\\d+)\\.\\s*(.+)")
            for (i in planQIdx + 1 until footerIdx) {
                val m = optionPattern.matchEntire(lines[i])
                if (m != null) options.add("${m.groupValues[1]}. ${m.groupValues[2].trim()}")
            }
            if (options.isNotEmpty()) return@CliPatterns options
            return@CliPatterns listOf("Yes", "No")
        }
        // Permission dialog
        val permFooterIdx = lines.indexOfFirst {
            Regex("Esc\\s*to\\s*cancel.*Tab\\s*to\\s*amend.*ctrl\\+e\\s*to\\s*explain", RegexOption.IGNORE_CASE).containsMatchIn(it)
        }
        if (permFooterIdx >= 0) {
            val options = mutableListOf<String>()
            val optionPattern = Regex("^\\s*[❯>]?\\s*(\\d+)\\.\\s*(.+)")
            for (i in 0 until permFooterIdx) {
                val m = optionPattern.matchEntire(lines[i])
                if (m != null) options.add("${m.groupValues[1]}. ${m.groupValues[2].trim()}")
            }
            if (options.isNotEmpty()) return@CliPatterns options
        }
        // Trust dialog
        for (line in lines) {
            if (Regex("Yes,\\s+I\\s+trust\\s+this\\s+folder").containsMatchIn(line)) {
                return@CliPatterns listOf("Yes, I trust this folder", "No, I don't trust this folder")
            }
        }
        // Multi-question selection widget
        val multiQFooterIdx = lines.indexOfFirst {
            Regex("Enter\\s*to\\s*select.*(?:Tab/Arrow|Tab).*Esc\\s*to\\s*cancel", RegexOption.IGNORE_CASE).containsMatchIn(it)
        }
        if (multiQFooterIdx >= 0) {
            val options = mutableListOf<String>()
            val optionPattern = Regex("^\\s*[❯>]?\\s*(\\d+)\\.\\s*(.+)")
            for (i in 0 until multiQFooterIdx) {
                val m = optionPattern.matchEntire(lines[i])
                if (m != null) options.add("${m.groupValues[1]}. ${m.groupValues[2].trim()}")
            }
            if (options.isNotEmpty()) return@CliPatterns options
        }
        // Submit tab
        val submitTabIdx = lines.indexOfFirst {
            Regex("Ready\\s+to\\s+submit\\s+your\\s+answers\\?", RegexOption.IGNORE_CASE).containsMatchIn(it)
        }
        if (submitTabIdx >= 0) {
            val options = mutableListOf<String>()
            val optionPattern = Regex("^\\s*[❯>]?\\s*(\\d+)\\.\\s*(.+)")
            for (i in submitTabIdx + 1 until lines.size) {
                val m = optionPattern.matchEntire(lines[i])
                if (m != null) options.add("${m.groupValues[1]}. ${m.groupValues[2].trim()}")
            }
            if (options.isNotEmpty()) return@CliPatterns options
        }
        // Selection widget
        for (i in lines.indices) {
            if (Regex("↑/↓\\s+to\\s+navigate").containsMatchIn(lines[i])) {
                val options = mutableListOf<String>()
                for (j in i - 1 downTo maxOf(0, i - 25)) {
                    val raw = lines[j]
                    val trimmed = raw.trim()
                    if (trimmed.isEmpty()) continue
                    if (Regex("^─+$").matches(trimmed)) continue
                    val m = Regex("^\\s{0,3}[❯>]?\\s*(\\d+)\\.\\s*(.+)").matchEntire(raw)
                    if (m != null) {
                        options.add(0, "${m.groupValues[1]}. ${m.groupValues[2].trim()}")
                        continue
                    }
                    if (Regex("^[✶✢✽✻✳·*]").containsMatchIn(trimmed)) continue
                    if (Regex("^[>❯]").containsMatchIn(trimmed)) continue
                    if (Regex("^[←→]").containsMatchIn(trimmed) || Regex("✔\\s*Submit").containsMatchIn(trimmed)) break
                }
                return@CliPatterns if (options.isNotEmpty()) options else null
            }
        }
        null
    },
    multiSelectDetector = { text ->
        text.split("\n").any { Regex("^\\s*[❯>]?\\s*\\d+\\.\\s*\\[[\\s✓✔]\\]").containsMatchIn(it) }
    },
    multiQuestionDetector = { text ->
        Regex("[☐☒].*✔\\s*Submit").containsMatchIn(text) || Regex("←.*[☐☒].*Submit.*→").containsMatchIn(text)
    },
    reviewAnswersExtractor = null,
    cursorIndexExtractor = null,
)

// === SECTION 4 END ===

// ── Codex patterns ────────────────────────────────────────────────────────────

private val codexPatterns = CliPatterns(
    statusPatterns = listOf(
        StatusPattern(AgentStatus.BLOCKED,
            Regex("^\\s*Question\\s+\\d+/\\d+", RegexOption.MULTILINE), 10),
        StatusPattern(AgentStatus.BLOCKED,
            Regex("tab\\s+to\\s+add\\s+notes.*enter\\s+to\\s+submit", RegexOption.IGNORE_CASE), 10),
        StatusPattern(AgentStatus.BLOCKED,
            Regex("enter\\s+to\\s+submit\\s+all", RegexOption.IGNORE_CASE), 10),
        StatusPattern(AgentStatus.BLOCKED,
            Regex("Press\\s+enter\\s+to\\s+(?:confirm|continue)\\s+or\\s+esc\\s+to\\s+(?:cancel|go\\s+back)", RegexOption.IGNORE_CASE), 10),
        StatusPattern(AgentStatus.BLOCKED,
            Regex("^(?:Approve|Allow)\\b.*\\b(?:y/n|yes/no|yes|no)\\b", setOf(RegexOption.IGNORE_CASE, RegexOption.MULTILINE)), 9),
        StatusPattern(AgentStatus.BLOCKED,
            Regex("allow\\s+Codex\\s+to\\s+work\\s+in\\s+this\\s+folder", RegexOption.IGNORE_CASE), 10),
        StatusPattern(AgentStatus.BLOCKED,
            Regex("Do you trust the contents of this directory\\?", RegexOption.IGNORE_CASE), 10),
        StatusPattern(AgentStatus.WORKING,
            Regex("•.*\\(\\d+s\\s*•\\s*esc\\s+to\\s+interrupt\\)"), 8),
        StatusPattern(AgentStatus.IDLE,
            Regex("^\\s*(?:❯|›|codex>)\\s*$", RegexOption.MULTILINE), 5),
    ),
    questionExtractor = { text ->
        val lines = text.split("\n")
        // request_user_input: question text after "Question N/M"
        for (i in lines.indices) {
            if (Regex("^\\s*Question\\s+\\d+/\\d+", RegexOption.IGNORE_CASE).containsMatchIn(lines[i])) {
                for (j in i + 1 until lines.size) {
                    val trimmed = lines[j].trim()
                    if (trimmed.isNotEmpty()) return@CliPatterns trimmed
                }
            }
        }
        // Approval overlay
        for (line in lines) {
            val m = Regex("^\\s*(Would you like to .+)$", RegexOption.IGNORE_CASE).matchEntire(line)
            if (m != null) return@CliPatterns m.groupValues[1].trim()
        }
        for (line in lines) {
            val m = Regex("^\\s*(Do you want to approve .+)$", RegexOption.IGNORE_CASE).matchEntire(line)
            if (m != null) return@CliPatterns m.groupValues[1].trim()
        }
        // Trust prompt
        for (line in lines) {
            if (Regex("Do you trust the contents of this directory\\?", RegexOption.IGNORE_CASE).containsMatchIn(line)) {
                return@CliPatterns "Do you trust the contents of this directory?"
            }
        }
        for (line in lines) {
            if (Regex("allow\\s+Codex\\s+to\\s+work\\s+in\\s+this\\s+folder", RegexOption.IGNORE_CASE).containsMatchIn(line)) {
                return@CliPatterns "Allow Codex to work in this folder?"
            }
        }
        // Legacy approval prompt
        for (line in lines) {
            if (Regex("^(?:Approve|Allow)\\b.*\\b(?:y/n|yes/no)\\b", RegexOption.IGNORE_CASE).containsMatchIn(line)) {
                return@CliPatterns line.trim()
            }
        }
        null
    },
    optionsExtractor = { text ->
        val lines = text.split("\n")
        var contentStartIdx = -1
        for (i in lines.indices) {
            if (Regex("^\\s*Question\\s+\\d+/\\d+", RegexOption.IGNORE_CASE).containsMatchIn(lines[i]) ||
                Regex("^\\s*Would you like to ", RegexOption.IGNORE_CASE).containsMatchIn(lines[i]) ||
                Regex("^\\s*Do you want to approve ", RegexOption.IGNORE_CASE).containsMatchIn(lines[i])) {
                contentStartIdx = i; break
            }
        }
        val startIdx = if (contentStartIdx >= 0) contentStartIdx else 0
        val opts = mutableListOf<String>()
        for (i in startIdx until lines.size) {
            val m = Regex("^\\s*[› ]\\s*(\\d+\\.\\s+.+)$").matchEntire(lines[i])
            if (m != null) {
                val full = m.groupValues[1].trim()
                val labelOnly = full.replace(Regex("\\s{2,}.+$"), "").trim()
                opts.add(labelOnly.ifEmpty { full })
            }
        }
        if (opts.isNotEmpty()) return@CliPatterns opts
        // Legacy approval prompt
        for (line in lines) {
            if (Regex("^(?:Approve|Allow)\\b.*\\b(?:y/n|yes/no)\\b", RegexOption.IGNORE_CASE).containsMatchIn(line)) {
                return@CliPatterns listOf("Yes (y)", "No (n)")
            }
        }
        // Trust prompt
        for (line in lines) {
            if (Regex("allow\\s+Codex\\s+to\\s+work\\s+in\\s+this\\s+folder", RegexOption.IGNORE_CASE).containsMatchIn(line) ||
                Regex("Do you trust the contents of this directory\\?", RegexOption.IGNORE_CASE).containsMatchIn(line)) {
                return@CliPatterns listOf("Yes, continue", "No, quit")
            }
        }
        null
    },
    multiSelectDetector = null,
    multiQuestionDetector = { text ->
        Regex("^\\s*Question\\s+\\d+/\\d+", RegexOption.MULTILINE).containsMatchIn(text)
    },
    reviewAnswersExtractor = null,
    cursorIndexExtractor = null,
)

// === SECTION 5 END ===

// ── Pattern registry ──────────────────────────────────────────────────────────

private val PATTERNS: Map<CliType, CliPatterns> = mapOf(
    CliType.DEVIN to devinPatterns,
    CliType.OPENCODE to opencodePatterns,
    CliType.CLAUDE_CODE to claudeCodePatterns,
    CliType.CODEX to codexPatterns,
)

/** Get the patterns for a specific CLI type. */
fun getPatterns(cli: CliType): CliPatterns? = PATTERNS[cli]

/**
 * Detect status from screen text using CLI-specific patterns.
 * Patterns are checked in priority order (highest first).
 * @return detected status, or null if no pattern matched.
 */
fun detectStatusFromScreen(cli: CliType, screenText: String): AgentStatus? {
    val patterns = PATTERNS[cli] ?: return null
    val sorted = patterns.statusPatterns.sortedByDescending { it.priority }
    for (p in sorted) {
        if (p.pattern.containsMatchIn(screenText)) return p.status
    }
    return null
}

/** Extract question text from screen when blocked. */
fun extractQuestion(cli: CliType, screenText: String): String? =
    PATTERNS[cli]?.questionExtractor?.invoke(screenText)

/** Extract answer options from screen when blocked. */
fun extractOptions(cli: CliType, screenText: String): List<String>? =
    PATTERNS[cli]?.optionsExtractor?.invoke(screenText)

/** Detect if the current blocked dialog is multi-select. */
fun detectMultiSelect(cli: CliType, screenText: String): Boolean =
    PATTERNS[cli]?.multiSelectDetector?.invoke(screenText) ?: false

/** Detect if the current blocked dialog is multi-question. */
fun detectMultiQuestion(cli: CliType, screenText: String): Boolean =
    PATTERNS[cli]?.multiQuestionDetector?.invoke(screenText) ?: false

/** Extract review answers from the Confirm tab (multi-question dialogs). */
fun extractReviewAnswers(cli: CliType, screenText: String): List<String>? =
    PATTERNS[cli]?.reviewAnswersExtractor?.invoke(screenText)

/** Detect the cursor position (❭ marker) in single-select mode. */
fun extractCursorIndex(cli: CliType, screenText: String): Int? =
    PATTERNS[cli]?.cursorIndexExtractor?.invoke(screenText)

// === SECTION 6 END ===

