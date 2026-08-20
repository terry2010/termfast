package com.termfast.app.ui.screen

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Terminal
import androidx.compose.material.icons.filled.Devices
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.navigation.NavController
import com.termfast.app.data.PairingStore
import com.termfast.app.data.RustRepository
import com.termfast.app.data.RemoteTunnelManager
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * Wait for a RemoteTunnelManager's protocol to become ready (HELLO exchange done).
 * Polls every 200ms up to [timeoutMs]. Returns true if ready, false on timeout.
 */
private suspend fun waitForProtocolReady(tm: RemoteTunnelManager, timeoutMs: Long = 10000): Boolean {
    val deadline = System.currentTimeMillis() + timeoutMs
    while (System.currentTimeMillis() < deadline) {
        if (tm.protocolReady.value) return true
        delay(200)
    }
    return tm.protocolReady.value
}

@Composable
fun TerminalsScreen(
    navController: NavController,
    focusSessionId: String? = null,
    focusServerId: String? = null,
) {
    val repo = remember { RustRepository }
    val scope = rememberCoroutineScope()
    var sessions by remember { mutableStateOf(TerminalSessionManager.getAllSessions()) }
    val servers by remember { mutableStateOf(repo.listServers().associateBy { it.id }) }
    val listState = rememberLazyListState()

    // Pre-load pairing desktop names for remote terminal display
    val pairingNames by remember {
        mutableStateOf(PairingStore.getAllPairings().associate { it.pairingId to it.desktopName })
    }

    // Group sessions by serverId, with the focused server first (if any)
    val grouped = sessions.groupBy { it.serverId }.let { map ->
        if (focusServerId != null) {
            // Move focused server's group to the top
            val focused = map[focusServerId]
            if (focused != null) {
                linkedMapOf(focusServerId to focused) + (map - focusServerId)
            } else map
        } else map
    }

    // Scroll to focused session or server on first composition
    LaunchedEffect(focusSessionId, focusServerId, sessions.size) {
        if (focusSessionId != null) {
            // Find the flat index of the focused session
            var flatIndex = 0
            var foundIndex: Int? = null
            grouped.forEach { (_, serverSessions) ->
                flatIndex++ // header
                serverSessions.forEach { s ->
                    if (s.sessionId == focusSessionId) foundIndex = flatIndex
                    flatIndex++
                }
            }
            if (foundIndex != null) {
                listState.animateScrollToItem(foundIndex)
            }
        } else if (focusServerId != null) {
            // Scroll to the focused server's group header (index 0, since we
            //   moved it to the top of the list).
            listState.animateScrollToItem(0)
        }
    }

    fun refresh() {
        sessions = TerminalSessionManager.getAllSessions()
    }

    Scaffold { inner ->
        if (sessions.isEmpty()) {
            // Empty state
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(inner)
                    .padding(32.dp),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Icon(
                    Icons.Filled.Terminal,
                    contentDescription = null,
                    modifier = Modifier.size(64.dp),
                    tint = MaterialTheme.colorScheme.outlineVariant,
                )
                Spacer(Modifier.height(16.dp))
                Text(
                    "还没有打开的终端",
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(8.dp))
                Text(
                    "从服务器列表点击「SSH终端」按钮打开终端",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.outline,
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                )
                Spacer(Modifier.height(24.dp))
                Button(onClick = {
                    navController.navigate("servers") {
                        popUpTo("servers") { inclusive = false }
                    }
                }) {
                    Icon(Icons.Filled.Add, contentDescription = null, modifier = Modifier.size(18.dp))
                    Spacer(Modifier.width(8.dp))
                    Text("去服务器列表")
                }
                // Remote terminals entry (only if paired)
                if (com.termfast.app.data.PairingStore.getAllPairings().isNotEmpty()) {
                    Spacer(Modifier.height(16.dp))
                    var showRemotePicker by remember { mutableStateOf(false) }
                    OutlinedButton(onClick = { showRemotePicker = true }) {
                        Icon(Icons.Filled.Devices, contentDescription = null, modifier = Modifier.size(18.dp))
                        Spacer(Modifier.width(8.dp))
                        Text("远程终端")
                    }
                    RemoteTerminalPickerDialog(
                        visible = showRemotePicker,
                        onTerminalClick = { terminalId, name, pairingId ->
                            showRemotePicker = false
                            val encodedName = java.net.URLEncoder.encode(name, "UTF-8")
                            navController.navigate("remote_terminal/$pairingId/$terminalId/$encodedName")
                        },
                        onDismiss = { showRemotePicker = false },
                    )
                }
            }
            return@Scaffold
        }

        LazyColumn(
            state = listState,
            modifier = Modifier
                .fillMaxSize()
                .padding(inner)
                .padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
            contentPadding = PaddingValues(vertical = 16.dp),
        ) {
            grouped.forEach { (serverId, serverSessions) ->
                // Server group header
                item(key = "header_$serverId") {
                    val serverName = if (serverId.startsWith("remote:")) {
                        val pid = serverId.removePrefix("remote:")
                        pairingNames[pid]?.ifBlank { null } ?: "远程终端"
                    } else {
                        servers[serverId]?.name?.ifBlank { servers[serverId]?.ssh?.host ?: serverId }
                            ?: serverId
                    }
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(top = 8.dp, bottom = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Text(
                            serverName,
                            style = MaterialTheme.typography.labelLarge,
                            fontWeight = FontWeight.SemiBold,
                            color = MaterialTheme.colorScheme.primary,
                        )
                        Spacer(
                            Modifier
                                .weight(1f)
                                .height(1.dp)
                                .background(MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.3f))
                        )
                        Text(
                            "${serverSessions.size}",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.outline,
                        )
                        // + button: create new terminal on this server
                        IconButton(
                            onClick = {
                                if (serverId.startsWith("remote:")) {
                                    // Remote terminal: send NEW_TERMINAL frame via tunnel
                                    val pid = serverId.removePrefix("remote:")
                                    val tm = TerminalSessionManager.getTunnelManager(pid)
                                    if (tm != null && tm.protocolReady.value) {
                                        tm.sendNewTerminal()
                                    } else {
                                        // Tunnel not ready — need to start it first
                                        val pairing = PairingStore.getPairing(pid)
                                        if (pairing != null) {
                                            scope.launch {
                                                val key = pairing.pairingKey.chunked(2)
                                                    .map { it.toInt(16).toByte() }.toByteArray()
                                                val newTm = TerminalSessionManager.getOrCreateTunnelManager(
                                                    pid, key, pairing.relayUrl, pairing.pairingJwt,
                                                    pairing.pairingRefreshToken,
                                                )
                                                newTm.start()
                                                // Wait for protocol ready, then send NEW_TERMINAL
                                                val ready = waitForProtocolReady(newTm, timeoutMs = 10000)
                                                if (ready) {
                                                    newTm.sendNewTerminal()
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    // Local SSH: create new session and navigate
                                    val newSessionId = TerminalSessionManager.getOrCreateSession(serverId)
                                    navController.navigate("terminal/$serverId/$newSessionId")
                                }
                            },
                            modifier = Modifier.size(28.dp),
                        ) {
                            Icon(
                                Icons.Filled.Add,
                                contentDescription = "新建终端",
                                modifier = Modifier.size(20.dp),
                                tint = MaterialTheme.colorScheme.primary,
                            )
                        }
                    }
                }
                // Terminal cards for this server
                items(serverSessions, key = { it.sessionId }) { session ->
                    TerminalCard(
                        session = session,
                        serverName = if (serverId.startsWith("remote:")) {
                            val pid = serverId.removePrefix("remote:")
                            pairingNames[pid]?.ifBlank { null } ?: "远程终端"
                        } else {
                            servers[serverId]?.name?.ifBlank { servers[serverId]?.ssh?.host ?: "" } ?: ""
                        },
                        isFocused = session.sessionId == focusSessionId,
                        onClick = {
                            navController.navigate("terminal/${session.serverId}/${session.sessionId}")
                        },
                        onClose = {
                            scope.launch {
                                TerminalSessionManager.closeSessionBySessionId(session.sessionId)
                                refresh()
                            }
                        },
                    )
                }
            }
        }
    }
}

@Composable
private fun TerminalCard(
    session: TerminalSessionManager.SessionState,
    serverName: String,
    isFocused: Boolean,
    onClick: () -> Unit,
    onClose: () -> Unit,
) {
    var showCloseDialog by remember { mutableStateOf(false) }
    val borderColor = if (isFocused) MaterialTheme.colorScheme.primary
                     else MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.3f)
    val bgColor = if (isFocused) MaterialTheme.colorScheme.primaryContainer
                  else MaterialTheme.colorScheme.surface

    // Preview: last few non-empty lines from preview cache
    val previewLines = session.previewCache.lines().filter { it.isNotBlank() }.takeLast(3)
    val preview = if (previewLines.isNotEmpty()) previewLines.joinToString(" ⏵ ")
                  else "（无输出）"
    val timeStr = remember(session.createdAt) {
        val diff = System.currentTimeMillis() - session.createdAt
        val mins = diff / 60000
        when {
            mins < 1 -> "刚刚"
            mins < 60 -> "${mins}分钟前"
            mins < 1440 -> "${mins / 60}小时前"
            else -> "${mins / 1440}天前"
        }
    }

    Card(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick),
        shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(containerColor = bgColor),
        border = androidx.compose.foundation.BorderStroke(if (isFocused) 2.dp else 1.dp, borderColor),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(14.dp),
            verticalAlignment = Alignment.Top,
        ) {
            Icon(
                Icons.Filled.Terminal,
                contentDescription = null,
                modifier = Modifier
                    .size(20.dp)
                    .padding(top = 2.dp),
                tint = if (session.connected) MaterialTheme.colorScheme.primary
                       else MaterialTheme.colorScheme.outline,
            )
            Spacer(Modifier.width(12.dp))
            Column(modifier = Modifier.weight(1f)) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Text(
                        session.name.ifBlank { "终端" },
                        style = MaterialTheme.typography.titleSmall,
                        fontWeight = FontWeight.Medium,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    // Connection status dot
                    Box(
                        modifier = Modifier
                            .size(6.dp)
                            .clip(RoundedCornerShape(3.dp))
                            .background(
                                if (session.connected) MaterialTheme.colorScheme.primary
                                else MaterialTheme.colorScheme.outlineVariant
                            )
                    )
                }
                Spacer(Modifier.height(4.dp))
                Text(
                    preview,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    fontSize = 12.sp,
                )
                Spacer(Modifier.height(6.dp))
                Text(
                    "$serverName · $timeStr",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.outline,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            // Close button — opens confirmation dialog
            IconButton(
                onClick = { showCloseDialog = true },
                modifier = Modifier.size(32.dp),
            ) {
                Icon(
                    Icons.Filled.Close,
                    contentDescription = "关闭终端",
                    modifier = Modifier.size(18.dp),
                    tint = MaterialTheme.colorScheme.outline,
                )
            }
        }
    }

    // Close confirmation dialog
    if (showCloseDialog) {
        AlertDialog(
            onDismissRequest = { showCloseDialog = false },
            title = { Text("关闭终端") },
            text = { Text("确定要关闭「${session.name.ifBlank { "终端" }}」并断开连接吗？") },
            confirmButton = {
                TextButton(onClick = {
                    showCloseDialog = false
                    onClose()
                }) { Text("关闭", color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = {
                TextButton(onClick = { showCloseDialog = false }) { Text("取消") }
            },
        )
    }
}
