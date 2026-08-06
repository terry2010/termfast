package com.termfast.app.ui.screen

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Computer
import androidx.compose.material.icons.filled.Devices
import androidx.compose.material.icons.filled.Terminal
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.termfast.app.data.PairingStore
import com.termfast.app.data.RemoteTunnelConfig
import com.termfast.app.data.RustEvent
import com.termfast.app.data.RustRepository
import com.termfast.app.data.RemoteTunnelManager
import com.termfast.app.data.TunnelState
import kotlinx.coroutines.launch
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
 * On terminal select → onTerminalClick(id, name, pairingId), tunnel handed off.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RemoteTerminalPickerDialog(
    visible: Boolean,
    onTerminalClick: (terminalId: Int, name: String, pairingId: String) -> Unit,
    onDismiss: () -> Unit,
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

    // If only 1 desktop, skip DesktopList stage
    var stage by remember {
        mutableStateOf<PickerStage>(
            if (pairings.size == 1) PickerStage.TerminalList(pairings[0])
            else PickerStage.DesktopList
        )
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
                if (stage is PickerStage.TerminalList && pairings.size > 1) {
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
                is PickerStage.DesktopList -> DesktopListContent(
                    pairings = pairings,
                    onSelect = { pairing ->
                        stage = PickerStage.TerminalList(pairing)
                    },
                )
                is PickerStage.TerminalList -> TerminalListContent(
                    pairing = s.pairing,
                    onTerminalClick = { terminalId, name ->
                        terminalSelected = true
                        onTerminalClick(terminalId, name, s.pairing.pairingId)
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
    onTerminalClick: (terminalId: Int, name: String) -> Unit,
    onTunnelManagerReady: (RemoteTunnelManager) -> Unit,
) {
    val pairingId = pairing.pairingId
    val pairingKeyHex = pairing.pairingKey
    val relayUrl = pairing.relayUrl
    val pairingJwt = pairing.pairingJwt

    val pairingKey = remember(pairingKeyHex) {
        pairingKeyHex.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
    }

    var terminals by remember { mutableStateOf<List<TerminalEntry>>(emptyList()) }
    var loading by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf<String?>(null) }
    var transportState by remember { mutableStateOf<TunnelState>(TunnelState.Disconnected) }
    var protocolReady by remember { mutableStateOf(false) }

    val tunnelManager = remember(pairingId) {
        TerminalSessionManager.getOrCreateTunnelManager(pairingId, pairingKey, relayUrl, pairingJwt)
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
            Text("请在桌面端打开终端", fontSize = 11.sp, color = MaterialTheme.colorScheme.outline)
            return@Column
        }

        // Group terminals by serverName
        val grouped = terminals.groupBy { it.serverName }
        grouped.forEach { (serverName, groupTerminals) ->
            Text(
                serverName,
                fontSize = 12.sp,
                fontWeight = FontWeight.SemiBold,
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier.padding(top = 8.dp, bottom = 4.dp),
            )
            groupTerminals.forEach { terminal ->
                Surface(
                    modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
                    shape = RoundedCornerShape(8.dp),
                    tonalElevation = 1.dp,
                    onClick = { onTerminalClick(terminal.id, terminal.name) },
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
