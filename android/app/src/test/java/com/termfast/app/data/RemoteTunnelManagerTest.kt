package com.termfast.app.data

import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertTrue

/**
 * Mock FFI backend for testing RemoteTunnelManager state transitions.
 * Records all calls and returns configurable results.
 */
class MockRemoteTunnelFfi : RemoteTunnelFfi {
    var initCalls = 0
    var onBinaryCalls = 0
    var listRequestCalls = 0
    var subscribeCalls = mutableListOf<Int>()
    var unsubscribeCalls = mutableListOf<Int>()
    var inputCalls = mutableListOf<Pair<Int, ByteArray>>()
    var resizeCalls = mutableListOf<Triple<Int, Int, Int>>()
    var closeCalls = 0

    /** Configurable return values (null = simulate FFI error). */
    var initResult: ByteArray = byteArrayOf(0x01, 0x02, 0x03)
    var listRequestResult: ByteArray? = byteArrayOf(0x10)
    var subscribeResult: ByteArray? = byteArrayOf(0x20)
    var unsubscribeResult: ByteArray? = byteArrayOf(0x30)
    var inputResult: ByteArray? = byteArrayOf(0x40)
    var resizeResult: ByteArray? = byteArrayOf(0x50)
    var closeResult: ByteArray? = byteArrayOf(0x60)

    /** If set, init() throws this exception (simulates FFI failure). */
    var initException: Exception? = null

    override fun init(pairingId: String, pairingKey: ByteArray): ByteArray {
        initCalls++
        initException?.let { throw it }
        return initResult
    }

    override fun onBinary(pairingId: String, data: ByteArray) {
        onBinaryCalls++
    }

    override fun sendListRequest(pairingId: String): ByteArray? {
        listRequestCalls++
        return listRequestResult
    }

    override fun subscribe(pairingId: String, terminalId: Int): ByteArray? {
        subscribeCalls.add(terminalId)
        return subscribeResult
    }

    override fun unsubscribe(pairingId: String, terminalId: Int): ByteArray? {
        unsubscribeCalls.add(terminalId)
        return unsubscribeResult
    }

    override fun sendInput(pairingId: String, terminalId: Int, data: ByteArray): ByteArray? {
        inputCalls.add(terminalId to data)
        return inputResult
    }

    override fun sendResize(pairingId: String, terminalId: Int, cols: Int, rows: Int): ByteArray? {
        resizeCalls.add(Triple(terminalId, cols, rows))
        return resizeResult
    }

    override fun close(pairingId: String): ByteArray? {
        closeCalls++
        return closeResult
    }
}

@OptIn(kotlinx.coroutines.ExperimentalCoroutinesApi::class)
class RemoteTunnelManagerTest {

    private val testScope = TestScope(UnconfinedTestDispatcher())
    private val mockFfi = MockRemoteTunnelFfi()

    private fun createManager(): RemoteTunnelManager {
        return RemoteTunnelManager(
            pairingId = "test-pairing-id",
            pairingKey = ByteArray(32) { it.toByte() },
            relayUrl = "wss://example.com",
            pairingJwt = "test-jwt",
            scope = testScope,
            ffi = mockFfi,
        )
    }

    @Test
    fun testInitialState() {
        val manager = createManager()
        assertIs<TunnelState.Disconnected>(manager.transportState.value)
        assertFalse(manager.protocolReady.value)
    }

    @Test
    fun testOnPeerConnectedTransitionsToConnectedAndCallsInit() {
        val manager = createManager()
        manager.testCallbacks.onPeerConnected()
        assertIs<TunnelState.Connected>(manager.transportState.value)
        assertEquals(1, mockFfi.initCalls)
        // protocolReady should still be false (HELLO exchange not complete yet)
        assertFalse(manager.protocolReady.value)
    }

    @Test
    fun testOnPeerConnectedInitFailureTransitionsToError() {
        val manager = createManager()
        mockFfi.initException = RuntimeException("JNI error")
        manager.testCallbacks.onPeerConnected()
        assertIs<TunnelState.Error>(manager.transportState.value)
        assertTrue((manager.transportState.value as TunnelState.Error).message.contains("HELLO init failed"))
        assertFalse(manager.protocolReady.value)
    }

    @Test
    fun testOnPeerDisconnectedResetsState() {
        val manager = createManager()
        // First connect
        manager.testCallbacks.onPeerConnected()
        manager.onProtocolReady()
        assertTrue(manager.protocolReady.value)
        // Then disconnect
        manager.testCallbacks.onPeerDisconnected()
        assertIs<TunnelState.Disconnected>(manager.transportState.value)
        assertFalse(manager.protocolReady.value)
    }

    @Test
    fun testOnPeerTimeoutResetsState() {
        val manager = createManager()
        manager.testCallbacks.onPeerConnected()
        manager.onProtocolReady()
        // Peer timeout
        manager.testCallbacks.onPeerTimeout()
        assertIs<TunnelState.PeerTimeout>(manager.transportState.value)
        assertFalse(manager.protocolReady.value)
    }

    @Test
    fun testOnErrorTransitionsToErrorState() {
        val manager = createManager()
        manager.testCallbacks.onPeerConnected()
        manager.onProtocolReady()
        // Error from transport
        manager.testCallbacks.onError("connection lost")
        assertIs<TunnelState.Error>(manager.transportState.value)
        assertEquals("connection lost", (manager.transportState.value as TunnelState.Error).message)
        assertFalse(manager.protocolReady.value)
    }

    @Test
    fun testOnProtocolReadySetsProtocolReady() {
        val manager = createManager()
        manager.testCallbacks.onPeerConnected()
        assertFalse(manager.protocolReady.value)
        manager.onProtocolReady()
        assertTrue(manager.protocolReady.value)
    }

    @Test
    fun testSendListRequestBeforeProtocolReadyFails() {
        val manager = createManager()
        // Don't call onProtocolReady
        val result = manager.sendListRequest()
        assertFalse(result)
        assertEquals(0, mockFfi.listRequestCalls)
    }

    @Test
    fun testSendListRequestAfterProtocolReadyCallsFfi() {
        val manager = createManager()
        manager.onProtocolReady()
        // sendListRequest calls ffi but sendRaw returns false (no WebSocket connection)
        // The FFI call should still happen
        manager.sendListRequest()
        assertEquals(1, mockFfi.listRequestCalls)
    }

    @Test
    fun testSendSubscribeBeforeProtocolReadyFails() {
        val manager = createManager()
        val result = manager.sendSubscribe(42)
        assertFalse(result)
        assertEquals(0, mockFfi.subscribeCalls.size)
    }

    @Test
    fun testSendSubscribeAfterProtocolReadyCallsFfiWithTerminalId() {
        val manager = createManager()
        manager.onProtocolReady()
        manager.sendSubscribe(42)
        assertEquals(1, mockFfi.subscribeCalls.size)
        assertEquals(42, mockFfi.subscribeCalls[0])
    }

    @Test
    fun testSendInputBeforeProtocolReadyFails() {
        val manager = createManager()
        val result = manager.sendInput(5, byteArrayOf(0x41))
        assertFalse(result)
        assertEquals(0, mockFfi.inputCalls.size)
    }

    @Test
    fun testSendInputAfterProtocolReadyCallsFfiWithData() {
        val manager = createManager()
        manager.onProtocolReady()
        val inputData = byteArrayOf(0x41, 0x42, 0x43)
        manager.sendInput(5, inputData)
        assertEquals(1, mockFfi.inputCalls.size)
        assertEquals(5, mockFfi.inputCalls[0].first)
        assertEquals(inputData.toList(), mockFfi.inputCalls[0].second.toList())
    }

    @Test
    fun testSendResizeBeforeProtocolReadyFails() {
        val manager = createManager()
        val result = manager.sendResize(3, 120, 40)
        assertFalse(result)
        assertEquals(0, mockFfi.resizeCalls.size)
    }

    @Test
    fun testSendResizeAfterProtocolReadyCallsFfiWithDimensions() {
        val manager = createManager()
        manager.onProtocolReady()
        manager.sendResize(3, 120, 40)
        assertEquals(1, mockFfi.resizeCalls.size)
        assertEquals(3, mockFfi.resizeCalls[0].first)
        assertEquals(120, mockFfi.resizeCalls[0].second)
        assertEquals(40, mockFfi.resizeCalls[0].third)
    }

    @Test
    fun testStopCallsFfiCloseAndResetsState() {
        val manager = createManager()
        manager.testCallbacks.onPeerConnected()
        manager.onProtocolReady()
        manager.stop()
        assertEquals(1, mockFfi.closeCalls)
        assertIs<TunnelState.Disconnected>(manager.transportState.value)
        assertFalse(manager.protocolReady.value)
    }

    @Test
    fun testOnBinaryFrameCallsFfiOnBinary() {
        val manager = createManager()
        val frameData = byteArrayOf(0x01, 0x02, 0x03, 0x04)
        manager.testCallbacks.onBinaryFrame(frameData)
        assertEquals(1, mockFfi.onBinaryCalls)
    }

    @Test
    fun testOnBinaryFrameSwallowsFfiExceptions() {
        val manager = createManager()
        // Make onBinary throw by using a special mock
        val throwingFfi = object : RemoteTunnelFfi {
            override fun init(pairingId: String, pairingKey: ByteArray) = ByteArray(0)
            override fun onBinary(pairingId: String, data: ByteArray) { throw RuntimeException("decrypt error") }
            override fun sendListRequest(pairingId: String) = null
            override fun subscribe(pairingId: String, terminalId: Int) = null
            override fun unsubscribe(pairingId: String, terminalId: Int) = null
            override fun sendInput(pairingId: String, terminalId: Int, data: ByteArray) = null
            override fun sendResize(pairingId: String, terminalId: Int, cols: Int, rows: Int) = null
            override fun close(pairingId: String) = null
        }
        val mgr = RemoteTunnelManager(
            pairingId = "test",
            pairingKey = ByteArray(32),
            relayUrl = "wss://example.com",
            pairingJwt = "jwt",
            scope = testScope,
            ffi = throwingFfi,
        )
        // Should not throw — exception is swallowed
        mgr.testCallbacks.onBinaryFrame(byteArrayOf(0x01))
        // State should remain unchanged (no crash)
        assertIs<TunnelState.Disconnected>(mgr.transportState.value)
    }

    @Test
    fun testReconnectAfterDisconnectReinitializesTunnel() {
        val manager = createManager()
        // First connection
        manager.testCallbacks.onPeerConnected()
        assertEquals(1, mockFfi.initCalls)
        manager.onProtocolReady()
        assertTrue(manager.protocolReady.value)
        // Disconnect
        manager.testCallbacks.onPeerDisconnected()
        assertFalse(manager.protocolReady.value)
        // Reconnect — should call init again (new HELLO with fresh client_random)
        manager.testCallbacks.onPeerConnected()
        assertEquals(2, mockFfi.initCalls)
        assertFalse(manager.protocolReady.value) // not ready until HELLO response
    }

    @Test
    fun testSendUnsubscribeAfterProtocolReadyCallsFfi() {
        val manager = createManager()
        manager.onProtocolReady()
        manager.sendUnsubscribe(7)
        assertEquals(1, mockFfi.unsubscribeCalls.size)
        assertEquals(7, mockFfi.unsubscribeCalls[0])
    }

    @Test
    fun testFfiReturnsNullPreventsSend() {
        val manager = createManager()
        manager.onProtocolReady()
        mockFfi.listRequestResult = null
        val result = manager.sendListRequest()
        // FFI returned null → sendRaw not called → returns false
        assertFalse(result)
    }
}
