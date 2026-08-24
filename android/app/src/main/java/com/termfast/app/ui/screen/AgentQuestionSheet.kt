package com.termfast.app.ui.screen

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
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
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.snapshots.SnapshotStateMap
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.termfast.app.agent.AgentAction
import com.termfast.app.agent.AgentStatusMonitor
import com.termfast.app.agent.AgentStatusState
import com.termfast.app.agent.CliBehaviorRegistry
import com.termfast.app.agent.CliType
import kotlinx.coroutines.launch

// === SECTION 1 END ===

// === SECTION 2: production AgentQuestionSheet ===

/**
 * Production AgentQuestionSheet — renders AI CLI questions from AgentStatusMonitor.
 *
 * Replaces the demo version (hardcoded DemoQuestion data) with real data from
 * AgentStatusMonitor.getStatusState(). The sheet auto-shows when the agent
 * status is BLOCKED and auto-hides when status changes to non-BLOCKED.
 *
 * User interactions (answer, toggle, text answer, navigate, confirm) are
 * dispatched through AgentStatusMonitor.executeAction() which generates the
 * correct keystrokes for the detected CLI.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AgentQuestionSheet(
    sessionId: String,
    onDismiss: () -> Unit,
    // Hoisted state — survives show/hide cycles (caller stores these)
    activeTabIndex: MutableState<Int>,
    selectedOptions: SnapshotStateMap<Int, Int>,
    checkedMap: SnapshotStateMap<Int, Boolean>,
    textAnswers: SnapshotStateMap<Int, String>,
    textExpanded: SnapshotStateMap<Int, Boolean>,
    // Callback to send keystrokes to the PTY
    sendKeystrokes: (String) -> Unit,
    // FP9: callback to notify desktop of autonomous answer (remote mode only).
    // Sends __answered__ via sendInputAnswer so desktop closes overlay without double keypress.
    // Null for local mode (no desktop overlay to close).
    onAnsweredRemotely: (() -> Unit)? = null,
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = false)

    // Poll AgentStatusMonitor for current status state
    var statusState by remember(sessionId) { mutableStateOf(AgentStatusMonitor.getStatusState(sessionId)) }
    LaunchedEffect(sessionId) {
        while (true) {
            kotlinx.coroutines.delay(300)
            statusState = AgentStatusMonitor.getStatusState(sessionId)
        }
    }

    val cli = statusState.cli
    val behavior = remember(cli) { CliBehaviorRegistry.getBehavior(cli) }
    val isMultiSelect = statusState.isMultiSelect
    val isMultiQuestion = statusState.isMultiQuestion
    val totalTabs = if (isMultiQuestion) statusState.totalTabs else 1
    val currentTab = if (isMultiQuestion) activeTabIndex.value else 0
    val question = statusState.question ?: "Agent needs input"
    val options = statusState.options ?: emptyList()
    val isSubmitTab = isMultiQuestion && currentTab == behavior.lastQuestionIndex(totalTabs) + 1
    var submitting by remember { mutableStateOf(false) }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 20.dp)
                .verticalScroll(rememberScrollState())
                .padding(bottom = 28.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            // ── Top bar: CLI badge + 问题 N/M + nav buttons ──
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                // CLI badge
                Text(
                    cliBadgeText(cli),
                    fontSize = 11.sp,
                    fontWeight = FontWeight.Bold,
                    color = MaterialTheme.colorScheme.primary,
                    modifier = Modifier
                        .clip(RoundedCornerShape(4.dp))
                        .background(MaterialTheme.colorScheme.primaryContainer)
                        .padding(horizontal = 6.dp, vertical = 2.dp),
                )
                // 问题 N/M (only for multi-question)
                if (isMultiQuestion && totalTabs > 0) {
                    Text(
                        "问题 ${currentTab + 1}/$totalTabs",
                        fontSize = 13.sp,
                        fontWeight = FontWeight.Bold,
                        color = MaterialTheme.colorScheme.onSurface,
                    )
                }
                Spacer(Modifier.weight(1f))
                // 上一题 (multi-question only)
                if (isMultiQuestion) {
                    val uiState = com.termfast.app.agent.UiState(
                        isFirstQuestion = currentTab == 0,
                        isLastQuestion = currentTab == totalTabs - 1,
                        isMultiSelect = isMultiSelect,
                        totalTabs = totalTabs,
                        activeTabIndex = currentTab,
                    )
                    if (!behavior.hidePrev(uiState)) {
                        OutlinedButton(
                            onClick = {
                                if (currentTab > 0) {
                                    executeAndSend(sessionId, AgentAction.PrevQuestion, sendKeystrokes, onAnsweredRemotely)
                                    activeTabIndex.value = currentTab - 1
                                }
                            },
                            enabled = !submitting && currentTab > 0,
                            contentPadding = PaddingValues(horizontal = 10.dp, vertical = 4.dp),
                        ) {
                            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = null, modifier = Modifier.size(16.dp))
                            Spacer(Modifier.width(2.dp))
                            Text("上一题", fontSize = 12.sp)
                        }
                    }
                    if (!behavior.hideNext(uiState)) {
                        OutlinedButton(
                            onClick = {
                                if (currentTab < totalTabs - 1) {
                                    executeAndSend(sessionId, AgentAction.NextQuestion, sendKeystrokes, onAnsweredRemotely)
                                    activeTabIndex.value = currentTab + 1
                                }
                            },
                            enabled = !submitting && currentTab < totalTabs - 1,
                            contentPadding = PaddingValues(horizontal = 10.dp, vertical = 4.dp),
                        ) {
                            Text("下一题", fontSize = 12.sp)
                            Spacer(Modifier.width(2.dp))
                            Icon(Icons.AutoMirrored.Filled.ArrowForward, contentDescription = null, modifier = Modifier.size(16.dp))
                        }
                    }
                }
                // 提交 (multi-question only)
                if (isMultiQuestion) {
                    Button(
                        onClick = {
                            submitting = true
                            val hasAnswers = selectedOptions.isNotEmpty() || checkedMap.values.any { it } ||
                                textAnswers.values.any { it.isNotBlank() }
                            executeAndSend(
                                sessionId = sessionId,
                                action = AgentAction.Confirm(hasAnswers),
                                sendKeystrokes = sendKeystrokes,
                                onAnsweredRemotely = onAnsweredRemotely,
                                onComplete = { submitting = false },
                            )
                        },
                        enabled = !submitting,
                        contentPadding = PaddingValues(horizontal = 12.dp, vertical = 4.dp),
                    ) {
                        Icon(Icons.Filled.Check, contentDescription = null, modifier = Modifier.size(16.dp))
                        Spacer(Modifier.width(2.dp))
                        Text("提交", fontSize = 12.sp)
                    }
                }
                // 忽略
                TextButton(
                    onClick = onDismiss,
                    enabled = !submitting,
                    contentPadding = PaddingValues(horizontal = 8.dp, vertical = 4.dp),
                ) {
                    Icon(Icons.Filled.Close, contentDescription = null, modifier = Modifier.size(16.dp))
                    Spacer(Modifier.width(2.dp))
                    Text("忽略", fontSize = 12.sp)
                }
            }

            HorizontalDivider()

            if (isSubmitTab && isMultiQuestion) {
                // ── Submit tab: review all answers ──
                Text(
                    "确认提交",
                    fontSize = 15.sp,
                    fontWeight = FontWeight.Bold,
                    color = MaterialTheme.colorScheme.onSurface,
                )
                Spacer(Modifier.height(4.dp))
                val reviewAnswers = statusState.reviewAnswers
                if (reviewAnswers != null && reviewAnswers.isNotEmpty()) {
                    // OpenCode Confirm tab: "label: values" lines
                    for (answer in reviewAnswers) {
                        Text(
                            answer,
                            fontSize = 13.sp,
                            color = MaterialTheme.colorScheme.onSurface,
                            modifier = Modifier.padding(vertical = 2.dp),
                        )
                    }
                } else {
                    // Generic review: show selected options/text for each tab
                    for (i in 0 until totalTabs - 1) {
                        val sel = selectedOptions[i]
                        val txt = textAnswers[i]
                        val checkedIndices = (0 until 20).filter { checkedMap[i * 100 + it] == true }
                        val answer = when {
                            !txt.isNullOrBlank() -> txt
                            isMultiSelect && checkedIndices.isNotEmpty() ->
                                checkedIndices.joinToString(", ") { "选项${it + 1}" }
                            sel != null && options.isNotEmpty() -> options.getOrNull(sel) ?: "（未选择）"
                            else -> "（未选择）"
                        }
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            Text(
                                "问题${i + 1}",
                                modifier = Modifier.width(48.dp),
                                fontSize = 12.sp,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                            Text(
                                answer,
                                modifier = Modifier.weight(1f),
                                fontSize = 12.sp,
                                color = MaterialTheme.colorScheme.onSurface,
                            )
                        }
                    }
                }
            } else {
                // ── Question text ──
                Text(
                    question,
                    fontSize = 14.sp,
                    lineHeight = 20.sp,
                    color = MaterialTheme.colorScheme.onSurface,
                )

                // ── Options ──
                options.forEachIndexed { index, option ->
                    if (isMultiSelect) {
                        val checkKey = currentTab * 100 + index
                        val checked = checkedMap[checkKey] == true
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clip(RoundedCornerShape(8.dp))
                                .clickable(enabled = !submitting) {
                                    checkedMap[checkKey] = !checked
                                    executeAndSend(sessionId, AgentAction.Toggle(option, index), sendKeystrokes, onAnsweredRemotely)
                                }
                                .padding(horizontal = 8.dp, vertical = 6.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Checkbox(
                                checked = checked,
                                onCheckedChange = {
                                    checkedMap[checkKey] = it
                                    executeAndSend(sessionId, AgentAction.Toggle(option, index), sendKeystrokes, onAnsweredRemotely)
                                },
                                enabled = !submitting,
                                modifier = Modifier.size(28.dp),
                            )
                            Spacer(Modifier.width(4.dp))
                            Text(
                                option,
                                fontSize = 13.sp,
                                color = if (checked) MaterialTheme.colorScheme.primary
                                    else MaterialTheme.colorScheme.onSurface,
                                fontWeight = if (checked) FontWeight.Medium else FontWeight.Normal,
                            )
                        }
                    } else {
                        val isSelected = selectedOptions[currentTab] == index
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clip(RoundedCornerShape(8.dp))
                                .background(
                                    if (isSelected) MaterialTheme.colorScheme.primaryContainer
                                    else androidx.compose.ui.graphics.Color.Transparent
                                )
                                .clickable(enabled = !submitting) {
                                    selectedOptions[currentTab] = index
                                    textExpanded[currentTab] = false
                                    executeAndSend(sessionId, AgentAction.Answer(option, index), sendKeystrokes, onAnsweredRemotely)
                                    // Multi-question: auto-advance to next tab
                                    if (isMultiQuestion && currentTab < totalTabs - 1) {
                                        activeTabIndex.value = currentTab + 1
                                    }
                                }
                                .padding(horizontal = 12.dp, vertical = 8.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(
                                option,
                                modifier = Modifier.weight(1f),
                                fontSize = 13.sp,
                                color = if (isSelected) MaterialTheme.colorScheme.onPrimaryContainer
                                    else MaterialTheme.colorScheme.onSurface,
                                fontWeight = if (isSelected) FontWeight.Medium else FontWeight.Normal,
                            )
                            if (isSelected) {
                                Icon(
                                    Icons.Filled.Check,
                                    contentDescription = null,
                                    modifier = Modifier.size(16.dp),
                                    tint = MaterialTheme.colorScheme.primary,
                                )
                            }
                        }
                    }
                }

                // ── Custom text answer (collapsed by default) ──
                val expanded = textExpanded[currentTab] == true
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(8.dp))
                        .clickable(enabled = !submitting) {
                            textExpanded[currentTab] = !expanded
                        }
                        .padding(horizontal = 12.dp, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(
                        Icons.Filled.Edit,
                        contentDescription = null,
                        modifier = Modifier.size(16.dp),
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Spacer(Modifier.width(8.dp))
                    Text(
                        "自定义回答",
                        fontSize = 13.sp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    if (!expanded) {
                        Spacer(Modifier.weight(1f))
                        Box(
                            modifier = Modifier
                                .width(60.dp)
                                .height(20.dp)
                                .clip(RoundedCornerShape(4.dp))
                                .background(MaterialTheme.colorScheme.surfaceVariant),
                        )
                    }
                }
                AnimatedVisibility(
                    visible = expanded,
                    enter = expandVertically() + fadeIn(),
                    exit = shrinkVertically() + fadeOut(),
                ) {
                    var localText by remember(currentTab) { mutableStateOf(textAnswers[currentTab] ?: "") }
                    OutlinedTextField(
                        value = localText,
                        onValueChange = {
                            localText = it
                            textAnswers[currentTab] = it
                        },
                        modifier = Modifier.fillMaxWidth(),
                        placeholder = { Text("输入自定义回答...", fontSize = 13.sp) },
                        enabled = !submitting,
                        maxLines = 4,
                        textStyle = androidx.compose.ui.text.TextStyle(fontSize = 13.sp),
                        trailingIcon = {
                            TextButton(
                                onClick = {
                                    if (localText.isNotBlank()) {
                                        textAnswers[currentTab] = localText
                                        // Find the "Other" option index (usually last)
                                        val otherIndex = options.indexOfLast { true } + 1
                                        val otherOption = if (otherIndex > 0) options[otherIndex - 1] else "Other"
                                        executeAndSend(
                                            sessionId,
                                            AgentAction.TextAnswer(otherOption, localText, otherIndex - 1),
                                            sendKeystrokes,
                                            onAnsweredRemotely,
                                        )
                                        if (isMultiQuestion && currentTab < totalTabs - 1) {
                                            activeTabIndex.value = currentTab + 1
                                        }
                                    }
                                },
                                enabled = !submitting && localText.isNotBlank(),
                                contentPadding = PaddingValues(horizontal = 8.dp, vertical = 4.dp),
                            ) {
                                Text("发送", fontSize = 12.sp)
                            }
                        },
                    )
                }
            }

            if (submitting) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.Center,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    CircularProgressIndicator(modifier = Modifier.size(18.dp), strokeWidth = 2.dp)
                    Spacer(Modifier.width(8.dp))
                    Text("发送中...", fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }
    }
}

// === SECTION 2 END ===

// === SECTION 3: helpers ===

/** Convert CliType to a short badge string for the UI. */
private fun cliBadgeText(cli: CliType): String = when (cli) {
    CliType.DEVIN -> "Devin"
    CliType.OPENCODE -> "OpenCode"
    CliType.CLAUDE_CODE -> "Claude"
    CliType.CODEX -> "Codex"
    CliType.SHELL -> "Shell"
    CliType.UNKNOWN -> "Agent"
}

/**
 * Execute an agent action and send the resulting keystrokes to the PTY.
 * Handles multi-step keystrokes with delays.
 * @param onComplete called when all keystrokes have been sent (used to reset submitting state)
 */
private fun executeAndSend(
    sessionId: String,
    action: AgentAction,
    sendKeystrokes: (String) -> Unit,
    onAnsweredRemotely: (() -> Unit)? = null,
    onComplete: (() -> Unit)? = null,
) {
    val result = AgentStatusMonitor.executeAction(sessionId, action)
    val scope = kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.Dispatchers.Main)
    scope.launch {
        for ((index, step) in result.steps.withIndex()) {
            sendKeystrokes(step.data)
            if (step.delayAfter != null && index < result.steps.size - 1) {
                kotlinx.coroutines.delay(step.delayAfter)
            }
        }
        // FP9: If this action resolves the question (dismiss=true) and we're in remote mode,
        // notify desktop so it closes overlay without double keypress.
        if (result.dismiss && onAnsweredRemotely != null) {
            onAnsweredRemotely()
        }
        onComplete?.invoke()
    }
}

// === SECTION 3 END ===
