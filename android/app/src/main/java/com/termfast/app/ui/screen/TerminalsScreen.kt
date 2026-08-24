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
import androidx.compose.material.icons.filled.Computer
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material.icons.filled.ExpandLess
import androidx.compose.material.icons.filled.UnfoldMore
import androidx.compose.material.icons.filled.UnfoldLess
import androidx.compose.material.icons.filled.DragHandle
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

@OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
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
    // Drag-to-reorder state (terminal cards within a group)
    var draggedSessionId by remember { mutableStateOf<String?>(null) }
    var dragOffsetY by remember { mutableStateOf(0f) }
    // Drag-to-reorder state (top-level groups)
    var draggedTopKey by remember { mutableStateOf<String?>(null) }
    var topDragOffsetY by remember { mutableStateOf(0f) }
    var topReorderMode by remember { mutableStateOf(false) } // true after long-press activates reorder
    // Local top-level order during drag (avoids flicker from refresh())
    var localTopOrder by remember { mutableStateOf<List<String>?>(null) }
    // Collapse state: which top-level / sub-level groups are collapsed
    var collapsedTopKeys by remember { mutableStateOf(setOf<String>()) }
    var collapsedSubKeys by remember { mutableStateOf(setOf<String>()) }
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
                is RustEvent.RemoteTerminalError -> {
                    // Tunnel error / peer disconnected — sessions may have been
                    // marked as disconnected. Refresh to update the UI.
                    refresh()
                }
                else -> {}
            }
        }
    }

    // Three-level grouping:
    // Level 1: paired desktop (remotePairingId) or local SSH server (serverId)
    // Level 2 (remote only): desktop's server group (remoteServerId: "__local__" or SSH server ID)
    // Level 3: terminal cards
    data class TopGroup(
        val topKey: String,        // "remote:<pid>" or SSH serverId
        val topName: String,       // desktop name or SSH server name
        val isRemote: Boolean,
        val subGroups: Map<String, List<TerminalSessionManager.SessionState>>, // remoteServerId → sessions
    )

    val grouped: List<TopGroup> = remember(sessions, pairingNames, servers, localTopOrder) {
        val remoteSessions = sessions.filter { it.remotePairingId != null }
        val localSessions = sessions.filter { it.remotePairingId == null }

        val result = mutableListOf<TopGroup>()

        // Remote groups: group by pairingId (level 1), then by remoteServerId (level 2)
        val byPairing = remoteSessions.groupBy { it.remotePairingId!! }
        byPairing.forEach { (pid, pidSessions) ->
            val desktopName = pairingNames[pid]?.ifBlank { null } ?: "远程终端"
            val subGroups = pidSessions.groupBy { it.remoteServerId }
            result.add(TopGroup(
                topKey = "remote:$pid",
                topName = desktopName,
                isRemote = true,
                subGroups = subGroups,
            ))
        }

        // Local SSH groups: each serverId is its own top-level group
        val byServer = localSessions.groupBy { it.serverId }
        byServer.forEach { (sid, sidSessions) ->
            val serverName = servers[sid]?.name?.ifBlank { servers[sid]?.ssh?.host ?: sid } ?: sid
            result.add(TopGroup(
                topKey = sid,
                topName = serverName,
                isRemote = false,
                subGroups = mapOf(sid to sidSessions),
            ))
        }

        // Sort: use localTopOrder during drag, otherwise topLevelOrder from manager
        val orderMap = localTopOrder?.mapIndexed { idx, key -> key to idx }?.toMap()
        result.sortBy { orderMap?.get(it.topKey) ?: TerminalSessionManager.getTopLevelOrder(it.topKey) ?: Int.MAX_VALUE }

        // Move focused group to top if needed
        if (focusServerId != null) {
            val focused = result.find { it.topKey == focusServerId }
            if (focused != null) {
                result.remove(focused)
                result.add(0, focused)
            }
        }

        result
    }

    // Scroll to focused session or server on first composition
    LaunchedEffect(focusSessionId, focusServerId, sessions.size) {
        if (focusSessionId != null) {
            var flatIndex = 0
            var foundIndex: Int? = null
            grouped.forEach { topGroup ->
                flatIndex++ // top header
                topGroup.subGroups.forEach { (_, subSessions) ->
                    if (topGroup.isRemote) flatIndex++ // sub header (remote only)
                    subSessions.forEach { s ->
                        if (s.sessionId == focusSessionId) foundIndex = flatIndex
                        flatIndex++
                    }
                }
            }
            if (foundIndex != null) {
                listState.animateScrollToItem(foundIndex)
            }
        } else if (focusServerId != null) {
            listState.animateScrollToItem(0)
        }
    }

    val allTopCollapsed = grouped.isNotEmpty() && grouped.all { it.topKey in collapsedTopKeys }
    Scaffold(
        topBar = {
            androidx.compose.material3.TopAppBar(
                title = { Text("终端", fontSize = 16.sp) },
                actions = {
                    if (grouped.isNotEmpty()) {
                        // Expand/collapse all toggle
                        IconButton(onClick = {
                            if (allTopCollapsed) {
                                // Expand all
                                collapsedTopKeys = emptySet()
                                collapsedSubKeys = emptySet()
                            } else {
                                // Collapse all
                                collapsedTopKeys = grouped.map { it.topKey }.toSet()
                            }
                        }) {
                            Icon(
                                if (allTopCollapsed) Icons.Filled.UnfoldMore else Icons.Filled.UnfoldLess,
                                contentDescription = if (allTopCollapsed) "展开所有" else "收起所有",
                            )
                        }
                    }
                },
            )
        },
    ) { inner ->
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
                        onTerminalClick = { terminalId, name, pairingId, serverId, serverName ->
                            showRemotePicker = false
                            val encodedName = java.net.URLEncoder.encode(name, "UTF-8")
                            navController.navigate("remote_terminal/$pairingId/$terminalId/$encodedName/$serverId/$serverName")
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
            grouped.forEach { topGroup ->
                val isTopCollapsed = topGroup.topKey in collapsedTopKeys
                // === Level 1: Top-level header ===
                item(key = "top_${topGroup.topKey}") {
                    val isDraggingTop = draggedTopKey == topGroup.topKey
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(top = 16.dp, bottom = 4.dp)
                            .zIndex(if (isDraggingTop) 1f else 0f)
                            .graphicsLayer {
                                if (isDraggingTop) {
                                    translationY = topDragOffsetY
                                    shadowElevation = 8f
                                    shape = RoundedCornerShape(12.dp)
                                    alpha = 0.9f
                                }
                            }
                            .pointerInput(topGroup.topKey) {
                                detectDragGesturesAfterLongPress(
                                    onDragStart = {
                                        // Long-press: enter reorder mode, collapse all top groups
                                        draggedTopKey = topGroup.topKey
                                        topDragOffsetY = 0f
                                        topReorderMode = true
                                        collapsedTopKeys = grouped.map { it.topKey }.toSet()
                                        // Snapshot current order into localTopOrder
                                        localTopOrder = grouped.map { it.topKey }
                                    },
                                    onDragEnd = {
                                        // Persist local order to manager
                                        localTopOrder?.let { TerminalSessionManager.reorderTopLevels(it) }
                                        localTopOrder = null
                                        draggedTopKey = null
                                        topDragOffsetY = 0f
                                        topReorderMode = false
                                    },
                                    onDragCancel = {
                                        localTopOrder = null
                                        draggedTopKey = null
                                        topDragOffsetY = 0f
                                        topReorderMode = false
                                    },
                                    onDrag = { change, dragAmount ->
                                        change.consume()
                                        topDragOffsetY += dragAmount.y
                                        // Live reorder: swap top groups when dragged crosses target
                                        val layoutInfo = listState.layoutInfo
                                        val draggedInfo = layoutInfo.visibleItemsInfo.find { it.key == "top_${topGroup.topKey}" }
                                        if (draggedInfo != null) {
                                            val draggedCenter = draggedInfo.offset + draggedInfo.size / 2 + topDragOffsetY.toInt()
                                            val currentOrder = localTopOrder ?: return@detectDragGesturesAfterLongPress
                                            val topKeys = currentOrder.toSet()
                                            val target = layoutInfo.visibleItemsInfo.firstOrNull { vi ->
                                                val viKey = vi.key as? String ?: return@firstOrNull false
                                                if (!viKey.startsWith("top_")) return@firstOrNull false
                                                val tKey = viKey.removePrefix("top_")
                                                if (tKey !in topKeys || tKey == topGroup.topKey) return@firstOrNull false
                                                val targetMid = vi.offset + vi.size / 2
                                                if (draggedInfo.offset > vi.offset) draggedCenter < targetMid else draggedCenter > targetMid
                                            }
                                            if (target != null) {
                                                val targetKey = (target.key as String).removePrefix("top_")
                                                val fromIdx = currentOrder.indexOfFirst { it == topGroup.topKey }
                                                val toIdx = currentOrder.indexOfFirst { it == targetKey }
                                                if (fromIdx != -1 && toIdx != -1 && fromIdx != toIdx) {
                                                    // Adjust offset so the header stays under the finger
                                                    val offsetDiff = target.offset - draggedInfo.offset
                                                    topDragOffsetY -= offsetDiff
                                                    // Update local order only (no refresh, no flicker)
                                                    localTopOrder = currentOrder.toMutableList().also {
                                                        val moved = it.removeAt(fromIdx)
                                                        it.add(toIdx, moved)
                                                    }
                                                }
                                            }
                                        }
                                    },
                                )
                            },
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        // Drag handle (visible in reorder mode)
                        if (topReorderMode) {
                            Icon(
                                Icons.Filled.DragHandle,
                                contentDescription = null,
                                modifier = Modifier.size(18.dp),
                                tint = MaterialTheme.colorScheme.outline,
                            )
                        }
                        // Collapse/expand arrow — clickable to toggle
                        Icon(
                            if (isTopCollapsed) Icons.Filled.ExpandMore else Icons.Filled.ExpandLess,
                            contentDescription = if (isTopCollapsed) "展开" else "收起",
                            modifier = Modifier
                                .size(20.dp)
                                .clickable {
                                    collapsedTopKeys = if (isTopCollapsed) {
                                        collapsedTopKeys - topGroup.topKey
                                    } else {
                                        collapsedTopKeys + topGroup.topKey
                                    }
                                },
                            tint = MaterialTheme.colorScheme.primary,
                        )
                        // Title text — clickable to toggle collapse
                        Text(
                            topGroup.topName,
                            fontSize = 14.sp,
                            fontWeight = FontWeight.Bold,
                            color = MaterialTheme.colorScheme.primary,
                            modifier = Modifier
                                .weight(1f)
                                .clickable {
                                    collapsedTopKeys = if (isTopCollapsed) {
                                        collapsedTopKeys - topGroup.topKey
                                    } else {
                                        collapsedTopKeys + topGroup.topKey
                                    }
                                },
                        )
                        // Terminal count
                        val totalCount = topGroup.subGroups.values.sumOf { it.size }
                        Text(
                            "$totalCount",
                            fontSize = 11.sp,
                            color = MaterialTheme.colorScheme.outline,
                        )
                        // New terminal button (hidden in reorder mode or when collapsed)
                        if (!topReorderMode && !isTopCollapsed) {
                            if (!topGroup.isRemote) {
                                Surface(
                                    shape = RoundedCornerShape(6.dp),
                                    tonalElevation = 1.dp,
                                    onClick = {
                                        val newSessionId = TerminalSessionManager.getOrCreateSession(topGroup.topKey)
                                        navController.navigate("terminal/${topGroup.topKey}/$newSessionId")
                                    },
                                ) {
                                    Row(
                                        modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp),
                                        verticalAlignment = Alignment.CenterVertically,
                                    ) {
                                        Icon(Icons.Filled.Add, contentDescription = null, modifier = Modifier.size(14.dp), tint = MaterialTheme.colorScheme.primary)
                                        Spacer(Modifier.width(4.dp))
                                        Text("新建SSH终端", fontSize = 11.sp, fontWeight = FontWeight.Medium, color = MaterialTheme.colorScheme.primary)
                                    }
                                }
                            }
                        }
                    }
                }

                // === Level 2 + Level 3: only render if top group is expanded ===
                if (!isTopCollapsed) {
                topGroup.subGroups.forEach { (subKey, subSessions) ->
                    val subGroupKey = "${topGroup.topKey}_$subKey"
                    val isSubCollapsed = subGroupKey in collapsedSubKeys
                    if (topGroup.isRemote) {
                        // === Level 2: Sub-group header (remote only) ===
                        item(key = "sub_$subGroupKey") {
                            val isLocalDesktop = subKey == "__local__"
                            val subName = subSessions.firstOrNull()?.remoteServerName
                                ?: if (isLocalDesktop) "桌面端" else subKey
                            Row(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .padding(start = 24.dp, top = 8.dp, bottom = 4.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Icon(
                                    if (isLocalDesktop) Icons.Filled.Devices else Icons.Filled.Computer,
                                    contentDescription = null,
                                    modifier = Modifier.size(16.dp),
                                    tint = MaterialTheme.colorScheme.primary,
                                )
                                Spacer(Modifier.width(6.dp))
                                // Collapse/expand arrow
                                Icon(
                                    if (isSubCollapsed) Icons.Filled.ExpandMore else Icons.Filled.ExpandLess,
                                    contentDescription = if (isSubCollapsed) "展开" else "收起",
                                    modifier = Modifier
                                        .size(16.dp)
                                        .clickable {
                                            collapsedSubKeys = if (isSubCollapsed) {
                                                collapsedSubKeys - subGroupKey
                                            } else {
                                                collapsedSubKeys + subGroupKey
                                            }
                                        },
                                    tint = MaterialTheme.colorScheme.primary,
                                )
                                Spacer(Modifier.width(4.dp))
                                // Title text — clickable to toggle
                                Text(
                                    subName,
                                    fontSize = 12.sp,
                                    fontWeight = FontWeight.SemiBold,
                                    color = MaterialTheme.colorScheme.primary,
                                    modifier = Modifier
                                        .weight(1f)
                                        .clickable {
                                            collapsedSubKeys = if (isSubCollapsed) {
                                                collapsedSubKeys - subGroupKey
                                            } else {
                                                collapsedSubKeys + subGroupKey
                                            }
                                        },
                                )
                                Text(
                                    "${subSessions.size}",
                                    fontSize = 11.sp,
                                    color = MaterialTheme.colorScheme.outline,
                                )
                                // New terminal button
                                Surface(
                                    shape = RoundedCornerShape(6.dp),
                                    tonalElevation = 1.dp,
                                    onClick = {
                                        val pid = topGroup.topKey.removePrefix("remote:")
                                        val pairing = PairingStore.getPairing(pid)
                                        if (pairing != null) {
                                            showRemotePickerFor = pairing
                                        } else {
                                            Toast.makeText(context, "未找到配对信息", Toast.LENGTH_SHORT).show()
                                        }
                                    },
                                ) {
                                    Row(
                                        modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp),
                                        verticalAlignment = Alignment.CenterVertically,
                                    ) {
                                        Icon(Icons.Filled.Add, contentDescription = null, modifier = Modifier.size(14.dp), tint = MaterialTheme.colorScheme.primary)
                                        Spacer(Modifier.width(4.dp))
                                        Text(
                                            if (isLocalDesktop) "新建电脑终端" else "新建SSH终端",
                                            fontSize = 11.sp,
                                            fontWeight = FontWeight.Medium,
                                            color = MaterialTheme.colorScheme.primary,
                                        )
                                    }
                                }
                            }
                        }
                    }

                    // === Level 3: Terminal cards (only if sub-group expanded) ===
                    if (!isSubCollapsed) {
                    items(subSessions, key = { it.sessionId }) { session ->
                        val isDragging = draggedSessionId == session.sessionId
                        val dragModifier = Modifier
                            .fillMaxWidth()
                            .padding(start = if (topGroup.isRemote) 24.dp else 0.dp)
                            .zIndex(if (isDragging) 1f else 0f)
                            .graphicsLayer {
                                if (isDragging) {
                                    translationY = dragOffsetY
                                    shadowElevation = 8f
                                    shape = RoundedCornerShape(12.dp)
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
                                        val groupSessionIds = subSessions.map { it.sessionId }
                                        TerminalSessionManager.reorderSessions(groupSessionIds)
                                        draggedSessionId = null
                                        dragOffsetY = 0f
                                    },
                                    onDragCancel = {
                                        refresh()
                                        draggedSessionId = null
                                        dragOffsetY = 0f
                                    },
                                    onDrag = { change, dragAmount ->
                                        change.consume()
                                        dragOffsetY += dragAmount.y
                                        val layoutInfo = listState.layoutInfo
                                        val draggedInfo = layoutInfo.visibleItemsInfo.find { it.key == session.sessionId }
                                        if (draggedInfo != null) {
                                            val draggedCenter = draggedInfo.offset + draggedInfo.size / 2 + dragOffsetY.toInt()
                                            val groupSessionIds = subSessions.map { it.sessionId }.toSet()
                                            val target = layoutInfo.visibleItemsInfo.firstOrNull { vi ->
                                                if (vi.key !in groupSessionIds || vi.key == session.sessionId) return@firstOrNull false
                                                val targetMid = vi.offset + vi.size / 2
                                                if (draggedInfo.offset > vi.offset) draggedCenter < targetMid else draggedCenter > targetMid
                                            }
                                            if (target != null && target.key != session.sessionId) {
                                                val fromIdx = sessions.indexOfFirst { it.sessionId == session.sessionId }
                                                val toIdx = sessions.indexOfFirst { it.sessionId == target.key }
                                                if (fromIdx != -1 && toIdx != -1) {
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
                            isRemote = topGroup.isRemote,
                            serverName = if (topGroup.isRemote) {
                                session.remoteServerName
                            } else {
                                topGroup.topName
                            },
                            isFocused = session.sessionId == focusSessionId,
                            onClick = {
                                if (draggedSessionId == null && draggedTopKey == null) {
                                    if (session.serverId.startsWith("remote:") && session.remotePairingId != null && session.remoteTerminalId != null) {
                                        val pid = session.remotePairingId!!
                                        val tid = session.remoteTerminalId!!
                                        val encodedName = java.net.URLEncoder.encode(session.name.ifBlank { "Terminal" }, "UTF-8")
                                        navController.navigate("remote_terminal/$pid/$tid/$encodedName/${session.remoteServerId}/${session.remoteServerName}")
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
                    } // end if !isSubCollapsed
                }
                } // end if !isTopCollapsed
            }
        }
    }

    // Remote terminal picker dialog — opened by clicking a remote server group header.
    // Uses initialPairing to skip the desktop list and go straight to the terminal list.
    showRemotePickerFor?.let { pairing ->
        RemoteTerminalPickerDialog(
            visible = true,
            initialPairing = pairing,
            onTerminalClick = { terminalId, name, pairingId, serverId, serverName ->
                showRemotePickerFor = null
                val encodedName = java.net.URLEncoder.encode(name, "UTF-8")
                navController.navigate("remote_terminal/$pairingId/$terminalId/$encodedName/$serverId/$serverName")
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
    isRemote: Boolean = false,
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
                    if (isRemote) Icons.Filled.Devices else Icons.Filled.Computer,
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
