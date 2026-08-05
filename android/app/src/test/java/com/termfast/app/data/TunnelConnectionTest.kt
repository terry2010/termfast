package com.termfast.app.data

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import okhttp3.OkHttpClient
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class TunnelConnectionTest {
    private lateinit var server: MockWebServer
    private lateinit var client: OkHttpClient
    private lateinit var scope: CoroutineScope

    @BeforeTest
    fun setup() {
        server = MockWebServer()
        server.start()
        client = OkHttpClient.Builder()
            .pingInterval(30, java.util.concurrent.TimeUnit.SECONDS)
            .connectTimeout(5, java.util.concurrent.TimeUnit.SECONDS)
            .readTimeout(0, java.util.concurrent.TimeUnit.SECONDS)
            .build()
        scope = CoroutineScope(Dispatchers.IO + SupervisorJob())
    }

    @AfterTest
    fun teardown() {
        // Cancel all coroutines (stops reconnect loops)
        scope.cancel()
        // Give time for WebSocket connections to fully close
        Thread.sleep(1000)
        try {
            server.shutdown()
        } catch (e: java.io.IOException) {
            // MockWebServer may throw if WebSocket connections are still closing
            // This is safe to ignore in tests
        }
    }

    private fun makeConfig(jwt: String = "test-jwt", pairingId: String = "pair-123"): TunnelConfig {
        return TunnelConfig(
            relayUrl = "ws://${server.hostName}:${server.port}",
            pairingJwt = jwt,
            pairingId = pairingId,
        )
    }

    private class TestCallbacks : TunnelCallbacks {
        @Volatile var peerConnected = false
        @Volatile var peerDisconnected = false
        @Volatile var peerTimeout = false
        val errors = mutableListOf<String>()
        val binaryFrames = mutableListOf<ByteArray>()

        override fun onPeerConnected() { peerConnected = true }
        override fun onPeerDisconnected() { peerDisconnected = true }
        override fun onPeerTimeout() { peerTimeout = true }
        override fun onBinaryFrame(data: ByteArray) { synchronized(binaryFrames) { binaryFrames.add(data) } }
        override fun onError(message: String) { synchronized(errors) { errors.add(message) } }
    }

    /** Wait up to 3 seconds for condition to become true. */
    private fun waitForCondition(timeoutMs: Long = 3000, condition: () -> Boolean): Boolean {
        val deadline = System.currentTimeMillis() + timeoutMs
        while (System.currentTimeMillis() < deadline) {
            if (condition()) return true
            Thread.sleep(50)
        }
        return false
    }

    // === SECTION 1 END ===

    @Test
    fun testJwtHeaderSent() {
        server.enqueue(MockResponse()
            .withWebSocketUpgrade(object : okhttp3.WebSocketListener() {}))

        val callbacks = TestCallbacks()
        val conn = TunnelConnection(makeConfig(), callbacks, client, scope)
        conn.connect()

        // Wait for the HTTP upgrade request to arrive
        waitForCondition { server.requestCount >= 1 }

        val recordedRequest = server.takeRequest()
        assertEquals("Bearer test-jwt", recordedRequest.getHeader("Authorization"))
        assertEquals("/tunnel", recordedRequest.path)

        conn.close()
        Thread.sleep(500)
    }

    @Test
    fun testHttp401StopsReconnecting() {
        // Return 401 for all requests
        server.enqueue(MockResponse().setResponseCode(401).setBody("unauthorized"))

        val callbacks = TestCallbacks()
        val conn = TunnelConnection(makeConfig(), callbacks, client, scope)
        conn.connect()

        // Wait for 401 error callback
        assertTrue(
            waitForCondition { callbacks.errors.any { it.contains("401") } },
            "expected 401 error, got: ${callbacks.errors}"
        )

        // State should be Error
        assertIs<TunnelState.Error>(conn.state)

        // Wait 2 seconds and verify no reconnection (no additional requests)
        val requestCount = server.requestCount
        Thread.sleep(2000)
        assertEquals(requestCount, server.requestCount, "should not reconnect after 401")
    }

    @Test
    fun testPeerTimeoutStopsReconnecting() {
        server.enqueue(MockResponse()
            .withWebSocketUpgrade(object : okhttp3.WebSocketListener() {
                override fun onOpen(webSocket: okhttp3.WebSocket, response: okhttp3.Response) {
                    webSocket.send("""{"type":"peer_timeout"}""")
                }
            }))

        val callbacks = TestCallbacks()
        val conn = TunnelConnection(makeConfig(), callbacks, client, scope)
        conn.connect()

        assertTrue(
            waitForCondition { callbacks.peerTimeout },
            "should receive peer_timeout callback"
        )
        assertIs<TunnelState.PeerTimeout>(conn.state)

        // Should not reconnect after peer_timeout
        val requestCount = server.requestCount
        Thread.sleep(2000)
        assertEquals(requestCount, server.requestCount, "should not reconnect after peer_timeout")
    }

    @Test
    fun testCloseStopsReconnecting() {
        server.enqueue(MockResponse()
            .withWebSocketUpgrade(object : okhttp3.WebSocketListener() {}))

        val callbacks = TestCallbacks()
        val conn = TunnelConnection(makeConfig(), callbacks, client, scope)
        conn.connect()

        waitForCondition { server.requestCount >= 1 }

        conn.close()

        assertTrue(
            waitForCondition { conn.state is TunnelState.Disconnected },
            "state should be Disconnected after close"
        )

        // Should not reconnect after close()
        val requestCount = server.requestCount
        Thread.sleep(1000)
        assertEquals(requestCount, server.requestCount, "should not reconnect after close()")

        // Give WebSocket time to fully close before server shutdown
        Thread.sleep(500)
    }

    @Test
    fun testPeerConnectedTransitionsToConnected() {
        server.enqueue(MockResponse()
            .withWebSocketUpgrade(object : okhttp3.WebSocketListener() {
                override fun onOpen(webSocket: okhttp3.WebSocket, response: okhttp3.Response) {
                    webSocket.send("""{"type":"peer_connected"}""")
                }
            }))

        val callbacks = TestCallbacks()
        val conn = TunnelConnection(makeConfig(), callbacks, client, scope)
        conn.connect()

        // Verify peer_connected callback fires
        assertTrue(
            waitForCondition { callbacks.peerConnected },
            "should receive peer_connected callback"
        )
        // Verify state transitions to Connected
        assertIs<TunnelState.Connected>(conn.state)

        // sendBinary should now succeed (state guard allows Connected)
        assertTrue(conn.sendBinary(byteArrayOf(1, 2, 3)), "sendBinary should succeed when Connected")

        conn.close()
        Thread.sleep(500)
    }

    @Test
    fun testSendBinaryRejectedBeforeConnected() {
        server.enqueue(MockResponse()
            .withWebSocketUpgrade(object : okhttp3.WebSocketListener() {}))

        val callbacks = TestCallbacks()
        val conn = TunnelConnection(makeConfig(), callbacks, client, scope)
        conn.connect()

        // Wait for WS to connect (state = WaitingForPeer, NOT Connected)
        waitForCondition { server.requestCount >= 1 }
        Thread.sleep(300) // give time for onOpen → WaitingForPeer transition

        // sendBinary should fail — state is WaitingForPeer, not Connected
        assertIs<TunnelState.WaitingForPeer>(conn.state)
        assertTrue(!conn.sendBinary(byteArrayOf(1, 2, 3)), "sendBinary should fail before Connected")

        conn.close()
        Thread.sleep(500)
    }

    @Test
    fun testPeerDisconnectedTriggersReconnect() {
        // First WS: send peer_disconnected on open
        server.enqueue(MockResponse()
            .withWebSocketUpgrade(object : okhttp3.WebSocketListener() {
                override fun onOpen(webSocket: okhttp3.WebSocket, response: okhttp3.Response) {
                    webSocket.send("""{"type":"peer_disconnected"}""")
                }
            }))
        // Second WS: for the reconnect (accept and stay open)
        server.enqueue(MockResponse()
            .withWebSocketUpgrade(object : okhttp3.WebSocketListener() {}))

        val callbacks = TestCallbacks()
        val conn = TunnelConnection(makeConfig(), callbacks, client, scope)
        conn.connect()

        // Verify peer_disconnected callback fires
        assertTrue(
            waitForCondition { callbacks.peerDisconnected },
            "should receive peer_disconnected callback"
        )

        // Verify client proactively closed WS and reconnected (second HTTP request)
        // backoff is 1s, so reconnect happens after ~1s
        assertTrue(
            waitForCondition(timeoutMs = 5000) { server.requestCount >= 2 },
            "should reconnect after peer_disconnected (got ${server.requestCount} requests)"
        )

        conn.close()
        Thread.sleep(500)
    }

    @Test
    fun testExponentialBackoffReconnectSequence() {
        // Three WS upgrades, each sends peer_disconnected on open to trigger reconnect.
        // peer_disconnected causes the client to close WS + scheduleReconnect with backoff.
        repeat(3) {
            server.enqueue(MockResponse()
                .withWebSocketUpgrade(object : okhttp3.WebSocketListener() {
                    override fun onOpen(webSocket: okhttp3.WebSocket, response: okhttp3.Response) {
                        webSocket.send("""{"type":"peer_disconnected"}""")
                    }
                }))
        }

        val callbacks = TestCallbacks()
        val conn = TunnelConnection(makeConfig(), callbacks, client, scope)
        conn.connect()

        // Record wall-clock time when each request count is observed
        // Backoff: 1s (1st reconnect) + 2s (2nd reconnect) = ~3s total
        assertTrue(waitForCondition { server.requestCount >= 1 }, "first request should arrive")
        val t1 = System.currentTimeMillis()

        assertTrue(
            waitForCondition(timeoutMs = 5000) { server.requestCount >= 2 },
            "second request (1st reconnect) should arrive within backoff 1s"
        )
        val t2 = System.currentTimeMillis()

        assertTrue(
            waitForCondition(timeoutMs = 6000) { server.requestCount >= 3 },
            "third request (2nd reconnect) should arrive within backoff 2s"
        )
        val t3 = System.currentTimeMillis()

        val gap1 = t2 - t1
        val gap2 = t3 - t2

        // First backoff ~1s, second backoff ~2s (doubled)
        // Verify backoff is increasing (gap2 should be notably larger than gap1)
        assertTrue(
            gap2 > gap1,
            "backoff should increase: gap1=${gap1}ms, gap2=${gap2}ms"
        )
        // First gap should be around 1s (allow tolerance for polling overhead)
        assertTrue(
            gap1 >= 800,
            "first backoff should be ~1s, got ${gap1}ms"
        )

        conn.close()
        Thread.sleep(500)
    }

    // === SECTION 2 END ===

    @Test
    fun testTunnelManagerGetOrCreateIsolation() {
        val manager = TunnelManager(scope)
        val callbacks1 = TestCallbacks()
        val callbacks2 = TestCallbacks()

        val config1 = makeConfig(pairingId = "pair-A")
        val config2 = makeConfig(pairingId = "pair-B")

        val conn1 = manager.getOrCreate(config1, callbacks1)
        val conn2 = manager.getOrCreate(config2, callbacks2)

        // Different pairing_ids → different connections
        assertTrue(conn1 !== conn2, "different pairing_ids should have different connections")

        // Same pairing_id → same connection
        val conn1Again = manager.getOrCreate(config1, callbacks1)
        assertTrue(conn1 === conn1Again, "same pairing_id should return same connection")

        // getConnection
        assertTrue(manager.getConnection("pair-A") === conn1)
        assertTrue(manager.getConnection("pair-B") === conn2)
        assertEquals(null, manager.getConnection("nonexistent"))

        manager.closeAll()
    }

    @Test
    fun testTunnelManagerCloseSingleDoesNotAffectOthers() {
        val manager = TunnelManager(scope)
        val callbacks1 = TestCallbacks()
        val callbacks2 = TestCallbacks()

        val conn1 = manager.getOrCreate(makeConfig(pairingId = "pair-A"), callbacks1)
        val conn2 = manager.getOrCreate(makeConfig(pairingId = "pair-B"), callbacks2)

        // Close pair-A only
        manager.close("pair-A")

        // pair-A should be removed
        assertEquals(null, manager.getConnection("pair-A"))

        // pair-B should still exist
        assertTrue(manager.getConnection("pair-B") === conn2, "pair-B should still exist after closing pair-A")

        manager.closeAll()
    }

    @Test
    fun testTunnelManagerCloseAll() {
        val manager = TunnelManager(scope)
        manager.getOrCreate(makeConfig(pairingId = "pair-A"), TestCallbacks())
        manager.getOrCreate(makeConfig(pairingId = "pair-B"), TestCallbacks())
        manager.getOrCreate(makeConfig(pairingId = "pair-C"), TestCallbacks())

        manager.closeAll()

        assertEquals(null, manager.getConnection("pair-A"))
        assertEquals(null, manager.getConnection("pair-B"))
        assertEquals(null, manager.getConnection("pair-C"))
    }
}
