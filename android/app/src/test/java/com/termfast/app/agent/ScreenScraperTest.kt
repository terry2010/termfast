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
}
