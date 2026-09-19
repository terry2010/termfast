package com.termfast.app.agent

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class ScreenScraperTest {

    private fun makeLine(text: String, cells: List<ScrapedCell>? = null): ScrapedLine {
        return ScrapedLine(text = text, cells = cells ?: text.map { ScrapedCell(it, false, false, 0, 0L, 1) })
    }

    private fun makeSnapshot(lines: List<ScrapedLine>, title: String = ""): ScrapedSnapshot {
        return ScrapedSnapshot(
            lines = lines,
            terminalTitle = title,
            cursorRow = 0,
            cursorCol = 0,
            rows = lines.size,
            cols = if (lines.isEmpty()) 0 else lines[0].text.length,
        )
    }

    @Test
    fun testScrapeScreenBasic() {
        val snapshot = makeSnapshot(listOf(
            makeLine("line 1"),
            makeLine("line 2"),
            makeLine("line 3"),
        ))
        val result = ScreenScraper.scrapeScreen(snapshot)
        assertEquals(3, result.size)
        assertEquals("line 1", result[0])
        assertEquals("line 2", result[1])
        assertEquals("line 3", result[2])
    }

    @Test
    fun testScrapeScreenTrimsTrailingWhitespace() {
        val snapshot = makeSnapshot(listOf(makeLine("text   ")))
        val result = ScreenScraper.scrapeScreen(snapshot)
        assertEquals("text", result[0])
    }

    @Test
    fun testScrapeScreenEmptyLines() {
        val snapshot = makeSnapshot(listOf(
            makeLine("hello"),
            makeLine(""),
            makeLine("world"),
        ))
        val result = ScreenScraper.scrapeScreen(snapshot)
        assertEquals(3, result.size)
        assertEquals("", result[1])
    }

    @Test
    fun testScrapeScreenMaxLinesLimit() {
        val lines = (1..60).map { makeLine("line $it") }
        val snapshot = makeSnapshot(lines)
        val result = ScreenScraper.scrapeScreen(snapshot)
        // Should only return the bottom 50 lines
        assertEquals(50, result.size)
        assertEquals("line 11", result[0])
        assertEquals("line 60", result[49])
    }

    @Test
    fun testExtractLineText() {
        val line = makeLine("  hello world  ")
        assertEquals("  hello world", ScreenScraper.extractLineText(line))
    }

    @Test
    fun testJoinLines() {
        val lines = listOf("a", "b", "c")
        assertEquals("a\nb\nc", ScreenScraper.joinLines(lines))
    }

    @Test
    fun testGetBottomLinesFiltersEmpty() {
        val lines = listOf("a", "", "  ", "b", "c")
        val result = ScreenScraper.getBottomLines(lines, 2)
        assertEquals(listOf("b", "c"), result)
    }

    @Test
    fun testGetBottomLinesAllEmpty() {
        val lines = listOf("", "  ", "")
        val result = ScreenScraper.getBottomLines(lines, 3)
        assertTrue(result.isEmpty())
    }

    @Test
    fun testGetBottomLinesFewerThanN() {
        val lines = listOf("a", "b")
        val result = ScreenScraper.getBottomLines(lines, 5)
        assertEquals(listOf("a", "b"), result)
    }

    @Test
    fun testExtractTabInfoNoTabs() {
        val snapshot = makeSnapshot(listOf(
            makeLine("just output"),
            makeLine("no tabs here"),
        ))
        val result = ScreenScraper.extractTabInfo(snapshot)
        assertNull(result)
    }

    @Test
    fun testExtractTabInfoEmptySnapshot() {
        val snapshot = makeSnapshot(emptyList())
        val result = ScreenScraper.extractTabInfo(snapshot)
        assertNull(result)
    }

    // ── Positive tab-detection cases ──

    private fun makeLineWithCells(text: String, boldRanges: List<IntRange> = emptyList(),
                                  reverseRanges: List<IntRange> = emptyList(),
                                  fgColorRanges: List<Pair<IntRange, Long>> = emptyList()): ScrapedLine {
        val cells = text.mapIndexed { i, c ->
            val bold = boldRanges.any { i in it }
            val reverse = reverseRanges.any { i in it }
            val fg = fgColorRanges.firstOrNull { i in it.first }?.second ?: 0L
            ScrapedCell(c, bold, reverse, 0, fg, 1)
        }
        return ScrapedLine(text = text, cells = cells)
    }

    @Test
    fun testExtractTabInfoClaudeFormatActiveByReverse() {
        // "← ☐ Lang ☐ OS ✔ Submit →" — second tab has reverse video
        val text = "← ☐ Lang  ☐ OS  ✔ Submit →"
        val osStart = text.indexOf("OS")
        val line = makeLineWithCells(text, reverseRanges = listOf(osStart until osStart + 2))
        val result = ScreenScraper.extractTabInfo(makeSnapshot(listOf(line)))
        assertNotNull(result)
        assertEquals(listOf("Lang", "OS", "Submit"), result.labels)
        assertEquals(1, result.activeIndex)
    }

    @Test
    fun testExtractTabInfoClaudeFormatNoActive() {
        val line = makeLine("← ☐ Lang  ☐ OS  ✔ Submit →")
        val result = ScreenScraper.extractTabInfo(makeSnapshot(listOf(line)))
        assertNotNull(result)
        assertEquals(3, result.labels.size)
        assertEquals(-1, result.activeIndex)
    }

    @Test
    fun testExtractTabInfoDevinFormatActiveByBold() {
        val text = "── Build · Test ──"
        val testStart = text.indexOf("Test")
        val line = makeLineWithCells(text, boldRanges = listOf(testStart until testStart + 4))
        val result = ScreenScraper.extractTabInfo(makeSnapshot(listOf(line)))
        assertNotNull(result)
        assertEquals(listOf("Build", "Test"), result.labels)
        assertEquals(1, result.activeIndex)
    }

    @Test
    fun testExtractTabInfoDevinFormatActiveByFgColor() {
        // Compose sRGB Color: A<<56 | R<<48 | G<<40 | B<<32, colorspace id 0.
        // Bright blue 0xFF89B4FA → r+g+b = 137+180+250 = 567 > 400
        val brightBlue = (0xFFL shl 56) or (0x89L shl 48) or (0xB4L shl 40) or (0xFAL shl 32)
        val text = "── Build · Test ──"
        val testStart = text.indexOf("Test")
        val line = makeLineWithCells(text, fgColorRanges = listOf((testStart until testStart + 4) to brightBlue))
        val result = ScreenScraper.extractTabInfo(makeSnapshot(listOf(line)))
        assertNotNull(result)
        assertEquals(1, result.activeIndex)
    }

    @Test
    fun testExtractTabInfoDevinFormatDimColorNotActive() {
        // Dim gray should NOT be detected as active (sum < 400)
        val dimGray = (0xFFL shl 56) or (0x40L shl 48) or (0x40L shl 40) or (0x40L shl 32)
        val text = "── Build · Test ──"
        val testStart = text.indexOf("Test")
        val line = makeLineWithCells(text, fgColorRanges = listOf((testStart until testStart + 4) to dimGray))
        val result = ScreenScraper.extractTabInfo(makeSnapshot(listOf(line)))
        assertNotNull(result)
        assertEquals(-1, result.activeIndex)
    }

    @Test
    fun testDecodeColorChannelsSrgb() {
        // sRGB: A<<56|R<<48|G<<40|B<<32, colorspace id = 0
        val color = (0xFFL shl 56) or (0x12L shl 48) or (0x34L shl 40) or (0x56L shl 32)
        val (r, g, b) = ScreenScraper.decodeColorChannels(color)
        assertEquals(0x12, r)
        assertEquals(0x34, g)
        assertEquals(0x56, b)
    }

    @Test
    fun testDecodeColorChannelsHalfFloat() {
        // Non-sRGB colorspace id != 0 → 16-bit half-float channels.
        // half-float 1.0 = 0x3C00, 0.5 = 0x3800, 0.0 = 0x0000
        val color = (0x3C00L shl 48) or (0x3800L shl 32) or (0x0000L shl 16) or 1L
        val (r, g, b) = ScreenScraper.decodeColorChannels(color)
        assertEquals(255, r)
        assertEquals(127, g)
        assertEquals(0, b)
    }
}
