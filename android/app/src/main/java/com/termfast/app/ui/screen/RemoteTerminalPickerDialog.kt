package com.termfast.app.ui.screen

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
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
import com.termfast.app.data.RustEvent
import com.termfast.app.data.RustRepository
import com.termfast.app.data.RemoteTunnelManager
import com.termfast.app.data.TunnelState
import kotlinx.coroutines.launch
import java.net.URLEncoder

/**
 * Remote terminal picker dialog — shown when user taps "远程终端" on the
 * remote terminal card in ServerListScreen.
 *
 * Mimics TmuxSessionPickerDialog:
 * 1. Connects tunnel to relay via RemoteTunnelManager
 * 2. Sends LIST_REQUEST after HELLO exchange
 * 3. Shows list of available remote terminals
 * 4. On select → onTerminalClick(terminalId, name)
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RemoteTerminalPickerDialog(
    visible: Boolean,
    onTerminalClick: (terminalId: Int, name: String) -> Unit,
    onDismiss: () -> Unit,
) {
    if (!visible) return

    val pairingId = PairingStore.getPairingId()
    val pairingKeyHex = PairingStore.getPairingKey()
    val relayUrl = PairingStore.getRelayUrl()
    val pairingJwt = PairingStore.getPairingJwt()

    if (pairingId == null || pairingKeyHex == null || relayUrl == null || pairingJwt == null) {
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

    val pairingKey = remember(pairingKeyHex) {
        pairingKeyHex.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
    }

    var terminals by remember { mutableStateOf<List<TerminalEntry>>(emptyList()) }
    var loading by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf<String?>(null) }
    var transportState by remember { mutableStateOf<TunnelState>(TunnelState.Disconnected) }
    var protocolReady by remember { mutableStateOf(false) }

    // Create or reuse shared tunnel manager
    val tunnelManager = remember(pairingId) {
        TerminalSessionManager.getOrCreateTunnelManager(pairingId, pairingKey, relayUrl, pairingJwt)
    }
    val scope = rememberCoroutineScope()

    // Collect transport + protocol state
    LaunchedEffect(tunnelManager, visible) {
        if (!visible) return@LaunchedEffect
        tunnelManager.transportState.collect { state ->
            transportState = state
            when (state) {
                is TunnelState.Error -> {
                    error = state.message
                    loading = false
                }
                is TunnelState.Disconnected -> {
                    // Normal disconnect (e.g. desktop went offline) — show
                    // loading while auto-reconnect is in progress.
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
    LaunchedEffect(tunnelManager, visible) {
        if (!visible) return@LaunchedEffect
        tunnelManager.protocolReady.collect { ready ->
            protocolReady = ready
            if (ready) {
                tunnelManager.sendListRequest()
            }
        }
    }

    // Listen for Rust FFI events
    LaunchedEffect(tunnelManager, visible) {
        if (!visible) return@LaunchedEffect
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

    // Start tunnel when dialog opens
    LaunchedEffect(tunnelManager, visible) {
        if (visible) {
            loading = true
            error = null
            terminals = emptyList()
            tunnelManager.start()
        }
    }

    // Auto-retry when desktop is offline — check every 5 seconds
    // so the dialog recovers automatically when desktop comes online.
    LaunchedEffect(tunnelManager, visible, error) {
        if (!visible) return@LaunchedEffect
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

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("远程终端", fontWeight = FontWeight.Bold) },
        text = {
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
                        Text(
                            "桌面端离线",
                            color = MaterialTheme.colorScheme.error,
                            fontSize = 14.sp,
                            fontWeight = FontWeight.Medium,
                        )
                        Spacer(Modifier.height(4.dp))
                        Text(
                            "请确认桌面端已启动并登录",
                            fontSize = 12.sp,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    } else {
                        Text(
                            "连接失败: $error",
                            color = MaterialTheme.colorScheme.error,
                            fontSize = 13.sp,
                        )
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
                    Text(
                        "没有可用的远程终端",
                        fontSize = 13.sp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Spacer(Modifier.height(4.dp))
                    Text(
                        "请在桌面端打开终端",
                        fontSize = 11.sp,
                        color = MaterialTheme.colorScheme.outline,
                    )
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
                            onClick = {
                                onTerminalClick(terminal.id, terminal.name)
                            },
                        ) {
                            Row(
                                modifier = Modifier.padding(12.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Icon(
                                    if (terminal.isLocal) Icons.Filled.Devices
                                    else Icons.Filled.Computer,
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
                                        Text(
                                            "tmux: $it",
                                            fontSize = 11.sp,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        )
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
        },
        confirmButton = {},
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("取消") }
        },
    )
}
