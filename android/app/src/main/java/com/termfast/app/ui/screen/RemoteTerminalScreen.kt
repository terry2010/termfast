package com.termfast.app.ui.screen

import android.graphics.Typeface
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.navigation.NavController
import com.termfast.app.data.PairingStore
import com.termfast.app.data.RustEvent
import com.termfast.app.data.RustRepository
import com.termfast.app.data.RemoteTunnelManager
import com.termfast.app.data.TunnelState
import com.termfast.app.ui.TerminalThemes
import kotlinx.coroutines.launch
import org.connectbot.terminal.Terminal

/**
 * Remote terminal screen — renders a remote terminal session via relay tunnel.
 *
 * Lifecycle:
 * 1. Load tunnel credentials from PairingStore
 * 2. Start RemoteTunnelManager (WebSocket + HELLO + frame crypto)
 * 3. On protocolReady → send SUBSCRIBE for the requested terminalId
 * 4. Create remote session via TerminalSessionManager.getOrCreateRemoteSession
 * 5. Render emulator with Terminal composable
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

    // Dark status bar
    val view = LocalView.current
    DisposableEffect(view) {
        view.post {
            val window = (view.context as android.app.Activity).window
            window.statusBarColor = android.graphics.Color.parseColor("#1E1E2E")
            val controller = androidx.core.view.WindowCompat.getInsetsController(window, view)
            controller?.isAppearanceLightStatusBars = false
        }
        onDispose {
            view.post {
                val window = (view.context as android.app.Activity).window
                window.statusBarColor = android.graphics.Color.TRANSPARENT
                val controller = androidx.core.view.WindowCompat.getInsetsController(window, view)
                controller?.isAppearanceLightStatusBars = true
            }
        }
    }

    // Load tunnel credentials
    val pairingKeyHex = PairingStore.getPairingKey()
    val relayUrl = PairingStore.getRelayUrl()
    val pairingJwt = PairingStore.getPairingJwt()

    if (pairingKeyHex == null || relayUrl == null || pairingJwt == null) {
        // No config — shouldn't happen (list screen checks), but handle gracefully
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
                Text("未配对桌面端，无法打开远程终端")
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

    var connected by remember { mutableStateOf(false) }
    var connecting by remember { mutableStateOf(true) }
    var errorMsg by remember { mutableStateOf<String?>(null) }
    var protocolReady by remember { mutableStateOf(false) }
    var sessionId by remember { mutableStateOf<String?>(null) }

    // Collect tunnel state
    LaunchedEffect(tunnelManager) {
        tunnelManager.transportState.collect { state ->
            if (state is TunnelState.Error) {
                errorMsg = state.message
                connecting = false
            }
        }
    }
    LaunchedEffect(tunnelManager) {
        tunnelManager.protocolReady.collect { ready ->
            protocolReady = ready
            if (ready) {
                // Session key established → send SUBSCRIBE
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
                        // First output → create session + mark connected
                        if (sessionId == null) {
                            sessionId = TerminalSessionManager.getOrCreateRemoteSession(
                                pairingId, terminalId, tunnelManager, terminalName
                            )
                            connected = true
                            connecting = false
                        }
                    }
                }
                is RustEvent.RemoteTerminalHistory -> {
                    if (event.pairing_id == pairingId && event.terminal_id.toInt() == terminalId) {
                        // HISTORY arrives before OUTPUT — create session here too
                        if (sessionId == null) {
                            sessionId = TerminalSessionManager.getOrCreateRemoteSession(
                                pairingId, terminalId, tunnelManager, terminalName
                            )
                            connected = true
                            connecting = false
                        }
                    }
                }
                is RustEvent.RemoteTerminalError -> {
                    if (event.pairing_id == pairingId) {
                        errorMsg = event.error
                        connecting = false
                        connected = false
                    }
                }
                else -> {}
            }
        }
    }

    // Start tunnel on screen entry
    LaunchedEffect(tunnelManager) {
        tunnelManager.start()
    }

    // Send UNSUBSCRIBE on screen exit (but don't stop shared tunnel)
    DisposableEffect(tunnelManager) {
        onDispose {
            tunnelManager.sendUnsubscribe(terminalId)
        }
    }

    // Terminal theme
    val config = remember { RustRepository.getConfig() }
    val themeId = config?.general?.terminal_theme ?: "catppuccin-mocha"
    val theme = TerminalThemes.byId(themeId)
    val terminalBg = Color(theme.background)
    val terminalFg = Color(theme.foreground)
    var baseFontSize by remember { mutableStateOf(config?.general?.terminal_font_size ?: 10) }

    // Get emulator once session is created
    val emulator = remember(sessionId) {
        sessionId?.let { TerminalSessionManager.getEmulatorBySession(it) }
    }

    // After connection is established or font size changes, send the
    // emulator's actual dimensions to the desktop so TUI apps render correctly.
    // Per design #10: mobile sends @termfast_size dimensions to desktop.
    LaunchedEffect(connected, baseFontSize, emulator) {
        if (connected && emulator != null) {
            kotlinx.coroutines.delay(500)
            val dims = emulator.dimensions
            tunnelManager.sendResize(terminalId, dims.columns, dims.rows)
        }
    }

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
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .background(terminalBg)
        ) {
            if (connecting) {
                Row(
                    modifier = Modifier.align(Alignment.Center),
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
            } else if (errorMsg != null) {
                Column(
                    modifier = Modifier.align(Alignment.Center).padding(16.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Text(
                        "Error: $errorMsg",
                        color = MaterialTheme.colorScheme.error,
                    )
                    Spacer(Modifier.height(16.dp))
                    Button(onClick = {
                        errorMsg = null
                        connecting = true
                        connected = false
                        sessionId = null
                        scope.launch { tunnelManager.start() }
                    }) {
                        Text("Retry")
                    }
                }
            } else if (emulator != null) {
                Terminal(
                    terminalEmulator = emulator,
                    modifier = Modifier.fillMaxSize(),
                    typeface = Typeface.MONOSPACE,
                    initialFontSize = baseFontSize.sp,
                    minFontSize = 4.sp,
                    maxFontSize = 32.sp,
                    backgroundColor = terminalBg,
                    foregroundColor = terminalFg,
                    keyboardEnabled = false,
                )
            }
        }
    }
}
