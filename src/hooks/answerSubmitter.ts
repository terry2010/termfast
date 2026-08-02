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
export function submitAnswer(cli: CliType, option: string, index: number): string {
  switch (cli) {
    case "devin":
      return submitDevin(option, index);
    case "opencode":
      return submitOpenCode(option, index);
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
// OpenCode permission dialog shows "Allow" / "Deny" buttons.
// The TUI uses Tab to move between buttons and Space/Enter to activate.
// "Allow" is typically the first (focused) button, "Deny" is the second.
function submitOpenCode(option: string, index: number): string {
  const normalized = option.toLowerCase().trim();
  if (normalized === "allow") {
    // Allow is typically the first button — just press Enter
    return "\r";
  }
  if (normalized === "deny") {
    // Tab to Deny button, then Enter
    return "\t\r";
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
