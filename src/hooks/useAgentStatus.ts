// useAgentStatus — React hook that binds an xterm.js Terminal to the agent
// state machine.
//
// Registers OSC handlers (0, 777, 1337) and monitors PTY output activity +
// screen content to drive status transitions for multiple AI CLIs.
//
// Supported CLIs:
//   Devin:       OSC 777 (blocked) + OSC 1337 (done) — native signals
//   OpenCode:    OSC 0 title + screen scrape (esc interrupt / △ Permission)
//   Claude Code: OSC 0 title + screen scrape (spinner / ↑/↓ navigate)
//   Codex:       OSC 0 title + screen scrape (Approve y/n / trust prompts)
//
// Usage:
//   const term = new Terminal({ ... });
//   const { status, cli, blockedMessage, question, options } = useAgentStatus(term, sessionId);

import { useEffect, useRef, useState } from "react";
import type { Terminal } from "@xterm/xterm";
import { parseOsc, type CliType } from "./oscParser";
import {
  createAgentState,
  applySignal,
  applyScreenStatus,
  notifyOutput,
  tick,
  setCliType,
  resetAgentState,
  type AgentState,
  type AgentStatus,
} from "./agentStateMachine";
import { scrapeScreen, joinLines } from "./screenScraper";
import { detectCli, detectStatus, prepareScreenText } from "./cliDetector";
import { extractQuestion, extractOptions } from "./agentPatterns";

/** Tick interval for time-based transitions + screen scraping. */
const TICK_INTERVAL_MS = 500;

/**
 * Detect if the screen shows a plain shell prompt (user exited the AI CLI).
 * Matches common shell prompt patterns:
 *   - zsh:   user@host path %
 *   - bash:  user@host:path$
 *   - fish:  user@host path>
 *
 * Requires user@host pattern to avoid false positives from AI CLI output
 * that may contain lone >, $, or % characters.
 */
function isShellPrompt(screenText: string): boolean {
  // user@host path %  (zsh default)
  // user@host:path$   (bash default)
  // Must have user@host pattern to be a real shell prompt.
  // Lone ❯/› are CLI prompts (Devin, Codex), NOT shell prompts.
  if (/\S+@\S+\s.*[%$]\s*$/.test(screenText)) return true;
  // user@host:path$  (bash default, no space before $)
  if (/\S+@\S+:[^\s]*\$\s*$/.test(screenText)) return true;
  return false;
}

export interface AgentStatusInfo {
  status: AgentStatus;
  cli: CliType;
  blockedMessage: string | null;
  /** Question text extracted from screen when blocked (null if not blocked or not extractable). */
  question: string | null;
  /** Answer options extracted from screen when blocked (null if not extractable). */
  options: string[] | null;
}

/**
 * Bind an xterm.js Terminal instance to the agent state machine.
 *
 * @param term      the xterm.js Terminal (or null if not yet created)
 * @param sessionId stable session ID (used for logging only)
 * @returns current agent status info { status, cli, blockedMessage }
 */
export function useAgentStatus(
  term: Terminal | null,
  sessionId: string,
): AgentStatusInfo {
  const [info, setInfo] = useState<AgentStatusInfo>({
    status: "unknown",
    cli: "unknown",
    blockedMessage: null,
    question: null,
    options: null,
  });

  // State machine lives in a ref — mutated in place, no re-allocation per tick.
  const stateRef = useRef<AgentState>(createAgentState(performance.now()));
  // Track disposables so we can clean up on unmount / term change.
  const disposablesRef = useRef<Array<{ dispose: () => void }>>([]);
  const tickTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    if (!term) return;

    const state = stateRef.current;

    // Sync state changes to React state (triggers re-render).
    // Also scrapes screen for question/options when blocked.
    const syncToReact = () => {
      let question: string | null = null;
      let options: string[] | null = null;

      // Only scrape screen for question/options when blocked
      if (state.status === "blocked" && state.cli !== "unknown") {
        try {
          const lines = scrapeScreen(term);
          const screenText = prepareScreenText(joinLines(lines));
          question = extractQuestion(state.cli, screenText);
          options = extractOptions(state.cli, screenText);
        } catch {
          // Screen scrape can fail if terminal is in a weird state — ignore
        }
      }

      setInfo({
        status: state.status,
        cli: state.cli,
        blockedMessage: state.blockedMessage,
        question,
        options,
      });
    };

    // ── Register OSC handlers ──────────────────────────────────────────

    // OSC 0 (set title) — CLI detection for OpenCode, Claude Code, Codex
    const osc0 = term.parser.registerOscHandler(0, (data) => {
      const signal = parseOsc(0, data);
      if (signal && signal.kind === "title") {
        console.log(`[agentStatus] OSC 0: title="${signal.title}" cli=${signal.cli} currentCli=${state.cli} currentStatus=${state.status}`);
        let changed = false;
        // Update CLI type if not already detected
        if (state.cli === "unknown" && signal.cli !== "unknown") {
          setCliType(state, signal.cli);
          changed = true;
        }
        // For Codex, "Action Required" title = blocked
        if (signal.cli === "codex" && signal.title === "Action Required") {
          applyScreenStatus(state, "blocked", "Action required", performance.now());
          changed = true;
        }
        if (changed) syncToReact();
      }
      return false;
    });

    // OSC 777 (notify) — Devin notification (done or blocked depending on message)
    const osc777 = term.parser.registerOscHandler(777, (data) => {
      const signal = parseOsc(777, data);
      if (signal) {
        console.log(`[agentStatus] OSC 777: kind=${signal.kind} cli=${"cli" in signal ? signal.cli : "?"} message="${"message" in signal ? signal.message : ""}" done=${"done" in signal ? signal.done : "?"} currentStatus=${state.status}`);
        applySignal(state, signal, performance.now());
        syncToReact();
      }
      return false;
    });

    // OSC 9 (iTerm2 notification) — Devin emits this alongside OSC 777
    // with the same message body (e.g. "Devin finished", "Devin needs input")
    const osc9 = term.parser.registerOscHandler(9, (data) => {
      const signal = parseOsc(9, data);
      if (signal) {
        console.log(`[agentStatus] OSC 9: kind=${signal.kind} message="${"message" in signal ? signal.message : ""}" done=${"done" in signal ? signal.done : "?"} currentStatus=${state.status}`);
        applySignal(state, signal, performance.now());
        syncToReact();
      }
      return false;
    });

    // OSC 1337 (devin-idle) — Devin done signal (legacy, may not be emitted in current versions)
    const osc1337 = term.parser.registerOscHandler(1337, (data) => {
      const signal = parseOsc(1337, data);
      if (signal) {
        console.log(`[agentStatus] OSC 1337: kind=${signal.kind} currentStatus=${state.status}`);
        applySignal(state, signal, performance.now());
        syncToReact();
      }
      return false;
    });

    // BEL (^G) — Devin emits BEL alongside OSC 9/777 as a terminal bell.
    // When we receive a BEL and a CLI is detected, trigger a screen scrape
    // to check for done/blocked patterns (fallback for when OSC 9/777 are
    // suppressed by notify=smart mode with focused terminal).
    const bellHandler = term.onBell(() => {
      if (state.cli !== "unknown" && state.status === "working") {
        console.log(`[agentStatus] BEL received: cli=${state.cli} status=${state.status} — triggering screen check`);
        // The tick timer will scrape the screen on the next interval.
        // Force an immediate screen check by resetting lastOutputAt so
        // the working-idle timeout doesn't fire prematurely, and let
        // the tick screen scrape detect the current state.
        state.lastOutputAt = performance.now();
      }
    });

    disposablesRef.current.push(osc0, osc9, osc777, osc1337, bellHandler);

    // ── Tick timer: time-based transitions + screen scraping ───────────
    let lastSyncedStatus = state.status;
    let lastSyncedCli = state.cli;
    let tickCount = 0;
    tickTimerRef.current = setInterval(() => {
      tickCount++;
      const prevStatus = state.status;
      const prevCli = state.cli;
      tick(state, performance.now());

      // Screen scrape for CLIs that don't have native OSC status signals
      // (OpenCode, Claude Code, Codex) + Devin permission dialog fallback.
      // Devin primarily uses OSC 777/1337, but new permission dialogs (1-7 options)
      // may not trigger OSC, so we also screen-scrape for blocked status.
      if (state.cli !== "unknown") {
        try {
          const lines = scrapeScreen(term);
          const screenText = prepareScreenText(joinLines(lines));
          const screenStatus = detectStatus(state.cli, screenText);
          if (screenStatus) {
            // Apply status from screen (blocked, done, idle, working)
            if (screenStatus === "blocked" || screenStatus !== prevStatus) {
              console.log(`[agentStatus] tick#${tickCount} cli=${state.cli}: screenStatus=${screenStatus} prevStatus=${prevStatus} → applying`);
              applyScreenStatus(state, screenStatus, null, performance.now());
            }
          } else {
            // No CLI-specific status patterns found on screen.
            let correctedFromBlocked = false;
            if (state.status === "blocked" && !state.blockedFromOsc) {
              // Was blocked (by screen scrape) but screen no longer shows
              // blocked pattern. User answered the question / dialog was
              // dismissed → CLI resumed. Transition to working so the
              // spinner shows while CLI runs.
              //
              // NOTE: Only do this for screen-scrape-set blocked, NOT for
              // OSC-set blocked (blockedFromOsc=true). Devin's OSC 777
              // "Devin needs input" sets blocked authoritatively — the
              // screen may not show a blocked pattern (Devin's screen
              // patterns only cover permission dialogs), but the state
              // should remain blocked until another OSC signal clears it.
              console.log(`[agentStatus] tick#${tickCount} cli=${state.cli}: blocked (screen) but no blocked pattern on screen → working`);
              applyScreenStatus(state, "working", null, performance.now());
              correctedFromBlocked = true;
            }
            // Correct false "working" from user-input echo.
            // When the user types in an AI CLI's input box, PTY echoes the
            // chars back, notifyOutput() treats the echo as "AI is working"
            // and schedules a debounced transition to "working". But the
            // screen shows no spinner — just the user's input text.
            // detectStatus returns null because none of the CLI's patterns
            // match. If we don't correct here, the spin dot stays for the
            // entire typing duration.
            //
            // Skip if:
            // - just corrected from blocked→working (CLI resumed, spinner
            //   hasn't appeared yet — give it a tick to show up)
            // - screen is empty (can't determine status from an empty screen;
            //   the terminal might not have drawn anything yet)
            //
            // When a CLI IS working, its spinner is on screen and
            // detectStatus returns "working", so this branch is not reached.
            // Brief flicker possible if the debounce fires between the last
            // keystroke and the spinner appearing, but self-corrects on the
            // next tick (500ms).
            const hasScreenContent = screenText.trim().length > 0;
            if (!correctedFromBlocked && hasScreenContent &&
                state.status === "working") {
              console.log(`[agentStatus] tick#${tickCount} cli=${state.cli}: working but no spinner on screen → idle (echo correction)`);
              applyScreenStatus(state, "idle", null, performance.now());
              // Cancel any pending "working" debounce from recent echo
              state.pendingStatus = null;
            }
            // Check if the CLI has exited: if the LAST LINE of the screen
            // shows a plain shell prompt (user@host path % or $), reset to
            // unknown so the status dot disappears.
            //
            // We check only the last line, not the full screen, because the
            // CLI's banner/logo may still be visible above the shell prompt
            // after exit. detectCli on the full screen would still detect
            // the CLI from the banner, preventing the reset.
            if (state.status !== "blocked") {
              const allLines = screenText.split("\n").filter((l) => l.trim().length > 0);
              const lastLine = allLines.length > 0 ? allLines[allLines.length - 1] : "";
              if (isShellPrompt(lastLine)) {
                console.log(`[agentStatus] tick#${tickCount} cli=${state.cli}: shell prompt on last line, resetting to unknown`);
                setCliType(state, "unknown");
                state.status = "unknown";
                state.pendingStatus = null;
                state.blockedMessage = null;
                state.blockedFromOsc = false;
              }
            }
          }
        } catch {
          // ignore scrape errors
        }
      } else {
        // Try to detect CLI from screen content, then immediately detect status
        try {
          const lines = scrapeScreen(term);
          const screenText = prepareScreenText(joinLines(lines));
          const detected = detectCli("", screenText);
          if (detected !== "unknown") {
            console.log(`[agentStatus] tick#${tickCount}: detected CLI=${detected} from screen`);
            setCliType(state, detected);
            // Immediately detect status on same tick
            const screenStatus = detectStatus(detected, screenText);
            if (screenStatus) {
              applyScreenStatus(state, screenStatus, null, performance.now());
            }
          }
        } catch {
          // ignore scrape errors
        }
      }

      if (state.status !== prevStatus || state.status !== lastSyncedStatus || state.cli !== prevCli || state.cli !== lastSyncedCli) {
        console.log(`[agentStatus] tick#${tickCount}: syncing React — status ${lastSyncedStatus}→${state.status} cli ${lastSyncedCli}→${state.cli}`);
        lastSyncedStatus = state.status;
        lastSyncedCli = state.cli;
        syncToReact();
      }
    }, TICK_INTERVAL_MS);

    // Register state so TerminalView can call notifyAgentOutput(sessionId)
    registerAgentState(sessionId, state);
    registerAgentListener(sessionId, syncToReact);

    return () => {
      unregisterAgentState(sessionId);
      for (const d of disposablesRef.current) {
        try {
          d.dispose();
        } catch {
          // ignore
        }
      }
      disposablesRef.current = [];
      if (tickTimerRef.current) {
        clearInterval(tickTimerRef.current);
        tickTimerRef.current = null;
      }
    };
  }, [term, sessionId]);

  return info;
}

/**
 * Notify the agent state machine that PTY output was received.
 *
 * TerminalView should call this whenever it writes data to the terminal
 * (i.e. receives output from the SSH session). This drives the "working"
 * state inference: any output = AI is active.
 *
 * We use a module-level map (sessionId → state + listener) so TerminalView
 * can call this without needing a direct reference to the hook's state.
 * The listener triggers a React re-render when state changes.
 */
const agentStates = new Map<string, { state: AgentState; listener: (() => void) | null }>();

/** Register a session's agent state (called by useAgentStatus). */
export function registerAgentState(sessionId: string, state: AgentState): void {
  agentStates.set(sessionId, { state, listener: null });
}

/** Register a listener that fires when the state changes externally. */
export function registerAgentListener(sessionId: string, listener: (() => void) | null): void {
  const entry = agentStates.get(sessionId);
  if (entry) {
    entry.listener = listener;
  }
}

/** Unregister a session's agent state. */
export function unregisterAgentState(sessionId: string): void {
  agentStates.delete(sessionId);
}

/**
 * Notify that PTY output was received for a session.
 * TerminalView calls this in its output handler.
 */
export function notifyAgentOutput(sessionId: string): void {
  const entry = agentStates.get(sessionId);
  if (entry) {
    const prevStatus = entry.state.status;
    notifyOutput(entry.state, performance.now());
    // If status changed (e.g. blocked → working), trigger listener to sync React
    if (entry.state.status !== prevStatus && entry.listener) {
      console.log(`[agentStatus] notifyAgentOutput: status ${prevStatus}→${entry.state.status} cli=${entry.state.cli}`);
      entry.listener();
    }
  }
}

/**
 * Reset the agent state for a session (e.g. when the terminal/SSH connection closes).
 * This clears the status to "unknown" so the tab stops showing a spinner.
 * TerminalView calls this when it receives a "terminal:closed" event.
 */
export function resetAgentStatus(sessionId: string): void {
  const entry = agentStates.get(sessionId);
  if (entry) {
    const prevStatus = entry.state.status;
    resetAgentState(entry.state, performance.now());
    if (entry.state.status !== prevStatus && entry.listener) {
      console.log(`[agentStatus] resetAgentStatus: status ${prevStatus}→${entry.state.status}`);
      entry.listener();
    }
  }
}
