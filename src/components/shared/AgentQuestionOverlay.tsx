// AgentQuestionOverlay — floating popup showing AI CLI questions + answer options
//
// When an AI CLI is blocked (needs user input), this overlay appears at the
// bottom-right of the terminal area, showing:
//   - The CLI name + status icon
//   - The question text (extracted from screen)
//   - Answer option buttons (extracted from screen)
//
// Three modes:
//   1. Single-select (default): click an option → submitAnswer → dismiss
//   2. Multi-select (isMultiSelect=true): checkboxes + Submit button
//      Click toggles selection locally; Submit sends all toggles + confirms
//   3. Type your own answer: if user clicks "Type your own answer" option,
//      overlay switches to text input mode with a Send button
//
// When the user clicks an option, answerSubmitter generates the keystrokes
// and TerminalView sends them to the PTY.

import { memo, useState, useEffect, useRef } from "react";
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
  /** True if the current blocked dialog is multi-select. */
  isMultiSelect: boolean;
  /** True if the current blocked dialog is multi-question (has Confirm tab). */
  isMultiQuestion: boolean;
  /** Active tab index in multi-question dialog (-1 if unknown). */
  activeTabIndex: number;
  /** Total number of tabs in multi-question dialog (0 if not multi-question). */
  totalTabs: number;
  /** Review answers extracted from the Confirm tab (null if not on Confirm tab). */
  reviewAnswers: string[] | null;
  /** Blocked message (from OSC 777 for Devin, or screen scrape). */
  blockedMessage: string | null;
  /** Called when user clicks an answer option (single-select). */
  onAnswer: (option: string, index: number) => void;
  /** Called when user toggles an option in multi-select mode.
   *  The parent sends the toggle keystrokes to the PTY. */
  onToggle: (option: string, index: number) => void;
  /** Called when user clicks Submit in multi-select mode.
   *  The parent sends the confirm keystrokes (Tab + Enter) to the PTY. */
  onSubmitMultiSelect: () => void;
  /** Called when user submits a text answer (Type your own answer).
   *  The parent navigates to the option, enters text mode, and types. */
  onTextAnswer: (option: string, text: string, index: number) => void;
  /** Called when user cancels text input mode ("Back to options").
   *  The parent sends Escape to the terminal to exit the CLI's text editing
   *  mode (Devin's 'e' select+type mode). */
  onTextCancel: () => void;
  /** Called when user clicks "Previous question" (multi-question mode). */
  onPrevQuestion: () => void;
  /** Called when user clicks "Next question" (multi-question mode). */
  onNextQuestion: () => void;
  /** Called when user clicks the final action, with whether any answer was entered. */
  onConfirm: (hasAnswers: boolean) => void;
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

/** Check if an option is the "Type your own answer" entry.
 *  Matches:
 *  - "Type your own answer" (OpenCode)
 *  - "Other (type your own)" (Devin)
 *  - "Type something." (Claude Code v2.1+ multi-question)
 *  - "Tell Claude what to change" (Claude Code Plan Mode) */
function isTypeYourOwnAnswer(option: string): boolean {
  return /type\s+your\s+own/i.test(option)
    || /type\s+something/i.test(option)
    || /tell\s+\S+\s+what\s+to\s+change/i.test(option);
}

/** Strip "[ ]" or "[✔]" checkbox prefix from a multi-select option label.
 *  "1. [✔] 选项A" → "1. 选项A"
 *  "5. [ ] Type something" → "5. Type something" */
function stripCheckbox(option: string): string {
  return option.replace(/^(\d+\.\s*)\[[\s✓]\]\s*/, "$1");
}

/** Check if a multi-select option has a [✔] checkbox (is checked on screen). */
function isCheckedOnScreen(option: string): boolean {
  return /^\d+\.\s*\[✔\]/.test(option);
}

function AgentQuestionOverlayImpl({
  visible,
  status,
  cli,
  question,
  options,
  isMultiSelect,
  isMultiQuestion,
  activeTabIndex,
  totalTabs,
  reviewAnswers,
  blockedMessage,
  onAnswer,
  onToggle,
  onSubmitMultiSelect,
  onTextAnswer,
  onTextCancel,
  onPrevQuestion,
  onNextQuestion,
  onConfirm,
  onDismiss,
}: AgentQuestionOverlayProps) {
  const { t } = useTranslation();
  // Multi-select: track which options are checked locally
  const [checked, setChecked] = useState<Set<number>>(new Set());
  // Track text answers typed via "Type your own answer" (option index → text)
  const [textAnswers, setTextAnswers] = useState<Map<number, string>>(new Map());
  // Cache checked/textAnswers per tab in multi-question mode.
  // When navigating between questions (←→), the questionKey changes and
  // triggers a reset. We cache per-tab state so switching back restores
  // the previously checked options and text answers.
  const tabStateRef = useRef<Map<number, { checked: Set<number>; textAnswers: Map<number, string> }>>(new Map());
  const prevTabRef = useRef<number>(-1);
  // Single-select answers submit immediately and advance tabs, so track them
  // separately from the per-tab multi-select state.
  // Map: tabIndex → optionIndex (which option was selected on that tab)
  const answeredTabsRef = useRef<Map<number, number>>(new Map());

  // In multi-question mode, determine which tab is the "last question" tab.
  // OpenCode has a separate "Confirm" tab at the end (totalTabs - 1),
  // so the last question is at totalTabs - 2.
  // Devin has NO Confirm tab — all tabs are question tabs,
  // so the last question is at totalTabs - 1.
  // Claude Code has a "Submit" tab at the end (totalTabs - 1),
  // so the last question is at totalTabs - 2 (same as OpenCode).
  const lastQuestionIndex = cli === "devin" ? totalTabs - 1 : totalTabs - 2;
  const isLastQuestion = isMultiQuestion && totalTabs > 0 && activeTabIndex === lastQuestionIndex;
  // First/last question detection for Devin — hide Prev/Next to prevent wrap.
  // Devin's ←→ is circular (wraps around), and there's no Confirm tab.
  // OpenCode has a Confirm tab at the end, so Next on last question is useful.
  const isFirstQuestion = isMultiQuestion && totalTabs > 0 && activeTabIndex === 0;
  // For Claude Code multi-select with only 1 question tab + Submit tab
  // (totalTabs === 2), hide both Prev and Next — there's only one question,
  // so navigation buttons are meaningless. The "确认提交" button is enough.
  const isSingleQuestionMultiSelect = cli === "claude-code" && isMultiSelect && totalTabs === 2;
  // Submit tab (last tab): keep Prev (to go back and modify answers),
  // hide Next (there's nothing after Submit).
  const isSubmitTab = cli === "claude-code" && isMultiQuestion && activeTabIndex === totalTabs - 1;
  const hidePrev = (cli === "devin" && isFirstQuestion) || isSingleQuestionMultiSelect;
  const hideNext = (cli === "devin" && isLastQuestion) || isSingleQuestionMultiSelect || isSubmitTab;
  // Text input mode: when user clicks "Type your own answer"
  const [textMode, setTextMode] = useState(false);
  const [textModeIndex, setTextModeIndex] = useState(-1);
  const [textOption, setTextOption] = useState<string>("");
  const [textValue, setTextValue] = useState("");
  // Ref to prevent resetting textMode when screen redraws during text editing
  // (Devin's text editing mode removes option numbers, causing question extraction
  // to fail and questionKey to change to a fallback — we don't want that to
  // close the text input)
  const textModeRef = useRef(false);
  useEffect(() => { textModeRef.current = textMode; }, [textMode]);

  // Reset local state when the question changes (new question in sequence).
  // Skip the reset if we're in text editing mode AND the new questionKey is
  // empty or a fallback string — this happens when Devin redraws the screen
  // during text editing (option numbers disappear, question extraction fails).
  // A real question change (next question in multi-question mode) will have
  // a non-empty, non-fallback questionKey and will correctly reset textMode.
  const questionKey = question ?? blockedMessage ?? "";

  // Clear all cached state when the overlay closes.
  // The overlay is "closed" when either:
  //   1. visible becomes false (dismissed by user or after submit), OR
  //   2. status becomes non-blocked (AI resumed working)
  // This ensures the next dialog starts fresh, regardless of how the
  // previous dialog ended.
  const prevVisibleRef = useRef(false);
  const prevStatusRef = useRef<AgentStatus>("unknown");
  useEffect(() => {
    const closed = (!visible && prevVisibleRef.current) ||
      (status !== "blocked" && prevStatusRef.current === "blocked");
    if (closed) {
      tabStateRef.current = new Map();
      prevTabRef.current = -1;
      answeredTabsRef.current = new Map();
      setChecked(new Set());
      setTextAnswers(new Map());
      setTextMode(false);
      setTextModeIndex(-1);
      setTextOption("");
      setTextValue("");
      textModeRef.current = false;
    }
    prevVisibleRef.current = visible;
    prevStatusRef.current = status;
  }, [visible, status]);

  // When activeTabIndex changes in multi-question mode, cache the old tab's
  // state and restore the new tab's cached state (if any).
  useEffect(() => {
    if (!isMultiQuestion) return;
    const prevTab = prevTabRef.current;
    const currTab = activeTabIndex;
    if (prevTab === currTab) return;
    // Cache old tab state
    if (prevTab >= 0) {
      tabStateRef.current.set(prevTab, {
        checked: new Set(checked),
        textAnswers: new Map(textAnswers),
      });
    }
    // Restore new tab state
    const cached = tabStateRef.current.get(currTab);
    if (cached) {
      setChecked(new Set(cached.checked));
      setTextAnswers(new Map(cached.textAnswers));
    } else {
      setChecked(new Set());
      setTextAnswers(new Map());
    }
    setTextMode(false);
    setTextValue("");
    prevTabRef.current = currTab;
  }, [activeTabIndex, isMultiQuestion]);

  useEffect(() => {
    if (textModeRef.current) {
      // In text mode: only skip reset if the new key is a fallback
      // (empty, null, or "Devin is asking a question" / "OpenCode is asking a question")
      const isFallback = !questionKey || /is asking a question/i.test(questionKey);
      if (isFallback) return;
      // Real question change while in text mode — reset
    }
    // Only reset if NOT in multi-question mode (multi-question handles reset
    // in the activeTabIndex effect above to preserve per-tab state)
    if (!isMultiQuestion) {
      setChecked(new Set());
      setTextAnswers(new Map());
    }
    setTextMode(false);
    setTextValue("");
  }, [questionKey]);

  // Sync checked state from screen for Claude Code multi-select.
  // Claude Code's options include [✔] markers that indicate which options
  // are actually checked on screen. We sync our local `checked` Set with
  // these markers so the overlay reflects the real state (e.g. after the
  // user toggles options directly in the terminal, or after text input
  // changes the screen state).
  useEffect(() => {
    if (!isMultiSelect || !options || textModeRef.current) return;
    const screenChecked = new Set<number>();
    options.forEach((option, index) => {
      if (isCheckedOnScreen(option)) {
        screenChecked.add(index);
      }
    });
    setChecked(screenChecked);
  }, [options, isMultiSelect]);

  if (!visible || status !== "blocked") return null;

  const cliName = CLI_NAMES[cli] ?? "AI";
  const displayQuestion = question || blockedMessage || t("server.agent_question_default");
  const hasAnyAnswers = answeredTabsRef.current.size > 0 || checked.size > 0 || textAnswers.size > 0 ||
    [...tabStateRef.current.values()].some((state) => state.checked.size > 0 || state.textAnswers.size > 0);

  // ── Single-select: click option → submit ──────────────────────────
  const handleSingleSelect = (option: string, index: number) => {
    if (isTypeYourOwnAnswer(option)) {
      // Switch to text input mode
      setTextOption(option);
      setTextModeIndex(index);
      setTextMode(true);
      setTextValue("");
      return;
    }
    if (isMultiQuestion && activeTabIndex >= 0) {
      answeredTabsRef.current.set(activeTabIndex, index);
    }
    onAnswer(option, index);
  };

  // ── Multi-select: toggle checkbox ──────────────────────────────────
  const handleMultiToggle = (option: string, index: number) => {
    // Detect "Type something" input option:
    // 1. By keyword ("Type something" / "Type your own answer")
    // 2. By textAnswers — if user previously typed text on this option,
    //    the screen label changes to the typed value (e.g. "121212"),
    //    so keyword detection fails. textAnswers remembers which index
    //    is the text input option.
    if (isTypeYourOwnAnswer(option) || textAnswers.has(index)) {
      // Switch to text input mode
      setTextOption(option);
      setTextModeIndex(index);
      setTextMode(true);
      // Pre-fill with existing text answer if any
      setTextValue(textAnswers.get(index) ?? "");
      return;
    }
    // Toggle local checked state
    setChecked((prev) => {
      const next = new Set(prev);
      if (next.has(index)) {
        next.delete(index);
      } else {
        next.add(index);
      }
      return next;
    });
    // Send toggle keystroke to PTY
    onToggle(option, index);
  };

  // ── Multi-select: Submit ───────────────────────────────────────────
  const handleMultiSubmit = () => {
    onSubmitMultiSelect();
    setChecked(new Set());
  };

  // ── Text answer: Send ──────────────────────────────────────────────
  const handleTextSubmit = () => {
    if (textValue.trim()) {
      if (isMultiQuestion && activeTabIndex >= 0) {
        answeredTabsRef.current.set(activeTabIndex, textModeIndex);
      }
      onTextAnswer(textOption, textValue.trim(), textModeIndex);
      // Mark the option as checked and store the text answer
      setChecked((prev) => new Set(prev).add(textModeIndex));
      setTextAnswers((prev) => new Map(prev).set(textModeIndex, textValue.trim()));
      setTextMode(false);
      setTextValue("");
    }
  };

  // ── Text answer: Cancel ───────────────────────────────────────────
  const handleTextCancel = () => {
    // Notify the terminal to exit text editing mode (e.g. Devin's 'e' mode)
    onTextCancel();
    setTextMode(false);
    setTextValue("");
  };

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
              {cliName} {t("server.agent_needs_input")}
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

        {/* Text input mode (Type your own answer) */}
        {textMode && (
          <div className="px-4 pb-3">
            <div className="flex gap-2">
              <input
                type="text"
                value={textValue}
                onChange={(e) => setTextValue(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    handleTextSubmit();
                  }
                  if (e.key === "Escape") {
                    e.preventDefault();
                    handleTextCancel();
                  }
                }}
                placeholder={t("server.agent_type_answer_placeholder")}
                autoFocus
                className="flex-1 px-3 py-2 text-sm rounded-lg bg-gray-800 text-gray-100 border border-gray-700 focus:border-blue-500 focus:outline-none transition-colors"
              />
              <button
                className="px-4 py-2 text-sm rounded-lg bg-blue-600 hover:bg-blue-500 text-white transition-colors"
                onClick={handleTextSubmit}
              >
                {t("server.agent_type_answer_submit")}
              </button>
              <button
                className="px-3 py-2 text-sm rounded-lg bg-gray-700 hover:bg-gray-600 text-gray-300 transition-colors whitespace-nowrap"
                onClick={handleTextCancel}
              >
                {t("server.agent_back_to_options")}
              </button>
            </div>
          </div>
        )}

        {/* Options — multi-select mode (checkboxes + Submit/Confirm) */}
        {!textMode && isMultiSelect && options && options.length > 0 && (
          <div className="px-4 pb-3 flex flex-col gap-2">
            {options.map((option, index) => {
              const textAns = textAnswers.get(index);
              const displayOption = stripCheckbox(option);
              return (
                <button
                  key={index}
                  className="flex items-center gap-2 px-4 py-2 text-sm rounded-lg bg-gray-800 hover:bg-gray-700 text-gray-100 border border-gray-700 hover:border-gray-600 transition-colors text-left"
                  onClick={() => handleMultiToggle(option, index)}
                >
                  <span className={`inline-flex items-center justify-center w-4 h-4 rounded border ${checked.has(index) ? "bg-blue-600 border-blue-500" : "border-gray-600"}`}>
                    {checked.has(index) && (
                      <svg className="w-3 h-3 text-white" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={3}>
                        <path strokeLinecap="round" strokeLinejoin="round" d="M5 13l4 4L19 7" />
                      </svg>
                    )}
                  </span>
                  <span className="flex-1">
                    {displayOption}
                    {textAns && (
                      <span className="text-blue-400 ml-2">: {textAns}</span>
                    )}
                  </span>
                </button>
              );
            })}
            {/* Multi-question navigation + Confirm */}
            {isMultiQuestion ? (
              <div className="flex gap-2 mt-2">
                {!hidePrev && (
                  <button
                    className="flex-1 px-3 py-2 text-sm rounded-lg bg-gray-700 hover:bg-gray-600 text-gray-200 transition-colors"
                    onClick={onPrevQuestion}
                  >
                    ← {t("server.agent_prev_question")}
                  </button>
                )}
                {!hideNext && (
                  <button
                    className="flex-1 px-3 py-2 text-sm rounded-lg bg-gray-700 hover:bg-gray-600 text-gray-200 transition-colors"
                    onClick={onNextQuestion}
                  >
                    {t("server.agent_next_question")} →
                  </button>
                )}
                {isLastQuestion && isMultiSelect && (
                  <button
                    className="flex-1 px-3 py-2 text-sm rounded-lg bg-green-600 hover:bg-green-500 text-white transition-colors font-medium"
                    onClick={() => onConfirm(hasAnyAnswers)}
                  >
                    ✓ {t("server.agent_confirm")}
                  </button>
                )}
                {isLastQuestion && !isMultiSelect && cli === "devin" && !hasAnyAnswers && (
                  <button
                    className="flex-1 px-3 py-2 text-sm rounded-lg bg-gray-600 hover:bg-gray-500 text-gray-200 transition-colors"
                    onClick={() => onConfirm(hasAnyAnswers)}
                  >
                    {t("server.agent_skip")} (Esc)
                  </button>
                )}
                {isLastQuestion && !isMultiSelect && cli === "devin" && hasAnyAnswers && (
                  <div
                    className="flex-1 px-3 py-2 text-sm rounded-lg bg-gray-700 text-gray-400 cursor-not-allowed text-center"
                    title={t("server.agent_last_question_hint")}
                  >
                    {t("server.agent_select_to_submit")}
                  </div>
                )}
                {isLastQuestion && !isMultiSelect && cli !== "devin" && (
                  <button
                    className="flex-1 px-3 py-2 text-sm rounded-lg bg-green-600 hover:bg-green-500 text-white transition-colors font-medium"
                    onClick={() => onConfirm(hasAnyAnswers)}
                  >
                    ✓ {t("server.agent_confirm")}
                  </button>
                )}
              </div>
            ) : (
              <button
                className="w-full px-4 py-2 text-sm rounded-lg bg-blue-600 hover:bg-blue-500 text-white transition-colors mt-1"
                onClick={handleMultiSubmit}
              >
                {t("server.agent_submit")}
              </button>
            )}
          </div>
        )}

        {/* Options — single-select mode (click to submit) */}
        {!textMode && !isMultiSelect && options && options.length > 0 && (
          <div className="px-4 pb-3 flex flex-col gap-2">
            {options.map((option, index) => {
              const selectedAnswer = isMultiQuestion ? answeredTabsRef.current.get(activeTabIndex) : undefined;
              const isSelected = selectedAnswer === index;
              return (
                <button
                  key={index}
                  className={`px-4 py-2 text-sm rounded-lg border transition-colors text-left ${
                    isSelected
                      ? "bg-blue-900 border-blue-500 text-blue-100"
                      : "bg-gray-800 hover:bg-gray-700 text-gray-100 border-gray-700 hover:border-gray-600"
                  }`}
                  onClick={() => handleSingleSelect(option, index)}
                >
                  {isSelected && "✓ "}{option}
                </button>
              );
            })}
            {/* Multi-question navigation + Confirm */}
            {isMultiQuestion && (
              <div className="flex gap-2 mt-2">
                {!hidePrev && (
                  <button
                    className="flex-1 px-3 py-2 text-sm rounded-lg bg-gray-700 hover:bg-gray-600 text-gray-200 transition-colors"
                    onClick={onPrevQuestion}
                  >
                    ← {t("server.agent_prev_question")}
                  </button>
                )}
                {!hideNext && (
                  <button
                    className="flex-1 px-3 py-2 text-sm rounded-lg bg-gray-700 hover:bg-gray-600 text-gray-200 transition-colors"
                    onClick={onNextQuestion}
                  >
                    {t("server.agent_next_question")} →
                  </button>
                )}
                {isLastQuestion && isMultiSelect && (
                  <button
                    className="flex-1 px-3 py-2 text-sm rounded-lg bg-green-600 hover:bg-green-500 text-white transition-colors font-medium"
                    onClick={() => onConfirm(hasAnyAnswers)}
                  >
                    ✓ {t("server.agent_confirm")}
                  </button>
                )}
                {isLastQuestion && !isMultiSelect && cli === "devin" && !hasAnyAnswers && (
                  <button
                    className="flex-1 px-3 py-2 text-sm rounded-lg bg-gray-600 hover:bg-gray-500 text-gray-200 transition-colors"
                    onClick={() => onConfirm(hasAnyAnswers)}
                  >
                    {t("server.agent_skip")} (Esc)
                  </button>
                )}
                {isLastQuestion && !isMultiSelect && cli === "devin" && hasAnyAnswers && (
                  <div
                    className="flex-1 px-3 py-2 text-sm rounded-lg bg-gray-700 text-gray-400 cursor-not-allowed text-center"
                    title={t("server.agent_last_question_hint")}
                  >
                    {t("server.agent_select_to_submit")}
                  </div>
                )}
                {isLastQuestion && !isMultiSelect && cli !== "devin" && (
                  <button
                    className="flex-1 px-3 py-2 text-sm rounded-lg bg-green-600 hover:bg-green-500 text-white transition-colors font-medium"
                    onClick={() => onConfirm(hasAnyAnswers)}
                  >
                    ✓ {t("server.agent_confirm")}
                  </button>
                )}
              </div>
            )}
          </div>
        )}

        {/* Fallback: if no options extracted (e.g. on the Confirm tab) */}
        {(!options || options.length === 0) && !textMode && (
          <div className="px-4 pb-3 flex flex-col gap-2">
            {/* Multi-question Confirm tab: show review answers + Confirm button */}
            {isMultiQuestion && (
              <div className="flex flex-col gap-1">
                <p className="text-sm text-gray-300 leading-relaxed">
                  {t("server.agent_review_answers")}
                </p>
                {reviewAnswers && reviewAnswers.length > 0 && (
                  <ul className="text-sm text-gray-200 leading-relaxed list-none space-y-1 mt-1">
                    {reviewAnswers.map((ans, i) => (
                      <li key={i} className="px-2 py-1 rounded bg-gray-800/60 border border-gray-700">
                        {ans}
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            )}
            <div className="flex gap-2">
              {isMultiQuestion && !hidePrev && (
                <button
                  className="flex-1 px-3 py-2 text-sm rounded-lg bg-gray-700 hover:bg-gray-600 text-gray-200 transition-colors"
                  onClick={onPrevQuestion}
                >
                  ← {t("server.agent_prev_question")}
                </button>
              )}
              <button
                className={`flex-1 px-4 py-2 text-sm rounded-lg text-white transition-colors font-medium ${
                  isMultiQuestion
                    ? "bg-green-600 hover:bg-green-500"
                    : "bg-blue-600 hover:bg-blue-500"
                }`}
                onClick={isMultiQuestion ? () => onConfirm(hasAnyAnswers) : onDismiss}
              >
                {isMultiQuestion ? `✓ ${t("server.agent_confirm")}` : t("server.agent_go_to_terminal")}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

export const AgentQuestionOverlay = memo(AgentQuestionOverlayImpl);
