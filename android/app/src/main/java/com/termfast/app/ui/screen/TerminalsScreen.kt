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
import android.widget.Toast
import com.termfast.app.data.PairingStore
import com.termfast.app.data.RustEvent
import com.termfast.app.data.RustRepository
import com.termfast.app.data.RemoteTunnelManager
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
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
private suspend fun awaitNewTerminalOk(
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
    LaunchedEffect(Unit) {
        RustRepository.events.collect { event ->
            when (event) {
                is RustEvent.RemoteTunnelReady -> {
                    val tm = TerminalSessionManager.getTunnelManager(event.pairing_id)
                    tm?.onProtocolReady()
                    // Request list to sync: remove sessions whose terminals no longer exist on desktop
                    tm?.sendListRequest()
                }
                is RustEvent.RemoteTerminalNotify -> {
                    val pid = event.pairing_id
                    val tm = TerminalSessionManager.getTunnelManager(pid)
                    if (tm != null && tm.protocolReady.value) {
                        tm.sendListRequest()
                    }
                }
                is RustEvent.RemoteTerminalList -> {
                    val pid = event.pairing_id
                    try {
                        val arr = org.json.JSONArray(event.terminals)
                        val aliveIds = (0 until arr.length()).mapNotNull { i ->
                            arr.getJSONObject(i).optInt("id", -1).takeIf { it >= 0 }
                        }.toSet()
                        val toRemove = TerminalSessionManager.getAllSessions().filter { s ->
                            s.remotePairingId == pid && s.remoteTerminalId != null &&
                                s.remoteTerminalId !in aliveIds
                        }
                        if (toRemove.isNotEmpty()) {
                            val names = toRemove.mapNotNull { it.name }.ifEmpty { listOf("远程终端") }.joinToString(", ")
                            toRemove.forEach { s ->
                                TerminalSessionManager.closeSessionBySessionId(s.sessionId)
                            }
                            sessions = TerminalSessionManager.getAllSessions()
                            withContext(kotlinx.coroutines.Dispatchers.Main) {
                                Toast.makeText(context, "远程终端已关闭: $names", Toast.LENGTH_SHORT).show()
                            }
                        }
                    } catch (_: Exception) {}
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
                                android.util.Log.i("TerminalsScreen", "+ button clicked: serverId=$serverId")
                                if (serverId.startsWith("remote:")) {
                                    // Remote terminal: send NEW_TERMINAL frame via tunnel
                                    val pid = serverId.removePrefix("remote:")
                                    android.util.Log.i("TerminalsScreen", "remote terminal: pid=$pid")
                                    val tm = TerminalSessionManager.getTunnelManager(pid)
                                    if (tm != null && tm.protocolReady.value) {
                                        android.util.Log.i("TerminalsScreen", "tunnel ready, sending NEW_TERMINAL")
                                        val sent = tm.sendNewTerminal()
                                        android.util.Log.i("TerminalsScreen", "sendNewTerminal result=$sent")
                                        if (sent) {
                                            scope.launch {
                                                val result = awaitNewTerminalOk(pid)
                                                if (result != null) {
                                                    val (newTerminalId, termName) = result
                                                    val encodedName = java.net.URLEncoder.encode(termName.ifBlank { "Terminal" }, "UTF-8")
                                                    navController.navigate("remote_terminal/$pid/$newTerminalId/$encodedName")
                                                } else {
                                                    withContext(kotlinx.coroutines.Dispatchers.Main) {
                                                        Toast.makeText(context, "新建超时，请在远程终端列表中查看", Toast.LENGTH_SHORT).show()
                                                    }
                                                }
                                            }
                                        }
                                    } else {
                                        android.util.Log.i("TerminalsScreen", "tunnel not ready (tm=${tm != null}, ready=${tm?.protocolReady?.value}), starting tunnel...")
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
                                                // Listen for RemoteTunnelReady to mark protocol ready
                                                val readyJob = scope.launch {
                                                    RustRepository.events.collect { event ->
                                                        if (event is RustEvent.RemoteTunnelReady && event.pairing_id == pid) {
                                                            newTm.onProtocolReady()
                                                        }
                                                    }
                                                }
                                                newTm.start()
                                                val ready = waitForProtocolReady(newTm, timeoutMs = 10000)
                                                readyJob.cancel()
                                                if (ready) {
                                                    val sent = newTm.sendNewTerminal()
                                                    if (sent) {
                                                        val result = awaitNewTerminalOk(pid)
                                                        if (result != null) {
                                                            val (newTerminalId, termName) = result
                                                            val encodedName = java.net.URLEncoder.encode(termName.ifBlank { "Terminal" }, "UTF-8")
                                                            navController.navigate("remote_terminal/$pid/$newTerminalId/$encodedName")
                                                        } else {
                                                            withContext(kotlinx.coroutines.Dispatchers.Main) {
                                                                Toast.makeText(context, "新建超时，请在远程终端列表中查看", Toast.LENGTH_SHORT).show()
                                                            }
                                                        }
                                                    } else {
                                                        withContext(kotlinx.coroutines.Dispatchers.Main) {
                                                            Toast.makeText(context, "发送失败，请重试", Toast.LENGTH_SHORT).show()
                                                        }
                                                    }
                                                } else {
                                                    withContext(kotlinx.coroutines.Dispatchers.Main) {
                                                        Toast.makeText(context, "无法连接到电脑，请确认电脑端在线", Toast.LENGTH_SHORT).show()
                                                    }
                                                }
                                            }
                                        } else {
                                            android.util.Log.w("TerminalsScreen", "no pairing found for pid=$pid")
                                            Toast.makeText(context, "未找到配对信息", Toast.LENGTH_SHORT).show()
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
                            if (session.serverId.startsWith("remote:") && session.remotePairingId != null && session.remoteTerminalId != null) {
                                // Remote terminal: navigate to remote terminal screen
                                val pid = session.remotePairingId!!
                                val tid = session.remoteTerminalId!!
                                val encodedName = java.net.URLEncoder.encode(session.name.ifBlank { "Terminal" }, "UTF-8")
                                navController.navigate("remote_terminal/$pid/$tid/$encodedName")
                            } else {
                                // Local SSH: navigate to normal terminal screen
                                navController.navigate("terminal/${session.serverId}/${session.sessionId}")
                            }
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
