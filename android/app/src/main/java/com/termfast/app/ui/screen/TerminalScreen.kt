package com.termfast.app.ui.screen

import android.graphics.Typeface
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.calculateZoom
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material.icons.filled.Tab
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.blur
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.navigation.NavController
import com.termfast.app.data.RustEvent
import com.termfast.app.data.RustRepository
import com.termfast.app.ui.TerminalThemes
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import org.connectbot.terminal.Terminal

@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun TerminalScreen(
    navController: NavController,
    serverId: String,
    existingSessionId: String? = null,
    // Remote terminal mode: when isRemote=true, skip SSH connection logic and
    // use the pre-created remote session (emulator wired to RemoteTunnelManager).
    isRemote: Boolean = false,
    remoteSessionId: String? = null,
    remoteTerminalName: String = "Remote",
) {
    val repo = remember { RustRepository }
    val scope = rememberCoroutineScope()

    // Dark status bar: force light icons (white) on dark terminal background
    val view = LocalView.current
    DisposableEffect(view) {
        view.post {
            val window = (view.context as android.app.Activity).window
            // Set status bar background color (works even in edge-to-edge)
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
    val context = LocalContext.current
    // Use existing session if provided, otherwise get or create.
    // In remote mode, the session was already created by RemoteTerminalScreen
    // and passed in via remoteSessionId.
    val sessionId = remember(existingSessionId, remoteSessionId) {
        if (isRemote && remoteSessionId != null) {
            remoteSessionId
        } else if (existingSessionId != null) {
            TerminalSessionManager.getOrCreateSessionById(serverId, existingSessionId)
        } else {
            TerminalSessionManager.getOrCreateSession(serverId)
        }
    }
    val listState = null  // unused — termlib handles scrolling internally
    // Resolve title: session name or remote terminal name
    var sessionState by remember { mutableStateOf(TerminalSessionManager.getSessions(serverId).firstOrNull { it.sessionId == sessionId }) }
    val title = if (isRemote) {
        sessionState?.name?.ifBlank { null } ?: remoteTerminalName
    } else {
        sessionState?.name?.ifBlank { null } ?: "SSH 终端"
    }

    // Session action sheet
    var showSheet by remember { mutableStateOf(false) }
    var showRenameDialog by remember { mutableStateOf(false) }
    var showDeleteDialog by remember { mutableStateOf(false) }
    var renameText by remember { mutableStateOf(sessionState?.name ?: "") }
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)

    // Agent question sheet — production: auto-show when agent is BLOCKED
    var showAgentSheet by remember(sessionId) { mutableStateOf(false) }
    // Track whether the sheet was shown by user (manual) vs auto-detected
    var agentSheetUserToggled by remember(sessionId) { mutableStateOf(false) }
    // Hoisted sheet state — persists across show/hide cycles
    val agentActiveTab = remember(sessionId) { mutableIntStateOf(0) }
    val agentSelectedOptions = remember(sessionId) { mutableStateMapOf<Int, Int>() }
    val agentCheckedMap = remember(sessionId) { mutableStateMapOf<Int, Boolean>() }
    val agentTextAnswers = remember(sessionId) { mutableStateMapOf<Int, String>() }
    val agentTextExpanded = remember(sessionId) { mutableStateMapOf<Int, Boolean>() }

    // Collect snapshotFlow and feed to AgentStatusMonitor for autonomous parsing
    LaunchedEffect(sessionId) {
        val flow = TerminalSessionManager.snapshotFlow(sessionId) ?: return@LaunchedEffect
        @Suppress("UNCHECKED_CAST")
        val stateFlow = flow as? kotlinx.coroutines.flow.StateFlow<Any?> ?: return@LaunchedEffect
        stateFlow.collect { snapshot ->
            if (snapshot != null) {
                // Process on Dispatchers.Default to avoid blocking the main thread
                // (regex matching on full screen text is CPU-intensive)
                kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Default) {
                    val scraped = com.termfast.app.agent.TermlibAccess.toScrapedSnapshot(snapshot)
                    if (scraped != null) {
                        com.termfast.app.agent.AgentStatusMonitor.processSnapshot(sessionId, scraped)
                    }
                }
                // Auto-show/hide on main thread (UI state update)
                // Don't override user manual toggle (agentSheetUserToggled)
                if (!agentSheetUserToggled) {
                    val status = com.termfast.app.agent.AgentStatusMonitor.getStatusState(sessionId)
                    if (status.status == com.termfast.app.agent.AgentStatus.BLOCKED) {
                        showAgentSheet = true
                    } else if (status.status != com.termfast.app.agent.AgentStatus.BLOCKED) {
                        showAgentSheet = false
                    }
                }
            }
        }
    }

    // "New terminal created" hint — shown for 3s when this server has >1
    //   active terminal sessions. Tapping it opens the terminals list.
    val snackbarHostState = remember { SnackbarHostState() }
    var hintShown by remember(sessionId) { mutableStateOf(false) }

    // Refresh session state periodically
    LaunchedEffect(sessionId) {
        while (true) {
            kotlinx.coroutines.delay(500)
            sessionState = TerminalSessionManager.getSessions(serverId).firstOrNull { it.sessionId == sessionId }
        }
    }

    // Terminal config from Rust settings
    val config = remember { RustRepository.getConfig() }
    val themeId = config?.general?.terminal_theme ?: "catppuccin-mocha"
    var baseFontSize by remember(sessionId) { mutableStateOf(config?.general?.terminal_font_size ?: 10) }
    // Float-precision accumulator for smooth pinch-zoom (Int truncation
    // loses small zoom deltas like 10 * 1.04 = 10.4 → 10)
    var floatFontSize by remember(sessionId) { mutableStateOf(baseFontSize.toFloat()) }
    val theme = TerminalThemes.byId(themeId)

    // Get the termlib emulator for this session
    val emulator = remember(sessionId) { TerminalSessionManager.getEmulatorBySession(sessionId) }

    // For remote sessions: track desktop PTY dimensions for forcedSize.
    // When non-zero, the Terminal composable uses these as forcedSize,
    // preventing it from resizing the emulator to the mobile screen size.
    var remotePtySize by remember(sessionId) {
        val s = TerminalSessionManager.getSessionState(sessionId)
        android.util.Log.d("termfast", "TerminalScreen init: sessionId=$sessionId remotePtyCols=${s?.remotePtyCols} remotePtyRows=${s?.remotePtyRows}")
        mutableStateOf(s?.let {
            if (it.remotePtyCols > 0 && it.remotePtyRows > 0) it.remotePtyRows to it.remotePtyCols else null
        })
    }

    var connected by remember(sessionId) {
        if (isRemote) {
            // Remote sessions are created already connected
            mutableStateOf(true)
        } else {
            val s = TerminalSessionManager.getSessions(serverId).firstOrNull { it.sessionId == sessionId }
            android.util.Log.d("termfast", "TerminalScreen init: sessionId=$sessionId found=${s != null} connected=${s?.connected}")
            mutableStateOf(s?.connected ?: false)
        }
    }
    var connecting by remember(sessionId) { mutableStateOf(if (isRemote) false else !(connected)) }
    // tmux picker state
    var showTmuxPicker by remember(sessionId) { mutableStateOf(false) }
    var tmuxMode by remember(sessionId) { mutableStateOf("ask") }
    // Was this terminal already connected when the screen was entered?
    // Used to suppress the "new terminal" hint when switching between sessions.
    val wasAlreadyConnected by remember(sessionId) { mutableStateOf(connected) }
    var errorMsg by remember { mutableStateOf<String?>(null) }

    // Collect terminal events — only for connection state, not rendering.
    // In remote mode, connection state is managed by RemoteTunnelManager.
    LaunchedEffect(sessionId) {
        if (isRemote) {
            // Remote mode: listen for RESIZE events to update forcedSize
            RustRepository.events.collect { event ->
                when (event) {
                    is RustEvent.RemoteTerminalResize -> {
                        val s = TerminalSessionManager.getSessionState(sessionId)
                        if (s != null && s.remotePairingId == event.pairing_id &&
                            s.remoteTerminalId == event.terminal_id.toInt()) {
                            remotePtySize = event.rows to event.cols
                            android.util.Log.d("termfast", "TerminalScreen: updated forcedSize to rows=${event.rows} cols=${event.cols}")
                        }
                    }
                    is RustEvent.RemoteTunnelReady -> {
                        // Tunnel ready — re-read PTY size from SessionState in case
                        // RESIZE arrived before TerminalScreen started listening
                        val s = TerminalSessionManager.getSessionState(sessionId)
                        if (s != null && s.remotePtyCols > 0 && s.remotePtyRows > 0) {
                            remotePtySize = s.remotePtyRows to s.remotePtyCols
                            android.util.Log.d("termfast", "TerminalScreen: recovered forcedSize from SessionState rows=${s.remotePtyRows} cols=${s.remotePtyCols}")
                        }
                    }
                    else -> {}
                }
            }
            return@LaunchedEffect
        }
        // Local SSH mode: listen for closed/error events
        RustRepository.events.collect { event ->
            when (event) {
                is RustEvent.TerminalClosed -> {
                    android.util.Log.d("termfast", "TerminalClosed event: event.session_id=${event.session_id} this.sessionId=$sessionId")
                    if (event.session_id == sessionId) {
                        connected = false
                        connecting = false
                        TerminalSessionManager.setConnectedBySession(sessionId, false)
                    }
                }
                is RustEvent.TerminalError -> {
                    android.util.Log.d("termfast", "TerminalError event: event.session_id=${event.session_id} this.sessionId=$sessionId error=${event.error}")
                    if (event.session_id == sessionId) {
                        errorMsg = event.error
                        connecting = false
                        connected = false
                        TerminalSessionManager.setConnectedBySession(sessionId, false)
                    }
                }
                else -> {}
            }
        }
    }

    // Remote mode: delayed check for PTY size — handles the case where
    // RESIZE frame arrived before TerminalScreen started listening.
    LaunchedEffect(sessionId, isRemote) {
        if (!isRemote) return@LaunchedEffect
        kotlinx.coroutines.delay(1000)
        if (remotePtySize == null) {
            val s = TerminalSessionManager.getSessionState(sessionId)
            if (s != null && s.remotePtyCols > 0 && s.remotePtyRows > 0) {
                remotePtySize = s.remotePtyRows to s.remotePtyCols
                android.util.Log.d("termfast", "TerminalScreen: delayed recovery forcedSize rows=${s.remotePtyRows} cols=${s.remotePtyCols}")
            }
        }
    }

    // Apply theme colors to emulator when it's first available or theme changes.
    LaunchedEffect(emulator, themeId) {
        emulator?.applyColorScheme(theme.ansiColors, theme.foreground, theme.background)
    }

    // After connection is established or font size changes, send the
    // emulator's actual dimensions to the remote shell so TUI apps
    // (top, htop, vim) render correctly. Debounce 500ms to avoid
    // flooding resize commands during continuous pinch-zoom.
    // In remote mode, the Terminal composable's onSizeChanged automatically
    // calls emulator.resize() which triggers the onResize callback →
    // RemoteTunnelManager.sendResize. So we do NOT manually resize here
    // (doing so would overwrite the Terminal composable's calculated dimensions
    // with the old 24x80 default, causing the half-screen issue).
    LaunchedEffect(connected, baseFontSize) {
        if (connected && emulator != null) {
            kotlinx.coroutines.delay(500)
            val dims = emulator.dimensions
            if (!isRemote) {
                repo.resizeTerminal(sessionId, dims.columns, dims.rows)
            }
            android.util.Log.d("termfast", "resize sent: ${dims.columns}x${dims.rows} fontSize=$baseFontSize remote=$isRemote")
        }
    }

    // Open terminal session on screen entry (only if not already connected)
    // In remote mode, the tunnel + session are already established by
    // RemoteTerminalScreen — skip SSH connection entirely.
    LaunchedEffect(serverId, sessionId) {
        if (isRemote) return@LaunchedEffect
        android.util.Log.d("termfast", "TerminalScreen LaunchedEffect: sessionId=$sessionId connected=$connected connecting=$connecting")
        if (connected) return@LaunchedEffect
        scope.launch {
            withContext(Dispatchers.IO) {
                // Wait for credential store to be ready (unlocked or pending).
                val deadline = System.currentTimeMillis() + 3000
                while (System.currentTimeMillis() < deadline) {
                    if (com.termfast.app.data.CredentialManager.isUnlocked()) break
                    kotlinx.coroutines.delay(50)
                }
                // Ensure SSH is connected first
                val status = repo.getServerStatus(serverId)
                if (status.status != "connected") {
                    val ok = repo.connectServer(serverId)
                    if (!ok) {
                        withContext(Dispatchers.Main) {
                            errorMsg = "无法连接到 SSH 服务器，请检查服务器配置"
                            connecting = false
                        }
                        return@withContext
                    }
                }
                // Read tmux_mode from config
                val config = repo.getConfig()
                val mode = config?.servers?.find { it.id == serverId }?.tmux_mode ?: "ask"
                withContext(Dispatchers.Main) { tmuxMode = mode }

                when (mode) {
                    "disabled" -> {
                        // Plain shell, no tmux
                        val ok = repo.openTerminal(serverId, sessionId, 80, 24)
                        withContext(Dispatchers.Main) {
                            if (ok) {
                                connected = true
                                connecting = false
                                TerminalSessionManager.setConnectedBySession(sessionId, true)
                            } else {
                                errorMsg = "无法打开终端会话"
                                connecting = false
                            }
                        }
                    }
                    "always_new" -> {
                        // Always create new tmux session
                        val result = repo.tmuxNewSession(serverId, sessionId, "", 80, 24)
                        withContext(Dispatchers.Main) {
                            connected = true
                            connecting = false
                            TerminalSessionManager.setConnectedBySession(sessionId, true)
                        }
                    }
                    "auto" -> {
                        // Auto: if existing sessions, attach to most recent; else create new
                        val json = repo.tmuxListSessions(serverId)
                        var attachedTmuxName: String? = null
                        try {
                            val resp = Json.decodeFromString<TmuxListResponse>(json)
                            if (resp.tmux_installed && resp.sessions.isNotEmpty()) {
                                val mostRecent = resp.sessions.maxByOrNull { it.last_activity }
                                if (mostRecent != null) {
                                    repo.tmuxAttachSession(serverId, sessionId, mostRecent.name, 80, 24)
                                    attachedTmuxName = mostRecent.name
                                } else {
                                    repo.tmuxNewSession(serverId, sessionId, "", 80, 24)
                                }
                            } else if (resp.tmux_installed) {
                                repo.tmuxNewSession(serverId, sessionId, "", 80, 24)
                            } else {
                                // tmux not installed, plain shell
                                repo.openTerminal(serverId, sessionId, 80, 24)
                            }
                        } catch (e: Exception) {
                            repo.openTerminal(serverId, sessionId, 80, 24)
                        }
                        withContext(Dispatchers.Main) {
                            connected = true
                            connecting = false
                            TerminalSessionManager.setConnectedBySession(sessionId, true)
                            TerminalSessionManager.setTmuxSessionName(sessionId, attachedTmuxName)
                        }
                    }
                    else -> {
                        // "ask" mode — show picker dialog
                        val json = repo.tmuxListSessions(serverId)
                        try {
                            val resp = Json.decodeFromString<TmuxListResponse>(json)
                            if (resp.tmux_installed && resp.sessions.isNotEmpty()) {
                                withContext(Dispatchers.Main) {
                                    showTmuxPicker = true
                                    // Keep connecting=true so the terminal shows
                                    // a neutral "connecting" state instead of
                                    // "disconnected" while the picker is open.
                                }
                            } else if (resp.tmux_installed) {
                                // tmux installed but no sessions — create new
                                repo.tmuxNewSession(serverId, sessionId, "", 80, 24)
                                withContext(Dispatchers.Main) {
                                    connected = true
                                    connecting = false
                                    TerminalSessionManager.setConnectedBySession(sessionId, true)
                                    TerminalSessionManager.setTmuxSessionName(sessionId, null)
                                }
                            } else {
                                // tmux not installed, plain shell
                                val ok = repo.openTerminal(serverId, sessionId, 80, 24)
                                withContext(Dispatchers.Main) {
                                    if (ok) {
                                        connected = true
                                        connecting = false
                                        TerminalSessionManager.setConnectedBySession(sessionId, true)
                                    } else {
                                        errorMsg = "无法打开终端会话"
                                        connecting = false
                                    }
                                }
                            }
                        } catch (e: Exception) {
                            val ok = repo.openTerminal(serverId, sessionId, 80, 24)
                            withContext(Dispatchers.Main) {
                                if (ok) {
                                    connected = true
                                    connecting = false
                                    TerminalSessionManager.setConnectedBySession(sessionId, true)
                                } else {
                                    errorMsg = "无法打开终端会话"
                                    connecting = false
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Don't close terminal on dispose — keep it running in background for reuse

    // Detect when a NEW terminal was just created (connecting → connected
    //   transition) and there are >1 terminals. Only show the hint on the
    //   initial connection, not when switching to an already-connected session.
    LaunchedEffect(connected) {
        if (isRemote) return@LaunchedEffect
        if (connected && !wasAlreadyConnected && !hintShown) {
            val count = TerminalSessionManager.getSessions(serverId).size
            if (count > 1) {
                hintShown = true
            }
        }
    }

    // Show the snackbar when hintShown becomes true. This effect only depends
    //   on hintShown, so it won't be cancelled by connected state changes.
    LaunchedEffect(hintShown) {
        if (hintShown) {
            val count = TerminalSessionManager.getSessions(serverId).size
            val result = snackbarHostState.showSnackbar(
                message = "已新建终端，本服务器共 $count 个活跃终端",
                actionLabel = "查看全部",
                duration = SnackbarDuration.Short,
            )
            if (result == SnackbarResult.ActionPerformed) {
                navController.navigate("terminals/$sessionId")
            }
        }
    }

    val terminalBg = Color(theme.background)
    val terminalFg = Color(theme.foreground)
    val terminalGreen = Color(0xFFA6E3A1)
    val configuration = androidx.compose.ui.platform.LocalConfiguration.current
    val isLandscape = configuration.orientation == android.content.res.Configuration.ORIENTATION_LANDSCAPE
    // Landscape: which side is the keyboard on? false = right (default), true = left
    var keyboardOnLeft by remember { mutableStateOf(false) }
    // System IME mode (externally managed so the keyboard can collapse to 0
    //   height and a floating toggle button overlays the terminal).
    var useSystemKeyboard by remember { mutableStateOf(false) }
    val toggleSystemKeyboard = {
        useSystemKeyboard = !useSystemKeyboard
    }

    Box(Modifier.fillMaxSize()) {
    if (isLandscape) {
        // Landscape: terminal + keyboard side by side. Keyboard width is fixed
        //   to the short edge so keys keep portrait size. Side is toggleable.
        val keyboardWidth = configuration.screenHeightDp.dp
        val keyboardModifier = Modifier.width(keyboardWidth)
        // In remote mode, keyboard input goes through TerminalSessionManager
        // which routes to RemoteTunnelManager. In SSH mode, it goes through
        // repo.writeTerminal (SSH PTY).
        val onKeyLambda = { key: String ->
            if (connected) {
                TerminalSessionManager.writeToSession(sessionId, key)
            }
        }
        val togglePosition = { keyboardOnLeft = !keyboardOnLeft }

        Row(
            modifier = Modifier
                .fillMaxSize()
                .background(terminalBg)
                .statusBarsPadding()
                .imePadding()
        ) {
            // Order depends on keyboard side. Terminal Box always weight(1f).
            if (keyboardOnLeft && !useSystemKeyboard) {
                TerminalKeyboard(
                    onKey = onKeyLambda,
                    enabled = connected,
                    modifier = keyboardModifier,
                    onTogglePosition = togglePosition,
                    keyboardOnLeft = true,
                    onToggleSystemKeyboard = toggleSystemKeyboard,
                )
            }
            // Terminal output area
            Box(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxHeight()
                    .background(terminalBg),
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
                        color = terminalGreen,
                    )
                    Text(
                        "正在连接终端...",
                        color = terminalFg,
                        fontSize = 14.sp,
                    )
                }
            } else if (errorMsg != null && emulator == null) {
                Column(
                    modifier = Modifier.align(Alignment.Center),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Text(
                        "⚠ $errorMsg",
                        color = MaterialTheme.colorScheme.error,
                        fontSize = 14.sp,
                    )
                    Spacer(Modifier.height(12.dp))
                    Text(
                        "请先在服务器详情页启动 VPN 或代理",
                        color = terminalFg.copy(alpha = 0.6f),
                        fontSize = 12.sp,
                    )
                }
            } else if (emulator != null) {
                // Interceptor Box — Terminal is a CHILD of this Box.
                // PointerEventPass.Initial dispatches parent → child, so this
                // Box sees 2+ finger events BEFORE Terminal. Consuming them
                // blocks termlib's built-in graphicsLayer zoom (which snaps
                // back). We change font size instead. Single-finger events
                // pass through to Terminal for selection/scroll/menu.
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .pointerInput(sessionId) {
                            awaitPointerEventScope {
                                var wasZooming = false
                                while (true) {
                                    val event = awaitPointerEvent(
                                        PointerEventPass.Initial
                                    )
                                    val pressedCount =
                                        event.changes.count { it.pressed }
                                    if (pressedCount >= 2) {
                                        // Consume to block termlib's zoom
                                        event.changes.forEach { it.consume() }
                                        val zoom = event.calculateZoom()
                                        if (zoom != 1f) {
                                            // Accumulate in float only — don't
                                            // update baseFontSize mid-gesture
                                            // to avoid termlib relayout thrash
                                            floatFontSize *= zoom
                                            floatFontSize = floatFontSize.coerceIn(4f, 32f)
                                        }
                                        wasZooming = true
                                    } else if (pressedCount == 0 && wasZooming) {
                                        // Gesture ended: apply final font size
                                        val newInt = floatFontSize.toInt()
                                        if (newInt != baseFontSize) {
                                            baseFontSize = newInt
                                        }
                                        wasZooming = false
                                    }
                                }
                            }
                        },
                ) {
                    android.util.Log.d("termfast", "Terminal composable: isRemote=$isRemote baseFontSize=$baseFontSize")
                    Terminal(
                        terminalEmulator = emulator,
                        modifier = Modifier.fillMaxSize(),
                        typeface = Typeface.MONOSPACE,
                        initialFontSize = baseFontSize.sp,
                        minFontSize = 4.sp,
                        maxFontSize = 32.sp,
                        backgroundColor = terminalBg,
                        foregroundColor = terminalFg,
                        keyboardEnabled = useSystemKeyboard,
                    )
                }
            }

            // Agent question button — glassmorphism chip left of session name
            GlassChip(
                text = "AI",
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .padding(top = 4.dp, end = 72.dp)
                    .clickable {
                        agentSheetUserToggled = true
                        showAgentSheet = true
                    },
                textColor = terminalFg,
            )

            // Session name — glassmorphism card top-right, click for actions
            GlassChip(
                text = title,
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .padding(top = 4.dp, end = 4.dp)
                    .clickable { showSheet = true },
                textColor = terminalFg,
            )

            // Font size +/- buttons below the session name chip
            FontSizeButtons(
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .padding(top = 44.dp, end = 4.dp),
                onIncrease = { baseFontSize = (baseFontSize + 1).coerceIn(4, 32); floatFontSize = baseFontSize.toFloat() },
                onDecrease = { baseFontSize = (baseFontSize - 1).coerceIn(4, 32); floatFontSize = baseFontSize.toFloat() },
                textColor = terminalFg,
            )

            // Paste button — reads system clipboard and sends to terminal
            PasteButton(
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .padding(top = 116.dp, end = 4.dp),
                onPaste = {
                    val clipboard = context.getSystemService(android.content.Context.CLIPBOARD_SERVICE)
                        as android.content.ClipboardManager
                    val text = clipboard.primaryClip?.getItemAt(0)?.text?.toString() ?: ""
                    if (text.isNotEmpty() && connected) {
                        TerminalSessionManager.writeToSession(sessionId, text)
                    }
                },
                textColor = terminalFg,
            )

            // System-IME mode: floating "switch back" button at bottom-right
            if (useSystemKeyboard) {
                Box(
                    modifier = Modifier
                        .align(Alignment.BottomEnd)
                        .padding(bottom = 4.dp, end = 4.dp)
                        .height(28.dp)
                        .clip(RoundedCornerShape(6.dp))
                        .background(Color(0xFF89B4FA))
                        .clickable { toggleSystemKeyboard() }
                        .padding(horizontal = 12.dp, vertical = 4.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        "⌨ 切换终端键盘",
                        color = Color(0xFF181825),
                        fontSize = 10.sp,
                        fontWeight = FontWeight.Medium,
                        maxLines = 1,
                    )
                }
            }

            // Disconnected banner — overlaid at bottom of terminal area
            if (!connected && !connecting && emulator != null) {
                Row(
                    modifier = Modifier
                        .align(Alignment.BottomCenter)
                        .fillMaxWidth()
                        .background(MaterialTheme.colorScheme.errorContainer)
                        .padding(horizontal = 16.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Icon(
                        Icons.Filled.Warning,
                        contentDescription = null,
                        modifier = Modifier.size(20.dp),
                        tint = MaterialTheme.colorScheme.onErrorContainer,
                    )
                    Text(
                        "连接已断开",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onErrorContainer,
                        modifier = Modifier.weight(1f),
                    )
                    TextButton(
                        onClick = {
                            if (isRemote) {
                                // Remote: pop back to terminal list to reconnect
                                navController.popBackStack()
                            } else {
                                connecting = true
                                errorMsg = null
                                scope.launch {
                                    withContext(Dispatchers.IO) {
                                        val status = repo.getServerStatus(serverId)
                                        if (status.status != "connected") {
                                            repo.connectServer(serverId)
                                        }
                                        val ok = repo.openTerminal(serverId, sessionId, 80, 24)
                                        withContext(Dispatchers.Main) {
                                            if (ok) {
                                                connected = true
                                                connecting = false
                                                TerminalSessionManager.setConnectedBySession(sessionId, true)
                                            } else {
                                                errorMsg = "重连失败"
                                                connecting = false
                                            }
                                        }
                                    }
                                }
                            }
                        },
                        contentPadding = PaddingValues(horizontal = 12.dp, vertical = 4.dp),
                    ) {
                        Text("重连", color = MaterialTheme.colorScheme.onErrorContainer)
                    }
                }
            }
        }

        // Keyboard on the right side (default). Left side already rendered
        //   above when keyboardOnLeft == true. Hidden in system-IME mode.
        if (!keyboardOnLeft && !useSystemKeyboard) {
            TerminalKeyboard(
                onKey = onKeyLambda,
                enabled = connected,
                modifier = keyboardModifier,
                onTogglePosition = togglePosition,
                keyboardOnLeft = false,
                onToggleSystemKeyboard = toggleSystemKeyboard,
            )
        }
    }
} else {
    // Portrait: terminal on top, keyboard at bottom
    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(terminalBg)
            .statusBarsPadding()
            .imePadding()
    ) {
        // Terminal output area
        Box(
            modifier = Modifier
                .weight(1f)
                .fillMaxWidth()
                .background(terminalBg),
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
                        color = terminalGreen,
                    )
                    Text(
                        "正在连接终端...",
                        color = terminalFg,
                        fontSize = 14.sp,
                    )
                }
            } else if (errorMsg != null && emulator == null) {
                Column(
                    modifier = Modifier.align(Alignment.Center),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Text(
                        "⚠ $errorMsg",
                        color = MaterialTheme.colorScheme.error,
                        fontSize = 14.sp,
                    )
                    Spacer(Modifier.height(12.dp))
                    Text(
                        "请先在服务器详情页启动 VPN 或代理",
                        color = terminalFg.copy(alpha = 0.6f),
                        fontSize = 12.sp,
                    )
                }
            } else if (emulator != null) {
                // Interceptor Box — Terminal is a CHILD of this Box.
                // PointerEventPass.Initial dispatches parent → child, so this
                // Box sees 2+ finger events BEFORE Terminal. Consuming them
                // blocks termlib's built-in graphicsLayer zoom (which snaps
                // back). We change font size instead. Single-finger events
                // pass through to Terminal for selection/scroll/menu.
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .pointerInput(sessionId) {
                            awaitPointerEventScope {
                                var wasZooming = false
                                while (true) {
                                    val event = awaitPointerEvent(
                                        PointerEventPass.Initial
                                    )
                                    val pressedCount =
                                        event.changes.count { it.pressed }
                                    if (pressedCount >= 2) {
                                        // Consume to block termlib's zoom
                                        event.changes.forEach { it.consume() }
                                        val zoom = event.calculateZoom()
                                        if (zoom != 1f) {
                                            // Accumulate in float only — don't
                                            // update baseFontSize mid-gesture
                                            // to avoid termlib relayout thrash
                                            floatFontSize *= zoom
                                            floatFontSize = floatFontSize.coerceIn(4f, 32f)
                                        }
                                        wasZooming = true
                                    } else if (pressedCount == 0 && wasZooming) {
                                        // Gesture ended: apply final font size
                                        val newInt = floatFontSize.toInt()
                                        if (newInt != baseFontSize) {
                                            baseFontSize = newInt
                                        }
                                        wasZooming = false
                                    }
                                }
                            }
                        },
                ) {
                    android.util.Log.d("termfast", "Terminal composable: isRemote=$isRemote baseFontSize=$baseFontSize")
                    Terminal(
                        terminalEmulator = emulator,
                        modifier = Modifier.fillMaxSize(),
                        typeface = Typeface.MONOSPACE,
                        initialFontSize = baseFontSize.sp,
                        minFontSize = 4.sp,
                        maxFontSize = 32.sp,
                        backgroundColor = terminalBg,
                        foregroundColor = terminalFg,
                        keyboardEnabled = useSystemKeyboard,
                    )
                }
            }

            // Agent question button — glassmorphism chip left of session name
            GlassChip(
                text = "AI",
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .padding(top = 4.dp, end = 72.dp)
                    .clickable {
                        agentSheetUserToggled = true
                        showAgentSheet = true
                    },
                textColor = terminalFg,
            )

            // Session name — glassmorphism card top-right, click for actions
            GlassChip(
                text = title,
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .padding(top = 4.dp, end = 4.dp)
                    .clickable { showSheet = true },
                textColor = terminalFg,
            )

            // Font size +/- buttons below the session name chip
            FontSizeButtons(
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .padding(top = 44.dp, end = 4.dp),
                onIncrease = { baseFontSize = (baseFontSize + 1).coerceIn(4, 32); floatFontSize = baseFontSize.toFloat() },
                onDecrease = { baseFontSize = (baseFontSize - 1).coerceIn(4, 32); floatFontSize = baseFontSize.toFloat() },
                textColor = terminalFg,
            )

            // Paste button — reads system clipboard and sends to terminal
            PasteButton(
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .padding(top = 116.dp, end = 4.dp),
                onPaste = {
                    val clipboard = context.getSystemService(android.content.Context.CLIPBOARD_SERVICE)
                        as android.content.ClipboardManager
                    val text = clipboard.primaryClip?.getItemAt(0)?.text?.toString() ?: ""
                    if (text.isNotEmpty() && connected) {
                        TerminalSessionManager.writeToSession(sessionId, text)
                    }
                },
                textColor = terminalFg,
            )

            // System-IME mode: floating "switch back" button at bottom-right
            if (useSystemKeyboard) {
                Box(
                    modifier = Modifier
                        .align(Alignment.BottomEnd)
                        .padding(bottom = 4.dp, end = 4.dp)
                        .height(28.dp)
                        .clip(RoundedCornerShape(6.dp))
                        .background(Color(0xFF89B4FA))
                        .clickable { toggleSystemKeyboard() }
                        .padding(horizontal = 12.dp, vertical = 4.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        "⌨ 切换终端键盘",
                        color = Color(0xFF181825),
                        fontSize = 10.sp,
                        fontWeight = FontWeight.Medium,
                        maxLines = 1,
                    )
                }
            }

            // Disconnected banner — overlaid at bottom of terminal area
            if (!connected && !connecting && emulator != null) {
                Row(
                    modifier = Modifier
                        .align(Alignment.BottomCenter)
                        .fillMaxWidth()
                        .background(MaterialTheme.colorScheme.errorContainer)
                        .padding(horizontal = 16.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Icon(
                        Icons.Filled.Warning,
                        contentDescription = null,
                        modifier = Modifier.size(20.dp),
                        tint = MaterialTheme.colorScheme.onErrorContainer,
                    )
                    Text(
                        "连接已断开",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onErrorContainer,
                        modifier = Modifier.weight(1f),
                    )
                    TextButton(
                        onClick = {
                            if (isRemote) {
                                // Remote: pop back to terminal list to reconnect
                                navController.popBackStack()
                            } else {
                                connecting = true
                                errorMsg = null
                                scope.launch {
                                    withContext(Dispatchers.IO) {
                                        val status = repo.getServerStatus(serverId)
                                        if (status.status != "connected") {
                                            repo.connectServer(serverId)
                                        }
                                        val ok = repo.openTerminal(serverId, sessionId, 80, 24)
                                        withContext(Dispatchers.Main) {
                                            if (ok) {
                                                connected = true
                                                connecting = false
                                                TerminalSessionManager.setConnectedBySession(sessionId, true)
                                            } else {
                                                errorMsg = "重连失败"
                                                connecting = false
                                            }
                                        }
                                    }
                                }
                            }
                        },
                        contentPadding = PaddingValues(horizontal = 12.dp, vertical = 4.dp),
                    ) {
                        Text("重连", color = MaterialTheme.colorScheme.onErrorContainer)
                    }
                }
            }
        }

        // Custom terminal keyboard — hidden in system-IME mode so it
        //   collapses to 0 height and the terminal fills the space above IME.
        if (!useSystemKeyboard) {
            TerminalKeyboard(
                onKey = { key ->
                    if (connected) TerminalSessionManager.writeToSession(sessionId, key)
                },
                enabled = connected,
                onToggleSystemKeyboard = toggleSystemKeyboard,
            )
        }
    }
    } // end if-else orientation

    // Snackbar host for "new terminal" hint — placed at top to avoid
    //   being covered by the custom keyboard at the bottom. Apply status
    //   bar padding so it's not hidden under the system status bar.
    SnackbarHost(
        hostState = snackbarHostState,
        modifier = Modifier
            .align(Alignment.TopCenter)
            .statusBarsPadding(),
    )
    } // end Box

    // === System IME mode ===
    // termlib's Terminal composable with keyboardEnabled=true handles IME
    // input directly. The old hidden BasicTextField is no longer needed.
    // When useSystemKeyboard is true, Terminal's keyboardEnabled is set to
    // true (see the Terminal() calls above), and termlib manages focus + IME.

    // === tmux session picker dialog === (SSH only, not for remote terminals)
    if (!isRemote) {
        TmuxSessionPickerDialog(
            visible = showTmuxPicker,
            serverId = serverId,
            sessionId = sessionId,
        onAttach = { attachSessionId, tmuxName ->
            showTmuxPicker = false
            if (attachSessionId != sessionId) {
                // Reusing an existing terminal card that's already attached
                // to this tmux session. Navigate to it.
                navController.navigate("terminal/$serverId/$attachSessionId") {
                    popUpTo("terminal/$serverId") { inclusive = true }
                }
            } else {
                // Current session just attached to tmux.
                connected = true
                connecting = false
                TerminalSessionManager.setConnectedBySession(sessionId, true)
                TerminalSessionManager.setTmuxSessionName(sessionId, tmuxName)
            }
        },
        onCreate = { _sessionId, _desc ->
            showTmuxPicker = false
            connected = true
            connecting = false
            TerminalSessionManager.setConnectedBySession(sessionId, true)
        },
        onSkip = {
            showTmuxPicker = false
            scope.launch {
                withContext(Dispatchers.IO) {
                    val ok = repo.openTerminal(serverId, sessionId, 80, 24)
                    withContext(Dispatchers.Main) {
                        if (ok) {
                            connected = true
                            connecting = false
                            TerminalSessionManager.setConnectedBySession(sessionId, true)
                        } else {
                            errorMsg = "无法打开终端会话"
                            connecting = false
                        }
                    }
                }
            }
        },
        onDismiss = {
            showTmuxPicker = false
            // Clean up the empty session and go back immediately.
            // Don't set connecting=false — that would flash "disconnected"
            // before popBackStack takes effect.
            TerminalSessionManager.closeSessionBySessionId(sessionId)
            navController.popBackStack()
        },
    )
    } // end if (!isRemote) — tmux picker

    // === Session action bottom sheet ===
    if (showSheet) {
        ModalBottomSheet(
            onDismissRequest = { showSheet = false },
            sheetState = sheetState,
        ) {
            Column(
                modifier = Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 8.dp),
            ) {
                Text(
                    title,
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                )
                Text(
                    if (connected) "已连接" else "已断开",
                    style = MaterialTheme.typography.bodySmall,
                    color = if (connected)
                        MaterialTheme.colorScheme.primary
                    else
                        MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            HorizontalDivider(modifier = Modifier.padding(vertical = 8.dp))
            ListItem(
                headlineContent = { Text("所有终端") },
                leadingContent = { Icon(Icons.Filled.Tab, contentDescription = null, modifier = Modifier.size(24.dp)) },
                modifier = Modifier.clickable {
                    showSheet = false
                    if (isRemote) {
                        navController.navigate("terminals") {
                            popUpTo("terminals") { inclusive = false }
                        }
                    } else {
                        navController.navigate("terminals/$sessionId")
                    }
                },
            )
            HorizontalDivider(modifier = Modifier.padding(vertical = 8.dp))
            ListItem(
                headlineContent = { Text("重命名") },
                leadingContent = { Icon(Icons.Filled.Edit, contentDescription = null, modifier = Modifier.size(24.dp)) },
                modifier = Modifier.clickable {
                    showSheet = false
                    renameText = sessionState?.name ?: ""
                    showRenameDialog = true
                },
            )
            // Reconnect — SSH only (remote reconnect is handled differently)
            if (!isRemote) {
                ListItem(
                    headlineContent = { Text("重连") },
                    leadingContent = { Icon(Icons.Filled.Refresh, contentDescription = null, modifier = Modifier.size(24.dp)) },
                    modifier = Modifier.clickable {
                        showSheet = false
                        TerminalSessionManager.reconnectSession(serverId, sessionId) { }
                        connecting = true
                        scope.launch {
                            withContext(Dispatchers.IO) {
                                kotlinx.coroutines.delay(500)
                                val s = TerminalSessionManager.getSessions(serverId).firstOrNull { it.sessionId == sessionId }
                                withContext(Dispatchers.Main) {
                                    connected = s?.connected ?: false
                                    connecting = false
                                }
                            }
                        }
                    },
                )
            }
            ListItem(
                headlineContent = { Text(if (connected) "断开" else "已断开") },
                leadingContent = { Icon(Icons.Filled.Stop, contentDescription = null, modifier = Modifier.size(24.dp)) },
                modifier = Modifier.clickable {
                    if (connected) {
                        showSheet = false
                        TerminalSessionManager.disconnectSession(sessionId)
                        connected = false
                    }
                },
                colors = ListItemDefaults.colors(
                    headlineColor = if (connected)
                        MaterialTheme.colorScheme.onSurface
                    else
                        MaterialTheme.colorScheme.onSurfaceVariant,
                ),
            )
            HorizontalDivider(modifier = Modifier.padding(vertical = 4.dp))
            ListItem(
                headlineContent = { Text("删除", color = MaterialTheme.colorScheme.error) },
                leadingContent = {
                    Icon(
                        Icons.Filled.Delete,
                        contentDescription = null,
                        modifier = Modifier.size(24.dp),
                        tint = MaterialTheme.colorScheme.error,
                    )
                },
                modifier = Modifier.clickable {
                    showSheet = false
                    showDeleteDialog = true
                },
            )
            Spacer(Modifier.height(16.dp))
        }
    }

    // Rename dialog
    if (showRenameDialog) {
        AlertDialog(
            onDismissRequest = { showRenameDialog = false },
            title = { Text("重命名终端") },
            text = {
                OutlinedTextField(
                    value = renameText,
                    onValueChange = { renameText = it },
                    label = { Text("名称") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    TerminalSessionManager.renameSession(sessionId, renameText)
                    sessionState = TerminalSessionManager.getSessions(serverId).firstOrNull { it.sessionId == sessionId }
                    showRenameDialog = false
                }) { Text("确定") }
            },
            dismissButton = {
                TextButton(onClick = { showRenameDialog = false }) { Text("取消") }
            },
        )
    }

    // Delete confirmation dialog
    if (showDeleteDialog) {
        AlertDialog(
            onDismissRequest = { showDeleteDialog = false },
            title = { Text("删除终端会话") },
            text = { Text("确定要删除「$title」并断开连接吗？") },
            confirmButton = {
                TextButton(
                    onClick = {
                        showDeleteDialog = false
                        if (!isRemote) {
                            RustRepository.closeTerminal(sessionId)
                        } else {
                            TerminalSessionManager.disconnectSession(sessionId)
                        }
                        TerminalSessionManager.closeSessionBySessionId(sessionId)
                        navController.popBackStack()
                    },
                    colors = ButtonDefaults.textButtonColors(
                        contentColor = MaterialTheme.colorScheme.error,
                    ),
                ) { Text("删除") }
            },
            dismissButton = {
                TextButton(onClick = { showDeleteDialog = false }) { Text("取消") }
            },
        )
    }

    // === Agent question bottom sheet (production — driven by AgentStatusMonitor) ===
    if (showAgentSheet) {
        AgentQuestionSheet(
            sessionId = sessionId,
            onDismiss = {
                showAgentSheet = false
                agentSheetUserToggled = false
            },
            activeTabIndex = agentActiveTab,
            selectedOptions = agentSelectedOptions,
            checkedMap = agentCheckedMap,
            textAnswers = agentTextAnswers,
            textExpanded = agentTextExpanded,
            sendKeystrokes = { data ->
                // Send keystrokes to the PTY via the appropriate input path
                val bytes = data.toByteArray(Charsets.UTF_8)
                val ss = sessionState
                if (ss?.remotePairingId != null) {
                    // Remote terminal: send via tunnel manager
                    val tm = TerminalSessionManager.getTunnelManager(ss.remotePairingId)
                    tm?.sendInput(ss.remoteTerminalId, bytes)
                } else {
                    // Local terminal: send via RustRepository
                    RustRepository.writeTerminalBytes(sessionId, bytes)
                }
            },
            // FP9: notify desktop of autonomous answer (remote mode only)
            onAnsweredRemotely = {
                val ss = sessionState
                if (ss?.remotePairingId != null) {
                    val tm = TerminalSessionManager.getTunnelManager(ss.remotePairingId)
                    if (tm != null && ss.remoteTerminalId >= 0) {
                        // Generate a synthetic questionId for mobile autonomous mode.
                        // Desktop uses this to remove from pending_questions + close overlay.
                        // The metadata (cli, options, etc.) is ignored by Rust __answered__ branch.
                        val questionId = "mobile-auto-$sessionId-${System.currentTimeMillis()}"
                        tm.sendInputAnswer(
                            terminalId = ss.remoteTerminalId,
                            questionId = questionId,
                            answer = "__answered__",
                            optionIndex = 0,
                            cli = "unknown",
                            options = emptyArray(),
                            isMultiSelect = false,
                            isMultiQuestion = false,
                        )
                    }
                }
            },
        )
    }
}

// === SECTION 1: Legacy cursor renderer — removed, termlib handles cursor ===
// TerminalLineWithCursor was here; termlib's Terminal composable has built-in
// blinking cursor support.

// === SECTION 2: Glassmorphism chip ===

@Composable
private fun PasteButton(
    modifier: Modifier = Modifier,
    onPaste: () -> Unit,
    textColor: Color,
) {
    Box(
        modifier = modifier
            .size(30.dp)
            .clip(RoundedCornerShape(10.dp))
            .background(Color(0x33FFFFFF))
            .border(0.5.dp, Color(0x44FFFFFF), RoundedCornerShape(10.dp))
            .clickable { onPaste() },
        contentAlignment = Alignment.Center,
    ) {
        Text("📋", color = textColor.copy(alpha = 0.7f), fontSize = 14.sp)
    }
}

@Composable
private fun FontSizeButtons(
    modifier: Modifier = Modifier,
    onIncrease: () -> Unit,
    onDecrease: () -> Unit,
    textColor: Color,
) {
    Column(
        modifier = modifier,
        verticalArrangement = Arrangement.spacedBy(6.dp),
        horizontalAlignment = Alignment.End,
    ) {
        // Increase button (top)
        Box(
            modifier = Modifier
                .size(30.dp)
                .clip(RoundedCornerShape(10.dp))
                .background(Color(0x33FFFFFF))
                .border(0.5.dp, Color(0x44FFFFFF), RoundedCornerShape(10.dp))
                .clickable { onIncrease() },
            contentAlignment = Alignment.Center,
        ) {
            Text("+", color = textColor.copy(alpha = 0.7f), fontSize = 16.sp, fontWeight = FontWeight.Bold)
        }
        // Decrease button (bottom)
        Box(
            modifier = Modifier
                .size(30.dp)
                .clip(RoundedCornerShape(10.dp))
                .background(Color(0x33FFFFFF))
                .border(0.5.dp, Color(0x44FFFFFF), RoundedCornerShape(10.dp))
                .clickable { onDecrease() },
            contentAlignment = Alignment.Center,
        ) {
            Text("−", color = textColor.copy(alpha = 0.7f), fontSize = 16.sp, fontWeight = FontWeight.Bold)
        }
    }
}

@Composable
private fun GlassChip(
    text: String,
    modifier: Modifier = Modifier,
    textColor: Color = Color.White,
) {
    Box(
        modifier = modifier
            .clip(RoundedCornerShape(12.dp))
            .background(Color(0x33FFFFFF))
            .border(
                width = 0.5.dp,
                color = Color(0x44FFFFFF),
                shape = RoundedCornerShape(12.dp),
            )
            .padding(horizontal = 10.dp, vertical = 5.dp),
    ) {
        Text(
            text,
            color = textColor.copy(alpha = 0.7f),
            fontSize = 11.sp,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}