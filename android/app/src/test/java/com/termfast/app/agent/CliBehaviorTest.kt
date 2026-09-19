package com.termfast.app.agent

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class CliBehaviorTest {

    private fun makeCtx(
        options: List<String>? = null,
        isMultiSelect: Boolean = false,
        isMultiQuestion: Boolean = false,
        cursorPos: Int = 0,
    ): BehaviorContext = BehaviorContext(
        options = options,
        isMultiSelect = isMultiSelect,
        isMultiQuestion = isMultiQuestion,
        activeTabIndex = 0,
        totalTabs = 0,
        cursorPos = cursorPos,
    )

    // === Codex behavior: toggle navigates to the target row, then Space ===

    @Test
    fun testCodexToggleSendsSpace() {
        val ctx = makeCtx(options = listOf("1. Yes (y)", "2. No (n)"), isMultiSelect = true)
        val result = CodexBehavior.toggle("1. Yes (y)", 0, ctx)
        assertEquals(1, result.steps.size)
        assertEquals(" ", result.steps[0].data, "Codex toggle must send Space (not shortcut key)")
        assertFalse(result.dismiss, "Toggle should not dismiss the sheet")
        assertEquals(0, result.newCursorPos)
    }

    @Test
    fun testCodexToggleNavigatesToTargetRow() {
        val opts = listOf("1. Yes (y)", "2. No (n)", "3. Cancel (esc)")
        val ctx = makeCtx(options = opts, isMultiSelect = true, cursorPos = 0)
        // Toggle option 2 from cursor 0: Down×2 + Space
        val result = CodexBehavior.toggle(opts[2], 2, ctx)
        assertEquals("[B[B ", result.steps[0].data)
        assertEquals(2, result.newCursorPos)
        // Toggle option 0 from cursor 2: Up×2 + Space
        val result2 = CodexBehavior.toggle(opts[0], 0, makeCtx(options = opts, isMultiSelect = true, cursorPos = 2))
        assertEquals("[A[A ", result2.steps[0].data)
        assertEquals(0, result2.newCursorPos)
    }

    @Test
    fun testCodexSubmitMultiSelectSendsEnter() {
        val ctx = makeCtx(isMultiSelect = true)
        val result = CodexBehavior.submitMultiSelect(ctx)
        assertEquals(1, result.steps.size)
        assertEquals("\r", result.steps[0].data, "Codex submitMultiSelect must send Enter")
        assertTrue(result.dismiss, "submitMultiSelect should dismiss the sheet")
    }

    // === Registry tests ===

    @Test
    fun testGetBehaviorCodex() {
        val behavior = CliBehaviorRegistry.getBehavior(CliType.CODEX)
        assertEquals(CodexBehavior, behavior)
    }

    @Test
    fun testGetBehaviorDevin() {
        val behavior = CliBehaviorRegistry.getBehavior(CliType.DEVIN)
        assertEquals(DevinBehavior, behavior)
    }

    @Test
    fun testGetBehaviorUnknownReturnsDefault() {
        val behavior = CliBehaviorRegistry.getBehavior(CliType.UNKNOWN)
        assertEquals(DefaultBehavior, behavior)
    }
}
