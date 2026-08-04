package com.termfast.app.data

import okhttp3.*
import okio.ByteString
import java.util.concurrent.ConcurrentHashMap

/**
 * WebSocket tunnel client — connects to relay server and relays encrypted frames.
 *
 * Frame protocol is handled by Rust FFI (encryption/decryption), this class only
 * manages the WebSocket connection lifecycle and raw byte transport.
 */
class TunnelClient(
    private val relayUrl: String,
) {
    private val client: OkHttpClient = OkHttpClient.Builder()
        .pingInterval(30, java.util.concurrent.TimeUnit.SECONDS)
        .build()
    private var webSocket: WebSocket? = null
    private val listeners = ConcurrentHashMap<String, (ByteArray) -> Unit>()

    fun connect(pairingJwt: String, onOpen: () -> Unit, onClose: () -> Unit) {
        val url = "$relayUrl/tunnel?token=$pairingJwt"
        val request = Request.Builder().url(url).build()
        webSocket = client.newWebSocket(request, object : WebSocketListener() {
            override fun onOpen(webSocket: WebSocket, response: Response) {
                onOpen()
            }
            override fun onMessage(webSocket: WebSocket, bytes: ByteString) {
                val data = bytes.toByteArray()
                listeners.values.forEach { it(data) }
            }
            override fun onMessage(webSocket: WebSocket, text: String) {
                // Text messages not used in frame protocol
            }
            override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
                onClose()
            }
            override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                onClose()
            }
        })
    }

    fun send(data: ByteArray): Boolean {
        return webSocket?.send(ByteString.of(*data)) ?: false
    }

    fun addListener(id: String, listener: (ByteArray) -> Unit) {
        listeners[id] = listener
    }

    fun removeListener(id: String) {
        listeners.remove(id)
    }

    fun close() {
        webSocket?.close(1000, "client closing")
        webSocket = null
    }
}
