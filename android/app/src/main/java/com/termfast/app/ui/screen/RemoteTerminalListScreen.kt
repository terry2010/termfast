package com.termfast.app.ui.screen

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Devices
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.termfast.app.data.RustRepository

/**
 * Remote terminal list screen — shows terminals shared from desktop via relay tunnel.
 *
 * Fetches the list via LIST_REQUEST frame through the Rust FFI tunnel client.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RemoteTerminalListScreen(
    pairingJwt: String,
    onTerminalClick: (String) -> Unit,
    onBack: () -> Unit,
) {
    var terminals by remember { mutableStateOf<List<TerminalEntry>>(emptyList()) }
    var loading by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(pairingJwt) {
        // Request terminal list via Rust FFI (sends LIST_REQUEST frame through tunnel)
        try {
            val result = RustRepository.requestRemoteTerminalList(pairingJwt)
            // Parse JSON response
            terminals = parseTerminalList(result)
            loading = false
        } catch (e: Exception) {
            error = e.message
            loading = false
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Remote Terminals") },
                navigationIcon = {
                    TextButton(onClick = onBack) { Text("Back") }
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
                    CircularProgressIndicator(
                        modifier = Modifier.align(Alignment.Center)
                    )
                }
                error != null -> {
                    Text(
                        text = "Error: $error",
                        color = MaterialTheme.colorScheme.error,
                        modifier = Modifier.align(Alignment.Center).padding(16.dp)
                    )
                }
                terminals.isEmpty() -> {
                    Column(
                        modifier = Modifier.align(Alignment.Center),
                        horizontalAlignment = Alignment.CenterHorizontally
                    ) {
                        Icon(Icons.Default.Devices, contentDescription = null, modifier = Modifier.size(64.dp))
                        Spacer(Modifier.height(16.dp))
                        Text("No remote terminals available")
                        Text("Open a terminal on desktop and enable sharing", style = MaterialTheme.typography.bodySmall)
                    }
                }
                else -> {
                    LazyColumn(
                        modifier = Modifier.fillMaxSize(),
                        contentPadding = PaddingValues(16.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp)
                    ) {
                        items(terminals) { terminal ->
                            TerminalCard(terminal, onClick = { onTerminalClick(terminal.id) })
                        }
                    }
                }
            }
        }
    }
}

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

data class TerminalEntry(
    val id: String,
    val name: String,
    val serverId: String,
    val isLocal: Boolean,
    val tmuxSessionName: String?,
)

private fun parseTerminalList(json: String): List<TerminalEntry> {
    // Simple JSON parsing — in production use kotlinx.serialization
    val entries = mutableListOf<TerminalEntry>()
    // This is a placeholder — actual parsing happens in Rust FFI
    // The Rust side returns a JSON array of terminal info objects
    try {
        val array = org.json.JSONArray(json)
        for (i in 0 until array.length()) {
            val obj = array.getJSONObject(i)
            val tmuxSession = obj.optString("tmux_session_name", "")
            entries.add(TerminalEntry(
                id = obj.optString("session_id"),
                name = obj.optString("name", "Terminal"),
                serverId = obj.optString("server_id", ""),
                isLocal = obj.optBoolean("is_local", false),
                tmuxSessionName = tmuxSession.ifEmpty { null },
            ))
        }
    } catch (e: Exception) {
        // Return empty list on parse error
    }
    return entries
}
