package com.termfast.app.ui.screen

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Computer
import androidx.compose.material.icons.filled.Terminal
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.navigation.NavController

/**
 * SSH terminal picker dialog — shows existing terminal sessions for a local SSH
 * server, with a "new terminal" button. Similar UI to RemoteTerminalPickerDialog
 * but for locally-managed SSH connections (no relay tunnel needed).
 *
 * Flow:
 * 1. Show list of existing sessions for this serverId
 * 2. Click a session → navigate to terminal/{serverId}/{sessionId}
 * 3. Click "新建终端" → create new session and navigate
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SshTerminalPickerDialog(
    visible: Boolean,
    serverId: String,
    serverName: String,
    navController: NavController,
    onDismiss: () -> Unit,
) {
    if (!visible) return

    val sessions = remember { TerminalSessionManager.getSessions(serverId) }
    var creating by remember { mutableStateOf(false) }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Filled.Computer, contentDescription = null, modifier = Modifier.size(20.dp), tint = MaterialTheme.colorScheme.primary)
                Spacer(Modifier.width(8.dp))
                Text(serverName, fontWeight = FontWeight.Bold)
            }
        },
        text = {
            Column {
                if (sessions.isEmpty()) {
                    Text("没有打开的终端", fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    Spacer(Modifier.height(4.dp))
                    Text("点击下方按钮新建终端", fontSize = 11.sp, color = MaterialTheme.colorScheme.outline)
                } else {
                    sessions.forEach { session ->
                        Surface(
                            modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
                            shape = RoundedCornerShape(8.dp),
                            tonalElevation = 1.dp,
                            onClick = {
                                onDismiss()
                                navController.navigate("terminal/$serverId/${session.sessionId}")
                            },
                        ) {
                            Row(
                                modifier = Modifier.padding(12.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Icon(
                                    Icons.Filled.Terminal,
                                    contentDescription = null,
                                    modifier = Modifier.size(20.dp),
                                    tint = if (session.connected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outline,
                                )
                                Spacer(Modifier.width(12.dp))
                                Column(modifier = Modifier.weight(1f)) {
                                    Text(
                                        session.name.ifBlank { "终端" },
                                        fontFamily = FontFamily.Monospace,
                                        fontSize = 13.sp,
                                        fontWeight = FontWeight.Medium,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                    )
                                    Text(
                                        if (session.connected) "在线" else "离线",
                                        fontSize = 11.sp,
                                        color = if (session.connected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error,
                                    )
                                }
                            }
                        }
                    }
                }
            }
        },
        confirmButton = {
            if (creating) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    CircularProgressIndicator(modifier = Modifier.size(16.dp), strokeWidth = 2.dp)
                    Spacer(Modifier.width(8.dp))
                    Text("正在新建...", fontSize = 12.sp)
                }
            } else {
                TextButton(onClick = {
                    creating = true
                    val newSessionId = TerminalSessionManager.getOrCreateSession(serverId)
                    onDismiss()
                    navController.navigate("terminal/$serverId/$newSessionId")
                }) {
                    Icon(Icons.Filled.Add, contentDescription = null, modifier = Modifier.size(18.dp))
                    Spacer(Modifier.width(4.dp))
                    Text("新建终端")
                }
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("取消") }
        },
    )
}
