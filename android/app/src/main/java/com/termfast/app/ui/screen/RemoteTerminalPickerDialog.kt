package com.termfast.app.ui.screen

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Computer
import androidx.compose.material.icons.filled.Devices
import androidx.compose.material.icons.filled.Terminal
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.termfast.app.data.PairingApi
import com.termfast.app.data.PairingStore
import com.termfast.app.data.RemoteTunnelConfig
import com.termfast.app.data.RustEvent
import com.termfast.app.data.RustRepository
import com.termfast.app.data.RemoteTunnelManager
import com.termfast.app.data.TunnelState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import java.net.URLEncoder

sealed class PickerStage {
    object DesktopList : PickerStage()
    data class TerminalList(val pairing: RemoteTunnelConfig) : PickerStage()
}

/**
 * Remote terminal picker dialog — two-stage:
 * 1. DesktopList: shows all paired desktops, user picks one
 * 2. TerminalList: connects tunnel, shows terminals for selected desktop
 *
 * On back from TerminalList → return to DesktopList (or dismiss if only 1 desktop).
 * On terminal select → onTerminalClick(id, name, pairingId, serverId, serverName), tunnel handed off.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RemoteTerminalPickerDialog(
    visible: Boolean,
    onTerminalClick: (terminalId: Int, name: String, pairingId: String, serverId: String, serverName: String) -> Unit,
    onDismiss: () -> Unit,
    initialPairing: RemoteTunnelConfig? = null,
) {
    if (!visible) return

    val pairings = remember { PairingStore.getAllPairings() }

    if (pairings.isEmpty()) {
        AlertDialog(
            onDismissRequest = onDismiss,
            title = { Text("远程终端", fontWeight = FontWeight.Bold) },
            text = { Text("未配对桌面端，请先在设置中完成配对。") },
            confirmButton = {
                TextButton(onClick = onDismiss) { Text("确定") }
            },
        )
        return
    }

    // Fetch online status from backend — only show desktops that are online
    var onlinePairingIds by remember { mutableStateOf<Set<String>?>(null) }
    LaunchedEffect(Unit) {
        try {
            val devices = withContext(Dispatchers.IO) { PairingApi.listDevices() }
            onlinePairingIds = devices
                .filter { it.pairingType == "mobile" && it.status == "completed" && it.isOnline }
                .map { it.pairingId }
                .toSet()
        } catch (_: Exception) {
            // If fetch fails, show all pairings as fallback
            onlinePairingIds = pairings.map { it.pairingId }.toSet()
        }
    }

    // Filter to only online desktops
    val onlinePairings = remember(pairings, onlinePairingIds) {
        if (onlinePairingIds == null) emptyList() // still loading
        else pairings.filter { it.pairingId in onlinePairingIds!! }
    }

    // If initialPairing is set, skip DesktopList stage and go directly to TerminalList.
    // If only 1 online desktop and no initialPairing, also skip DesktopList.
    var stage by remember {
        mutableStateOf<PickerStage>(
            if (initialPairing != null) {
                PickerStage.TerminalList(initialPairing)
            } else {
                PickerStage.DesktopList
            }
        )
    }
    // Auto-skip DesktopList when only 1 desktop is online (after fetch completes)
    LaunchedEffect(onlinePairingIds) {
        if (onlinePairingIds != null && initialPairing == null &&
            stage is PickerStage.DesktopList && onlinePairings.size == 1) {
            stage = PickerStage.TerminalList(onlinePairings[0])
        }
    }
    // Track whether a terminal was selected (vs dismissed) — when selected,
    // tunnel is handed off to RemoteTerminalScreen and must NOT be stopped.
    var terminalSelected by remember { mutableStateOf(false) }
    // Track current tunnel manager for dismiss handling
    var currentTunnelManager by remember { mutableStateOf<RemoteTunnelManager?>(null) }

    val scope = rememberCoroutineScope()

    // Handle dismiss — stop tunnel if in TerminalList stage (unless terminal selected)
    val handleDismiss: () -> Unit = {
        if (stage is PickerStage.TerminalList && !terminalSelected) {
            currentTunnelManager?.stop()
        }
        onDismiss()
    }

    AlertDialog(
        onDismissRequest = handleDismiss,
        title = {
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (stage is PickerStage.TerminalList && onlinePairings.size > 1) {
                    IconButton(
                        onClick = {
                            if (!terminalSelected) {
                                currentTunnelManager?.stop()
                                currentTunnelManager = null
                            }
                            stage = PickerStage.DesktopList
                        },
                        modifier = Modifier.size(36.dp),
                    ) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "返回")
                    }
                    Spacer(Modifier.width(8.dp))
                }
                Text(
                    if (stage is PickerStage.DesktopList) "选择桌面端"
                    else (stage as PickerStage.TerminalList).pairing.desktopName.ifEmpty { "远程终端" },
                    fontWeight = FontWeight.Bold,
                )
            }
        },
        text = {
            when (val s = stage) {
                is PickerStage.DesktopList -> {
                    if (onlinePairingIds == null) {
                        // Loading online status
                        Box(
                            modifier = Modifier.fillMaxWidth().padding(24.dp),
                            contentAlignment = Alignment.Center,
                        ) {
                            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                                CircularProgressIndicator()
                                Spacer(Modifier.height(12.dp))
                                Text("正在获取在线状态...", fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        }
                    } else if (onlinePairings.isEmpty()) {
                        Column(
                            modifier = Modifier.fillMaxWidth().padding(16.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                        ) {
                            Icon(
                                Icons.Filled.Computer,
                                contentDescription = null,
                                modifier = Modifier.size(32.dp),
                                tint = MaterialTheme.colorScheme.outline,
                            )
                            Spacer(Modifier.height(8.dp))
                            Text("没有桌面端在线", fontSize = 14.sp, fontWeight = FontWeight.Medium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            Spacer(Modifier.height(4.dp))
                            Text("请先在电脑上打开 TermFast 桌面端", fontSize = 12.sp, color = MaterialTheme.colorScheme.outline)
                        }
                    } else {
                        DesktopListContent(
                            pairings = onlinePairings,
                            onSelect = { pairing ->
                                stage = PickerStage.TerminalList(pairing)
                            },
                        )
                    }
                }
                is PickerStage.TerminalList -> TerminalListContent(
                    pairing = s.pairing,
                    onTerminalClick = { terminalId, name, serverId, serverName ->
                        terminalSelected = true
                        onTerminalClick(terminalId, name, s.pairing.pairingId, serverId, serverName)
                    },
                    onTunnelManagerReady = { tm -> currentTunnelManager = tm },
                )
            }
        },
        confirmButton = {},
        dismissButton = {
            TextButton(onClick = handleDismiss) { Text("取消") }
        },
    )
}

@Composable
private fun DesktopListContent(
    pairings: List<RemoteTunnelConfig>,
    onSelect: (RemoteTunnelConfig) -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .verticalScroll(rememberScrollState()),
    ) {
        pairings.forEach { pairing ->
            Surface(
                modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
                shape = RoundedCornerShape(8.dp),
                tonalElevation = 1.dp,
                onClick = { onSelect(pairing) },
            ) {
                Row(
                    modifier = Modifier.padding(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(
                        Icons.Filled.Computer,
                        contentDescription = null,
                        modifier = Modifier.size(20.dp),
                        tint = MaterialTheme.colorScheme.primary,
                    )
                    Spacer(Modifier.width(12.dp))
                    Text(
                        pairing.desktopName.ifEmpty { pairing.pairingId.take(8) },
                        fontFamily = FontFamily.Monospace,
                        fontSize = 14.sp,
                        fontWeight = FontWeight.Medium,
                        modifier = Modifier.weight(1f),
                    )
                    Icon(
                        Icons.Filled.Terminal,
                        contentDescription = null,
                        modifier = Modifier.size(16.dp),
                        tint = MaterialTheme.colorScheme.outline,
                    )
                }
            }
        }
    }
}
// === SECTION 1 END ===

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TerminalListContent(
    pairing: RemoteTunnelConfig,
    onTerminalClick: (terminalId: Int, name: String, serverId: String, serverName: String) -> Unit,
    onTunnelManagerReady: (RemoteTunnelManager) -> Unit,
) {
    val pairingId = pairing.pairingId
    val pairingKeyHex = pairing.pairingKey
    val relayUrl = pairing.relayUrl
    val pairingJwt = pairing.pairingJwt
    val pairingRefreshToken = pairing.pairingRefreshToken
    val context = LocalContext.current

    val pairingKey = remember(pairingKeyHex) {
        pairingKeyHex.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
    }

    var terminals by remember { mutableStateOf<List<TerminalEntry>>(emptyList()) }
    var loading by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf<String?>(null) }
    var transportState by remember { mutableStateOf<TunnelState>(TunnelState.Disconnected) }
    var protocolReady by remember { mutableStateOf(false) }
    var creatingTerminal by remember { mutableStateOf(false) }

    val tunnelManager = remember(pairingId) {
        TerminalSessionManager.getOrCreateTunnelManager(pairingId, pairingKey, relayUrl, pairingJwt, pairingRefreshToken)
    }
    val scope = rememberCoroutineScope()

    // Report tunnel manager to parent for dismiss handling
    LaunchedEffect(tunnelManager) {
        onTunnelManagerReady(tunnelManager)
    }

    // Collect transport + protocol state
    LaunchedEffect(tunnelManager) {
        tunnelManager.transportState.collect { state ->
            transportState = state
            when (state) {
                is TunnelState.Error -> {
                    if (state.message == "pairing_revoked") {
                        // Pairing was revoked by the desktop — remove from local
                        // store so it doesn't show up in the list anymore.
                        PairingStore.removePairing(pairingId)
                        error = "配对已被撤销"
                    } else {
                        error = state.message
                    }
                    loading = false
                }
                is TunnelState.Disconnected -> {
                    if (error == null) {
                        loading = true
                    }
                }
                is TunnelState.Connected -> {
                    error = null
                }
                else -> {}
            }
        }
    }
    LaunchedEffect(tunnelManager) {
        tunnelManager.protocolReady.collect { ready ->
            protocolReady = ready
            if (ready) {
                tunnelManager.sendListRequest()
            }
        }
    }

    // Listen for Rust FFI events
    LaunchedEffect(tunnelManager) {
        RustRepository.events.collect { event ->
            when (event) {
                is RustEvent.RemoteTunnelReady -> {
                    tunnelManager.onProtocolReady()
                }
                is RustEvent.RemoteTerminalList -> {
                    if (event.pairing_id == pairingId) {
                        terminals = parseTerminalList(event.terminals)
                        loading = false
                        error = null
                    }
                }
                is RustEvent.RemoteTerminalNotify -> {
                    if (event.pairing_id == pairingId && tunnelManager.protocolReady.value) {
                        tunnelManager.sendListRequest()
                    }
                }
                is RustEvent.RemoteTerminalError -> {
                    if (event.pairing_id == pairingId) {
                        error = event.error
                        loading = false
                    }
                }
                else -> {}
            }
        }
    }

    // Start tunnel when entering TerminalList stage
    LaunchedEffect(tunnelManager) {
        loading = true
        error = null
        terminals = emptyList()
        tunnelManager.start()
    }

    // Auto-retry when desktop is offline
    LaunchedEffect(tunnelManager, error) {
        if (error != null && error!!.contains("desktop_offline")) {
            kotlinx.coroutines.delay(5_000)
            if (error != null && error!!.contains("desktop_offline")) {
                error = null
                loading = true
                terminals = emptyList()
                tunnelManager.start()
            }
        }
    }

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .verticalScroll(rememberScrollState()),
    ) {
        if (loading) {
            Box(
                modifier = Modifier.fillMaxWidth().padding(16.dp),
                contentAlignment = Alignment.Center,
            ) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    CircularProgressIndicator()
                    Spacer(Modifier.height(12.dp))
                    Text(
                        when (transportState) {
                            is TunnelState.Connecting -> "正在连接中继..."
                            is TunnelState.WaitingForPeer -> "等待桌面端..."
                            is TunnelState.Connected -> "正在建立加密通道..."
                            else -> "加载中..."
                        },
                        fontSize = 12.sp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            return@Column
        }

        if (error != null) {
            val isOffline = error!!.contains("desktop_offline")
            if (isOffline) {
                Text("桌面端离线", color = MaterialTheme.colorScheme.error, fontSize = 14.sp, fontWeight = FontWeight.Medium)
                Spacer(Modifier.height(4.dp))
                Text("请确认桌面端已启动并登录", fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
            } else {
                Text("连接失败: $error", color = MaterialTheme.colorScheme.error, fontSize = 13.sp)
            }
            Spacer(Modifier.height(8.dp))
            TextButton(onClick = {
                error = null
                loading = true
                terminals = emptyList()
                scope.launch { tunnelManager.start() }
            }) { Text("重试") }
            return@Column
        }

        if (terminals.isEmpty()) {
            Text("没有可用的远程终端", fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Spacer(Modifier.height(4.dp))
            Text("请在桌面端打开终端，或点击分组右侧按钮新建", fontSize = 11.sp, color = MaterialTheme.colorScheme.outline)
        }

        // Loading indicator while creating a new terminal
        if (creatingTerminal) {
            Box(
                modifier = Modifier.fillMaxWidth().padding(12.dp),
                contentAlignment = Alignment.Center,
            ) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    CircularProgressIndicator(modifier = Modifier.size(16.dp), strokeWidth = 2.dp)
                    Text("正在新建终端...", fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }

        // Group terminals by serverName, with a "new terminal" button per group
        val grouped = terminals.groupBy { it.serverId }
        grouped.forEach { (serverId, groupTerminals) ->
            val serverName = groupTerminals.first().serverName
            val isLocalGroup = serverId == "__local__"
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(top = 8.dp, bottom = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    serverName,
                    fontSize = 12.sp,
                    fontWeight = FontWeight.SemiBold,
                    color = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.weight(1f),
                )
                if (protocolReady && !creatingTerminal) {
                    // New terminal button for this group
                    Surface(
                        shape = RoundedCornerShape(6.dp),
                        tonalElevation = 1.dp,
                        onClick = {
                            creatingTerminal = true
                            scope.launch {
                                val sid = if (isLocalGroup) "" else serverId
                                val sent = tunnelManager.sendNewTerminal(serverId = sid)
                                if (!sent) {
                                    creatingTerminal = false
                                    withContext(kotlinx.coroutines.Dispatchers.Main) {
                                        android.widget.Toast.makeText(context, "发送失败，请重试", android.widget.Toast.LENGTH_SHORT).show()
                                    }
                                    return@launch
                                }
                                val result = awaitNewTerminalOk(pairingId)
                                creatingTerminal = false
                                if (result != null) {
                                    val (newTerminalId, termName) = result
                                    onTerminalClick(newTerminalId, termName.ifBlank { "Terminal" }, serverId, serverName)
                                } else {
                                    withContext(kotlinx.coroutines.Dispatchers.Main) {
                                        android.widget.Toast.makeText(context, "新建超时，请重试", android.widget.Toast.LENGTH_SHORT).show()
                                    }
                                }
                            }
                        },
                    ) {
                        Row(
                            modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Icon(
                                Icons.Filled.Add,
                                contentDescription = null,
                                modifier = Modifier.size(14.dp),
                                tint = MaterialTheme.colorScheme.primary,
                            )
                            Spacer(Modifier.width(4.dp))
                            Text(
                                if (isLocalGroup) "新建电脑终端" else "新建SSH终端",
                                fontSize = 11.sp,
                                fontWeight = FontWeight.Medium,
                                color = MaterialTheme.colorScheme.primary,
                            )
                        }
                    }
                }
            }
            groupTerminals.forEach { terminal ->
                Surface(
                    modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
                    shape = RoundedCornerShape(8.dp),
                    tonalElevation = 1.dp,
                    onClick = { onTerminalClick(terminal.id, terminal.name, terminal.serverId, terminal.serverName) },
                ) {
                    Row(
                        modifier = Modifier.padding(12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Icon(
                            if (terminal.isLocal) Icons.Filled.Devices else Icons.Filled.Computer,
                            contentDescription = null,
                            modifier = Modifier.size(20.dp),
                            tint = MaterialTheme.colorScheme.primary,
                        )
                        Spacer(Modifier.width(12.dp))
                        Column(modifier = Modifier.weight(1f)) {
                            Text(
                                terminal.name,
                                fontFamily = FontFamily.Monospace,
                                fontSize = 13.sp,
                                fontWeight = FontWeight.Medium,
                            )
                            terminal.tmuxSessionName?.let {
                                Text("tmux: $it", fontSize = 11.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        }
                        Icon(
                            Icons.Filled.Terminal,
                            contentDescription = null,
                            modifier = Modifier.size(16.dp),
                            tint = MaterialTheme.colorScheme.outline,
                        )
                    }
                }
            }
        }
    }
}
// === SECTION 2 END ===

data class TerminalEntry(
    val id: Int,
    val name: String,
    val serverId: String,
    val serverName: String,
    val isLocal: Boolean,
    val terminalType: String,
    val tmuxSessionName: String?,
)

@Serializable
private data class TerminalJson(
    val terminal_id: Int? = null,
    val id: Int? = null,
    val name: String? = null,
    val server_id: String? = null,
    val server_name: String? = null,
    val is_local: Boolean? = null,
    val terminal_type: String? = null,
    val tmux_session_name: String? = null,
)

private val terminalListJson = Json { ignoreUnknownKeys = true; isLenient = true }

internal fun parseTerminalList(json: String): List<TerminalEntry> {
    return try {
        val items = terminalListJson.decodeFromString<List<TerminalJson>>(json)
        items.map { item ->
            val serverId = item.server_id ?: ""
            val isLocal = item.is_local ?: (serverId == "__local__")
            TerminalEntry(
                id = item.terminal_id ?: item.id ?: -1,
                name = item.name ?: "Terminal",
                serverId = serverId,
                serverName = item.server_name ?: if (isLocal) "桌面端" else serverId,
                isLocal = isLocal,
                terminalType = item.terminal_type ?: if (isLocal) "local" else "ssh",
                tmuxSessionName = item.tmux_session_name?.ifEmpty { null },
            )
        }
    } catch (_: Exception) {
        emptyList()
    }
}

internal fun isTmuxUnavailableError(error: String): Boolean {
    val lower = error.lowercase()
    return lower.contains("tmux") ||
        lower.contains("tmux_unavailable") ||
        lower.contains("multi_terminal") ||
        lower.contains("desktop_offline") ||
        lower.contains("terminal_not_found") ||
        lower.contains("hello_required") ||
        lower.contains("hello_already_done")
}
