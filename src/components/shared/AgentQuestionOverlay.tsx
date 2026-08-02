// AgentQuestionOverlay — floating popup showing AI CLI questions + answer options
//
// When an AI CLI is blocked (needs user input), this overlay appears at the
// bottom-right of the terminal area, showing:
//   - The CLI name + status icon
//   - The question text (extracted from screen)
//   - Answer option buttons (extracted from screen)
//
// When the user clicks an option, answerSubmitter generates the keystrokes
// and TerminalView sends them to the PTY.

import { memo, useCallback } from "react";
import { useTranslation } from "react-i18next";
import type { AgentStatus, CliType } from "@/hooks/agentStateMachine";

interface AgentQuestionOverlayProps {
  /** Whether the overlay should be visible. */
  visible: boolean;
  /** Current agent status. */
  status: AgentStatus;
  /** Which CLI is running. */
  cli: CliType;
  /** Question text extracted from screen. */
  question: string | null;
  /** Answer options extracted from screen. */
  options: string[] | null;
  /** Blocked message (from OSC 777 for Devin, or screen scrape). */
  blockedMessage: string | null;
  /** Called when user clicks an answer option. */
  onAnswer: (option: string, index: number) => void;
  /** Called when user dismisses the overlay without answering. */
  onDismiss: () => void;
}

const CLI_NAMES: Record<CliType, string> = {
  unknown: "AI",
  devin: "Devin",
  opencode: "OpenCode",
  "claude-code": "Claude Code",
  codex: "Codex",
  shell: "Shell",
};

function AgentQuestionOverlayImpl({
  visible,
  status,
  cli,
  question,
  options,
  blockedMessage,
  onAnswer,
  onDismiss,
}: AgentQuestionOverlayProps) {
  const { t } = useTranslation();

  if (!visible || status !== "blocked") return null;

  const cliName = CLI_NAMES[cli] ?? "AI";
  const displayQuestion = question || blockedMessage || t("agent_question_default");

  return (
    <div
      className="absolute bottom-4 right-4 z-20 max-w-md pointer-events-auto"
      role="dialog"
      aria-label="agent-question"
    >
      <div className="rounded-lg bg-gray-900/95 dark:bg-gray-950/95 backdrop-blur-sm border border-red-500/50 shadow-2xl overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-2.5 bg-red-500/10 border-b border-red-500/30">
          <div className="flex items-center gap-2">
            <span className="inline-block w-2 h-2 rounded-full bg-red-500 animate-pulse" />
            <span className="text-sm font-medium text-red-400">
              {cliName} {t("agent_needs_input")}
            </span>
          </div>
          <button
            className="text-gray-400 hover:text-white transition-colors text-lg leading-none"
            onClick={onDismiss}
            aria-label="dismiss"
          >
            ×
          </button>
        </div>

        {/* Question text */}
        <div className="px-4 py-3">
          <p className="text-sm text-gray-200 leading-relaxed whitespace-pre-wrap">
            {displayQuestion}
          </p>
        </div>

        {/* Options */}
        {options && options.length > 0 && (
          <div className="px-4 pb-3 flex flex-col gap-2">
            {options.map((option, index) => (
              <button
                key={index}
                className="px-4 py-2 text-sm rounded-lg bg-gray-800 hover:bg-gray-700 text-gray-100 border border-gray-700 hover:border-gray-600 transition-colors text-left"
                onClick={() => onAnswer(option, index)}
              >
                {option}
              </button>
            ))}
          </div>
        )}

        {/* Fallback: if no options extracted, show a "Go to terminal" button */}
        {(!options || options.length === 0) && (
          <div className="px-4 pb-3">
            <button
              className="w-full px-4 py-2 text-sm rounded-lg bg-blue-600 hover:bg-blue-500 text-white transition-colors"
              onClick={onDismiss}
            >
              {t("agent_go_to_terminal")}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

export const AgentQuestionOverlay = memo(AgentQuestionOverlayImpl);
