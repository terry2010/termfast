package com.termfast.app.agent

/**
 * Termlib reflection access — encapsulates all reflection to access termlib's
 * internal types (TerminalSnapshot, TerminalLine, TerminalEmulatorImpl).
 *
 * ⚠️ termlib marks TerminalSnapshot, TerminalLine, and TerminalEmulatorImpl as
 * `internal`. These are inaccessible from Kotlin directly. This object uses
 * reflection to access them, isolating the fragility to this single file.
 *
 * If termlib is upgraded and the internal API changes, this is the ONLY file
 * that needs updating. Regression tests in TermlibAccessTest verify the
 * method/field names are still valid.
 *
 * Public API of termlib (stable, no reflection needed):
 *   - TerminalEmulator (interface): writeInput, resize, clearScreen, etc.
 *   - TerminalEmulatorFactory: create emulator instances
 *   - Terminal (Compose composable): render the terminal
 *
 * Internal API accessed via reflection (unstable, isolated here):
 *   - TerminalEmulatorImpl.getSnapshot$lib(): StateFlow<TerminalSnapshot>
 *   - TerminalSnapshot.getLines(): List<TerminalLine>
 *   - TerminalSnapshot.getTerminalTitle(): String
 *   - TerminalSnapshot.getCursorRow(): Int
 *   - TerminalSnapshot.getCursorCol(): Int
 *   - TerminalLine.getText(): String
 *   - TerminalLine.getCells(): List<Cell>
 *   - Cell.getBold(): Boolean
 *   - Cell.getReverse(): Boolean
 *   - Cell.getUnderline(): Int
 *   - Cell.getFgColor-0d7_KjU(): Long (inline class, accessed via raw long)
 */

/** Plain data class for a scraped screen line (no termlib dependency). */
data class ScrapedLine(
    val text: String,
    val cells: List<ScrapedCell>,
)

/** Plain data class for a cell's style info (no termlib dependency). */
data class ScrapedCell(
    val char: Char,
    val bold: Boolean,
    val reverse: Boolean,
    val underline: Int,
    val fgColor: Long,
    val width: Int,
)

/** Plain data class for a screen snapshot (no termlib dependency). */
data class ScrapedSnapshot(
    val lines: List<ScrapedLine>,
    val terminalTitle: String,
    val cursorRow: Int,
    val cursorCol: Int,
    val rows: Int,
    val cols: Int,
)

object TermlibAccess {
    private const val TAG = "TermlibAccess"

    // Cached reflection references (initialized lazily)
    private val snapshotMethod by lazy {
        try {
            val implClass = Class.forName("org.connectbot.terminal.TerminalEmulatorImpl")
            implClass.getDeclaredMethod("getSnapshot\$lib")
        } catch (e: Exception) {
            null
        }
    }

    /**
     * Get the snapshot StateFlow from a TerminalEmulator.
     * @param emulator the TerminalEmulator (must be a TerminalEmulatorImpl instance)
     * @return the StateFlow<TerminalSnapshot> as Any, or null if reflection fails
     */
    fun getSnapshotFlow(emulator: Any): Any? {
        val method = snapshotMethod ?: return null
        return try {
            method.isAccessible = true
            method.invoke(emulator)
        } catch (e: Exception) {
            null
        }
    }

    /**
     * Extract the current snapshot value from a StateFlow.
     * @param flow the StateFlow<TerminalSnapshot> as Any
     * @return the TerminalSnapshot as Any, or null if reflection fails
     */
    fun getSnapshotValue(flow: Any): Any? {
        return try {
            val valueMethod = flow.javaClass.getMethod("getValue")
            valueMethod.invoke(flow)
        } catch (e: Exception) {
            null
        }
    }

    /**
     * Convert a termlib TerminalSnapshot (Any) to a plain ScrapedSnapshot.
     * Uses reflection to access lines, terminalTitle, cursorRow, cursorCol.
     */
    fun toScrapedSnapshot(snapshot: Any): ScrapedSnapshot? {
        return try {
            val lines = getLines(snapshot) ?: return null
            val title = getTerminalTitle(snapshot) ?: ""
            val cursorRow = getCursorRow(snapshot)
            val cursorCol = getCursorCol(snapshot)
            val rows = getRows(snapshot)
            val cols = getCols(snapshot)
            val scrapedLines = lines.mapNotNull { it?.let { obj -> toScrapedLine(obj) } }
            ScrapedSnapshot(scrapedLines, title, cursorRow, cursorCol, rows, cols)
        } catch (e: Exception) {
            null
        }
    }

    // ── TerminalSnapshot accessors ──

    private fun getLines(snapshot: Any): List<*>? {
        return try {
            val method = snapshot.javaClass.getMethod("getLines")
            method.invoke(snapshot) as? List<*>
        } catch (e: Exception) { null }
    }

    private fun getTerminalTitle(snapshot: Any): String? {
        return try {
            val method = snapshot.javaClass.getMethod("getTerminalTitle")
            method.invoke(snapshot) as? String
        } catch (e: Exception) { null }
    }

    private fun getCursorRow(snapshot: Any): Int {
        return try {
            val method = snapshot.javaClass.getMethod("getCursorRow")
            method.invoke(snapshot) as Int
        } catch (e: Exception) { 0 }
    }

    private fun getCursorCol(snapshot: Any): Int {
        return try {
            val method = snapshot.javaClass.getMethod("getCursorCol")
            method.invoke(snapshot) as Int
        } catch (e: Exception) { 0 }
    }

    private fun getRows(snapshot: Any): Int {
        return try {
            val method = snapshot.javaClass.getMethod("getRows")
            method.invoke(snapshot) as Int
        } catch (e: Exception) { 0 }
    }

    private fun getCols(snapshot: Any): Int {
        return try {
            val method = snapshot.javaClass.getMethod("getCols")
            method.invoke(snapshot) as Int
        } catch (e: Exception) { 0 }
    }

    // ── TerminalLine accessors ──

    private fun toScrapedLine(line: Any): ScrapedLine? {
        return try {
            val text = getLineText(line) ?: ""
            val cells = getLineCells(line)?.mapNotNull { it?.let { obj -> toScrapedCell(obj) } } ?: emptyList()
            ScrapedLine(text, cells)
        } catch (e: Exception) {
            null
        }
    }

    private fun getLineText(line: Any): String? {
        return try {
            val method = line.javaClass.getMethod("getText")
            method.invoke(line) as? String
        } catch (e: Exception) { null }
    }

    private fun getLineCells(line: Any): List<*>? {
        return try {
            val method = line.javaClass.getMethod("getCells")
            method.invoke(line) as? List<*>
        } catch (e: Exception) { null }
    }

    // ── Cell accessors ──

    private fun toScrapedCell(cell: Any): ScrapedCell? {
        return try {
            val char = getCellChar(cell)
            val bold = getCellBold(cell)
            val reverse = getCellReverse(cell)
            val underline = getCellUnderline(cell)
            val fgColor = getCellFgColor(cell)
            val width = getCellWidth(cell)
            ScrapedCell(char, bold, reverse, underline, fgColor, width)
        } catch (e: Exception) {
            null
        }
    }

    private fun getCellChar(cell: Any): Char {
        return try {
            val method = cell.javaClass.getMethod("getChar")
            method.invoke(cell) as Char
        } catch (e: Exception) { ' ' }
    }

    private fun getCellBold(cell: Any): Boolean {
        return try {
            val method = cell.javaClass.getMethod("getBold")
            method.invoke(cell) as Boolean
        } catch (e: Exception) { false }
    }

    private fun getCellReverse(cell: Any): Boolean {
        return try {
            val method = cell.javaClass.getMethod("getReverse")
            method.invoke(cell) as Boolean
        } catch (e: Exception) { false }
    }

    private fun getCellUnderline(cell: Any): Int {
        return try {
            val method = cell.javaClass.getMethod("getUnderline")
            method.invoke(cell) as Int
        } catch (e: Exception) { 0 }
    }

    private fun getCellWidth(cell: Any): Int {
        return try {
            val method = cell.javaClass.getMethod("getWidth")
            method.invoke(cell) as Int
        } catch (e: Exception) { 1 }
    }

    /**
     * Get the fg color of a cell.
     * The termlib Cell uses an inline class for fgColor (getFgColor-0d7_KjU).
     * The raw long value encodes RGB in the lower 24 bits.
     */
    private fun getCellFgColor(cell: Any): Long {
        return try {
            // Try the mangled method name first (inline class accessor)
            val method = cell.javaClass.methods.firstOrNull {
                it.name.startsWith("getFgColor") && it.parameterTypes.isEmpty()
            }
            method?.invoke(cell) as? Long ?: 0L
        } catch (e: Exception) { 0L }
    }
}
