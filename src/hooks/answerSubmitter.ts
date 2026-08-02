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
export function submitAnswer(cli: CliType, option: string, index: number, optionCount?: number): string {
  switch (cli) {
    case "devin":
      return submitDevin(option, index);
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
// Devin shows numbered options: "1. Yes" "2. No" etc.
// Submit: press the number key + Enter.
function submitDevin(option: string, _index: number): string {
  // Extract the number from the option string (e.g. "1. Yes" → "1")
  const match = option.match(/^(\d+)/);
  if (match) {
    return match[1] + "\r";
  }
  // Fallback: send the index + 1 as a number
  return String(_index + 1) + "\r";
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
 * Send a text answer in two parts with a delay between them.
 *
 * Part 1 (navigate): sent immediately — navigates to the "Type your own
 *   answer" option and presses Enter to enter text input mode.
 * Part 2 (type): sent after TEXT_ANSWER_DELAY_MS — types the answer text
 *   and submits. The delay gives OpenCode time to redraw the text input UI.
 *
 * @param parts     the { navigate, type } from submitOpenCodeTextAnswer
 * @param send      function that sends bytes to the PTY
 * @returns a cleanup function that clears the timeout (for unmount safety)
 */
export function sendTextAnswerWithDelay(
  parts: { navigate: string; type: string },
  send: (bytes: Uint8Array) => void,
): () => void {
  const encoder = new TextEncoder();
  // Part 1: send navigate immediately
  if (parts.navigate) {
    send(encoder.encode(parts.navigate));
  }
  // Part 2: send type after delay
  const timer = setTimeout(() => {
    send(encoder.encode(parts.type));
  }, TEXT_ANSWER_DELAY_MS);
  return () => clearTimeout(timer);
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
// Claude Code selection widget uses ↑/↓ to navigate and Space/Enter to select.
// For Yes/No prompts, "Yes" is typically first (index 0), "No" is second.
function submitClaudeCode(option: string, index: number): string {
  const normalized = option.toLowerCase().trim();

  // Yes/No prompts
  if (normalized.startsWith("yes")) {
    return "\r"; // Yes is usually the default — just Enter
  }
  if (normalized.startsWith("no")) {
    // Navigate down once to No, then Enter
    return "\x1b[B\r"; // Down arrow + Enter
  }

  // Selection widget: navigate to the option by pressing Down index times
  // Then press Space or Enter to select
  let keys = "";
  for (let i = 0; i < index; i++) {
    keys += "\x1b[B"; // Down arrow
  }
  keys += "\r"; // Enter to confirm
  return keys;
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
