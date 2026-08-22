package com.termfast.app.ui.screen

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.navigation.NavController
import com.termfast.app.data.PairingStore
import com.termfast.app.data.RemoteTunnelManager
import com.termfast.app.data.RustEvent
import com.termfast.app.data.RustRepository
import com.termfast.app.data.TunnelState
import com.termfast.app.service.RemoteTunnelService
import kotlinx.coroutines.launch
import org.json.JSONObject

// === SECTION 1 END ===

// === SECTION 2: main composable ===

/**
 * Agent Approval Screen — shown when the user taps a "AI needs input" notification.
 *
 * Renders the question and options based on the CLI type (P8):
 * - devin/opencode → single-select option list (tap to select, auto-submit)
 * - claude-code/codex → text input field + submit button
 * - unknown/shell → text input fallback (D5)
 *
 * On submit: sends INPUT_ANSWER frame via RemoteTunnelManager.
 * On QUESTION_RESOLVED event: auto-close (another device answered first).
 *
 * R3: questionId is used to look up the cached NOTIFY payload from RemoteTunnelService.
 * 边界-1: If cache is lost (process killed), fall back to Intent extras (cli + question).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AgentApprovalScreen(
    navController: NavController,
    questionId: String,
    fallbackCli: String = "",
    fallbackQuestion: String = "",
) {
    val scope = rememberCoroutineScope()
    val snackbarHostState = remember { SnackbarHostState() }
    var submitting by remember { mutableStateOf(false) }
    var resolved by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }

    // R3: Load payload from RemoteTunnelService cache, fall back to Intent extras (边界-1)
    val payload = remember {
        RemoteTunnelService.getPayload(questionId) ?: JSONObject().apply {
            put("cli", fallbackCli)
            put("question", fallbackQuestion)
            put("question_id", questionId)
            put("options", org.json.JSONArray())
            put("terminal_id", 0)
            put("pairing_id", "")
        }
    }

    val cli = payload.optString("cli", fallbackCli.ifEmpty { "unknown" })
    val question = payload.optString("question", fallbackQuestion)
    val optionsArr = payload.optJSONArray("options")
    val options = remember(optionsArr) {
        if (optionsArr != null) {
            (0 until optionsArr.length()).mapNotNull { optionsArr.optString(it).takeIf { s -> s.isNotEmpty() } }
        } else emptyList()
    }
    val terminalId = payload.optInt("terminal_id", 0)
    val pairingId = payload.optString("pairing_id", "")

    // Text input state for claude-code/codex/unknown/shell modes
    var textAnswer by remember { mutableStateOf("") }

    // P8: Determine UI mode based on cli
    val isOptionMode = cli == "devin" || cli == "opencode"
    val isTextMode = !isOptionMode  // claude-code, codex, unknown, shell → text input (D5)

    // Listen for QUESTION_RESOLVED event to auto-close
    LaunchedEffect(questionId) {
        RustRepository.events.collect { event ->
            if (event is RustEvent.RemoteTerminalNotify) {
                try {
                    val json = JSONObject(event.message)
                    val eventType = json.optString("event_type")
                    if (eventType == "agent_resolved") {
                        val resolvedQid = json.optString("question_id")
                        if (resolvedQid == questionId) {
                            resolved = true
                            // Clear notification + cache
                            val nm = androidx.core.app.NotificationManagerCompat.from(
                                navController.context
                            )
                            nm.cancel(questionId.hashCode())
                            RemoteTunnelService.removePayload(questionId)
                            // Auto-navigate back after short delay
                            kotlinx.coroutines.delay(500)
                            navController.popBackStack()
                            return@collect
                        }
                    }
                } catch (_: Exception) {}
            }
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("AI 需要输入", maxLines = 1, overflow = TextOverflow.Ellipsis) },
                navigationIcon = {
                    IconButton(onClick = { navController.popBackStack() }) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "返回")
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.surface,
                ),
            )
        },
        snackbarHost = { SnackbarHost(snackbarHostState) },
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .verticalScroll(rememberScrollState())
                .padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            // CLI badge
            Row(verticalAlignment = Alignment.CenterVertically) {
                Surface(
                    shape = RoundedCornerShape(4.dp),
                    color = MaterialTheme.colorScheme.primaryContainer,
                ) {
                    Text(
                        text = cli.uppercase(),
                        modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp),
                        fontSize = 12.sp,
                        fontWeight = FontWeight.Bold,
                        color = MaterialTheme.colorScheme.onPrimaryContainer,
                    )
                }
                Spacer(Modifier.width(8.dp))
                Text(
                    text = "terminal #$terminalId",
                    fontSize = 12.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }

            // Question text
            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.surfaceVariant,
                ),
            ) {
                Text(
                    text = question,
                    modifier = Modifier.padding(16.dp),
                    fontSize = 15.sp,
                    lineHeight = 22.sp,
                )
            }

            // P8: Option mode — single select list
            if (isOptionMode && options.isNotEmpty()) {
                Text(
                    "选择一个选项：",
                    fontSize = 14.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                options.forEachIndexed { index, option ->
                    OptionItem(
                        text = option,
                        enabled = !submitting && !resolved,
                        onClick = {
                            if (submitting || resolved) return@OptionItem
                            submitting = true
                            scope.launch {
                                val success = sendAnswer(
                                    pairingId, terminalId, questionId, option,
                                    optionIndex = index,
                                    cli = cli,
                                    options = options.toTypedArray(),
                                )
                                submitting = false
                                if (success) {
                                    // Wait for QUESTION_RESOLVED event to auto-close
                                    // Or close after 2 seconds if no event
                                    kotlinx.coroutines.delay(2000)
                                    if (!resolved) {
                                        RemoteTunnelService.removePayload(questionId)
                                        navController.popBackStack()
                                    }
                                } else {
                                    error = "发送失败，请重试"
                                    snackbarHostState.showSnackbar("发送失败，请检查连接")
                                }
                            }
                        },
                    )
                }
            }

            // P8: Text mode — text input + submit button (claude-code/codex/unknown/shell)
            if (isTextMode) {
                Text(
                    "输入你的回答：",
                    fontSize = 14.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                OutlinedTextField(
                    value = textAnswer,
                    onValueChange = { textAnswer = it },
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = 120.dp),
                    placeholder = { Text("输入回答...") },
                    enabled = !submitting && !resolved,
                    maxLines = 8,
                )
                Button(
                    onClick = {
                        if (textAnswer.isBlank() || submitting || resolved) return@Button
                        submitting = true
                        scope.launch {
                            val success = sendAnswer(
                                pairingId, terminalId, questionId, textAnswer,
                                cli = cli,
                                options = options.toTypedArray(),
                            )
                            submitting = false
                            if (success) {
                                kotlinx.coroutines.delay(2000)
                                if (!resolved) {
                                    RemoteTunnelService.removePayload(questionId)
                                    navController.popBackStack()
                                }
                            } else {
                                snackbarHostState.showSnackbar("发送失败，请检查连接")
                            }
                        }
                    },
                    enabled = textAnswer.isNotBlank() && !submitting && !resolved,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    if (submitting) {
                        CircularProgressIndicator(
                            modifier = Modifier.size(20.dp),
                            strokeWidth = 2.dp,
                            color = MaterialTheme.colorScheme.onPrimary,
                        )
                    } else {
                        Text("提交")
                    }
                }
            }

            // Dismiss button (B2: __dismissed__ → no PTY write)
            TextButton(
                onClick = {
                    if (submitting || resolved) return@TextButton
                    submitting = true
                    scope.launch {
                        val success = sendAnswer(pairingId, terminalId, questionId, "__dismissed__")
                        submitting = false
                        if (success) {
                            RemoteTunnelService.removePayload(questionId)
                            navController.popBackStack()
                        }
                    }
                },
                enabled = !submitting && !resolved,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Icon(Icons.Filled.Close, contentDescription = null, modifier = Modifier.size(18.dp))
                Spacer(Modifier.width(4.dp))
                Text("忽略")
            }

            if (resolved) {
                Text(
                    "已被回答",
                    modifier = Modifier.fillMaxWidth(),
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    fontSize = 14.sp,
                )
            }
        }
    }
}

// === SECTION 2 END ===

// === SECTION 3: helper functions + OptionItem composable ===

/**
 * Send an INPUT_ANSWER frame via the tunnel manager for the given pairing.
 * E2: Includes semantic metadata (cli, option_index, options) so desktop
 * frontend can use cliBehavior to generate correct keystrokes.
 * Returns true if the frame was sent successfully.
 */
private suspend fun sendAnswer(
    pairingId: String,
    terminalId: Int,
    questionId: String,
    answer: String,
    optionIndex: Int = 0,
    cli: String = "unknown",
    options: Array<String> = emptyArray(),
    isMultiSelect: Boolean = false,
    isMultiQuestion: Boolean = false,
): Boolean {
    if (pairingId.isEmpty()) return false
    val manager = TerminalSessionManager.getTunnelManager(pairingId) ?: return false
    return manager.sendInputAnswer(
        terminalId, questionId, answer, optionIndex, cli, options, isMultiSelect, isMultiQuestion,
    )
}

/**
 * A single option item in the option list (P8: devin/opencode single-select mode).
 */
@Composable
private fun OptionItem(
    text: String,
    enabled: Boolean,
    onClick: () -> Unit,
) {
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(enabled = enabled, onClick = onClick),
        colors = CardDefaults.cardColors(
            containerColor = if (enabled)
                MaterialTheme.colorScheme.surface
            else
                MaterialTheme.colorScheme.surfaceVariant,
        ),
        border = androidx.compose.foundation.BorderStroke(
            1.dp,
            MaterialTheme.colorScheme.outlineVariant,
        ),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = text,
                modifier = Modifier.weight(1f),
                fontSize = 15.sp,
                lineHeight = 22.sp,
                color = if (enabled)
                    MaterialTheme.colorScheme.onSurface
                else
                    MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (!enabled) {
                CircularProgressIndicator(
                    modifier = Modifier.size(20.dp),
                    strokeWidth = 2.dp,
                )
            }
        }
    }
}

// === SECTION 3 END ===

