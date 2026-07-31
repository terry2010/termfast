package com.termfast.app.ui.screen

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.selection.SelectionContainer
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
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
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
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun TerminalScreen(navController: NavController, serverId: String, existingSessionId: String? = null) {
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
    // Use existing session if provided, otherwise get or create
    val sessionId = remember(existingSessionId) {
        if (existingSessionId != null) {
            TerminalSessionManager.getOrCreateSessionById(serverId, existingSessionId)
        } else {
            TerminalSessionManager.getOrCreateSession(serverId)
        }
    }
    val listState = rememberLazyListState()

    // Resolve title: server name + session name
    var sessionState by remember { mutableStateOf(TerminalSessionManager.getSessions(serverId).firstOrNull { it.sessionId == sessionId }) }
    val title = sessionState?.name?.ifBlank { null } ?: "SSH 终端"

    // Session action sheet
    var showSheet by remember { mutableStateOf(false) }
    var showRenameDialog by remember { mutableStateOf(false) }
    var showDeleteDialog by remember { mutableStateOf(false) }
    var renameText by remember { mutableStateOf(sessionState?.name ?: "") }
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)

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

    // Terminal output lines — restore from cache if available
    var outputLines by remember(sessionId) { mutableStateOf(TerminalSessionManager.getOutputBySession(sessionId)) }
    var cursorCol by remember(sessionId) { mutableStateOf(TerminalSessionManager.getCursorColBySession(sessionId)) }
    var connected by remember(sessionId) {
        val s = TerminalSessionManager.getSessions(serverId).firstOrNull { it.sessionId == sessionId }
        mutableStateOf(s?.connected ?: false)
    }
    var connecting by remember(sessionId) { mutableStateOf(!(connected)) }
    var errorMsg by remember { mutableStateOf<String?>(null) }

    // Collect terminal events
    LaunchedEffect(sessionId) {
        RustRepository.events.collect { event ->
            when (event) {
                is RustEvent.TerminalData -> {
                    if (event.session_id == sessionId) {
                        // Global collector already processes data; just read updated state
                        outputLines = TerminalSessionManager.getOutputBySession(sessionId)
                        cursorCol = TerminalSessionManager.getCursorColBySession(sessionId)
                        if (outputLines.isNotEmpty()) {
                            listState.animateScrollToItem(outputLines.size - 1)
                        }
                    }
                }
                is RustEvent.TerminalClosed -> {
                    if (event.session_id == sessionId) {
                        connected = false
                        connecting = false
                        TerminalSessionManager.setConnectedBySession(sessionId, false)
                        outputLines = outputLines + "\n[连接已关闭]"
                        TerminalSessionManager.updateOutputBySession(sessionId, outputLines)
                    }
                }
                is RustEvent.TerminalError -> {
                    if (event.session_id == sessionId) {
                        errorMsg = event.error
                        connecting = false
                        connected = false
                        TerminalSessionManager.setConnectedBySession(sessionId, false)
                        outputLines = outputLines + "\n[错误: ${event.error}]"
                        TerminalSessionManager.updateOutputBySession(sessionId, outputLines)
                    }
                }
                else -> {}
            }
        }
    }

    // Resize terminal when orientation changes or keyboard shows/hides.
    // We approximate cols/rows from the screen dimensions and a fixed cell
    // size. The exact values aren't critical — the remote shell just needs
    // to know roughly how many columns/rows fit so it can redraw.
    val configuration = androidx.compose.ui.platform.LocalConfiguration.current
    LaunchedEffect(configuration.orientation, configuration.screenWidthDp, configuration.screenHeightDp, connected) {
        if (connected) {
            // Approximate cell size: 8dp wide x 16dp tall (monospace)
            val approxCols = (configuration.screenWidthDp / 8).coerceAtLeast(20)
            val approxRows = (configuration.screenHeightDp / 16).coerceAtLeast(10)
            repo.resizeTerminal(sessionId, approxCols, approxRows)
        }
    }

    // Open terminal session on screen entry (only if not already connected)
    LaunchedEffect(serverId, sessionId) {
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
                // Open PTY terminal (80x24 default)
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

    // Don't close terminal on dispose — keep it running in background for reuse

    // Detect when this server has >1 active terminals (set hintShown flag).
    //   Separate from the snackbar display effect so the snackbar coroutine
    //   isn't cancelled when hintShown flips to true.
    LaunchedEffect(connected) {
        if (connected && !hintShown) {
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

    val terminalBg = Color(0xFF1E1E2E)
    val terminalFg = Color(0xFFCDD6F4)
    val terminalGreen = Color(0xFFA6E3A1)
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
        val onKeyLambda = { key: String -> if (connected) repo.writeTerminal(sessionId, key) }
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
                    .background(terminalBg)
                    .padding(horizontal = 12.dp, vertical = 4.dp),
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
            } else if (errorMsg != null && outputLines.isEmpty()) {
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
            } else {
                SelectionContainer {
                    LazyColumn(
                        state = listState,
                        modifier = Modifier.fillMaxSize(),
                    ) {
                        itemsIndexed(outputLines) { index, line ->
                            val isLast = index == outputLines.lastIndex
                            // Append \n to non-last lines so copy preserves
                            //   line breaks across the selection.
                            val text = if (isLast) line else line + "\n"
                            if (isLast && connected) {
                                TerminalLineWithCursor(text, terminalFg, cursorCol)
                            } else {
                                Text(
                                    text,
                                    color = terminalFg,
                                    fontSize = 13.sp,
                                    lineHeight = 18.sp,
                                    modifier = Modifier.fillMaxWidth(),
                                )
                            }
                        }
                    }
                }
            }

            // Session name — glassmorphism card top-right, click for actions
            GlassChip(
                text = title,
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .padding(top = 4.dp, end = 4.dp)
                    .clickable { showSheet = true },
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
            if (!connected && !connecting && outputLines.isNotEmpty()) {
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
                .background(terminalBg)
                .padding(horizontal = 12.dp, vertical = 4.dp),
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
            } else if (errorMsg != null && outputLines.isEmpty()) {
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
            } else {
                SelectionContainer {
                    LazyColumn(
                        state = listState,
                        modifier = Modifier.fillMaxSize(),
                    ) {
                        itemsIndexed(outputLines) { index, line ->
                            val isLast = index == outputLines.lastIndex
                            // Append \n to non-last lines so copy preserves
                            //   line breaks across the selection.
                            val text = if (isLast) line else line + "\n"
                            if (isLast && connected) {
                                TerminalLineWithCursor(text, terminalFg, cursorCol)
                            } else {
                                Text(
                                    text,
                                    color = terminalFg,
                                    fontSize = 13.sp,
                                    lineHeight = 18.sp,
                                    modifier = Modifier.fillMaxWidth(),
                                )
                            }
                        }
                    }
                }
            }

            // Session name — glassmorphism card top-right, click for actions
            GlassChip(
                text = title,
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .padding(top = 4.dp, end = 4.dp)
                    .clickable { showSheet = true },
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
            if (!connected && !connecting && outputLines.isNotEmpty()) {
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
                    if (connected) repo.writeTerminal(sessionId, key)
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
    // Hidden BasicTextField that triggers the system IME. Always present so
    //   focus state survives mode toggles. In system-IME mode it requests
    //   focus; in custom-keyboard mode it clears focus.
    val systemInput = remember { mutableStateOf("") }
    val focusRequester = remember { FocusRequester() }
    if (useSystemKeyboard) {
        BasicTextField(
            value = systemInput.value,
            onValueChange = { newValue ->
                val added = newValue.substring(systemInput.value.length)
                if (added.isNotEmpty() && connected) {
                    repo.writeTerminal(sessionId, added)
                }
                systemInput.value = ""
            },
            modifier = Modifier
                .size(1.dp)
                .focusRequester(focusRequester),
            keyboardOptions = KeyboardOptions(),
        )
        LaunchedEffect(Unit) {
            focusRequester.requestFocus()
        }
    }

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
                    navController.navigate("terminals/$sessionId")
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
                        RustRepository.closeTerminal(sessionId)
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
}

// === SECTION 1: Terminal line with blinking cursor ===

@Composable
private fun TerminalLineWithCursor(line: String, terminalFg: Color, cursorCol: Int) {
    var cursorVisible by remember { mutableStateOf(true) }
    LaunchedEffect(Unit) {
        while (true) {
            cursorVisible = !cursorVisible
            kotlinx.coroutines.delay(530)
        }
    }
    // Split the line at cursorCol and insert a blinking block cursor.
    //   line may have a trailing "\n" (added for copy); keep it after cursor.
    val before = line.take(cursorCol)
    val after = line.drop(cursorCol)
    Row(modifier = Modifier.fillMaxWidth()) {
        Text(
            before,
            color = terminalFg,
            fontSize = 13.sp,
            lineHeight = 18.sp,
        )
        Text(
            "▋",
            color = if (cursorVisible) terminalFg else Color.Transparent,
            fontSize = 13.sp,
            lineHeight = 18.sp,
        )
        Text(
            after,
            color = terminalFg,
            fontSize = 13.sp,
            lineHeight = 18.sp,
        )
    }
}

// === SECTION 2: Glassmorphism chip ===

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