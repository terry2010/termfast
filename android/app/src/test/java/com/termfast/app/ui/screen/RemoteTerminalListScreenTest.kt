package com.termfast.app.ui.screen

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class RemoteTerminalListScreenTest {

    @Test
    fun testParseTerminalListEmptyArray() {
        val entries = parseTerminalList("[]")
        assertTrue(entries.isEmpty())
    }

    @Test
    fun testParseTerminalListSingleEntry() {
        val json = """[{"terminal_id":1,"name":"main","server_id":"srv1","is_local":false,"tmux_session_name":"main"}]"""
        val entries = parseTerminalList(json)
        assertEquals(1, entries.size)
        val t = entries[0]
        assertEquals(1, t.id)
        assertEquals("main", t.name)
        assertEquals("srv1", t.serverId)
        assertEquals(false, t.isLocal)
        assertEquals("main", t.tmuxSessionName)
    }

    @Test
    fun testParseTerminalListMultipleEntries() {
        val json = """[
            {"terminal_id":1,"name":"main","server_id":"srv1","is_local":false,"tmux_session_name":"main"},
            {"terminal_id":2,"name":"debug","server_id":"srv1","is_local":false,"tmux_session_name":"debug"},
            {"terminal_id":3,"name":"local","server_id":"","is_local":true}
        ]"""
        val entries = parseTerminalList(json)
        assertEquals(3, entries.size)
        assertEquals(1, entries[0].id)
        assertEquals("main", entries[0].name)
        assertEquals(2, entries[1].id)
        assertEquals("debug", entries[1].name)
        assertEquals(3, entries[2].id)
        assertEquals("local", entries[2].name)
        assertEquals(true, entries[2].isLocal)
        assertEquals(null, entries[2].tmuxSessionName) // empty string → null
    }

    @Test
    fun testParseTerminalListMissingFields() {
        // Missing fields should use defaults
        val json = """[{"terminal_id":5}]"""
        val entries = parseTerminalList(json)
        assertEquals(1, entries.size)
        assertEquals(5, entries[0].id)
        assertEquals("Terminal", entries[0].name) // default
        assertEquals("", entries[0].serverId) // default
        assertEquals(false, entries[0].isLocal) // default
        assertEquals(null, entries[0].tmuxSessionName) // default
    }

    @Test
    fun testParseTerminalListInvalidJson() {
        val entries = parseTerminalList("not json")
        assertTrue(entries.isEmpty())
    }

    @Test
    fun testParseTerminalListEmptyString() {
        val entries = parseTerminalList("")
        assertTrue(entries.isEmpty())
    }

    @Test
    fun testParseTerminalListEmptyTmuxSessionName() {
        val json = """[{"terminal_id":1,"name":"test","tmux_session_name":""}]"""
        val entries = parseTerminalList(json)
        assertEquals(1, entries.size)
        assertEquals(null, entries[0].tmuxSessionName) // empty → null
    }

    @Test
    fun testParseTerminalListFallsBackToIdField() {
        // Some servers might use "id" instead of "terminal_id"
        val json = """[{"id":42,"name":"legacy"}]"""
        val entries = parseTerminalList(json)
        assertEquals(1, entries.size)
        assertEquals(42, entries[0].id)
        assertEquals("legacy", entries[0].name)
    }

    @Test
    fun testTerminalEntryDataClass() {
        val entry = TerminalEntry(
            id = 7,
            name = "test",
            serverId = "srv2",
            isLocal = false,
            tmuxSessionName = "session1",
        )
        assertEquals(7, entry.id)
        assertEquals("test", entry.name)
        assertEquals("srv2", entry.serverId)
        assertEquals(false, entry.isLocal)
        assertEquals("session1", entry.tmuxSessionName)
    }

    // === isTmuxUnavailableError tests ===

    @Test
    fun testIsTmuxUnavailableErrorMatchesTmux() {
        assertTrue(isTmuxUnavailableError("tmux session main not found"))
        assertTrue(isTmuxUnavailableError("tmux not installed"))
        assertTrue(isTmuxUnavailableError("TMUX session missing")) // case insensitive
    }

    @Test
    fun testIsTmuxUnavailableErrorMatchesTmuxUnavailable() {
        assertTrue(isTmuxUnavailableError("tmux_unavailable"))
    }

    @Test
    fun testIsTmuxUnavailableErrorMatchesMultiTerminal() {
        assertTrue(isTmuxUnavailableError("multi_terminal not supported"))
    }

    @Test
    fun testIsTmuxUnavailableErrorMatchesDesktopOffline() {
        assertTrue(isTmuxUnavailableError("desktop_offline"))
    }

    @Test
    fun testIsTmuxUnavailableErrorMatchesTerminalNotFound() {
        assertTrue(isTmuxUnavailableError("terminal_not_found"))
    }

    @Test
    fun testIsTmuxUnavailableErrorMatchesHelloRequired() {
        assertTrue(isTmuxUnavailableError("hello_required"))
        assertTrue(isTmuxUnavailableError("hello_already_done"))
    }

    @Test
    fun testIsTmuxUnavailableErrorRejectsGenericErrors() {
        assertFalse(isTmuxUnavailableError("invalid_terminal_id"))
        assertFalse(isTmuxUnavailableError("input_failed"))
        assertFalse(isTmuxUnavailableError("unknown_frame_type"))
    }

    @Test
    fun testIsTmuxUnavailableErrorRejectsEmptyString() {
        assertFalse(isTmuxUnavailableError(""))
    }

    @Test
    fun testIsTmuxUnavailableErrorRejectsNullLike() {
        assertFalse(isTmuxUnavailableError("null"))
        assertFalse(isTmuxUnavailableError("none"))
    }
}
