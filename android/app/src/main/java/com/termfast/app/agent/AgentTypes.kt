package com.termfast.app.agent

/**
 * CLI type detected from OSC 0 title or screen patterns.
 * Mirrors desktop `CliType` from oscParser.ts.
 */
enum class CliType {
    UNKNOWN,
    DEVIN,
    OPENCODE,
    CLAUDE_CODE,
    CODEX,
    SHELL,
}

/**
 * Agent status — external status shown to the UI.
 * Mirrors desktop `AgentStatus` from agentStateMachine.ts.
 */
enum class AgentStatus {
    UNKNOWN,
    IDLE,
    WORKING,
    BLOCKED,
    DONE,
}

/**
 * Signal emitted by an AI CLI via OSC or screen detection.
 * Mirrors desktop `AgentSignal` from oscParser.ts.
 */
sealed class AgentSignal {
    abstract val cli: CliType

    data class Blocked(override val cli: CliType, val message: String) : AgentSignal()
    data class Done(override val cli: CliType) : AgentSignal()
    data class Title(override val cli: CliType, val title: String) : AgentSignal()
    data class Notify(override val cli: CliType, val message: String, val done: Boolean) : AgentSignal()
}

/**
 * Tab info extracted from multi-question dialog tab rows.
 * @param labels tab label strings
 * @param activeIndex 0-based index of the active tab (-1 if detection failed)
 */
data class TabInfo(
    val labels: List<String>,
    val activeIndex: Int,
)

/**
 * Agent status state — the output of AgentStatusMonitor.
 * Consumed by AgentQuestionSheet (FP7) for rendering.
 */
data class AgentStatusState(
    val status: AgentStatus = AgentStatus.UNKNOWN,
    val cli: CliType = CliType.UNKNOWN,
    val question: String? = null,
    val options: List<String>? = null,
    val isMultiSelect: Boolean = false,
    val isMultiQuestion: Boolean = false,
    val activeTabIndex: Int = -1,
    val totalTabs: Int = 0,
    val cursorIndex: Int? = null,
    val reviewAnswers: List<String>? = null,
    val blockedMessage: String? = null,
) {
    companion object {
        val IDLE = AgentStatusState(status = AgentStatus.IDLE)
    }
}
