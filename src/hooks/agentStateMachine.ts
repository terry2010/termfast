// Agent state machine — pure functions for AI CLI status transitions
//
// Design informed by TAME (Terminal_Agent_Management_Environment):
//   - Priority states (blocked, error) bypass debounce — fire immediately
//   - Non-priority transitions debounce 500ms to avoid status flicker
//   - "done" auto-decays to "idle" after 5s of no activity
//   - Any PTY output while idle/working resets the idle timer
//
// The state machine is CLI-agnostic. Different CLIs feed different signals
// via `applySignal()`; the transition rules are universal.

import type { AgentSignal, CliType } from "./oscParser";

/** Re-export CliType for convenience. */
export type { CliType } from "./oscParser";

/** External status shown to the UI. */
export type AgentStatus = "unknown" | "idle" | "working" | "blocked" | "done";

/** Internal state — includes timers that drive time-based transitions. */
export interface AgentState {
  status: AgentStatus;
  cli: CliType;
  /** Monotonic timestamp (ms) of last PTY output. */
  lastOutputAt: number;
  /** Monotonic timestamp (ms) of last status change. */
  lastStatusChangeAt: number;
  /** Pending status from a debounced transition (null = none pending). */
  pendingStatus: AgentStatus | null;
  /** Monotonic timestamp (ms) when the pending status should fire. */
  pendingFireAt: number;
  /** Blocked reason message (from OSC 777 body or screen scrape). */
  blockedMessage: string | null;
  /** True if blocked status was set by an OSC signal (not screen scrape).
   *  When true, screen scraping must NOT clear blocked just because the
   *  screen doesn't show a blocked pattern — Devin's OSC-set blocked state
   *  is authoritative and should only be cleared by another OSC signal. */
  blockedFromOsc: boolean;
}

/** Debounce window for non-priority transitions (ms). */
const DEBOUNCE_MS = 500;

/** After "done", auto-decay to "idle" after this long with no activity (ms). */
const DONE_TO_IDLE_MS = 5000;

/**
 * Fallback timeout: if in "working" state with no PTY output for this long
 * (ms), and no blocked/done signal received, transition to "done".
 *
 * This is a LAST RESORT fallback. The primary mechanism for detecting
 * "Devin finished" is screen scraping — when Devin completes a turn, the
 * screen shows the idle placeholder text ("❭ Ask Devin to build features...").
 * The tick's screen scraping detects this and transitions to idle directly.
 *
 * This timeout only fires if screen scraping fails for some reason
 * (e.g. alt buffer not accessible, pattern not matching). Set to 60s
 * to avoid false "done" during long API responses or command execution.
 */
const WORKING_IDLE_TIMEOUT_MS = 60000;

/** Priority states that bypass debounce. */
const PRIORITY_STATES: ReadonlySet<AgentStatus> = new Set(["blocked"]);

/** Create an initial agent state. */
export function createAgentState(now: number): AgentState {
  return {
    status: "unknown",
    cli: "unknown",
    lastOutputAt: now,
    lastStatusChangeAt: now,
    pendingStatus: null,
    pendingFireAt: 0,
    blockedMessage: null,
    blockedFromOsc: false,
  };
}

/**
 * Apply a signal to the state machine.
 *
 * @param state  current state (mutated in place for efficiency)
 * @param signal the signal to apply (or null for a tick)
 * @param now    current monotonic timestamp (ms)
 * @returns the new status (same reference as state.status)
 */
export function applySignal(
  state: AgentState,
  signal: AgentSignal | null,
  now: number,
): AgentStatus {
  if (signal) {
    // Update CLI type from signal (sticky — once detected, don't un-detect)
    if (signal.cli !== "unknown" && state.cli === "unknown") {
      state.cli = signal.cli;
    }

    if (signal.kind === "blocked") {
      transitionTo(state, "blocked", now);
      state.blockedMessage = signal.message;
      state.blockedFromOsc = true;
      state.pendingStatus = null;
      return state.status;
    }
    if (signal.kind === "done") {
      transitionTo(state, "done", now);
      state.blockedMessage = null;
      state.blockedFromOsc = false;
      state.pendingStatus = null;
      return state.status;
    }
    if (signal.kind === "notify") {
      // Devin OSC 777/9 notification — can be either done or blocked
      // depending on the message body ("Devin finished" vs "Devin needs input")
      if (signal.done) {
        transitionTo(state, "done", now);
        state.blockedMessage = null;
        state.blockedFromOsc = false;
      } else {
        transitionTo(state, "blocked", now);
        state.blockedMessage = signal.message;
        state.blockedFromOsc = true;
      }
      state.pendingStatus = null;
      return state.status;
    }
    if (signal.kind === "title") {
      // Title signal updates CLI type but doesn't directly change status.
      // Status from screen scraping will be applied via applyScreenStatus().
      return state.status;
    }
  }
  return state.status;
}

/**
 * Apply a status detected from screen scraping.
 * This is used by CLIs that don't emit OSC status signals (OpenCode, Claude Code, Codex).
 *
 * Screen-detected status follows the same transition rules as OSC signals,
 * but blocked status from screen scraping also carries a message.
 */
export function applyScreenStatus(
  state: AgentState,
  status: AgentStatus,
  message: string | null,
  now: number,
): void {
  if (status === "blocked") {
    transitionTo(state, "blocked", now);
    state.blockedMessage = message;
    state.blockedFromOsc = false;
    state.pendingStatus = null;
  } else if (status === "done") {
    transitionTo(state, "done", now);
    state.blockedMessage = null;
    state.blockedFromOsc = false;
    state.pendingStatus = null;
  } else if (status === "working") {
    // Screen-detected working is immediate (not debounced) — the screen
    // already shows the spinner, so there's no flicker risk.
    transitionTo(state, "working", now);
    state.pendingStatus = null;
  } else if (status === "idle") {
    // Don't override blocked or done with idle from screen.
    // - blocked: OSC-set blocked is authoritative (blockedFromOsc guard).
    // - done: the "done" state (blue dot) should persist for DONE_TO_IDLE_MS
    //   (5s) before decaying to "idle" via the tick timer. Without this guard,
    //   screen scrape would immediately replace "done" with "idle" when the
    //   screen shows the idle placeholder (e.g. Devin's "❭ Ask Devin to..."
    //   appears the moment Devin finishes), so the user would never see the
    //   blue "done" dot — it would flash blue then instantly turn gray.
    if (state.status !== "blocked" && state.status !== "done") {
      transitionTo(state, "idle", now);
      state.pendingStatus = null;
    }
  }
}

/**
 * Notify the state machine that PTY output was received.
 * Any output → working (unless already blocked).
 */
export function notifyOutput(state: AgentState, now: number): AgentStatus {
  state.lastOutputAt = now;

  // If blocked, do NOT transition to working on output.
  // Blocked status is managed by screen scraping (tick) or OSC signals.
  // PTY output during blocked (spinner animation, cursor blink) should not
  // clear the blocked state — only screen scrape detecting the dialog is gone
  // or an OSC signal should clear it.
  if (state.status === "blocked") {
    return state.status;
  }

  // Only transition to working if a CLI has been detected.
  // Plain shell output (MOTD, prompt) should NOT trigger "working" status —
  // without a detected AI CLI, the tab should stay "unknown" (no status dot).
  if (state.cli === "unknown") {
    return state.status;
  }

  // If idle/done/unknown (with CLI detected), new output → working (debounced)
  if (state.status === "idle" || state.status === "done" || state.status === "unknown") {
    scheduleDebounced(state, "working", now);
  }
  return state.status;
}

/**
 * Tick the state machine — called on a regular interval (e.g. every 500ms).
 * Handles debounced transitions and time-based decay (done → idle).
 */
export function tick(state: AgentState, now: number): AgentStatus {
  // Fire pending debounced transition
  if (state.pendingStatus !== null && now >= state.pendingFireAt) {
    transitionTo(state, state.pendingStatus, now);
    state.pendingStatus = null;
  }

  // done → idle after DONE_TO_IDLE_MS of no activity
  if (state.status === "done" && now - state.lastOutputAt >= DONE_TO_IDLE_MS) {
    transitionTo(state, "idle", now);
  }

  // working → done after WORKING_IDLE_TIMEOUT_MS of no output
  // This handles CLIs that don't emit an explicit "done" signal when
  // the terminal is focused (e.g. Devin with notify=smart mode).
  // The TUI spinner stops producing PTY output when the AI finishes,
  // so output silence reliably indicates the turn is over.
  if (state.status === "working" && state.cli !== "unknown" &&
      now - state.lastOutputAt >= WORKING_IDLE_TIMEOUT_MS) {
    transitionTo(state, "done", now);
    state.pendingStatus = null;
  }

  return state.status;
}

/** Transition to a new status, recording the timestamp. */
function transitionTo(state: AgentState, status: AgentStatus, now: number): void {
  if (state.status === status) return;
  state.status = status;
  state.lastStatusChangeAt = now;
}

/** Schedule a debounced transition (non-priority states only). */
function scheduleDebounced(state: AgentState, status: AgentStatus, now: number): void {
  if (PRIORITY_STATES.has(status)) {
    // Priority states fire immediately
    transitionTo(state, status, now);
    state.pendingStatus = null;
  } else {
    state.pendingStatus = status;
    state.pendingFireAt = now + DEBOUNCE_MS;
  }
}

/**
 * Set the CLI type explicitly (e.g. from process name detection).
 * Does not change the status.
 */
export function setCliType(state: AgentState, cli: CliType): void {
  state.cli = cli;
}

/**
 * Reset the state machine (e.g. when the terminal tab is reused for a new session).
 */
export function resetAgentState(state: AgentState, now: number): void {
  state.status = "unknown";
  state.cli = "unknown";
  state.lastOutputAt = now;
  state.lastStatusChangeAt = now;
  state.pendingStatus = null;
  state.pendingFireAt = 0;
  state.blockedMessage = null;
  state.blockedFromOsc = false;
}
// === SECTION 2 END ===
