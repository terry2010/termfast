// cliBehavior — per-CLI behavior abstraction for terminal interaction
//
// This module replaces the if-else chains in TerminalView/Overlay/useAgentStatus
// with a registry-based dispatch. Each CLI implements the CliBehavior interface;
// callers just do `getBehavior(cli).method(...)` without caring which CLI it is.
//
// Adding a new CLI: create a new behavior object + register it. No changes
// to TerminalView/Overlay/useAgentStatus needed (unless the new CLI introduces
// a completely new dimension of behavior — then extend the interface).

import type { CliType } from "./oscParser";
import type { AgentStatus } from "./agentStateMachine";
import {
  TEXT_ANSWER_DELAY_MS,
  TEXT_ANSWER_SUBMIT_DELAY_MS,
  submitAnswer,
  toggleDevinOption,
  submitDevinMultiSelect,
  submitDevinConfirm,
  submitDevinTextAnswer,
  navigatePrevQuestion,
  navigateNextQuestion,
  toggleOpenCodeOption,
  submitOpenCodeMultiSelect,
  submitOpenCodeConfirm,
  submitOpenCodeTextAnswer,
  toggleClaudeCodeOption,
  submitClaudeCodeMultiSelect,
  isClaudeCodePlanModeOption,
  buildClaudeCodePlanModeNavigate,
  submitClaudeCodeConfirm,
  submitClaudeCodeTextAnswer,
} from "./answerSubmitter";

// ── Types ────────────────────────────────────────────────────────────────────

/** A single keystroke to send, with an optional delay before the next step. */
export interface KeystrokeStep {
  /** The keystroke string (ANSI escape sequences allowed). */
  data: string;
  /** Milliseconds to wait AFTER sending this step before sending the next. */
  delayAfter?: number;
}

/** Result of a behavior action: what to send + UI side-effects. */
export interface ActionResult {
  /** Keystroke steps to send in order (with optional delays between them). */
  steps: KeystrokeStep[];
  /** Whether to dismiss the overlay after sending. */
  dismiss: boolean;
  /** New cursor position (Devin only — undefined = no change). */
  newCursorPos?: number;
}

/** Context passed to behavior action methods. */
export interface BehaviorContext {
  options: string[] | null;
  isMultiSelect: boolean;
  isMultiQuestion: boolean;
  activeTabIndex: number;
  totalTabs: number;
  /** Current cursor position (Devin circular list tracking). */
  cursorPos: number;
  /** Whether "Other" option is in text editing mode (Devin └ state). */
  otherEditing: boolean;
  /** Whether a text answer already exists (Claude Code re-edit). */
  hasExistingText?: boolean;
}

/** UI state for overlay rendering decisions. */
export interface UiState {
  isFirstQuestion: boolean;
  isLastQuestion: boolean;
  isMultiSelect: boolean;
  totalTabs: number;
  activeTabIndex: number;
}

// ── Interface ────────────────────────────────────────────────────────────────

export interface CliBehavior {
  // ── Keystroke actions (TerminalView calls these) ──

  /** Submit a single-select answer. */
  answer(option: string, index: number, ctx: BehaviorContext): ActionResult;
  /** Toggle a multi-select option. */
  toggle(option: string, index: number, ctx: BehaviorContext): ActionResult;
  /** Submit all multi-select toggles. */
  submitMultiSelect(ctx: BehaviorContext): ActionResult;
  /** Submit a text answer (Type your own). */
  textAnswer(option: string, text: string, index: number, ctx: BehaviorContext): ActionResult;
  /** Cancel text editing mode. */
  textCancel(ctx: BehaviorContext): ActionResult;
  /** Navigate to previous question tab. */
  prevQuestion(ctx: BehaviorContext): ActionResult;
  /** Navigate to next question tab. */
  nextQuestion(ctx: BehaviorContext): ActionResult;
  /** Confirm/submit in multi-question mode. */
  confirm(hasAnswers: boolean, ctx: BehaviorContext): ActionResult;

  // ── UI decisions (AgentQuestionOverlay reads these) ──

  /** Index of the last question tab. */
  lastQuestionIndex(totalTabs: number): number;
  /** Whether to hide the Prev button. */
  hidePrev(ui: UiState): boolean;
  /** Whether to hide the Next button. */
  hideNext(ui: UiState): boolean;
  /** Whether to sync checked state from screen markers (Claude Code [✓]). */
  readonly syncCheckedFromScreen: boolean;
  /** Whether this CLI shows a Skip button on last question (single-select, no answers). */
  readonly hasSkipOnLastQuestion: boolean;

  // ── State detection (useAgentStatus calls these) ──

  /** Whether to cache options when "Other" is selected (Devin-specific). */
  readonly cacheOptionsOnOther: boolean;
  /** Detect if "Other" option is in text editing mode. */
  detectOtherExpanded(screenText: string, isMultiSelect: boolean, isMultiQuestion: boolean): boolean;
  /** Handle OSC title signal (e.g. Codex "Action Required" = blocked). Returns null if N/A. */
  handleOscTitle(title: string): { status: AgentStatus; message: string } | null;
}

// ── Step executor ────────────────────────────────────────────────────────────

/**
 * Execute a keystroke plan: send each step in order, with optional delays
 * between steps. The send function handles logging + PTY delivery.
 */
export function executeSteps(
  steps: KeystrokeStep[],
  send: (bytes: Uint8Array) => void,
): void {
  if (steps.length === 0) return;
  const encoder = new TextEncoder();
  let i = 0;
  const sendNext = () => {
    while (i < steps.length) {
      const step = steps[i];
      send(encoder.encode(step.data));
      i++;
      if (step.delayAfter && i < steps.length) {
        setTimeout(sendNext, step.delayAfter);
        return;
      }
    }
  };
  sendNext();
}

// === SECTION 1 END ===

// ── Default behavior (unknown CLI) ───────────────────────────────────────────

const defaultBehavior: CliBehavior = {
  answer(option, index, ctx) {
    const ks = submitAnswer("unknown", option, index, ctx.options?.length, ctx.isMultiQuestion);
    return { steps: [{ data: ks }], dismiss: !ctx.isMultiQuestion };
  },
  toggle(option, index, _ctx) {
    const ks = submitAnswer("unknown", option, index);
    return { steps: [{ data: ks }], dismiss: false };
  },
  submitMultiSelect(_ctx) {
    return { steps: [{ data: "\r" }], dismiss: true };
  },
  textAnswer(option, text, _index, _ctx) {
    return {
      steps: [{ data: option + "\r" }, { data: text + "\r", delayAfter: TEXT_ANSWER_DELAY_MS }],
      dismiss: false,
    };
  },
  textCancel(_ctx) {
    return { steps: [], dismiss: false };
  },
  prevQuestion(_ctx) {
    return { steps: [{ data: navigatePrevQuestion("unknown") }], dismiss: false, newCursorPos: 0 };
  },
  nextQuestion(_ctx) {
    return { steps: [{ data: navigateNextQuestion("unknown") }], dismiss: false, newCursorPos: 0 };
  },
  confirm(_hasAnswers, _ctx) {
    return { steps: [{ data: "\r" }], dismiss: true };
  },
  lastQuestionIndex(totalTabs) { return totalTabs - 2; },
  hidePrev(_ui) { return false; },
  hideNext(_ui) { return false; },
  syncCheckedFromScreen: false,
  hasSkipOnLastQuestion: false,
  cacheOptionsOnOther: false,
  detectOtherExpanded(_text, _isMultiSelect, _isMultiQuestion) { return false; },
  handleOscTitle(_title) { return null; },
};

// ── Devin behavior ────────────────────────────────────────────────────────────

const devinBehavior: CliBehavior = {
  answer(option, index, ctx) {
    const ks = submitAnswer("devin", option, index, ctx.options?.length, ctx.isMultiQuestion);
    return { steps: [{ data: ks }], dismiss: !ctx.isMultiQuestion };
  },
  toggle(option, index, ctx) {
    const ks = toggleDevinOption(option, index, ctx.cursorPos);
    return { steps: [{ data: ks }], dismiss: false, newCursorPos: index };
  },
  submitMultiSelect(_ctx) {
    return { steps: [{ data: submitDevinMultiSelect() }], dismiss: true };
  },
  textAnswer(option, text, index, ctx) {
    const parts = submitDevinTextAnswer(
      option, text, ctx.isMultiSelect, index, ctx.options?.length, ctx.cursorPos,
    );
    return {
      steps: [{ data: parts.navigate }, { data: parts.type, delayAfter: TEXT_ANSWER_DELAY_MS }],
      dismiss: !ctx.isMultiQuestion,
      newCursorPos: 0,
    };
  },
  textCancel(_ctx) {
    return { steps: [{ data: "\x1b" }], dismiss: false };
  },
  prevQuestion(ctx) {
    if (ctx.otherEditing) {
      return {
        steps: [{ data: "\x1b[A" }, { data: "\x1b[D", delayAfter: 200 }],
        dismiss: false,
        newCursorPos: 0,
      };
    }
    return { steps: [{ data: navigatePrevQuestion("devin") }], dismiss: false, newCursorPos: 0 };
  },
  nextQuestion(ctx) {
    if (ctx.otherEditing) {
      return {
        steps: [{ data: "\x1b[A" }, { data: "\x1b[C", delayAfter: 200 }],
        dismiss: false,
        newCursorPos: 0,
      };
    }
    return { steps: [{ data: navigateNextQuestion("devin") }], dismiss: false, newCursorPos: 0 };
  },
  confirm(hasAnswers, ctx) {
    const hasOptions = !!(ctx.options && ctx.options.length > 0);
    const ks = submitDevinConfirm(hasOptions, ctx.activeTabIndex, ctx.totalTabs, ctx.isMultiSelect, hasAnswers);
    return { steps: [{ data: ks }], dismiss: true };
  },
  // Devin has NO Confirm tab — all tabs are question tabs.
  lastQuestionIndex(totalTabs) { return totalTabs - 1; },
  hidePrev(ui) { return ui.isFirstQuestion; },
  hideNext(ui) { return ui.isLastQuestion; },
  syncCheckedFromScreen: false,
  hasSkipOnLastQuestion: true,
  cacheOptionsOnOther: true,
  detectOtherExpanded(screenText, _isMultiSelect, isMultiQuestion) {
    if (!isMultiQuestion && !_isMultiSelect) return false;
    return /^\s+└/m.test(screenText);
  },
  handleOscTitle(_title) { return null; },
};

// === SECTION 2 END ===

// ── OpenCode behavior ──────────────────────────────────────────────────────────

const opencodeBehavior: CliBehavior = {
  answer(option, index, ctx) {
    const ks = submitAnswer("opencode", option, index, ctx.options?.length, ctx.isMultiQuestion);
    // OpenCode: don't dismiss after answer — status change from "blocked" to
    // "working" will naturally hide the overlay. This way, if the click didn't
    // activate the correct button (e.g. mouse hover changed focus), user can retry.
    return { steps: [{ data: ks }], dismiss: false };
  },
  toggle(option, _index, ctx) {
    const ks = toggleOpenCodeOption(option, ctx.options?.length);
    return { steps: [{ data: ks }], dismiss: false };
  },
  submitMultiSelect(_ctx) {
    return { steps: [{ data: submitOpenCodeMultiSelect() }], dismiss: true };
  },
  textAnswer(option, text, _index, ctx) {
    const parts = submitOpenCodeTextAnswer(option, text, ctx.isMultiSelect, ctx.options?.length);
    return {
      steps: [{ data: parts.navigate }, { data: parts.type, delayAfter: TEXT_ANSWER_DELAY_MS }],
      dismiss: !ctx.isMultiQuestion,
    };
  },
  textCancel(_ctx) {
    return { steps: [], dismiss: false };
  },
  prevQuestion(_ctx) {
    return { steps: [{ data: navigatePrevQuestion("opencode") }], dismiss: false, newCursorPos: 0 };
  },
  nextQuestion(_ctx) {
    return { steps: [{ data: navigateNextQuestion("opencode") }], dismiss: false, newCursorPos: 0 };
  },
  confirm(_hasAnswers, ctx) {
    const hasOptions = !!(ctx.options && ctx.options.length > 0);
    const ks = submitOpenCodeConfirm(hasOptions, ctx.activeTabIndex, ctx.totalTabs);
    return { steps: [{ data: ks }], dismiss: true };
  },
  // OpenCode has a Confirm tab at the end, so last question is totalTabs - 2.
  lastQuestionIndex(totalTabs) { return totalTabs - 2; },
  hidePrev(_ui) { return false; },
  hideNext(_ui) { return false; },
  syncCheckedFromScreen: false,
  hasSkipOnLastQuestion: false,
  cacheOptionsOnOther: false,
  detectOtherExpanded(_text, _isMultiSelect, _isMultiQuestion) { return false; },
  handleOscTitle(_title) { return null; },
};

// ── Claude Code behavior ────────────────────────────────────────────────────────

const claudeCodeBehavior: CliBehavior = {
  answer(option, index, ctx) {
    // Plan Mode: send Down arrows + Enter with delay (Ink TUI needs time
    // to process Down before Enter, otherwise Enter confirms default option).
    if (isClaudeCodePlanModeOption(option)) {
      const navKeys = buildClaudeCodePlanModeNavigate(index);
      return {
        steps: [{ data: navKeys }, { data: "\r", delayAfter: TEXT_ANSWER_DELAY_MS }],
        dismiss: true,
      };
    }
    const ks = submitAnswer("claude-code", option, index);
    return { steps: [{ data: ks }], dismiss: !ctx.isMultiQuestion };
  },
  toggle(option, _index, _ctx) {
    const ks = toggleClaudeCodeOption(option);
    return { steps: [{ data: ks }], dismiss: false };
  },
  submitMultiSelect(_ctx) {
    // Tab + delayed Enter: Tab navigates to Submit tab, Enter confirms.
    // Must be sent separately because setIsSubmitFocused is async React state.
    return {
      steps: [{ data: "\t" }, { data: "\r", delayAfter: TEXT_ANSWER_SUBMIT_DELAY_MS }],
      dismiss: true,
    };
  },
  textAnswer(option, text, index, ctx) {
    const parts = submitClaudeCodeTextAnswer(option, text, index, ctx.isMultiSelect, ctx.hasExistingText);
    const steps: KeystrokeStep[] = [
      { data: parts.navigate },
      { data: parts.type, delayAfter: TEXT_ANSWER_DELAY_MS },
    ];
    if (parts.submit) {
      // Submit chars are sent individually with delays (Ink TextInput state flush)
      for (const char of parts.submit) {
        steps.push({ data: char, delayAfter: TEXT_ANSWER_SUBMIT_DELAY_MS });
      }
    }
    return { steps, dismiss: !ctx.isMultiQuestion };
  },
  textCancel(_ctx) {
    return { steps: [], dismiss: false };
  },
  prevQuestion(_ctx) {
    return { steps: [{ data: navigatePrevQuestion("claude-code") }], dismiss: false, newCursorPos: 0 };
  },
  nextQuestion(_ctx) {
    return { steps: [{ data: navigateNextQuestion("claude-code") }], dismiss: false, newCursorPos: 0 };
  },
  confirm(_hasAnswers, ctx) {
    // Multi-select: use Tab+Enter (submitClaudeCodeMultiSelect).
    // Single-select: use Right arrow + Enter (submitClaudeCodeConfirm).
    if (ctx.isMultiSelect) {
      return { steps: [{ data: submitClaudeCodeMultiSelect() }], dismiss: true };
    }
    const hasOptions = !!(ctx.options && ctx.options.length > 0);
    const ks = submitClaudeCodeConfirm(hasOptions, ctx.activeTabIndex, ctx.totalTabs);
    return { steps: [{ data: ks }], dismiss: true };
  },
  // Claude Code has a Submit tab at the end, so last question is totalTabs - 2.
  lastQuestionIndex(totalTabs) { return totalTabs - 2; },
  hidePrev(ui) {
    // Single-question multi-select (1 question + Submit tab): hide Prev.
    return ui.isMultiSelect && ui.totalTabs === 2;
  },
  hideNext(ui) {
    // Single-question multi-select: hide Next.
    if (ui.isMultiSelect && ui.totalTabs === 2) return true;
    // Submit tab (last tab): hide Next (nothing after Submit).
    if (ui.activeTabIndex === ui.totalTabs - 1) return true;
    return false;
  },
  // Claude Code includes [✔] markers in option labels.
  syncCheckedFromScreen: true,
  hasSkipOnLastQuestion: false,
  cacheOptionsOnOther: false,
  detectOtherExpanded(_text, _isMultiSelect, _isMultiQuestion) { return false; },
  handleOscTitle(_title) { return null; },
};

// === SECTION 3 END ===

// ── Codex behavior ──────────────────────────────────────────────────────────────

const codexBehavior: CliBehavior = {
  answer(option, index, ctx) {
    const ks = submitAnswer("codex", option, index);
    return { steps: [{ data: ks }], dismiss: !ctx.isMultiQuestion };
  },
  toggle(option, index, _ctx) {
    // Codex doesn't have multi-select; fallback to answer keystrokes.
    const ks = submitAnswer("codex", option, index);
    return { steps: [{ data: ks }], dismiss: false };
  },
  submitMultiSelect(_ctx) {
    return { steps: [{ data: "\r" }], dismiss: true };
  },
  textAnswer(option, text, _index, _ctx) {
    return {
      steps: [{ data: option + "\r" }, { data: text + "\r", delayAfter: TEXT_ANSWER_DELAY_MS }],
      dismiss: false,
    };
  },
  textCancel(_ctx) {
    return { steps: [], dismiss: false };
  },
  prevQuestion(_ctx) {
    return { steps: [{ data: navigatePrevQuestion("codex") }], dismiss: false, newCursorPos: 0 };
  },
  nextQuestion(_ctx) {
    return { steps: [{ data: navigateNextQuestion("codex") }], dismiss: false, newCursorPos: 0 };
  },
  confirm(_hasAnswers, _ctx) {
    return { steps: [{ data: "\r" }], dismiss: true };
  },
  lastQuestionIndex(totalTabs) { return totalTabs - 2; },
  hidePrev(_ui) { return false; },
  hideNext(_ui) { return false; },
  syncCheckedFromScreen: false,
  hasSkipOnLastQuestion: false,
  cacheOptionsOnOther: false,
  detectOtherExpanded(_text, _isMultiSelect, _isMultiQuestion) { return false; },
  // Codex emits "Action Required" as OSC 0 title when blocked.
  handleOscTitle(title) {
    if (title === "Action Required") {
      return { status: "blocked", message: "Action required" };
    }
    return null;
  },
};

// ── Registry ──────────────────────────────────────────────────────────────────

const BEHAVIORS: Partial<Record<CliType, CliBehavior>> = {
  devin: devinBehavior,
  opencode: opencodeBehavior,
  "claude-code": claudeCodeBehavior,
  codex: codexBehavior,
};

/**
 * Get the behavior for a specific CLI type.
 * @returns the behavior, or defaultBehavior if no specific behavior is registered.
 */
export function getBehavior(cli: CliType): CliBehavior {
  return BEHAVIORS[cli] ?? defaultBehavior;
}

// === SECTION 4 END ===
