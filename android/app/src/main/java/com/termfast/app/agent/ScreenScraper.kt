package com.termfast.app.agent

/**
 * Screen scraper — extract visible text from termlib snapshot.
 *
 * Ported from desktop `src/hooks/screenScraper.ts` (377 lines).
 * Uses ScrapedSnapshot (plain data class) instead of termlib's internal
 * TerminalSnapshot, to avoid reflection leaking throughout the codebase.
 */
object ScreenScraper {

    /** Maximum number of lines to scrape from the viewport. */
    private const val MAX_LINES = 50

    /**
     * Extract visible text lines from a scraped snapshot.
     * Returns plain-text lines (no ANSI codes), trimmed of trailing whitespace.
     * Only the bottom MAX_LINES lines are returned.
     */
    fun scrapeScreen(snapshot: ScrapedSnapshot): List<String> {
        val lines = snapshot.lines
        val height = lines.size
        val startIdx = maxOf(0, height - MAX_LINES)
        val result = ArrayList<String>(height - startIdx)
        for (i in startIdx until height) {
            result.add(extractLineText(lines[i]))
        }
        return result
    }

    /**
     * Extract text from a single ScrapedLine.
     */
    fun extractLineText(line: ScrapedLine): String {
        return line.text.trimEnd()
    }

    /**
     * Join screen lines into a single string for regex matching.
     */
    fun joinLines(lines: List<String>): String = lines.joinToString("\n")

    /**
     * Get the last N non-empty lines from the screen scrape.
     */
    fun getBottomLines(lines: List<String>, n: Int): List<String> {
        val nonEmpty = lines.filter { it.trim().isNotEmpty() }
        return nonEmpty.takeLast(n)
    }

    /**
     * Extract tab info from multi-question dialog tab rows.
     *
     * Supports three formats (verified from CLI source code, §9 of design doc):
     *
     * 1. Claude Code: "← ☐ Q1 ☐ Q2 → ✔ Submit"
     *    - Active tab detected by reverse video (cell.reverse == true)
     *
     * 2. OpenCode: tab labels + "Confirm" (2+ space separated)
     *    - Active tab has accent background (detected via reverse/bold)
     *
     * 3. Devin: "── label · label · ... ──"
     *    - Active tab detected by foreground color brightness or bold
     *
     * @param snapshot the scraped snapshot
     * @return TabInfo or null if no tab row found
     */
    fun extractTabInfo(snapshot: ScrapedSnapshot): TabInfo? {
        val lines = snapshot.lines
        val height = lines.size
        val startIdx = maxOf(0, height - MAX_LINES)

        for (i in startIdx until height) {
            val line = lines[i]
            val text = extractLineText(line)
            val trimmed = text.replace(Regex("^\\s*[┃│║]\\s*"), "").trim()

            // ── Claude Code format: "← ☐ label ... ✔ Submit →" ──
            if (Regex("^←\\s+[☐☒].*✔\\s*Submit\\s*→").matches(trimmed)) {
                val content = trimmed.replace(Regex("^←\\s+"), "")
                    .replace(Regex("\\s*→$"), "").trim()
                val parts = content.split(Regex("\\s{2,}"))
                if (parts.size < 2) continue
                val labels = parts.map { it.replace(Regex("^[☐☒✔]\\s*"), "").trim() }
                val activeIndex = detectActiveTabByReverse(line, labels, text)
                return TabInfo(labels, activeIndex)
            }

            // ── OpenCode format: "Confirm" + 2+ space-separated labels ──
            if (Regex("\\bConfirm\\b").containsMatchIn(trimmed) &&
                Regex("\\s{2,}").containsMatchIn(trimmed)) {
                val labels = trimmed.split(Regex("\\s{2,}"))
                if (labels.size < 2) continue
                val activeIndex = detectActiveTabByReverse(line, labels, text)
                return TabInfo(labels, activeIndex)
            }

            // ── Devin format: "── label · label · ... ──" ──
            if (Regex("^──\\s+.+\\s·\\s.+\\s──").matches(trimmed)) {
                val content = trimmed.replace(Regex("^──\\s+"), "")
                    .replace(Regex("\\s──+$"), "").trim()
                val labels = content.split(Regex("\\s·\\s"))
                if (labels.size < 2) continue
                val activeIndex = detectActiveTabByFgColor(line, labels, text)
                return TabInfo(labels, activeIndex)
            }
        }
        return null
    }

    /**
     * Detect active tab by checking cell reverse-video flag.
     */
    private fun detectActiveTabByReverse(
        line: ScrapedLine,
        labels: List<String>,
        text: String,
    ): Int {
        val cells = line.cells
        var searchStart = 0
        for (j in labels.indices) {
            val label = labels[j].replace(Regex("\\s✓$"), "").trim()
            val charPos = text.indexOf(label, searchStart)
            if (charPos < 0) {
                searchStart += label.length
                continue
            }
            val cellPos = charPosToCellIndex(line, charPos)
            if (cellPos in cells.indices) {
                if (cells[cellPos].reverse) return j
            }
            searchStart = charPos + label.length
        }
        return -1
    }

    /**
     * Detect active tab by checking cell foreground color brightness or bold.
     * Devin uses bright blue fg for active tabs, gray for inactive.
     */
    private fun detectActiveTabByFgColor(
        line: ScrapedLine,
        labels: List<String>,
        text: String,
    ): Int {
        val cells = line.cells
        var searchStart = 0
        for (j in labels.indices) {
            val label = labels[j].replace(Regex("\\s✓$"), "").trim()
            val charPos = text.indexOf(label, searchStart)
            if (charPos < 0) {
                searchStart += label.length
                continue
            }
            val cellPos = charPosToCellIndex(line, charPos)
            if (cellPos in cells.indices) {
                val cell = cells[cellPos]
                if (cell.bold) return j
                val fgColor = cell.fgColor
                val r = ((fgColor shr 16) and 0xFF).toInt()
                val g = ((fgColor shr 8) and 0xFF).toInt()
                val b = (fgColor and 0xFF).toInt()
                if (r + g + b > 400) return j
            }
            searchStart = charPos + label.length
        }
        return -1
    }

    /**
     * Convert character index to cell index.
     * CJK characters take 2 cells (width 2), so char index != cell index.
     */
    private fun charPosToCellIndex(line: ScrapedLine, charPos: Int): Int {
        val cells = line.cells
        var charIdx = 0
        for (x in cells.indices) {
            val cell = cells[x]
            if (cell.width == 0) continue  // skip wide-char continuation
            if (charIdx == charPos) return x
            charIdx++
        }
        return charPos  // fallback: assume 1:1 mapping
    }
}
