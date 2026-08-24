package com.termfast.app.ui.screen

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.ArrowForward
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
    val initialPayload = remember {
        RemoteTunnelService.getPayload(questionId) ?: JSONObject().apply {
            put("cli", fallbackCli)
            put("question", fallbackQuestion)
            put("question_id", questionId)
            put("options", org.json.JSONArray())
            put("terminal_id", 0)
            put("pairing_id", "")
        }
    }

    val cli = initialPayload.optString("cli", fallbackCli.ifEmpty { "unknown" })
    val terminalId = initialPayload.optInt("terminal_id", 0)
    val pairingId = initialPayload.optString("pairing_id", "")
    // Multi-question metadata (opencode multi-question dialog)
    val isMultiQuestion = initialPayload.optBoolean("is_multi_question", false)

    // Mutable state for question content — updated when new agent_blocked arrives (multi-question tab change)
    var currentQuestionId by remember { mutableStateOf(questionId) }
    var currentQuestion by remember { mutableStateOf(initialPayload.optString("question", fallbackQuestion)) }
    var currentOptions by remember {
        mutableStateOf<List<String>>(
            run {
                val arr = initialPayload.optJSONArray("options")
                if (arr != null) {
                    (0 until arr.length()).mapNotNull { arr.optString(it).takeIf { s -> s.isNotEmpty() } }
                } else emptyList()
            }
        )
    }
    var currentActiveTab by remember { mutableStateOf(initialPayload.optInt("active_tab_index", -1)) }
    var currentTotalTabs by remember { mutableStateOf(initialPayload.optInt("total_tabs", 0)) }

    // Multi-question UI state
    var selectedOptionIndex by remember { mutableStateOf(-1) }  // -1 = none selected
    var confirmed by remember { mutableStateOf(false) }  // true after "确认提交" sent

    // Text input state for claude-code/codex/unknown/shell modes
    var textAnswer by remember { mutableStateOf("") }

    // P8: Determine UI mode based on cli
    // devin/opencode/claude-code/codex → option mode: clickable options + text input
    // unknown/shell → text mode: text input + submit button only
    val isOptionMode = cli == "devin" || cli == "opencode" || cli == "claude-code" || cli == "codex"

    // Listen for agent_resolved and agent_blocked events
    // Multi-question: on agent_resolved, don't close — wait for new agent_blocked with next question
    // Single-question: on agent_resolved, auto-close
    LaunchedEffect(questionId) {
        RustRepository.events.collect { event ->
            if (event is RustEvent.RemoteTerminalNotify) {
                try {
                    val json = JSONObject(event.message)
                    val eventType = json.optString("event_type")
                    if (eventType == "agent_resolved") {
                        val resolvedQid = json.optString("question_id")
                        if (resolvedQid == currentQuestionId) {
                            if (isMultiQuestion && !confirmed) {
                                // Multi-question: question answered, waiting for next tab
                                // Don't close — new agent_blocked will update the screen
                                resolved = true
                            } else {
                                // Single-question or confirmed: close
                                resolved = true
                                val nm = androidx.core.app.NotificationManagerCompat.from(
                                    navController.context
                                )
                                nm.cancel(questionId.hashCode())
                                RemoteTunnelService.removePayload(questionId)
                                kotlinx.coroutines.delay(500)
                                navController.popBackStack()
                                return@collect
                            }
                        }
                    } else if (eventType == "agent_blocked" && isMultiQuestion) {
                        // Multi-question: new question arrived (tab advanced)
                        val newTerminalId = json.optInt("terminal_id", 0)
                        if (newTerminalId == terminalId) {
                            val newQid = json.optString("question_id", "")
                            if (newQid.isNotEmpty() && newQid != currentQuestionId) {
                                // Update question content for new tab
                                currentQuestionId = newQid
                                currentQuestion = json.optString("question", currentQuestion)
                                val newArr = json.optJSONArray("options")
                                currentOptions = if (newArr != null) {
                                    (0 until newArr.length()).mapNotNull { newArr.optString(it).takeIf { s -> s.isNotEmpty() } }
                                } else currentOptions
                                currentActiveTab = json.optInt("active_tab_index", currentActiveTab)
                                currentTotalTabs = json.optInt("total_tabs", currentTotalTabs)
                                selectedOptionIndex = -1
                                resolved = false
                                submitting = false
                                // Update cache for new question
                                json.put("pairing_id", pairingId)
                                RemoteTunnelService.cachePayload(newQid, json)
                            }
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
                // Multi-question progress indicator: "问题 1/3"
                if (isMultiQuestion && currentTotalTabs > 0 && currentActiveTab >= 0) {
                    Spacer(Modifier.width(8.dp))
                    Surface(
                        shape = RoundedCornerShape(4.dp),
                        color = MaterialTheme.colorScheme.tertiaryContainer,
                    ) {
                        Text(
                            text = "问题 ${currentActiveTab + 1}/$currentTotalTabs",
                            modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp),
                            fontSize = 12.sp,
                            fontWeight = FontWeight.Bold,
                            color = MaterialTheme.colorScheme.onTertiaryContainer,
                        )
                    }
                }
            }

            // Question text
            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.surfaceVariant,
                ),
            ) {
                Text(
                    text = currentQuestion,
                    modifier = Modifier.padding(16.dp),
                    fontSize = 15.sp,
                    lineHeight = 22.sp,
                )
            }

            // P8: Option mode — single select list (shown above the text input)
            if (isOptionMode && currentOptions.isNotEmpty()) {
                Text(
                    "选择一个选项：",
                    fontSize = 14.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                currentOptions.forEachIndexed { index, option ->
                    val isSelected = selectedOptionIndex == index
                    OptionItem(
                        text = option,
                        enabled = !submitting && !resolved,
                        selected = isSelected,
                        onClick = {
                            if (submitting || resolved) return@OptionItem
                            selectedOptionIndex = index
                            if (isMultiQuestion) {
                                // Multi-question: send answer (number key auto-advances tab),
                                // don't close — wait for new agent_blocked with next question
                                submitting = true
                                scope.launch {
                                    val success = sendAnswer(
                                        pairingId, terminalId, currentQuestionId, option,
                                        optionIndex = index,
                                        cli = cli,
                                        options = currentOptions.toTypedArray(),
                                        isMultiQuestion = true,
                                        activeTabIndex = currentActiveTab,
                                        totalTabs = currentTotalTabs,
                                    )
                                    submitting = false
                                    if (!success) {
                                        selectedOptionIndex = -1
                                        snackbarHostState.showSnackbar("发送失败，请检查连接")
                                    }
                                    // On success: wait for agent_resolved + agent_blocked events
                                    // (handled by LaunchedEffect above)
                                }
                            } else {
                                // Single-question: send + close
                                submitting = true
                                scope.launch {
                                    val success = sendAnswer(
                                        pairingId, terminalId, currentQuestionId, option,
                                        optionIndex = index,
                                        cli = cli,
                                        options = currentOptions.toTypedArray(),
                                    )
                                    submitting = false
                                    if (success) {
                                        kotlinx.coroutines.delay(2000)
                                        if (!resolved) {
                                            RemoteTunnelService.removePayload(questionId)
                                            navController.popBackStack()
                                        }
                                    } else {
                                        selectedOptionIndex = -1
                                        snackbarHostState.showSnackbar("发送失败，请检查连接")
                                    }
                                }
                            }
                        },
                    )
                }
            }

            // Text input — shown in text mode (unknown/shell) as primary input,
            // AND in option mode (devin/opencode/claude-code/codex) as "自定义回答" below options.
            if (!isOptionMode || currentOptions.isNotEmpty()) {
                Text(
                    if (isOptionMode) "自定义回答：" else "输入你的回答：",
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
                                pairingId, terminalId, currentQuestionId, textAnswer,
                                cli = cli,
                                options = currentOptions.toTypedArray(),
                                isMultiQuestion = isMultiQuestion,
                                activeTabIndex = currentActiveTab,
                                totalTabs = currentTotalTabs,
                            )
                            submitting = false
                            if (success) {
                                if (isMultiQuestion) {
                                    // Multi-question: don't close, wait for next agent_blocked
                                    selectedOptionIndex = -1
                                    textAnswer = ""
                                } else {
                                    kotlinx.coroutines.delay(2000)
                                    if (!resolved) {
                                        RemoteTunnelService.removePayload(questionId)
                                        navController.popBackStack()
                                    }
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

            // Multi-question navigation buttons (上一题/下一题) + 确认提交
            if (isMultiQuestion && isOptionMode) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    // 上一题
                    OutlinedButton(
                        onClick = {
                            if (submitting || resolved) return@OutlinedButton
                            submitting = true
                            scope.launch {
                                sendAnswer(
                                    pairingId, terminalId, currentQuestionId, "__nav_prev__",
                                    cli = cli,
                                    options = currentOptions.toTypedArray(),
                                    isMultiQuestion = true,
                                    activeTabIndex = currentActiveTab,
                                    totalTabs = currentTotalTabs,
                                )
                                submitting = false
                                // Wait for new agent_blocked with prev tab content
                            }
                        },
                        enabled = !submitting && !resolved && currentActiveTab > 0,
                        modifier = Modifier.weight(1f),
                    ) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = null, modifier = Modifier.size(18.dp))
                        Spacer(Modifier.width(4.dp))
                        Text("上一题")
                    }
                    // 下一题
                    OutlinedButton(
                        onClick = {
                            if (submitting || resolved) return@OutlinedButton
                            submitting = true
                            scope.launch {
                                sendAnswer(
                                    pairingId, terminalId, currentQuestionId, "__nav_next__",
                                    cli = cli,
                                    options = currentOptions.toTypedArray(),
                                    isMultiQuestion = true,
                                    activeTabIndex = currentActiveTab,
                                    totalTabs = currentTotalTabs,
                                )
                                submitting = false
                                // Wait for new agent_blocked with next tab content
                            }
                        },
                        enabled = !submitting && !resolved && currentActiveTab >= 0 && currentActiveTab < currentTotalTabs - 1,
                        modifier = Modifier.weight(1f),
                    ) {
                        Text("下一题")
                        Spacer(Modifier.width(4.dp))
                        Icon(Icons.AutoMirrored.Filled.ArrowForward, contentDescription = null, modifier = Modifier.size(18.dp))
                    }
                }
                // 确认提交
                Button(
                    onClick = {
                        if (submitting || resolved) return@Button
                        confirmed = true
                        submitting = true
                        scope.launch {
                            val success = sendAnswer(
                                pairingId, terminalId, currentQuestionId, "__confirm__",
                                cli = cli,
                                options = currentOptions.toTypedArray(),
                                isMultiQuestion = true,
                                activeTabIndex = currentActiveTab,
                                totalTabs = currentTotalTabs,
                            )
                            submitting = false
                            if (!success) {
                                confirmed = false
                                snackbarHostState.showSnackbar("发送失败，请检查连接")
                            }
                            // On success: agent_resolved will arrive → close (confirmed=true)
                        }
                    },
                    enabled = !submitting && !resolved,
                    modifier = Modifier.fillMaxWidth(),
                    colors = ButtonDefaults.buttonColors(
                        containerColor = MaterialTheme.colorScheme.primary,
                    ),
                ) {
                    if (submitting && confirmed) {
                        CircularProgressIndicator(
                            modifier = Modifier.size(20.dp),
                            strokeWidth = 2.dp,
                            color = MaterialTheme.colorScheme.onPrimary,
                        )
                    } else {
                        Icon(Icons.Filled.Check, contentDescription = null, modifier = Modifier.size(18.dp))
                        Spacer(Modifier.width(4.dp))
                        Text("确认提交")
                    }
                }
            }

            // Dismiss button (B2: __dismissed__ → no PTY write)
            TextButton(
                onClick = {
                    if (submitting || resolved) return@TextButton
                    submitting = true
                    scope.launch {
                        val success = sendAnswer(pairingId, terminalId, currentQuestionId, "__dismissed__")
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

            if (resolved && !isMultiQuestion) {
                Text(
                    "已被回答",
                    modifier = Modifier.fillMaxWidth(),
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    fontSize = 14.sp,
                )
            }
            if (resolved && isMultiQuestion && !confirmed) {
                Text(
                    "已回答，等待下一题...",
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
    activeTabIndex: Int = -1,
    totalTabs: Int = 0,
): Boolean {
    android.util.Log.d("termfast", "sendAnswer: pairingId=$pairingId terminalId=$terminalId questionId=$questionId answer=$answer isMultiQuestion=$isMultiQuestion activeTab=$activeTabIndex totalTabs=$totalTabs")
    if (pairingId.isEmpty()) {
        android.util.Log.e("termfast", "sendAnswer: pairingId is empty!")
        return false
    }
    val manager = TerminalSessionManager.getTunnelManager(pairingId)
    if (manager == null) {
        android.util.Log.e("termfast", "sendAnswer: no tunnel manager for pairingId=$pairingId")
        return false
    }
    val success = manager.sendInputAnswer(
        terminalId, questionId, answer, optionIndex, cli, options, isMultiSelect, isMultiQuestion,
        activeTabIndex, totalTabs,
    )
    android.util.Log.d("termfast", "sendAnswer: manager.sendInputAnswer returned $success")
    return success
}

/**
 * A single option item in the option list (P8: devin/opencode single-select mode).
 */
@Composable
private fun OptionItem(
    text: String,
    enabled: Boolean,
    selected: Boolean = false,
    onClick: () -> Unit,
) {
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(enabled = enabled, onClick = onClick),
        colors = CardDefaults.cardColors(
            containerColor = when {
                selected -> MaterialTheme.colorScheme.primaryContainer
                enabled -> MaterialTheme.colorScheme.surface
                else -> MaterialTheme.colorScheme.surfaceVariant
            },
        ),
        border = androidx.compose.foundation.BorderStroke(
            if (selected) 2.dp else 1.dp,
            if (selected) MaterialTheme.colorScheme.primary
            else MaterialTheme.colorScheme.outlineVariant,
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
                color = when {
                    selected -> MaterialTheme.colorScheme.onPrimaryContainer
                    enabled -> MaterialTheme.colorScheme.onSurface
                    else -> MaterialTheme.colorScheme.onSurfaceVariant
                },
            )
            if (selected) {
                Icon(Icons.Filled.Check, contentDescription = null, modifier = Modifier.size(20.dp),
                    tint = MaterialTheme.colorScheme.primary)
            } else if (!enabled) {
                CircularProgressIndicator(
                    modifier = Modifier.size(20.dp),
                    strokeWidth = 2.dp,
                )
            }
        }
    }
}

// === SECTION 3 END ===

