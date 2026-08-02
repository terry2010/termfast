// OSC parser — extract AI CLI status signals from xterm.js OSC sequences
//
// Devin CLI emits two native OSC sequences (no hook required):
//   ESC ] 777 ; notify ; Devin ; <message> BEL   → blocked (needs input)
//   ESC ] 1337 ; devin-idle=true BEL              → done (idle)
//
// OpenCode emits OSC 0 title: "OpenCode" or "OC | <task>"
// Claude Code / Codex emit OSC 0 title changes detected via onTitleChange.
//
// This module is framework-agnostic — it only parses the OSC data payload
// string that xterm.js's parser.registerOscHandler delivers. Unit-testable
// without a Terminal instance.

/** Signal emitted by an AI CLI via OSC. */
export type AgentSignal =
  | { kind: "blocked"; cli: "devin"; message: string }
  | { kind: "done"; cli: "devin" }
  | { kind: "title"; cli: CliType; title: string }
  | { kind: "notify"; cli: "devin"; message: string; done: boolean };

/** Messages that indicate Devin finished (done state), not blocked. */
const DEVIN_DONE_MESSAGES = [
  "Devin finished",
  "Devin encountered an error",
  "Devin response truncated",
];

/** Messages that indicate Devin needs user input (blocked state). */
const DEVIN_BLOCKED_MESSAGES = [
  "Devin needs input",
  "Tool approval pending",
  "Question pending",
  "Network permission pending",
  "Input needed",
  "Devin needs authentication",
];

/**
 * Classify a Devin notification message as done or blocked.
 * @returns true if the message indicates "done", false if "blocked".
 */
function isDevinDoneMessage(message: string): boolean {
  const lower = message.toLowerCase();
  for (const done of DEVIN_DONE_MESSAGES) {
    if (lower.startsWith(done.toLowerCase())) return true;
  }
  // If it matches a known blocked message, it's blocked
  for (const blocked of DEVIN_BLOCKED_MESSAGES) {
    if (lower.startsWith(blocked.toLowerCase())) return false;
  }
  // Unknown message — default to blocked (safer to show attention needed)
  return false;
}

/** CLI type detected from OSC 0 title or screen patterns. */
export type CliType = "unknown" | "devin" | "opencode" | "claude-code" | "codex" | "shell";

/**
 * Parse an OSC 777 data payload.
 *
 * Format: `notify;<title>;<body>` (rxvt-unicode notification extension).
 * Devin uses title="Devin" and body=the reason ("Devin needs input",
 * "Tool approval pending", "Question pending", etc.).
 *
 * @returns parsed signal, or null if the payload is not a Devin notify.
 */
export function parseOsc777(data: string): AgentSignal | null {
  // Format: "notify;Devin;<message>"
  // Some terminals split differently; be lenient with the title check.
  const parts = data.split(";");
  if (parts.length < 3) return null;
  const [cmd, title, ...rest] = parts;
  if (cmd !== "notify") return null;
  const body = rest.join(";"); // body may contain semicolons
  if (title === "Devin") {
    // Classify the message body: "Devin finished" = done, "Devin needs input" = blocked
    const done = isDevinDoneMessage(body);
    return { kind: "notify", cli: "devin", message: body, done };
  }
  return null;
}

/**
 * Parse an OSC 1337 data payload (Devin's custom extension).
 *
 * Known sub-commands:
 *   `devin-idle=true`  → AI finished responding, terminal is idle
 *   `devin-idle=false` → AI resumed working (sent on user input or new turn)
 *
 * @returns parsed signal, or null if not a recognized devin-idle payload.
 */
export function parseOsc1337(data: string): AgentSignal | null {
  // Format: "devin-idle=true" or "devin-idle=false"
  const trimmed = data.trim();
  if (trimmed === "devin-idle=true") {
    return { kind: "done", cli: "devin" };
  }
  // devin-idle=false is informational (resumed working); the working state
  // is already inferred from PTY output activity, so we don't emit a signal.
  return null;
}

/**
 * Parse an OSC 9 data payload (iTerm2-style system notification).
 *
 * Devin emits: `ESC]9;Devin finishedBEL` and `ESC]9;Devin needs inputBEL`
 * This is the same notification content as OSC 777 but without the
 * `notify;Devin;` prefix — just the bare message.
 *
 * @returns parsed signal, or null if not a recognized Devin notification.
 */
export function parseOsc9(data: string): AgentSignal | null {
  const message = data.trim();
  if (!message) return null;
  // Devin notifications start with "Devin "
  if (!message.startsWith("Devin ")) return null;
  const done = isDevinDoneMessage(message);
  return { kind: "notify", cli: "devin", message, done };
}

/**
 * Parse an OSC 0 data payload (set window title).
 *
 * OpenCode: "OpenCode" or "OC | <task description>"
 * Claude Code: title contains Braille spinner chars during working
 * Codex: "Action Required" when blocked
 *
 * @returns a title signal with detected CLI type, or null for non-CLI titles.
 */
export function parseOsc0(data: string): AgentSignal | null {
  const title = data.trim();
  if (!title) return null;

  // OpenCode: "OpenCode" or "OC | ..."
  if (title === "OpenCode" || title.startsWith("OC |") || title.startsWith("OC  |")) {
    return { kind: "title", cli: "opencode", title };
  }

  // Codex: "Action Required" or "codex" in title
  if (title === "Action Required" || title.toLowerCase().includes("codex")) {
    return { kind: "title", cli: "codex", title };
  }

  // Claude Code: title often contains Braille spinner chars or "claude"
  if (title.toLowerCase().includes("claude")) {
    return { kind: "title", cli: "claude-code", title };
  }

  // Devin: title contains "Devin" (case-insensitive — Devin CLI emits
  // lowercase "devin: <workspace>" as the OSC 0 title)
  if (title.toLowerCase().includes("devin")) {
    return { kind: "title", cli: "devin", title };
  }

  return null;
}

/**
 * Dispatch an OSC payload to the right parser by ident number.
 *
 * @param ident  the OSC numeric identifier (e.g. 0, 777, 1337)
 * @param data   the payload string (without the `ESC ] <ident> ;` prefix)
 * @returns parsed signal or null
 */
export function parseOsc(ident: number, data: string): AgentSignal | null {
  switch (ident) {
    case 0:
      return parseOsc0(data);
    case 9:
      return parseOsc9(data);
    case 777:
      return parseOsc777(data);
    case 1337:
      return parseOsc1337(data);
    default:
      return null;
  }
}
// === SECTION 1 END ===
