package com.termfast.app.ui.screen

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.termfast.app.data.RustRepository
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

@Serializable
data class TmuxSessionInfo(
    val name: String = "",
    val description: String = "",
    val created: Long = 0,
    val server: String = "",
    val size: List<Int> = emptyList(),
    val windows: Int = 0,
    val attached_count: Int = 0,
    val last_activity: Long = 0,
)

@Serializable
data class TmuxListResponse(
    val sessions: List<TmuxSessionInfo> = emptyList(),
    val tmux_installed: Boolean = false,
    val error: String? = null,
)

@Serializable
data class TmuxNewSessionResponse(
    val session_id: String = "",
    val tmux_session_name: String? = null,
    val error: String? = null,
)

/**
 * tmux session picker dialog — shown when tmux_mode="ask" and there are
 * existing TermFast-tagged tmux sessions on the server.
 *
 * On select: calls onAttach(sessionId, tmuxSessionName)
 * On create: calls onCreate(sessionId, description)
 * On skip: calls onSkip()
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TmuxSessionPickerDialog(
    visible: Boolean,
    serverId: String,
    sessionId: String,
    onAttach: (sessionId: String, tmuxSessionName: String) -> Unit,
    onCreate: (sessionId: String, description: String) -> Unit,
    onSkip: () -> Unit,
    onDismiss: () -> Unit,
) {
    if (!visible) return

    val scope = rememberCoroutineScope()
    var loading by remember { mutableStateOf(true) }
    var sessions by remember { mutableStateOf<List<TmuxSessionInfo>>(emptyList()) }
    var tmuxInstalled by remember { mutableStateOf(true) }
    var showCreateForm by remember { mutableStateOf(false) }
    var description by remember { mutableStateOf("") }
    var actionInProgress by remember { mutableStateOf(false) }

    LaunchedEffect(visible) {
        if (visible) {
            loading = true
            scope.launch(Dispatchers.IO) {
                val json = RustRepository.tmuxListSessions(serverId)
                withContext(Dispatchers.Main) {
                    try {
                        val resp = Json.decodeFromString<TmuxListResponse>(json)
                        sessions = resp.sessions
                        tmuxInstalled = resp.tmux_installed
                    } catch (e: Exception) {
                        sessions = emptyList()
                        tmuxInstalled = false
                    }
                    loading = false
                }
            }
        }
    }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = {
            Text("tmux 会话", fontWeight = FontWeight.Bold)
        },
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
                        CircularProgressIndicator()
                    }
                    return@Column
                }

                if (!tmuxInstalled) {
                    Text("服务器未安装 tmux", color = MaterialTheme.colorScheme.error)
                    return@Column
                }

                if (showCreateForm) {
                    Text("创建新会话", fontWeight = FontWeight.Medium, fontSize = 14.sp)
                    Spacer(Modifier.height(8.dp))
                    OutlinedTextField(
                        value = description,
                        onValueChange = { description = it },
                        label = { Text("会话描述（可选）") },
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true,
                    )
                    Spacer(Modifier.height(12.dp))
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.End,
                    ) {
                        TextButton(onClick = { showCreateForm = false }) {
                            Text("取消")
                        }
                        Spacer(Modifier.width(8.dp))
                        Button(
                            onClick = {
                                actionInProgress = true
                                scope.launch(Dispatchers.IO) {
                                    // Reuse the current sessionId instead of
                                    // creating a new one — avoids orphan sessions
                                    val result = RustRepository.tmuxNewSession(
                                        serverId, sessionId, description, 80, 24,
                                    )
                                    withContext(Dispatchers.Main) {
                                        actionInProgress = false
                                        try {
                                            val resp = Json.decodeFromString<TmuxNewSessionResponse>(result)
                                            if (resp.error == null) {
                                                onAttach(sessionId, resp.tmux_session_name ?: "")
                                            }
                                        } catch (e: Exception) {
                                            // ignore
                                        }
                                    }
                                }
                            },
                            enabled = !actionInProgress,
                        ) {
                            Text("创建")
                        }
                    }
                    return@Column
                }

                Text(
                    "发现已有的 TermFast 会话，选择一个恢复或创建新会话",
                    fontSize = 12.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(8.dp))

                sessions.forEach { s ->
                    Surface(
                        modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
                        shape = RoundedCornerShape(8.dp),
                        tonalElevation = 1.dp,
                        onClick = {
                            if (!actionInProgress) {
                                actionInProgress = true
                                scope.launch(Dispatchers.IO) {
                                    // Check if another terminal is already attached
                                    // to this tmux session. If so, reuse it instead
                                    // of creating a duplicate card.
                                    val existing = com.termfast.app.ui.screen.TerminalSessionManager
                                        .findSessionByTmuxName(serverId, s.name)
                                    if (existing != null) {
                                        // Clean up the empty session created by "+" button
                                        com.termfast.app.ui.screen.TerminalSessionManager
                                            .closeSessionBySessionId(sessionId)
                                        withContext(Dispatchers.Main) {
                                            actionInProgress = false
                                            onAttach(existing.sessionId, s.name)
                                        }
                                        return@launch
                                    }
                                    // No existing terminal for this tmux session — attach
                                    val result = RustRepository.tmuxAttachSession(
                                        serverId, sessionId, s.name, 80, 24,
                                    )
                                    withContext(Dispatchers.Main) {
                                        actionInProgress = false
                                        try {
                                            val resp = Json.decodeFromString<TmuxNewSessionResponse>(result)
                                            if (resp.error == null) {
                                                onAttach(sessionId, s.name)
                                            }
                                        } catch (e: Exception) {
                                            // ignore
                                        }
                                    }
                                }
                            }
                        },
                    ) {
                        Column(modifier = Modifier.padding(12.dp)) {
                            Row(modifier = Modifier.fillMaxWidth()) {
                                Text(
                                    s.name,
                                    fontFamily = FontFamily.Monospace,
                                    fontSize = 13.sp,
                                    fontWeight = FontWeight.Medium,
                                    modifier = Modifier.weight(1f),
                                )
                                if (s.description.isNotBlank()) {
                                    Text(
                                        "— ${s.description}",
                                        fontSize = 13.sp,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                            }
                            Spacer(Modifier.height(4.dp))
                            Text(
                                "创建: ${formatDate(s.created)}  窗口: ${s.windows}  " +
                                    "已连接: ${s.attached_count}  活动: ${formatDate(s.last_activity)}",
                                fontSize = 11.sp,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }

                if (sessions.isEmpty()) {
                    Text("没有可恢复的会话", fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }

                Spacer(Modifier.height(8.dp))
                TextButton(onClick = { showCreateForm = true }) {
                    Text("创建新会话")
                }
            }
        },
        confirmButton = {
            TextButton(onClick = onSkip) {
                Text("直接连接（不使用 tmux）")
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) {
                Text("取消")
            }
        },
    )
}

private fun formatDate(timestamp: Long): String {
    if (timestamp == 0L) return "—"
    val sdf = SimpleDateFormat("MM-dd HH:mm", Locale.getDefault())
    return sdf.format(Date(timestamp * 1000))
}
