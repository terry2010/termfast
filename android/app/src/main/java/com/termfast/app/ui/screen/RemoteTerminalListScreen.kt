package com.termfast.app.ui.screen

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Devices
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.navigation.NavController
import com.termfast.app.data.PairingStore
import com.termfast.app.data.RustEvent
import com.termfast.app.data.RustRepository
import com.termfast.app.data.RemoteTunnelManager
import com.termfast.app.data.TunnelState
import kotlinx.coroutines.launch
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import java.net.URLEncoder

/**
 * Remote terminal list screen — shows terminals shared from desktop via relay tunnel.
 *
 * Uses RemoteTunnelManager to:
 * 1. Connect WebSocket tunnel to relay
 * 2. Complete HELLO exchange (Rust FFI frame crypto)
 * 3. Send LIST_REQUEST and render LIST_RESPONSE
 *
 * User taps a terminal → navigate to RemoteTerminalScreen for rendering.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RemoteTerminalListScreen(
    navController: NavController,
    onBack: () -> Unit,
) {
    // Load tunnel credentials from PairingStore
    val pairingId = PairingStore.getPairingId()
    val pairingKeyHex = PairingStore.getPairingKey()
    val relayUrl = PairingStore.getRelayUrl()
    val pairingJwt = PairingStore.getPairingJwt()

    if (pairingId == null || pairingKeyHex == null || relayUrl == null || pairingJwt == null) {
        // No pairing config — show error
        Scaffold(
            topBar = {
                TopAppBar(
                    title = { Text("Remote Terminals") },
                    navigationIcon = {
                        TextButton(onClick = onBack) { Text("Back") }
                    },
                )
            }
        ) { padding ->
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(padding),
                contentAlignment = Alignment.Center,
            ) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Icon(Icons.Default.Devices, contentDescription = null, modifier = Modifier.size(64.dp))
                    Spacer(Modifier.height(16.dp))
                    Text("未配对桌面端")
                    Text(
                        "请先在设置 → 设备配对中完成配对",
                        style = MaterialTheme.typography.bodySmall,
                    )
                    Spacer(Modifier.height(16.dp))
                    Button(onClick = { navController.navigate("pairing") }) {
                        Text("去配对")
                    }
                }
            }
        }
        return
    }

    val pairingKey = hexToBytes(pairingKeyHex)

    RemoteTerminalListContent(
        pairingId = pairingId,
        pairingKey = pairingKey,
        relayUrl = relayUrl,
        pairingJwt = pairingJwt,
        onTerminalClick = { terminalId, name ->
            val encodedName = URLEncoder.encode(name, "UTF-8")
            navController.navigate("remote_terminal/$pairingId/$terminalId/$encodedName")
        },
        onBack = onBack,
    )
}

/**
 * Convert hex string to ByteArray.
 */
private fun hexToBytes(hex: String): ByteArray {
    return hex.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RemoteTerminalListContent(
    pairingId: String,
    pairingKey: ByteArray,
    relayUrl: String,
    pairingJwt: String,
    onTerminalClick: (Int, String) -> Unit,
    onBack: () -> Unit,
) {
    var terminals by remember { mutableStateOf<List<TerminalEntry>>(emptyList()) }
    var loading by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf<String?>(null) }
    var transportState by remember { mutableStateOf<TunnelState>(TunnelState.Disconnected) }
    var protocolReady by remember { mutableStateOf(false) }

    // Create tunnel manager (shared via TerminalSessionManager)
    val tunnelManager = remember(pairingId) {
        TerminalSessionManager.getOrCreateTunnelManager(pairingId, pairingKey, relayUrl, pairingJwt)
    }
    val scope = rememberCoroutineScope()

    // Collect transport + protocol state
    LaunchedEffect(tunnelManager) {
        tunnelManager.transportState.collect { state ->
            transportState = state
            if (state is TunnelState.Error) {
                error = state.message
                loading = false
            }
        }
    }
    LaunchedEffect(tunnelManager) {
        tunnelManager.protocolReady.collect { ready ->
            protocolReady = ready
            if (ready) {
                // Session key established → send LIST_REQUEST
                tunnelManager.sendListRequest()
            }
        }
    }

    // Listen for Rust FFI events (RemoteTerminalList, RemoteTunnelReady, errors)
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
                    // Desktop broadcasts list_changed when terminals open/close.
                    // Re-send LIST_REQUEST to refresh the terminal list.
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
                else -> { /* ignore unrelated events */ }
            }
        }
    }

    // Start tunnel on screen entry
    LaunchedEffect(tunnelManager) {
        tunnelManager.start()
    }

    // Note: tunnel is shared via TerminalSessionManager — do NOT stop on exit,
    // RemoteTerminalScreen manages its own lifecycle.

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Remote Terminals") },
                navigationIcon = {
                    TextButton(onClick = onBack) { Text("Back") }
                },
                actions = {
                    // Connection status indicator
                    val statusText = when {
                        error != null -> "Error"
                        protocolReady -> "Connected"
                        transportState is TunnelState.Connected -> "Handshake"
                        transportState is TunnelState.WaitingForPeer -> "Waiting"
                        transportState is TunnelState.Connecting -> "Connecting"
                        else -> "Offline"
                    }
                    Text(
                        text = statusText,
                        style = MaterialTheme.typography.labelSmall,
                        modifier = Modifier.padding(end = 16.dp)
                    )
                }
            )
        }
    ) { padding ->
        Box(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
        ) {
            when {
                loading -> {
                    Column(
                        modifier = Modifier.align(Alignment.Center),
                        horizontalAlignment = Alignment.CenterHorizontally
                    ) {
                        CircularProgressIndicator()
                        Spacer(Modifier.height(16.dp))
                        Text(
                            when (transportState) {
                                is TunnelState.Connecting -> "Connecting to relay..."
                                is TunnelState.WaitingForPeer -> "Waiting for desktop..."
                                is TunnelState.Connected -> "Establishing secure channel..."
                                else -> "Loading..."
                            },
                            style = MaterialTheme.typography.bodyMedium
                        )
                    }
                }
                error != null -> {
                    val errStr = error!!
                    Column(
                        modifier = Modifier.align(Alignment.Center).padding(16.dp),
                        horizontalAlignment = Alignment.CenterHorizontally
                    ) {
                        if (isTmuxUnavailableError(errStr)) {
                            // #14: tmux 不可用或桌面端离线时，明确提示用户
                            Icon(
                                Icons.Default.Devices,
                                contentDescription = null,
                                modifier = Modifier.size(48.dp),
                                tint = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                            Spacer(Modifier.height(16.dp))
                            Text(
                                text = "多端协同需要 tmux 或桌面端在线",
                                style = MaterialTheme.typography.titleMedium,
                            )
                            Spacer(Modifier.height(8.dp))
                            Text(
                                text = "单手机 SSH 仍可用，但远程终端共享需要桌面端在线且安装 tmux。",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        } else {
                            Text(
                                text = "Error: $errStr",
                                color = MaterialTheme.colorScheme.error,
                            )
                        }
                        Spacer(Modifier.height(16.dp))
                        Button(onClick = {
                            error = null
                            loading = true
                            terminals = emptyList()
                            scope.launch { tunnelManager.start() }
                        }) {
                            Text("Retry")
                        }
                    }
                }
                terminals.isEmpty() -> {
                    Column(
                        modifier = Modifier.align(Alignment.Center),
                        horizontalAlignment = Alignment.CenterHorizontally
                    ) {
                        Icon(Icons.Default.Devices, contentDescription = null, modifier = Modifier.size(64.dp))
                        Spacer(Modifier.height(16.dp))
                        Text("No remote terminals available")
                        Text(
                            "Open a terminal on desktop and enable sharing",
                            style = MaterialTheme.typography.bodySmall
                        )
                    }
                }
                else -> {
                    LazyColumn(
                        modifier = Modifier.fillMaxSize(),
                        contentPadding = PaddingValues(16.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp)
                    ) {
                        items(terminals) { terminal ->
                            TerminalCard(terminal, onClick = {
                                onTerminalClick(terminal.id, terminal.name)
                            })
                        }
                    }
                }
            }
        }
    }
}

// === SECTION 1 END ===

@Composable
private fun TerminalCard(terminal: TerminalEntry, onClick: () -> Unit) {
    Card(
        onClick = onClick,
        modifier = Modifier.fillMaxWidth()
    ) {
        Row(
            modifier = Modifier.padding(16.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Icon(Icons.Default.Devices, contentDescription = null)
            Spacer(Modifier.width(16.dp))
            Column {
                Text(terminal.name, style = MaterialTheme.typography.titleMedium)
                Text(
                    if (terminal.isLocal) "Local terminal" else "SSH: ${terminal.serverId}",
                    style = MaterialTheme.typography.bodySmall
                )
                terminal.tmuxSessionName?.let {
                    Text("tmux: $it", style = MaterialTheme.typography.bodySmall)
                }
            }
        }
    }
}

/**
 * Terminal entry from LIST_RESPONSE JSON.
 * terminal_id is a u32 from the desktop protocol server.
 */
data class TerminalEntry(
    val id: Int,
    val name: String,
    val serverId: String,
    val isLocal: Boolean,
    val tmuxSessionName: String?,
)

@Serializable
private data class TerminalJson(
    val terminal_id: Int? = null,
    val id: Int? = null,
    val name: String? = null,
    val server_id: String? = null,
    val is_local: Boolean? = null,
    val tmux_session_name: String? = null,
)

private val terminalListJson = Json { ignoreUnknownKeys = true; isLenient = true }

/**
 * Parse LIST_RESPONSE JSON payload into TerminalEntry list.
 *
 * JSON format (from desktop protocol server):
 * [{"terminal_id": 1, "name": "main", "server_id": "srv1", "is_local": false,
 *   "tmux_session_name": "main"}, ...]
 */
internal fun parseTerminalList(json: String): List<TerminalEntry> {
    return try {
        val items = terminalListJson.decodeFromString<List<TerminalJson>>(json)
        items.map { item ->
            TerminalEntry(
                id = item.terminal_id ?: item.id ?: -1,
                name = item.name ?: "Terminal",
                serverId = item.server_id ?: "",
                isLocal = item.is_local ?: false,
                tmuxSessionName = item.tmux_session_name?.ifEmpty { null },
            )
        }
    } catch (_: Exception) {
        emptyList()
    }
}

/**
 * Check if an error string indicates tmux unavailability or desktop offline.
 *
 * Matches:
 * - "tmux" (e.g. "tmux session X not found", "tmux not installed")
 * - "tmux_unavailable" / "multi_terminal" / "desktop_offline" (defensive —
 *   future desktop protocol error codes)
 * - "terminal_not_found" (desktop terminal no longer exists — similar UX)
 * - "hello_required" / "hello_already_done" (tunnel handshake issues —
 *   user should retry, which is the same action as tmux unavailable)
 *
 * Returns false for generic errors like "invalid_terminal_id" or "input_failed"
 * which are per-operation errors, not tunnel-level unavailability.
 */
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
