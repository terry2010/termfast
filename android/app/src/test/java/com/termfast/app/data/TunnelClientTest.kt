package com.termfast.app.data

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertTrue

class TunnelClientTest {
    @Test
    fun testParseControlMessagePeerConnected() {
        val msg = parseControlMessage("""{"type":"peer_connected"}""")
        assertIs<ControlMessage.PeerConnected>(msg)
    }

    @Test
    fun testParseControlMessagePeerDisconnected() {
        val msg = parseControlMessage("""{"type":"peer_disconnected"}""")
        assertIs<ControlMessage.PeerDisconnected>(msg)
    }

    @Test
    fun testParseControlMessagePeerTimeout() {
        val msg = parseControlMessage("""{"type":"peer_timeout"}""")
        assertIs<ControlMessage.PeerTimeout>(msg)
    }

    @Test
    fun testParseControlMessageError() {
        val msg = parseControlMessage("""{"type":"error","message":"desktop already connected"}""")
        assertIs<ControlMessage.Error>(msg)
        assertEquals("desktop already connected", msg.message)
    }

    @Test
    fun testParseControlMessageUnknown() {
        val msg = parseControlMessage("""{"type":"server_restarting","reconnect_in":3}""")
        assertIs<ControlMessage.Unknown>(msg)
    }

    @Test
    fun testParseControlMessageInvalidJson() {
        val msg = parseControlMessage("not json at all")
        assertIs<ControlMessage.Unknown>(msg)
    }

    @Test
    fun testParseControlMessageEmptyString() {
        val msg = parseControlMessage("")
        assertIs<ControlMessage.Unknown>(msg)
    }

    @Test
    fun testTunnelConfigCreation() {
        val config = TunnelConfig(
            relayUrl = "wss://example.com",
            pairingJwt = "jwt-token",
            pairingId = "pairing-123",
        )
        assertEquals("wss://example.com", config.relayUrl)
        assertEquals("jwt-token", config.pairingJwt)
        assertEquals("pairing-123", config.pairingId)
    }

    @Test
    fun testTunnelStateSealedClass() {
        val disconnected: TunnelState = TunnelState.Disconnected
        val connecting: TunnelState = TunnelState.Connecting
        val waiting: TunnelState = TunnelState.WaitingForPeer
        val connected: TunnelState = TunnelState.Connected
        val error: TunnelState = TunnelState.Error("test error")
        val timeout: TunnelState = TunnelState.PeerTimeout

        assertTrue(disconnected is TunnelState.Disconnected)
        assertTrue(connecting is TunnelState.Connecting)
        assertTrue(waiting is TunnelState.WaitingForPeer)
        assertTrue(connected is TunnelState.Connected)
        assertTrue(error is TunnelState.Error)
        assertEquals("test error", error.message)
        assertTrue(timeout is TunnelState.PeerTimeout)
    }
}
