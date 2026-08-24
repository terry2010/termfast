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
    ): BehaviorContext = BehaviorContext(
        options = options,
        isMultiSelect = isMultiSelect,
        isMultiQuestion = isMultiQuestion,
        activeTabIndex = 0,
        totalTabs = 0,
    )

    // === Codex behavior (FP5-2: toggle must send Space) ===

    @Test
    fun testCodexToggleSendsSpace() {
        val ctx = makeCtx(options = listOf("1. Yes (y)", "2. No (n)"), isMultiSelect = true)
        val result = CodexBehavior.toggle("1. Yes (y)", 0, ctx)
        assertEquals(1, result.steps.size)
        assertEquals(" ", result.steps[0].data, "Codex toggle must send Space (not shortcut key)")
        assertFalse(result.dismiss, "Toggle should not dismiss the sheet")
    }

    @Test
    fun testCodexToggleSendsSpaceForAnyOption() {
        val opts = listOf("1. Yes (y)", "2. No (n)", "3. Cancel (esc)")
        val ctx = makeCtx(options = opts, isMultiSelect = true)
        // Test all options — all should send Space, not the shortcut key
        for (i in opts.indices) {
            val result = CodexBehavior.toggle(opts[i], i, ctx)
            assertEquals(" ", result.steps[0].data,
                "Codex toggle for option '$i' must send Space, got '${result.steps[0].data}'")
        }
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
