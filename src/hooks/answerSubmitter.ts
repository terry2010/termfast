// answerSubmitter — per-CLI strategies for submitting answers to the PTY
//
// When the user selects an answer in the question overlay, this module
// generates the keystrokes to send to the terminal to submit that answer.
//
// Each CLI has a different interaction model:
//   Devin:     numbered options → send digit + Enter
//   OpenCode:  permission dialog → Tab to focus button + Space/Enter
//   Claude Code: selection widget → Arrow keys + Space/Enter
//   Codex:     y/n prompts → send 'y' or 'n' key

import type { CliType } from "./oscParser";

/**
 * Generate the keystrokes to submit an answer for a given CLI.
 *
 * @param cli     the CLI type
 * @param option  the selected option string (e.g. "1. Yes", "Allow", "Yes (y)")
 * @param index   the 0-based index of the selected option
 * @returns a string of characters to send to the terminal (via term.onData
 *          or the PTY input path).
 */
export function submitAnswer(cli: CliType, option: string, index: number, optionCount?: number, isMultiQuestion?: boolean): string {
  switch (cli) {
    case "devin":
      return submitDevin(option, index, isMultiQuestion);
    case "opencode":
      return submitOpenCode(option, index, optionCount);
    case "claude-code":
      return submitClaudeCode(option, index);
    case "codex":
      return submitCodex(option, index);
    default:
      // Fallback: send the option text + Enter
      return option + "\r";
  }
}

// ── Devin ────────────────────────────────────────────────────────────────────
// Devin has two dialog types:
// 1. Permission dialog: numbered options "1 Yes (Approve once)" etc.
//    Submit: press the number key + Enter.
// 2. ask_user_question dialog: numbered options with ↑↓ navigate footer
//    Single-select: ↑↓ to navigate, Enter to select (auto-advances in multi-question)
//    Multi-select: ↑↓ to navigate, Space to toggle, Enter to submit (after all toggles)
//    The overlay uses index-based arrow navigation since Devin doesn't bind number keys
//    for ask_user_question dialogs (only the permission dialog uses number keys).

/**
 * Toggle a single option in multi-select mode for Devin's ask_user_question.
 *
 * Devin's multi-select footer: "↑↓ navigate · ␣ toggle · ↵ select"
 * The option list is CIRCULAR (wraps around), so we cannot use ↑N to go to
 * the top. Instead, we track the cursor position and compute the relative
 * movement from the current position to the target.
 *
 * @param option      the option string (unused, kept for API compatibility)
 * @param targetIndex 0-based index of the option to toggle
 * @param currentPos  current cursor position (0-based), tracked by the caller
 * @returns the keystroke string to send
 */
export function toggleDevinOption(
  _option: string,
  targetIndex: number,
  currentPos = 0,
): string {
  if (targetIndex > currentPos) {
    // Move down to target
    return "\x1b[B".repeat(targetIndex - currentPos) + " ";
  }
  if (targetIndex < currentPos) {
    // Move up to target
    return "\x1b[A".repeat(currentPos - targetIndex) + " ";
  }
  // Already at target — just toggle
  return " ";
}

/**
 * Submit multi-select for Devin's ask_user_question.
 * After toggling all desired options, press Enter to submit.
 * In multi-question mode, Enter auto-advances to the next question tab.
 */
export function submitDevinMultiSelect(): string {
  return "\r";
}

/**
 * Navigate to the Confirm tab in Devin's multi-question dialog.
 * Uses → arrow keys to switch to the last question tab, then Enter to submit.
 * Same as OpenCode: → arrows (confirmIndex - activeIndex) mod totalTabs times + Enter.
 */
export function submitDevinConfirm(hasOptions: boolean, activeIndex: number, totalTabs: number, isMultiSelect?: boolean, hasAnswers = false): string {
  if (!hasOptions) {
    // Already on the last tab — just Enter
    return "\r";
  }
  if (totalTabs <= 0) {
    return "\r";
  }
  // In Devin multi-question, → switches to next question.
  // The last tab is the "submit" tab (no options, just Enter to confirm).
  const confirmIndex = totalTabs - 1;
  const currentTab = activeIndex >= 0 ? activeIndex : 0;
  const arrowsNeeded = (confirmIndex - currentTab + totalTabs) % totalTabs;
  // For single-select multi-question: if we're on the last question tab
  // (arrowsNeeded === 0), pressing Enter would select the first option
  // (cursor default). Instead, send Esc to close the dialog without
  // selecting any option. The user should select an option first (which
  // auto-advances), or press Esc to skip.
  if (arrowsNeeded === 0 && !isMultiSelect && !hasAnswers) {
    return "\x1b";
  }
  return "\x1b[C".repeat(arrowsNeeded) + "\r";
}

/**
 * Submit "Type your own answer" for Devin's ask_user_question.
 * Press 'e' to enter text input mode, then type the text + Enter.
 * In multi-select mode, Enter adds the text but doesn't advance —
 * caller must then navigate to Confirm + Enter.
 *
 * For numbered options: number key navigates to the option, then 'e' enters text mode.
 * For "Other (type your own)" (no number): use relative arrow navigation from
 * the current cursor position (Devin's list is circular, so we can't use ↑N
 * to go to the top). The caller must track and pass the current cursor position.
 *
 * The type string starts with Ctrl+U (\x15) to clear any existing text in the
 * input field (e.g. when re-editing a previously answered "Other" option).
 */
export function submitDevinTextAnswer(
  option: string,
  text: string,
  _isMultiSelect?: boolean,
  index?: number,
  optionCount?: number,
  currentPos?: number,
): { navigate: string; type: string } {
  const numMatch = option.match(/^(\d+)/);
  if (numMatch) {
    // Numbered option: number key navigates, then 'e' enters text mode
    const num = parseInt(numMatch[1], 10);
    return {
      navigate: String(num) + "e",
      type: "\x15" + text + "\r",
    };
  }
  // No number (e.g. "Other (type your own)"): use relative arrow navigation
  const idx = index ?? 0;
  const cur = currentPos ?? 0;
  let nav = "";
  if (idx > cur) {
    nav = "\x1b[B".repeat(idx - cur) + "e";
  } else if (idx < cur) {
    nav = "\x1b[A".repeat(cur - idx) + "e";
  } else {
    nav = "e";
  }
  return {
    navigate: nav,
    type: "\x15" + text + "\r",
  };
}

function submitDevin(option: string, index: number, isMultiQuestion?: boolean): string {
  // Extract the number from the option string (e.g. "1. Yes" → "1")
  const match = option.match(/^(\d+)/);
  if (match) {
    // Permission dialog (single-question): number + Enter to confirm.
    // ask_user_question single-select multi-question: number key selects
    // and auto-advances to next question — NO Enter needed (Enter would
    // skip an extra question).
    if (isMultiQuestion) {
      return match[1];
    }
    return match[1] + "\r";
  }
  // "Other (type your own)" — no number, use 'e' to enter text mode
  if (/other/i.test(option)) {
    return "e";
  }
  // Fallback: send the index + 1 as a number
  if (isMultiQuestion) {
    return String(index + 1);
  }
  return String(index + 1) + "\r";
}

/**
 * Generate keystrokes to navigate to the previous question tab
 * in multi-question mode.
 *
 * All CLIs use Left arrow to switch to the previous question tab.
 * Devin's footer says "←→ switch question" even in "└ e..." text editing
 * state, so Left arrow should switch tabs directly.
 */
export function navigatePrevQuestion(_cli: CliType): string {
  return "\x1b[D";
}

/**
 * Generate keystrokes to navigate to the next question tab
 * in multi-question mode.
 *
 * All CLIs use Right arrow to switch to the next question tab.
 */
export function navigateNextQuestion(_cli: CliType): string {
  return "\x1b[C";
}

// ── OpenCode ──────────────────────────────────────────────────────────────────
// OpenCode has several types of blocked dialogs:
// 1. Permission dialog: buttons "Allow once", "Allow always", "Reject"
//    Navigation: ⇆ (Tab) to move between buttons, Enter to activate
// 2. Single-select question: numbered options "1. xxx", "2. xxx", ...
//    Footer: "↑↓ select  enter confirm/submit  esc dismiss"
//    Navigation: ↑↓ to move, Enter to confirm
// 3. Multi-select question: numbered options with [ ]/[✓] checkboxes
//    Footer: "↑↓ select  enter toggle  esc dismiss"
//    Navigation: ↑↓ to move, Space/Enter to toggle, Tab to Confirm button, Enter to submit
//    The UI calls submitOpenCodeMultiSelect with the selected indices,
//    then submitOpenCodeConfirm to submit.

/**
 * Toggle a single option in multi-select mode.
 * Uses number keys (1-9) to directly select/toggle an option.
 * OpenCode's question dialog binds number keys 1-9 to moveTo + selectOption,
 * which is simpler and more reliable than arrow navigation (↑↓ are circular
 * with modulo, making arrow-based positioning error-prone).
 */
export function toggleOpenCodeOption(option: string, _optionCount?: number): string {
  const numMatch = option.match(/^(\d+)/);
  if (numMatch) {
    const num = parseInt(numMatch[1], 10);
    // Number key directly toggles the option (OpenCode handles this internally)
    return String(num);
  }
  return " ";
}

/**
 * Submit the multi-select answer: Tab to Confirm button + Enter.
 */
export function submitOpenCodeMultiSelect(): string {
  // Tab to the "Confirm" button, then Enter
  return "\t\r";
}

/**
 * Calculate the keystrokes to confirm a multi-question dialog.
 *
 * Navigates to the Confirm tab (last tab) using → arrow keys, then Enter.
 * The number of → presses = (confirmIndex - activeIndex) % totalTabs.
 *
 * @param hasOptions   true if options are visible (on a question tab, not Confirm tab)
 * @param activeIndex   current tab index (-1 if unknown, falls back to 0)
 * @param totalTabs     total number of tabs (including Confirm)
 * @returns keystroke string to send to the PTY
 */
export function submitOpenCodeConfirm(hasOptions: boolean, activeIndex: number, totalTabs: number): string {
  if (!hasOptions) {
    // Already on Confirm tab — just Enter to submit
    return "\r";
  }
  if (totalTabs <= 0) {
    // Tab detection failed — fall back to Tab + Enter (best effort)
    return "\t\r";
  }
  const confirmIndex = totalTabs - 1;
  const currentTab = activeIndex >= 0 ? activeIndex : 0;
  const arrowsNeeded = (confirmIndex - currentTab + totalTabs) % totalTabs;
  return "\x1b[C".repeat(arrowsNeeded) + "\r";
}

/**
 * Submit "Type your own answer": navigate to the option, enter text input
 * mode, then type the answer + submit.
 *
 * Returns a two-part keystroke sequence:
 *   - navigate: keystrokes to navigate to the option and enter text mode
 *   - type: keystrokes to type the answer and submit
 *
 * The caller should send `navigate` first, wait ~300ms for OpenCode to
 * redraw the text input, then send `type`.
 *
 * Uses number keys (1-9) to directly select the "Type your own answer" option.
 * OpenCode binds number keys to moveTo + selectOption; when the custom option
 * is selected, selectOption sets editing=true (single-select) or toggles
 * (multi-select with existing text). This avoids circular ↑↓ navigation.
 *
 * In single-select: number key → selectOption → editing=true → type + Enter
 *   Enter in editing mode calls pick(text, true) which auto-advances tab.
 *
 * In multi-select: number key → selectOption → editing=true → type + Enter
 *   Enter in editing mode adds text to answers but doesn't advance.
 *   Caller must then Tab to Confirm + Enter to submit.
 */
export function submitOpenCodeTextAnswer(option: string, text: string, _isMultiSelect?: boolean, _optionCount?: number): { navigate: string; type: string } {
  const numMatch = option.match(/^(\d+)/);

  // Single-select: number key to enter text mode, then type + Enter
  if (numMatch) {
    const num = parseInt(numMatch[1], 10);
    return {
      navigate: String(num),
      type: text + "\r",
    };
  }
  return { navigate: "", type: text + "\r" };
}

/** Delay (ms) between sending navigate keystrokes and type keystrokes. */
export const TEXT_ANSWER_DELAY_MS = 300;

/**
 * Delay (ms) between sending the text and the submit (Enter) keystroke.
 *
 * Claude Code's Ink TextInput updates `textInputValue` via React state
 * (onChange). If text + Enter arrive in the same event loop tick, the
 * onSubmit handler reads stale state and the tool result becomes
 * "__other__" instead of the user's text. This delay gives React time
 * to flush the state update before Enter is processed.
 */
export const TEXT_ANSWER_SUBMIT_DELAY_MS = 300;

/**
 * Send a text answer in parts with delays between them.
 *
 * Part 1 (navigate): sent immediately — navigates to the "Type your own
 *   answer" option and presses Enter to enter text input mode.
 * Part 2 (type): sent after TEXT_ANSWER_DELAY_MS — types the answer text.
 * Part 3 (submit): sent after TEXT_ANSWER_SUBMIT_DELAY_MS (if provided) —
 *   presses Enter to submit. The extra delay gives Ink's TextInput time
 *   to flush React state (onChange → textInputValue) before onSubmit
 *   reads it. Without this, the tool result is "__other__" (empty text).
 *
 * @param parts     the { navigate, type, submit? } from submitXxxTextAnswer
 * @param send      function that sends bytes to the PTY
 * @returns a cleanup function that clears the timeouts (for unmount safety)
 */
export function sendTextAnswerWithDelay(
  parts: { navigate: string; type: string; submit?: string },
  send: (bytes: Uint8Array) => void,
): () => void {
  const encoder = new TextEncoder();
  const timers: ReturnType<typeof setTimeout>[] = [];
  // Part 1: send navigate immediately
  if (parts.navigate) {
    send(encoder.encode(parts.navigate));
  }
  // Part 2: send type after delay
  timers.push(setTimeout(() => {
    send(encoder.encode(parts.type));
  }, TEXT_ANSWER_DELAY_MS));
  // Part 3: send submit after another delay (if provided).
  // Each character of submit is sent separately with a delay between them.
  // This is critical for multi-select mode: submit is "\t\r" (Tab + Enter),
  // and Tab and Enter must be sent separately because setIsSubmitFocused
  // is async React state — if Enter arrives in the same tick as Tab,
  // isSubmitFocused is still false and Enter toggles the option instead
  // of submitting.
  if (parts.submit) {
    const submitChars = [...parts.submit];
    submitChars.forEach((char, i) => {
      timers.push(setTimeout(() => {
        send(encoder.encode(char));
      }, TEXT_ANSWER_DELAY_MS + TEXT_ANSWER_SUBMIT_DELAY_MS + i * TEXT_ANSWER_SUBMIT_DELAY_MS));
    });
  }
  return () => timers.forEach(clearTimeout);
}

function submitOpenCode(option: string, index: number, optionCount?: number): string {
  const normalized = option.toLowerCase().trim();

  // Permission dialog buttons
  // The permission dialog uses mouse hover to change the focused button.
  // When the user moves their mouse over the terminal (behind the overlay),
  // the focus changes. To reliably activate the desired button, we cycle
  // through all buttons (Tab optionCount times wraps back to button 0),
  // then Tab (index) more times to reach the desired button, then Enter.
  if (normalized === "allow once" || normalized === "allow always" || normalized === "reject") {
    const cycleTabs = optionCount && optionCount > 1 ? "\t".repeat(optionCount) : "";
    return cycleTabs + "\t".repeat(index) + "\r";
  }

  // Question/selector dialog (single-select): numbered options like "1. Rust"
  // OpenCode binds number keys 1-9 to directly select an option (moveTo +
  // selectOption). This is simpler and more reliable than arrow navigation
  // (↑↓ are circular with modulo, making position-based navigation error-prone
  // in multi-question dialogs where the cursor carries over between tabs).
  // Number key directly selects; pick() auto-advances to next tab.
  const numMatch = option.match(/^(\d+)/);
  if (numMatch) {
    const num = parseInt(numMatch[1], 10);
    return String(num);
  }

  // Fallback: Tab index times + Enter
  return "\t".repeat(index) + "\r";
}

// ── Claude Code ────────────────────────────────────────────────────────────────
// Claude Code selection widget: numbered options "1. Rust", "2. Python", etc.
// Multi-question dialog (v2.1+): tab row "←  ☐ label  ...  ✔ Submit  →"
// Footer: "Enter to select · Tab/Arrow keys to navigate · Esc to cancel"
// Multi-select: options have [ ]/[✓] checkboxes, same footer + tab row.
//
// Key insight: Claude Code's Ink TUI may use application cursor key mode,
// making \x1b[B (CSI B) not recognized as Down arrow. Instead of relying
// on arrow navigation, we use the NUMBER KEY from the option label to
// directly select it. This is simpler and more reliable.

/**
 * Toggle a single option in Claude Code multi-select mode.
 * Pressing the number key toggles the checkbox.
 */
export function toggleClaudeCodeOption(option: string): string {
  const numMatch = option.match(/^(\d+)/);
  return numMatch ? numMatch[1] : "";
}

/**
 * Submit the multi-select answer for Claude Code.
 * Tab to the "Submit" tab + Enter to confirm.
 */
export function submitClaudeCodeMultiSelect(): string {
  return "\t\r";
}

/** Check if an option is a Claude Code Plan Mode (ExitPlanMode) option.
 *  These options need arrow navigation + delayed Enter (not number keys).
 *  Used by TerminalView to split the send into two parts with a delay. */
export function isClaudeCodePlanModeOption(option: string): boolean {
  return /yes, and use auto mode/i.test(option)
    || /yes, manually approve edits/i.test(option)
    || /tell\s+\S+\s+what\s+to\s+change/i.test(option);
}

/** Build keystrokes for Claude Code Plan Mode navigation (Down*index).
 *  Returns the navigation part only (without Enter). */
export function buildClaudeCodePlanModeNavigate(index: number): string {
  return "\x1b[B".repeat(index);
}

function submitClaudeCode(option: string, index: number): string {
  const normalized = option.toLowerCase().trim();

  // Yes/No prompts
  if (normalized.startsWith("yes")) {
    return "\r"; // Yes is usually the default — just Enter
  }
  if (normalized.startsWith("no")) {
    return "\x1b[B\r"; // Down arrow + Enter
  }

  // Plan Mode (ExitPlanMode) dialog: "1. Yes, and use auto mode",
  // "2. Yes, manually approve edits", "3. Tell Claude what to change".
  // This dialog does NOT support number-key selection — pressing a
  // number key is interpreted as a regular character and triggers the
  // default action (approve option 1). We must use arrow navigation:
  // Down*index + Enter to select the desired option.
  //
  // IMPORTANT: Down and Enter must be sent with a delay between them.
  // If sent together, the Ink TUI may not process the Down arrow before
  // receiving Enter, causing Enter to confirm the default option (1)
  // instead of the navigated-to option. The caller (TerminalView) uses
  // isClaudeCodePlanModeOption to detect this and split the send.
  if (/yes, and use auto mode/i.test(option)
    || /yes, manually approve edits/i.test(option)
    || /tell\s+\S+\s+what\s+to\s+change/i.test(option)) {
    let keys = "";
    for (let i = 0; i < index; i++) {
      keys += "\x1b[B"; // Down arrow
    }
    keys += "\r"; // Enter to confirm
    return keys;
  }

  // Multi-question selection widget: send the number key directly.
  // Options are "1. Rust", "2. Python", "3. TypeScript", etc.
  // Pressing the number key selects the option and auto-advances to
  // the next question tab (same as OpenCode's number key behavior).
  const numMatch = option.match(/^(\d+)/);
  if (numMatch) {
    return String(numMatch[1]);
  }

  // Fallback: navigate with Down arrow + Enter
  let keys = "";
  for (let i = 0; i < index; i++) {
    keys += "\x1b[B"; // Down arrow
  }
  keys += "\r"; // Enter to confirm
  return keys;
}

/**
 * Confirm a Claude Code multi-question dialog.
 * Navigates to the Submit tab (last tab) using → arrow keys, then Enter.
 * Same logic as OpenCode/Devin: → arrows (confirmIndex - activeIndex) % totalTabs times + Enter.
 */
export function submitClaudeCodeConfirm(hasOptions: boolean, activeIndex: number, totalTabs: number): string {
  if (!hasOptions) {
    // Already on Submit tab — just Enter
    return "\r";
  }
  if (totalTabs <= 0) {
    return "\r";
  }
  const confirmIndex = totalTabs - 1;
  const currentTab = activeIndex >= 0 ? activeIndex : 0;
  const arrowsNeeded = (confirmIndex - currentTab + totalTabs) % totalTabs;
  return "\x1b[C".repeat(arrowsNeeded) + "\r";
}

/**
 * Submit "Type your own answer" for Claude Code.
 * Claude Code's "Type something." option (number 5) enters a text input
 * mode when selected. We send the number key to select it, then after a
 * delay, type the text, then after another delay, press Enter to submit.
 *
 * The text and Enter must be sent separately because Ink's TextInput
 * updates `textInputValue` via React state (onChange). If text + Enter
 * arrive in the same event loop tick, the onSubmit handler reads stale
 * state (empty string) and the tool result becomes "__other__" instead
 * of the user's text. Sending Enter after a delay gives React time to
 * flush the state update.
 *
 * In multi-select mode, pressing the number key only toggles the checkbox
 * — it does NOT enter text input mode. To enter text mode, we must
 * navigate to the option with Down arrows and press Enter. The `index`
 * parameter tells us how many Down arrows to send (from the top).
 *
 * Plan Mode's "Tell Claude what to change" option works differently:
 * after typing the text + Enter, the text fills into the option label
 * (e.g. "❯ 3. my feedback"), but the dialog stays open. The footer shows
 * "shift+tab to approve with this feedback". The user must press Enter
 * AGAIN to actually submit/approve. So we send text + "\r\r" (two Enters).
 *
 * @returns three-part keystroke sequence:
 *   - navigate: number key (single-select) or Down*index + Enter (multi-select)
 *   - type: the text to type (no Enter)
 *   - submit: Enter key(s) to submit ("\r" for single-question, "\r\r" for Plan Mode)
 */
export function submitClaudeCodeTextAnswer(option: string, text: string, index?: number, isMultiSelect?: boolean): { navigate: string; type: string; submit: string } {
  const numMatch = option.match(/^(\d+)/);
  const numKey = numMatch ? numMatch[1] : "5";
  // Plan Mode "Tell Claude what to change" needs an extra Enter to approve
  const isPlanModeFeedback = /tell\s+\S+\s+what\s+to\s+change/i.test(option);
  // Multi-select: number key only toggles checkbox, need Down arrows to
  // navigate to the option. When focused, TextInput auto-renders and
  // captures character input — no Enter needed to enter text mode.
  // Submit: Tab to Submit button + Enter. Tab and Enter must be sent
  // separately (with delay) because setIsSubmitFocused is async React
  // state — if Enter arrives in the same tick, isSubmitFocused is still
  // false and Enter toggles the option instead of submitting.
  if (isMultiSelect && index !== undefined && index > 0) {
    return {
      navigate: "\x1b[B".repeat(index),
      type: text,
      submit: "\t\r",
    };
  }
  // Multi-select with index 0: option is already focused, no navigation needed
  if (isMultiSelect && index === 0) {
    return {
      navigate: "",
      type: text,
      submit: "\t\r",
    };
  }
  return {
    navigate: numKey,
    type: text,
    submit: isPlanModeFeedback ? "\r\r" : "\r",
  };
}

// ── Codex ──────────────────────────────────────────────────────────────────────
// Codex uses y/n key prompts. Just send 'y' or 'n'.
function submitCodex(option: string, _index: number): string {
  const normalized = option.toLowerCase().trim();
  // Trust prompt: press Enter for default (usually "Yes")
  if (normalized.includes("trust") || normalized.includes("allow")) {
    return "\r";
  }
  if (normalized.startsWith("yes") || normalized === "y") {
    return "y\r";
  }
  if (normalized.startsWith("no") || normalized === "n") {
    return "n\r";
  }
  // Fallback: 'y' for first option, 'n' for second
  return _index === 0 ? "y\r" : "n\r";
}
