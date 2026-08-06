package com.termfast.app.ui.screen

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.navigation.NavController
import com.termfast.app.data.PairingStore
import com.termfast.app.data.RustEvent
import com.termfast.app.data.RustRepository
import com.termfast.app.data.RemoteTunnelManager
import com.termfast.app.data.TunnelState
import kotlinx.coroutines.launch

/**
 * Remote terminal screen — thin wrapper that establishes the relay tunnel,
 * creates a remote terminal session, then delegates rendering to TerminalScreen
 * (reusing the same UI as SSH terminals: custom keyboard, pinch-zoom, etc.).
 *
 * Lifecycle:
 * 1. Load tunnel credentials from PairingStore
 * 2. Start RemoteTunnelManager (WebSocket + HELLO + frame crypto)
 * 3. On protocolReady → send SUBSCRIBE for the requested terminalId
 * 4. On first OUTPUT/HISTORY → create remote session via TerminalSessionManager
 * 5. Delegate to TerminalScreen(isRemote=true) for rendering + keyboard
 * 6. On dispose → send UNSUBSCRIBE + stop tunnel
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RemoteTerminalScreen(
    navController: NavController,
    pairingId: String,
    terminalId: Int,
    terminalName: String,
) {
    val scope = rememberCoroutineScope()
    val terminalBg = Color(0xFF1E1E2E)
    val terminalFg = Color(0xFFCDD6F4)

    // Load tunnel credentials for this specific pairing
    val pairing = remember { PairingStore.getPairing(pairingId) }
    val pairingKeyHex = pairing?.pairingKey
    val relayUrl = pairing?.relayUrl
    val pairingJwt = pairing?.pairingJwt

    if (pairingKeyHex == null || relayUrl == null || pairingJwt == null) {
        Scaffold(
            topBar = {
                TopAppBar(
                    title = { Text(terminalName) },
                    navigationIcon = {
                        IconButton(onClick = { navController.popBackStack() }) {
                            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "返回")
                        }
                    },
                )
            }
        ) { padding ->
            Box(
                modifier = Modifier.fillMaxSize().padding(padding),
                contentAlignment = Alignment.Center,
            ) {
                Text("配对不存在，请重新配对")
            }
        }
        return
    }

    val pairingKey = remember(pairingKeyHex) {
        pairingKeyHex.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
    }

    // Create tunnel manager (shared via TerminalSessionManager)
    val tunnelManager = remember(pairingId) {
        TerminalSessionManager.getOrCreateTunnelManager(pairingId, pairingKey, relayUrl, pairingJwt)
    }

    var sessionId by remember { mutableStateOf<String?>(null) }
    var errorMsg by remember { mutableStateOf<String?>(null) }
    var connecting by remember { mutableStateOf(false) }

    // Collect tunnel state
    LaunchedEffect(tunnelManager) {
        tunnelManager.transportState.collect { state ->
            when (state) {
                is TunnelState.Error -> {
                    errorMsg = state.message
                    connecting = false
                }
                is TunnelState.Disconnected -> {
                    // Transport disconnected — if we had a session, show error
                    // so user can retry (otherwise just keep waiting for reconnect)
                    if (sessionId != null) {
                        errorMsg = "连接已断开"
                        connecting = false
                    }
                }
                is TunnelState.Connecting -> {
                    connecting = true
                }
                is TunnelState.WaitingForPeer -> {
                    connecting = true
                }
                is TunnelState.Connected -> {
                    connecting = false
                }
                else -> {}
            }
        }
    }
    LaunchedEffect(tunnelManager) {
        tunnelManager.protocolReady.collect { ready ->
            if (ready) {
                tunnelManager.sendSubscribe(terminalId)
            }
        }
    }

    // Listen for remote terminal events
    LaunchedEffect(tunnelManager) {
        RustRepository.events.collect { event ->
            when (event) {
                is RustEvent.RemoteTunnelReady -> {
                    if (event.pairing_id == pairingId) {
                        tunnelManager.onProtocolReady()
                    }
                }
                is RustEvent.RemoteTerminalOutput -> {
                    if (event.pairing_id == pairingId && event.terminal_id.toInt() == terminalId) {
                        if (sessionId == null) {
                            sessionId = TerminalSessionManager.getOrCreateRemoteSession(
                                pairingId, terminalId, tunnelManager, terminalName
                            )
                        }
                    }
                }
                is RustEvent.RemoteTerminalHistory -> {
                    if (event.pairing_id == pairingId && event.terminal_id.toInt() == terminalId) {
                        if (sessionId == null) {
                            sessionId = TerminalSessionManager.getOrCreateRemoteSession(
                                pairingId, terminalId, tunnelManager, terminalName
                            )
                        }
                    }
                }
                is RustEvent.RemoteTerminalError -> {
                    if (event.pairing_id == pairingId) {
                        errorMsg = event.error
                    }
                }
                else -> {}
            }
        }
    }

    // Start tunnel on screen entry — but only if not already connected.
    // When navigating from RemoteTerminalPickerDialog, the tunnel is already
    // established (handed off). Calling start() again would forceConnect(),
    // creating a new WebSocket that kicks the existing peer and causes the
    // desktop to disconnect/reconnect.
    LaunchedEffect(tunnelManager) {
        val state = tunnelManager.transportState.value
        if (state is TunnelState.Disconnected || state is TunnelState.Error) {
            tunnelManager.start()
        }
        // If already Connected/Connecting/WaitingForPeer, the tunnel from the
        // picker dialog is still active — just reuse it.
    }

    // Timeout: if still connecting after 30 seconds with no session, show error.
    // This handles cases where the tunnel is stuck in WaitingForPeer (desktop
    // offline) or HELLO exchange silently fails after a network reconnection.
    LaunchedEffect(tunnelManager, connecting, sessionId, errorMsg) {
        if (connecting && sessionId == null && errorMsg == null) {
            kotlinx.coroutines.delay(30_000)
            // Still connecting after 30s — show timeout error
            if (connecting && sessionId == null && errorMsg == null) {
                errorMsg = "连接超时，请重试"
                connecting = false
            }
        }
    }

    // On screen exit: send UNSUBSCRIBE and stop the tunnel to prevent
    // background reconnection loops that kick the desktop's peer.
    DisposableEffect(tunnelManager) {
        onDispose {
            tunnelManager.sendUnsubscribe(terminalId)
            tunnelManager.stop()
        }
    }

    // Once session is created, delegate to TerminalScreen for full rendering
    // (custom keyboard, pinch-zoom, landscape/portrait, action sheet, etc.)
    if (sessionId != null && errorMsg == null) {
        TerminalScreen(
            navController = navController,
            serverId = "remote:$pairingId",
            isRemote = true,
            remoteSessionId = sessionId,
            remoteTerminalName = terminalName,
        )
    } else {
        // Connecting or error state — simple UI before TerminalScreen takes over
        Scaffold(
            topBar = {
                TopAppBar(
                    title = { Text(terminalName) },
                    navigationIcon = {
                        IconButton(onClick = { navController.popBackStack() }) {
                            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "返回")
                        }
                    },
                )
            }
        ) { padding ->
            Box(
                modifier = Modifier.fillMaxSize().padding(padding).background(terminalBg),
                contentAlignment = Alignment.Center,
            ) {
                if (errorMsg != null) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Text("Error: $errorMsg", color = MaterialTheme.colorScheme.error)
                        Spacer(Modifier.height(16.dp))
                        Button(onClick = {
                            errorMsg = null
                            sessionId = null
                            connecting = true
                            // Stop first to clean up stale FFI state + close old WebSocket,
                            // then start fresh. Without stop(), the Rust-side tunnel
                            // session retains old encryption keys and HELLO fails silently.
                            scope.launch {
                                tunnelManager.stop()
                                kotlinx.coroutines.delay(200)
                                tunnelManager.start()
                            }
                        }) { Text("Retry") }
                    }
                } else {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        CircularProgressIndicator(
                            modifier = Modifier.size(20.dp),
                            strokeWidth = 2.dp,
                            color = terminalFg,
                        )
                        Text("正在连接远程终端...", color = terminalFg, fontSize = 14.sp)
                    }
                }
            }
        }
    }
}
