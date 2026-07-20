package com.termfast.app.ui.screen

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Keyboard
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Send
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.blur
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.navigation.NavController
import com.termfast.app.data.RustEvent
import com.termfast.app.data.RustRepository
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.util.UUID

@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun TerminalScreen(navController: NavController, serverId: String, existingSessionId: String? = null) {
    val repo = remember { RustRepository }
    val scope = rememberCoroutineScope()
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

    // Refresh session state periodically
    LaunchedEffect(sessionId) {
        while (true) {
            kotlinx.coroutines.delay(500)
            sessionState = TerminalSessionManager.getSessions(serverId).firstOrNull { it.sessionId == sessionId }
        }
    }

    // Terminal output lines — restore from cache if available
    var outputLines by remember(sessionId) { mutableStateOf(TerminalSessionManager.getOutputBySession(sessionId)) }
    var connected by remember(sessionId) {
        val s = TerminalSessionManager.getSessions(serverId).firstOrNull { it.sessionId == sessionId }
        mutableStateOf(s?.connected ?: false)
    }
    var connecting by remember(sessionId) { mutableStateOf(!(connected)) }
    var errorMsg by remember { mutableStateOf<String?>(null) }
    var inputText by remember { mutableStateOf("") }

    // Collect terminal events
    LaunchedEffect(sessionId) {
        RustRepository.events.collect { event ->
            when (event) {
                is RustEvent.TerminalData -> {
                    if (event.session_id == sessionId) {
                        // Global collector already processes data; just read updated state
                        outputLines = TerminalSessionManager.getOutputBySession(sessionId)
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

    val terminalBg = Color(0xFF1E1E2E)
    val terminalFg = Color(0xFFCDD6F4)
    val terminalGreen = Color(0xFFA6E3A1)

    // Track soft keyboard visibility
    val imeVisible = WindowInsets.isImeVisible
    var showKeyboard by remember { mutableStateOf(false) }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(terminalBg)
            .statusBarsPadding()
    ) {
        // Terminal output area — fills available space
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
                LazyColumn(
                    state = listState,
                    modifier = Modifier.fillMaxSize(),
                ) {
                    items(outputLines) { line ->
                        Text(
                            line,
                            color = terminalFg,
                            fontSize = 13.sp,
                            lineHeight = 18.sp,
                            modifier = Modifier.fillMaxWidth(),
                        )
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
        }

        // Disconnected banner — shown when connection was lost
        if (!connected && !connecting && outputLines.isNotEmpty()) {
            Row(
                modifier = Modifier
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

        // Input bar (only when connected)
        if (connected) {
            TerminalInputBar(
                text = inputText,
                onTextChange = { inputText = it },
                onSend = {
                    if (inputText.isNotEmpty()) {
                        val cmd = inputText + "\r"
                        repo.writeTerminal(sessionId, cmd)
                        inputText = ""
                    }
                },
                terminalBg = terminalBg,
                terminalFg = terminalFg,
            )
        }

        // Bottom auxiliary key bar
        TerminalKeyBar(
            onKey = { key ->
                if (connected) repo.writeTerminal(sessionId, key)
            },
            onToggleKeyboard = { showKeyboard = !showKeyboard },
            keyboardVisible = imeVisible || showKeyboard,
            terminalBg = terminalBg,
            terminalFg = terminalFg,
        )
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

@Composable
private fun TerminalInputBar(
    text: String,
    onTextChange: (String) -> Unit,
    onSend: () -> Unit,
    terminalBg: Color,
    terminalFg: Color,
) {
    val inputBg = Color(0xFF181825)
    val inputBorder = Color(0xFF45475A)
    val accentColor = Color(0xFF89B4FA)

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(inputBg)
            .padding(horizontal = 12.dp, vertical = 8.dp),
        verticalAlignment = Alignment.Bottom,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        // Prompt symbol
        Text(
            "$ ",
            color = accentColor,
            fontSize = 14.sp,
            fontWeight = FontWeight.Bold,
            modifier = Modifier.padding(bottom = 12.dp),
        )
        // Input field — multiline, max 5 lines, scrollable
        OutlinedTextField(
            value = text,
            onValueChange = onTextChange,
            modifier = Modifier
                .weight(1f)
                .heightIn(max = 100.dp)
                .verticalScroll(rememberScrollState()),
            placeholder = {
                Text(
                    "输入命令...",
                    color = terminalFg.copy(alpha = 0.4f),
                    fontSize = 14.sp,
                )
            },
            singleLine = false,
            maxLines = 5,
            shape = RoundedCornerShape(8.dp),
            colors = OutlinedTextFieldDefaults.colors(
                focusedTextColor = terminalFg,
                unfocusedTextColor = terminalFg,
                focusedBorderColor = accentColor,
                unfocusedBorderColor = inputBorder,
                cursorColor = accentColor,
                focusedContainerColor = Color.Transparent,
                unfocusedContainerColor = Color.Transparent,
            ),
            textStyle = androidx.compose.ui.text.TextStyle(
                fontSize = 14.sp,
            ),
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Default),
            trailingIcon = null,
        )
        // Send button
        IconButton(
            onClick = onSend,
            enabled = text.isNotEmpty(),
            modifier = Modifier
                .size(40.dp)
                .clip(RoundedCornerShape(8.dp))
                .background(if (text.isNotEmpty()) accentColor else inputBorder)
                .align(Alignment.Bottom),
        ) {
            Icon(
                Icons.Filled.Send,
                contentDescription = "发送",
                tint = if (text.isNotEmpty()) inputBg else terminalFg.copy(alpha = 0.3f),
                modifier = Modifier.size(18.dp),
            )
        }
    }
}

// === SECTION 2: Bottom auxiliary key bar ===

@Composable
private fun TerminalKeyBar(
    onKey: (String) -> Unit,
    onToggleKeyboard: () -> Unit,
    keyboardVisible: Boolean,
    terminalBg: Color,
    terminalFg: Color,
) {
    val keyBg = Color(0xFF181825)
    val keyBorder = Color(0xFF45475A)
    val accentColor = Color(0xFF89B4FA)

    val auxKeys = listOf(
        "ESC" to "\u001B",
        "TAB" to "\t",
        "↑" to "\u001B[A",
        "↓" to "\u001B[B",
        "←" to "\u001B[D",
        "→" to "\u001B[C",
        "⏎" to "\r",
        "⌫" to "\u007F",
    )

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(keyBg)
            .padding(horizontal = 6.dp, vertical = 6.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // Auxiliary keys
        auxKeys.forEach { (label, value) ->
            AuxKeyButton(
                label = label,
                onClick = { value?.let { onKey(it) } },
                bg = keyBg,
                border = keyBorder,
                fg = terminalFg,
            )
        }
        Spacer(Modifier.weight(1f))
        // Keyboard toggle button
        IconButton(
            onClick = onToggleKeyboard,
            modifier = Modifier
                .size(36.dp)
                .clip(RoundedCornerShape(8.dp))
                .background(if (keyboardVisible) accentColor else keyBorder),
        ) {
            Icon(
                Icons.Filled.Keyboard,
                contentDescription = "键盘",
                tint = if (keyboardVisible) keyBg else terminalFg.copy(alpha = 0.7f),
                modifier = Modifier.size(18.dp),
            )
        }
    }
}

@Composable
private fun AuxKeyButton(
    label: String,
    onClick: () -> Unit,
    bg: Color,
    border: Color,
    fg: Color,
) {
    Box(
        modifier = Modifier
            .clip(RoundedCornerShape(6.dp))
            .background(border.copy(alpha = 0.5f))
            .clickable(onClick = onClick)
            .padding(horizontal = 10.dp, vertical = 6.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            label,
            color = fg.copy(alpha = 0.8f),
            fontSize = 12.sp,
            fontWeight = FontWeight.Medium,
        )
    }
}

// === SECTION 3: Glassmorphism chip ===

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