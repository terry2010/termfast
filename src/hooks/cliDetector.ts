// cliDetector — detect which AI CLI is running in a terminal tab
//
// Detection signals (in priority order):
//   1. OSC 0 title: "OpenCode", "OC | ...", "claude", "codex", "Devin"
//   2. Screen content patterns (fallback when OSC 0 is not emitted)
//   3. Process name (future: from backend, not implemented yet)
//
// Once a CLI is detected, the type is sticky — we don't un-detect on title
// changes (e.g. OpenCode clears its title on idle, but it's still OpenCode).

import type { CliType } from "./oscParser";
import type { AgentStatus } from "./agentStateMachine";
import { detectStatusFromScreen, stripAnsi } from "./agentPatterns";

/**
 * Detect CLI type from an OSC 0 title string.
 *
 * @returns detected CLI type, or "unknown" if not a recognized AI CLI.
 */
export function detectCliFromTitle(title: string): CliType {
  const t = title.trim();
  if (!t) return "unknown";

  // OpenCode: "OpenCode" or "OC | <task>"
  if (t === "OpenCode" || t.startsWith("OC |") || t.startsWith("OC  |")) {
    return "opencode";
  }

  // Codex: "Action Required" or "codex" in title
  if (t === "Action Required" || t.toLowerCase().includes("codex")) {
    return "codex";
  }

  // Claude Code: "claude" in title
  if (t.toLowerCase().includes("claude")) {
    return "claude-code";
  }

  // Devin: "Devin" in title (case-insensitive — Devin CLI emits lowercase "devin: <workspace>")
  if (t.toLowerCase().includes("devin")) {
    return "devin";
  }

  return "unknown";
}

/**
 * Detect CLI type from screen content (fallback when no OSC 0 title).
 *
 * Checks for unique screen signatures of each CLI.
 *
 * @param screenText  ANSI-stripped screen text
 * @returns detected CLI type, or "unknown" if no signature matched.
 */
export function detectCliFromScreen(screenText: string): CliType {
  // Devin: "Devin CLI" text in the startup banner
  if (/Devin\s+CLI/i.test(screenText)) {
    return "devin";
  }
  // Devin: Braille art logo pattern (⣴⣾⣶⡄ etc.)
  if (/[⣴⣾⣶⡄⠛⠿⠟⠻⣤⣦⠻⢿⠃].*Devin/i.test(screenText)) {
    return "devin";
  }

  // OpenCode: "esc interrupt" + "ctrl+p commands" + "tab agents" footer
  // Note: "esc interrupt" appears in both idle and working states (static footer)
  if (/esc\s+interrupt/.test(screenText) && /ctrl\+p\s+commands/.test(screenText)) {
    return "opencode";
  }
  // OpenCode logo: █▀▀█ █▀▀█ pattern
  if (/█▀▀█\s+█▀▀█/.test(screenText)) {
    return "opencode";
  }
  // OpenCode permission dialog
  if (/△\s+(?:Permission required|Always allow)\b/.test(screenText)) {
    return "opencode";
  }
  // OpenCode question/selector dialog
  if (/↑↓\s+select.*enter\s+\w+.*esc\s+dismiss/.test(screenText)) {
    return "opencode";
  }
  // OpenCode completion marker
  if (/▣\s+\S+\s+·\s+.+?\s+·\s+(?:\d+m\s+)?\d+(?:\.\d+)?s/.test(screenText)) {
    return "opencode";
  }

  // Claude Code: Braille spinner + "❯" prompt
  if (/[✶✢✽✻✳·*][^\n]*…/.test(screenText) && /[>❯][\s\xa0]/.test(screenText)) {
    return "claude-code";
  }
  // Claude Code: multi-question selection widget footer (v2.1+)
  if (/Enter\s*to\s*select.*(?:Tab\/Arrow|Tab).*Esc\s*to\s*cancel/i.test(screenText)) {
    return "claude-code";
  }
  // Claude Code: selection widget footer (older)
  if (/↑\/↓\s+to\s+navigate/.test(screenText)) {
    return "claude-code";
  }
  // Claude Code: plan approval / trust dialog
  if (/Would you like to proceed\?/.test(screenText) || /Yes,\s+I\s+trust\s+this\s+folder/.test(screenText)) {
    return "claude-code";
  }
  // Claude Code: completion summary "✻ ... for Ns"
  if (/[✶✢✽✻✳][^\n…]*\bfor\s+\d+(?:\.\d+)?\s*s\b/.test(screenText)) {
    return "claude-code";
  }

  // Codex: "❯" or "›" prompt + "esc to interrupt" in progress
  if (/•.*\(\d+s\s*•\s*esc\s+to\s+interrupt\)/.test(screenText)) {
    return "codex";
  }
  // Codex: "codex>" prompt
  if (/^\s*codex>\s*$/m.test(screenText)) {
    return "codex";
  }
  // Codex: Approve/Allow y/n prompt
  if (/^(?:Approve|Allow)\b.*\b(?:y\/n|yes\/no)\b/im.test(screenText)) {
    return "codex";
  }
  // Codex: trust prompt
  if (/allow\s+Codex\s+to\s+work\s+in\s+this\s+folder/i.test(screenText) ||
      /Do you trust the contents of this directory\?/i.test(screenText)) {
    return "codex";
  }

  return "unknown";
}

/**
 * Combined detection: try OSC title first, then screen content.
 *
 * @param title      OSC 0 title (or empty if none)
 * @param screenText ANSI-stripped screen text (or empty if none)
 * @returns detected CLI type, or "unknown".
 */
export function detectCli(title: string, screenText: string): CliType {
  const fromTitle = detectCliFromTitle(title);
  if (fromTitle !== "unknown") return fromTitle;

  const fromScreen = detectCliFromScreen(screenText);
  if (fromScreen !== "unknown") return fromScreen;

  return "unknown";
}

/**
 * Detect status from screen content, given a known CLI type.
 * Wrapper around detectStatusFromScreen that handles unknown CLI gracefully.
 */
export function detectStatus(cli: CliType, screenText: string): AgentStatus | null {
  if (cli === "unknown") return null;
  return detectStatusFromScreen(cli, screenText);
}

/**
 * Strip ANSI and prepare screen text for pattern matching.
 * Convenience wrapper.
 */
export function prepareScreenText(rawText: string): string {
  return stripAnsi(rawText);
}
