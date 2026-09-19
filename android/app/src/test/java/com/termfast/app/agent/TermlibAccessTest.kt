package com.termfast.app.agent

import kotlin.test.Test
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * Regression tests for TermlibAccess reflection targets.
 *
 * TermlibAccess relies on termlib's internal API via reflection. If a termlib
 * upgrade renames or removes these methods/classes, these tests fail loudly
 * instead of silently degrading at runtime.
 */
class TermlibAccessTest {

    @Test
    fun testTerminalEmulatorImplExists() {
        val cls = Class.forName("org.connectbot.terminal.TerminalEmulatorImpl")
        assertNotNull(cls)
    }

    @Test
    fun testGetSnapshotLibMethodExists() {
        val cls = Class.forName("org.connectbot.terminal.TerminalEmulatorImpl")
        val method = cls.getDeclaredMethod("getSnapshot\$lib")
        assertNotNull(method)
        assertTrue(
            method.returnType.name.contains("StateFlow"),
            "getSnapshot\$lib should return a StateFlow, got ${method.returnType.name}",
        )
    }

    @Test
    fun testTerminalSnapshotAccessorsExist() {
        val cls = Class.forName("org.connectbot.terminal.TerminalSnapshot")
        for (name in listOf("getLines", "getTerminalTitle", "getCursorRow", "getCursorCol", "getRows", "getCols")) {
            assertNotNull(
                cls.methods.firstOrNull { it.name == name && it.parameterTypes.isEmpty() },
                "TerminalSnapshot missing accessor $name()",
            )
        }
    }

    @Test
    fun testTerminalLineAccessorsExist() {
        val cls = Class.forName("org.connectbot.terminal.TerminalLine")
        for (name in listOf("getText", "getCells")) {
            assertNotNull(
                cls.methods.firstOrNull { it.name == name && it.parameterTypes.isEmpty() },
                "TerminalLine missing accessor $name()",
            )
        }
    }

    @Test
    fun testCellAccessorsExist() {
        val cls = Class.forName("org.connectbot.terminal.TerminalLine\$Cell")
        for (name in listOf("getChar", "getBold", "getReverse", "getUnderline", "getWidth")) {
            assertNotNull(
                cls.methods.firstOrNull { it.name == name && it.parameterTypes.isEmpty() },
                "Cell missing accessor $name()",
            )
        }
        // fgColor is an inline class accessor — name is mangled (getFgColor-XXXX)
        val fg = cls.methods.firstOrNull { it.name.startsWith("getFgColor") && it.parameterTypes.isEmpty() }
        assertNotNull(fg, "Cell missing getFgColor* accessor")
        assertTrue(fg.returnType == Long::class.javaPrimitiveType || fg.returnType == java.lang.Long.TYPE,
            "getFgColor* should return long, got ${fg.returnType}")
    }

    // ── Graceful failure paths ──

    @Test
    fun testGetSnapshotFlowWrongTypeReturnsNull() {
        assertNull(TermlibAccess.getSnapshotFlow(Any()))
    }

    @Test
    fun testGetSnapshotFlowNullSafeOnString() {
        assertNull(TermlibAccess.getSnapshotFlow("not an emulator"))
    }

    @Test
    fun testGetSnapshotValueWrongTypeReturnsNull() {
        assertNull(TermlibAccess.getSnapshotValue(Any()))
    }

    @Test
    fun testToScrapedSnapshotWrongTypeReturnsNull() {
        assertNull(TermlibAccess.toScrapedSnapshot(Any()))
    }
}
