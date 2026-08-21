package com.termfast.app.ui.screen

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectDragGesturesAfterLongPress
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
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.zIndex
import androidx.navigation.NavController
import android.widget.Toast
import com.termfast.app.data.PairingStore
import com.termfast.app.data.RemoteTunnelConfig
import com.termfast.app.data.RustEvent
import com.termfast.app.data.RustRepository
import com.termfast.app.data.RemoteTunnelManager
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull

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

/**
 * Await a RemoteTerminalOk event for [pid] within [timeoutMs].
 * Returns (terminal_id, name) from the OK payload, or null on timeout.
 * The caller is responsible for sending NEW_TERMINAL before calling this.
 */
internal suspend fun awaitNewTerminalOk(
    pid: String,
    timeoutMs: Long = 10000,
): Pair<Int, String>? {
    val okResponse = CompletableDeferred<Pair<Int, String>>()
    val eventJob = kotlinx.coroutines.GlobalScope.launch {
        RustRepository.events.collect { event ->
            if (event is RustEvent.RemoteTerminalOk && event.pairing_id == pid) {
                val json = try { org.json.JSONObject(event.payload) } catch (_: Exception) { null }
                val realTid = json?.optInt("terminal_id", event.terminal_id) ?: event.terminal_id
                val termName = json?.optString("name", "") ?: ""
                okResponse.complete(realTid to termName)
            }
        }
    }
    val result = withTimeoutOrNull(timeoutMs) { okResponse.await() }
    eventJob.cancel()
    return result
}

@Composable
fun TerminalsScreen(
    navController: NavController,
    focusSessionId: String? = null,
    focusServerId: String? = null,
) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val repo = remember { RustRepository }
    val scope = rememberCoroutineScope()
    var sessions by remember { mutableStateOf(TerminalSessionManager.getAllSessions()) }
    val servers by remember { mutableStateOf(repo.listServers().associateBy { it.id }) }
    val listState = rememberLazyListState()
    // Drag-to-reorder state
    var draggedSessionId by remember { mutableStateOf<String?>(null) }
    var dragOffsetY by remember { mutableStateOf(0f) }
    // Remote picker dialog state: when non-null, show the picker for this pairing.
    // Clicking a remote server group header sets this to open the picker.
    var showRemotePickerFor by remember { mutableStateOf<RemoteTunnelConfig?>(null) }

    // Pre-load pairing desktop names for remote terminal display
    val pairingNames by remember {
        mutableStateOf(PairingStore.getAllPairings().associate { it.pairingId to it.desktopName })
    }

    // Keep tunnels alive for pairings that have remote sessions in the list,
    // so the desktop can notify us when a terminal is closed.
    // On NOTIFY(list_changed), re-fetch LIST and remove sessions whose terminalId
    // is no longer present on the desktop.
    // When all remote sessions for a pairing are gone, stop its tunnel to free resources.
    LaunchedEffect(sessions.size) {
        val remotePids = sessions.mapNotNull { it.remotePairingId }.toSet()
        // Start tunnels for pairings that still have remote sessions
        for (pid in remotePids) {
            val pairing = PairingStore.getAllPairings().find { it.pairingId == pid } ?: continue
            val key = pairing.pairingKey.chunked(2)
                .map { it.toInt(16).toByte() }.toByteArray()
            val tm = TerminalSessionManager.getOrCreateTunnelManager(
                pid, key, pairing.relayUrl, pairing.pairingJwt,
                pairing.pairingRefreshToken,
            )
            val state = tm.transportState.value
            if (state is com.termfast.app.data.TunnelState.Disconnected ||
                state is com.termfast.app.data.TunnelState.Error) {
                tm.start()
            }
        }
        // Stop tunnels for pairings that no longer have any remote sessions
        TerminalSessionManager.stopTunnelsNotIn(remotePids)
    }

    fun refresh() {
        sessions = TerminalSessionManager.getAllSessions()
    }

    LaunchedEffect(Unit) {
        RustRepository.events.collect { event ->
            when (event) {
                is RustEvent.RemoteTunnelReady -> {
                    // Global collector (TerminalSessionManager) handles session sync
                    // and sendListRequest. Just refresh the UI snapshot.
                    refresh()
                }
                is RustEvent.RemoteTerminalNotify -> {
                    val pid = event.pairing_id
                    val tm = TerminalSessionManager.getTunnelManager(pid)
                    if (tm != null && tm.protocolReady.value) {
                        tm.sendListRequest()
                    }
                }
                is RustEvent.RemoteTerminalList -> {
                    // Global collector already removed stale sessions.
                    // Refresh the UI snapshot to reflect the changes.
                    refresh()
                }
                else -> {}
            }
        }
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
                        // Server name — clickable to create new terminal:
                        // - remote: opens RemoteTerminalPickerDialog for this pairing
                        // - local SSH: creates a new session and navigates
                        Text(
                            serverName,
                            style = MaterialTheme.typography.labelLarge,
                            fontWeight = FontWeight.SemiBold,
                            color = MaterialTheme.colorScheme.primary,
                            modifier = Modifier.clickable {
                                if (serverId.startsWith("remote:")) {
                                    val pid = serverId.removePrefix("remote:")
                                    val pairing = PairingStore.getPairing(pid)
                                    if (pairing != null) {
                                        showRemotePickerFor = pairing
                                    } else {
                                        Toast.makeText(context, "未找到配对信息", Toast.LENGTH_SHORT).show()
                                    }
                                } else {
                                    // Local SSH: create new session and navigate
                                    val newSessionId = TerminalSessionManager.getOrCreateSession(serverId)
                                    navController.navigate("terminal/$serverId/$newSessionId")
                                }
                            },
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
                    }
                }
                // Terminal cards for this server
                items(serverSessions, key = { it.sessionId }) { session ->
                    val isDragging = draggedSessionId == session.sessionId
                    // Drag modifier stays on the outer Box — never changes between drag/non-drag
                    val dragModifier = Modifier
                        .fillMaxWidth()
                        .then(if (!isDragging) Modifier.animateItem() else Modifier)
                        .zIndex(if (isDragging) 1f else 0f)
                        .graphicsLayer {
                            if (isDragging) {
                                translationY = dragOffsetY
                                shadowElevation = 8f
                                alpha = 0.9f
                            }
                        }
                        .pointerInput(session.sessionId) {
                            detectDragGesturesAfterLongPress(
                                onDragStart = {
                                    draggedSessionId = session.sessionId
                                    dragOffsetY = 0f
                                },
                                onDragEnd = {
                                    // Persist the final order to TerminalSessionManager
                                    val groupSessionIds = serverSessions.map { it.sessionId }
                                    TerminalSessionManager.reorderSessions(groupSessionIds)
                                    draggedSessionId = null
                                    dragOffsetY = 0f
                                },
                                onDragCancel = {
                                    // Revert: reload from manager
                                    refresh()
                                    draggedSessionId = null
                                    dragOffsetY = 0f
                                },
                                onDrag = { change, dragAmount ->
                                    change.consume()
                                    dragOffsetY += dragAmount.y
                                    // Live reorder: swap when dragged center crosses another item's center
                                    val layoutInfo = listState.layoutInfo
                                    val draggedInfo = layoutInfo.visibleItemsInfo.find { it.key == session.sessionId }
                                    if (draggedInfo != null) {
                                        val draggedCenter = draggedInfo.offset + draggedInfo.size / 2 + dragOffsetY.toInt()
                                        // Only target sessions in the same server group
                                        val groupSessionIds = serverSessions.map { it.sessionId }.toSet()
                                        val target = layoutInfo.visibleItemsInfo.firstOrNull { vi ->
                                            vi.key != session.sessionId &&
                                            vi.key in groupSessionIds &&
                                            draggedCenter >= vi.offset &&
                                            draggedCenter < vi.offset + vi.size
                                        }
                                        if (target != null && target.key != session.sessionId) {
                                            // Swap in sessions state for live reorder
                                            val fromIdx = sessions.indexOfFirst { it.sessionId == session.sessionId }
                                            val toIdx = sessions.indexOfFirst { it.sessionId == target.key }
                                            if (fromIdx != -1 && toIdx != -1) {
                                                // Adjust dragOffsetY so the card stays under the finger
                                                val offsetDiff = target.offset - draggedInfo.offset
                                                dragOffsetY -= offsetDiff
                                                sessions = sessions.toMutableList().also {
                                                    val moved = it.removeAt(fromIdx)
                                                    it.add(toIdx, moved)
                                                }
                                            }
                                        }
                                    }
                                },
                            )
                        }
                    Box(modifier = dragModifier) {
                    TerminalCard(
                        isDragging = isDragging,
                        session = session,
                        serverName = if (serverId.startsWith("remote:")) {
                            val pid = serverId.removePrefix("remote:")
                            pairingNames[pid]?.ifBlank { null } ?: "远程终端"
                        } else {
                            servers[serverId]?.name?.ifBlank { servers[serverId]?.ssh?.host ?: "" } ?: ""
                        },
                        isFocused = session.sessionId == focusSessionId,
                        onClick = {
                            if (draggedSessionId == null) {
                                if (session.serverId.startsWith("remote:") && session.remotePairingId != null && session.remoteTerminalId != null) {
                                    val pid = session.remotePairingId!!
                                    val tid = session.remoteTerminalId!!
                                    val encodedName = java.net.URLEncoder.encode(session.name.ifBlank { "Terminal" }, "UTF-8")
                                    navController.navigate("remote_terminal/$pid/$tid/$encodedName")
                                } else {
                                    navController.navigate("terminal/${session.serverId}/${session.sessionId}")
                                }
                            }
                        },
                        onDisconnect = {
                            scope.launch {
                                TerminalSessionManager.removeSession(session.sessionId)
                                refresh()
                            }
                        },
                        onCloseTerminal = {
                            scope.launch {
                                TerminalSessionManager.closeTerminalSession(session.sessionId)
                                refresh()
                            }
                        },
                    )
                    }
                }
            }
        }
    }

    // Remote terminal picker dialog — opened by clicking a remote server group header.
    // Uses initialPairing to skip the desktop list and go straight to the terminal list.
    showRemotePickerFor?.let { pairing ->
        RemoteTerminalPickerDialog(
            visible = true,
            initialPairing = pairing,
            onTerminalClick = { terminalId, name, pairingId ->
                showRemotePickerFor = null
                val encodedName = java.net.URLEncoder.encode(name, "UTF-8")
                navController.navigate("remote_terminal/$pairingId/$terminalId/$encodedName")
            },
            onDismiss = { showRemotePickerFor = null },
        )
    }
}

@OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
@Composable
private fun TerminalCard(
    isDragging: Boolean = false,
    session: TerminalSessionManager.SessionState,
    serverName: String,
    isFocused: Boolean,
    onClick: () -> Unit,
    onDisconnect: () -> Unit,
    onCloseTerminal: () -> Unit,
) {
    var showDisconnectDialog by remember { mutableStateOf(false) }
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

    // Swipe-to-dismiss: left swipe reveals "关闭终端会话" action
    val swipeState = rememberSwipeToDismissBoxState(
        confirmValueChange = { value ->
            if (value == SwipeToDismissBoxValue.EndToStart) {
                showCloseDialog = true
            }
            // Always return false so the card snaps back; the dialog handles the action
            false
        },
        positionalThreshold = { distance -> distance * 0.4f },
    )

    // Card content shared between drag and non-drag modes
    val cardContent: @Composable () -> Unit = {
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
                        if (session.connected) {
                            // Online: small green dot
                            Box(
                                modifier = Modifier
                                    .size(6.dp)
                                    .clip(RoundedCornerShape(3.dp))
                                    .background(MaterialTheme.colorScheme.primary)
                            )
                        } else {
                            // Offline: prominent red label
                            Surface(
                                shape = RoundedCornerShape(4.dp),
                                color = MaterialTheme.colorScheme.errorContainer,
                            ) {
                                Text(
                                    "离线",
                                    modifier = Modifier.padding(horizontal = 6.dp, vertical = 1.dp),
                                    style = MaterialTheme.typography.labelSmall,
                                    fontSize = 10.sp,
                                    fontWeight = FontWeight.Medium,
                                    color = MaterialTheme.colorScheme.error,
                                )
                            }
                        }
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
                // X button — close terminal card (terminal stays alive on desktop)
                IconButton(
                    onClick = { showDisconnectDialog = true },
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
    }

    // When dragging, render card directly (no SwipeToDismissBox → no red background)
    if (isDragging) {
        cardContent()
    } else {
        SwipeToDismissBox(
            state = swipeState,
            backgroundContent = {
                // Red background with "关闭终端会话" label, revealed on left swipe
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .clip(RoundedCornerShape(12.dp))
                        .background(MaterialTheme.colorScheme.errorContainer)
                        .padding(horizontal = 20.dp),
                    contentAlignment = Alignment.CenterEnd,
                ) {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        Icon(
                            Icons.Filled.Close,
                            contentDescription = null,
                            modifier = Modifier.size(18.dp),
                            tint = MaterialTheme.colorScheme.error,
                        )
                        Text(
                            "关闭终端会话",
                            color = MaterialTheme.colorScheme.error,
                            fontWeight = FontWeight.Medium,
                            fontSize = 14.sp,
                        )
                    }
                }
            },
            enableDismissFromStartToEnd = false,
            modifier = Modifier.fillMaxWidth(),
        ) {
            cardContent()
        }
    }

    // Disconnect confirmation dialog (X button)
    if (showDisconnectDialog) {
        AlertDialog(
            onDismissRequest = { showDisconnectDialog = false },
            title = { Text("关闭终端") },
            text = {
                val msg = if (session.remotePairingId != null) {
                    "确定要关闭「${session.name.ifBlank { "终端" }}」吗？\n终端会话将保留在桌面端，可稍后从终端列表重新打开。"
                } else {
                    "确定要关闭「${session.name.ifBlank { "终端" }}」吗？"
                }
                Text(msg)
            },
            confirmButton = {
                TextButton(onClick = {
                    showDisconnectDialog = false
                    onDisconnect()
                }) { Text("关闭", color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = {
                TextButton(onClick = { showDisconnectDialog = false }) { Text("取消") }
            },
        )
    }

    // Close terminal confirmation dialog (left swipe)
    if (showCloseDialog) {
        AlertDialog(
            onDismissRequest = { showCloseDialog = false },
            title = { Text("关闭终端会话") },
            text = {
                val msg = if (session.remotePairingId != null) {
                    "确定要关闭「${session.name.ifBlank { "终端" }}」吗？\n终端中的进程将被终止，未保存的输出将丢失。"
                } else {
                    "确定要关闭「${session.name.ifBlank { "终端" }}」吗？\n终端中的进程将被终止，未保存的输出将丢失。"
                }
                Text(msg)
            },
            confirmButton = {
                TextButton(onClick = {
                    showCloseDialog = false
                    onCloseTerminal()
                }) { Text("关闭", color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = {
                TextButton(onClick = { showCloseDialog = false }) { Text("取消") }
            },
        )
    }
}
