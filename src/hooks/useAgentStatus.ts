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
  clearScreenBlocked,
  notifyOutput,
  tick,
  setCliType,
  resetAgentState,
  type AgentState,
  type AgentStatus,
  BLOCKED_MISS_THRESHOLD,
} from "./agentStateMachine";
import { scrapeScreen, joinLines, extractTabInfo } from "./screenScraper";
import { detectCli, detectCliFromScreen, detectStatus, prepareScreenText } from "./cliDetector";
import { extractQuestion, extractOptions, detectMultiSelect, detectMultiQuestion, extractReviewAnswers, extractCursorIndex } from "./agentPatterns";
import { getBehavior } from "./cliBehavior";
import { logTerminalDebug } from "./terminalLogger";

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
  /** True if the current blocked dialog is multi-select (checkboxes + Submit). */
  isMultiSelect: boolean;
  /** True if the current blocked dialog is multi-question (has Confirm tab). */
  isMultiQuestion: boolean;
  /** Active tab index in multi-question dialog (-1 if not multi-question or detection failed). */
  activeTabIndex: number;
  /** Total number of tabs in multi-question dialog (0 if not multi-question). */
  totalTabs: number;
  /** Review answers extracted from the Confirm tab (null if not on Confirm tab). */
  reviewAnswers: string[] | null;
  /** True if Devin's "Other" option is in "└ e" text editing mode (user pressed 'e' to type). */
  otherExpanded: boolean;
  /** Cursor position (❭ marker) in single-select mode — 0-based index, or null if not detectable. */
  cursorIndex: number | null;
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
    isMultiSelect: false,
    isMultiQuestion: false,
    activeTabIndex: -1,
    totalTabs: 0,
    reviewAnswers: null,
    otherExpanded: false,
    cursorIndex: null,
  });

  // State machine lives in a ref — mutated in place, no re-allocation per tick.
  const stateRef = useRef<AgentState>(createAgentState(performance.now()));
  // Track disposables so we can clean up on unmount / term change.
  const disposablesRef = useRef<Array<{ dispose: () => void }>>([]);
  // Cache options per active tab for Devin multi-select.
  // When the cursor moves to "Other (type your own)" in Devin's multi-select,
  // the terminal shows a text input field and the option list disappears.
  // extractOptions then only returns ["Other (type your own)"].
  // We cache the full option list per tab to avoid the overlay shrinking to 1 option.
  const cachedOptionsRef = useRef<Map<number, string[]>>(new Map());
  const tickTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    if (!term) return;

    const state = stateRef.current;

    // Sync state changes to React state (triggers re-render).
    // Also scrapes screen for question/options when blocked.
    const syncToReact = () => {
      let question: string | null = null;
      let options: string[] | null = null;
      let isMultiSelect = false;
      let isMultiQuestion = false;
      let activeTabIndex = -1;
      let totalTabs = 0;
      let reviewAnswers: string[] | null = null;
      let otherExpanded = false;
      let cursorIndex: number | null = null;

      // Only scrape screen for question/options when blocked
      if (state.status === "blocked" && state.cli !== "unknown") {
        try {
          const lines = scrapeScreen(term);
          const screenText = prepareScreenText(joinLines(lines));
          question = extractQuestion(state.cli, screenText);
          options = extractOptions(state.cli, screenText);
          isMultiSelect = detectMultiSelect(state.cli, screenText);
          isMultiQuestion = detectMultiQuestion(state.cli, screenText);
          // Detect "└" expanded state on Other option (Devin).
          // When the cursor is on "Other (type your own)" and it's expanded,
          // Detect "Other" text editing mode — delegated to behavior.
          // Devin shows "└ e" (editing) or "└ text" (display) or "└" (empty).
          // In all these states, ←→ moves the text cursor, not switches tabs.
          const behavior = getBehavior(state.cli);
          otherExpanded = behavior.detectOtherExpanded(screenText, isMultiSelect, isMultiQuestion);
          // Detect cursor position (❭ marker) for single-select mode
          if (!isMultiSelect) {
            cursorIndex = extractCursorIndex(state.cli, screenText);
          }
          // Debug: log screenText when multiSelect/multiQuestion detected
          if (isMultiSelect || isMultiQuestion) {
            logTerminalDebug(sessionId, `screenText for multiSelect=${isMultiSelect} multiQuestion=${isMultiQuestion}: ${JSON.stringify(screenText.slice(-500))}`);
          }
          // Extract tab info for multi-question dialogs (active tab detection)
          if (isMultiQuestion) {
            const tabInfo = extractTabInfo(term);
            if (tabInfo) {
              activeTabIndex = tabInfo.activeIndex;
              totalTabs = tabInfo.labels.length;
              // Debug: log tab info to file for diagnosis
              logTerminalDebug(sessionId, `tabInfo: labels=${JSON.stringify(tabInfo.labels)} activeIndex=${tabInfo.activeIndex} debug=${tabInfo.debug ?? "none"}`);
            }
            // Extract review answers when on the Confirm tab (last tab)
            if (tabInfo && tabInfo.activeIndex === tabInfo.labels.length - 1) {
              reviewAnswers = extractReviewAnswers(state.cli, screenText);
            }
          }
          // Cache options for CLIs that need it (Devin: when cursor is on
          // "Other (type your own)", the option list disappears — use cached
          // options to keep the overlay stable).
          if (behavior.cacheOptionsOnOther && isMultiSelect && options && options.length === 1 && /other/i.test(options[0])) {
            const cached = cachedOptionsRef.current.get(activeTabIndex);
            if (cached && cached.length > 1) {
              options = cached;
            }
          } else if (behavior.cacheOptionsOnOther && isMultiSelect && options && options.length > 1) {
            // Cache the full option list for this tab
            cachedOptionsRef.current.set(activeTabIndex, options);
          }
          // Update state machine's multiSelect flag
          state.isMultiSelect = isMultiSelect;
          // Debug: log extraction results for blocked state diagnosis
          const extractMsg = `blocked extraction: cli=${state.cli} question=${JSON.stringify(question)} options=${JSON.stringify(options)} multiSelect=${isMultiSelect} multiQuestion=${isMultiQuestion} activeTab=${activeTabIndex} totalTabs=${totalTabs} reviewAnswers=${JSON.stringify(reviewAnswers)}`;
          // console.log(`[agentStatus] ${extractMsg}`);  // too noisy — use logTerminalDebug only
          logTerminalDebug(sessionId, extractMsg);
          // Debug: log raw screen lines for option extraction diagnosis
          if (behavior.cacheOptionsOnOther && isMultiSelect && options && options.length > 3) {
            const nonEmpty = lines.map((l, i) => ({ i, l })).filter((x) => x.l.trim());
            logTerminalDebug(sessionId, `screenLines(${nonEmpty.length}): ${JSON.stringify(nonEmpty)}`);
          }
        } catch {
          // Screen scrape can fail if terminal is in a weird state — ignore
        }
      } else if (state.status !== "blocked") {
        // Not blocked: clear cached options so next dialog starts fresh
        cachedOptionsRef.current.clear();
      }

      setInfo({
        status: state.status,
        cli: state.cli,
        blockedMessage: state.blockedMessage,
        question,
        options,
        isMultiSelect,
        isMultiQuestion,
        activeTabIndex,
        totalTabs,
        reviewAnswers,
        otherExpanded,
        cursorIndex,
      });
    };

    // ── Register OSC handlers ──────────────────────────────────────────

    // OSC 0 (set title) — CLI detection for OpenCode, Claude Code, Codex
    const osc0 = term.parser.registerOscHandler(0, (data) => {
      const signal = parseOsc(0, data);
      if (signal && signal.kind === "title") {
        // console.log(`[agentStatus] OSC 0: title="${signal.title}" cli=${signal.cli} currentCli=${state.cli} currentStatus=${state.status}`);  // too noisy
        let changed = false;
        // Update CLI type if not already detected, OR if the title indicates
        // a DIFFERENT CLI than the one currently detected (e.g. user exited
        // opencode and launched claude-code in the same terminal tab — the
        // CLI type is sticky but a clear OSC 0 title from another CLI should
        // override it).
        if (signal.cli !== "unknown" && state.cli !== signal.cli) {
          setCliType(state, signal.cli);
          changed = true;
        }
        // Handle CLI-specific title signals (e.g. Codex "Action Required" = blocked)
        const titleBehavior = getBehavior(signal.cli);
        const titleResult = titleBehavior.handleOscTitle(signal.title);
        if (titleResult) {
          applyScreenStatus(state, titleResult.status, titleResult.message, performance.now());
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
        //         console.log(`[agentStatus] OSC 777: kind=${signal.kind} cli=${"cli" in signal ? signal.cli : "?"} message="${"message" in signal ? signal.message : ""}" done=${"done" in signal ? signal.done : "?"} currentStatus=${state.status}`);
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
        //         console.log(`[agentStatus] OSC 9: kind=${signal.kind} message="${"message" in signal ? signal.message : ""}" done=${"done" in signal ? signal.done : "?"} currentStatus=${state.status}`);
        applySignal(state, signal, performance.now());
        syncToReact();
      }
      return false;
    });

    // OSC 1337 (devin-idle) — Devin done signal (legacy, may not be emitted in current versions)
    const osc1337 = term.parser.registerOscHandler(1337, (data) => {
      const signal = parseOsc(1337, data);
      if (signal) {
        //         console.log(`[agentStatus] OSC 1337: kind=${signal.kind} currentStatus=${state.status}`);
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
        //         console.log(`[agentStatus] BEL received: cli=${state.cli} status=${state.status} — triggering screen check`);
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
          // CLI override: if the screen content strongly indicates a DIFFERENT CLI
          // than the one currently detected (e.g. user exited claude-code and launched
          // codex in the same terminal tab), override the sticky CLI type.
          // But don't override Devin — its CLI type is set by OSC signals which
          // are authoritative, and screen scrape patterns (like "esc interrupt")
          // can false-match OpenCode patterns when Devin's screen happens to
          // show similar footer text.
          const screenCli = detectCliFromScreen(screenText);
          if (screenCli !== "unknown" && screenCli !== state.cli && state.cli !== "devin") {
            setCliType(state, screenCli);
          }
          // Debug: log screen content every 10 ticks (~5s) for diagnosis
          if (tickCount % 10 === 0) {
            const nonEmpty = lines.filter((l) => l.trim().length > 0);
            const screenDump = nonEmpty.map((l, i) => `  [${i}] ${JSON.stringify(l)}`).join("\n");
            const msg = `tick#${tickCount} cli=${state.cli} status=${state.status} screen (${nonEmpty.length} non-empty lines):\n${screenDump}`;
            // console.log(`[agentStatus] ${msg}`);  // too noisy — use logTerminalDebug only
            logTerminalDebug(sessionId, msg);
          }
          const screenStatus = detectStatus(state.cli, screenText);
          if (tickCount % 10 === 0) {
            const msg2 = `tick#${tickCount} detectStatus=${screenStatus} prevStatus=${prevStatus}`;
            // console.log(`[agentStatus] ${msg2}`);  // too noisy — use logTerminalDebug only
            logTerminalDebug(sessionId, msg2);
          }
          if (screenStatus === "blocked") {
            // Blocked pattern detected — apply immediately, reset miss count
            state.blockedMissCount = 0;
            //             console.log(`[agentStatus] tick#${tickCount} cli=${state.cli}: screenStatus=blocked → applying (miss count reset)`);
            applyScreenStatus(state, "blocked", null, performance.now());
          } else if ((screenStatus === "working" || screenStatus === "done") &&
                     state.status === "blocked" && !state.blockedFromOsc) {
            // Definitive non-blocked signal (spinner or completion marker).
            // The CLI has clearly resumed — apply immediately, no miss count.
            //             console.log(`[agentStatus] tick#${tickCount} cli=${state.cli}: screenStatus=${screenStatus} while blocked → applying (definitive signal)`);
            applyScreenStatus(state, screenStatus, null, performance.now());
            state.blockedMissCount = 0;
          } else if (screenStatus === "idle" && state.status === "blocked" && !state.blockedFromOsc) {
            // Currently blocked (screen-set) but screen shows idle. This could be:
            //  A. Permission dialog dismissed → CLI idle
            //  B. Permission dialog in alt-screen redraw gap → still blocked
            // The idle footer (ctrl+p commands) is always visible in OpenCode,
            // even during permission dialogs, so idle is NOT definitive.
            // Use blockedMissCount: require N consecutive idle detections.
            state.blockedMissCount++;
            if (state.blockedMissCount >= BLOCKED_MISS_THRESHOLD) {
              //               console.log(`[agentStatus] tick#${tickCount} cli=${state.cli}: blocked miss count=${state.blockedMissCount} ≥ ${BLOCKED_MISS_THRESHOLD} → applying idle`);
              clearScreenBlocked(state, "idle", performance.now());
            } else {
              //               console.log(`[agentStatus] tick#${tickCount} cli=${state.cli}: blocked miss count=${state.blockedMissCount} < ${BLOCKED_MISS_THRESHOLD} → staying blocked (redraw gap?)`);
            }
          } else if (screenStatus) {
            // Normal case: not blocked, apply status change if different
            if (screenStatus !== prevStatus) {
              //               console.log(`[agentStatus] tick#${tickCount} cli=${state.cli}: screenStatus=${screenStatus} prevStatus=${prevStatus} → applying`);
              applyScreenStatus(state, screenStatus, null, performance.now());
            }
          } else {
            // No CLI-specific status patterns found on screen (null).
            let correctedFromBlocked = false;
            if (state.status === "blocked" && !state.blockedFromOsc) {
              // Currently blocked (screen-set) but no pattern matched at all.
              // Same redraw-gap logic: use miss count before clearing.
              state.blockedMissCount++;
              if (state.blockedMissCount >= BLOCKED_MISS_THRESHOLD) {
                //                 console.log(`[agentStatus] tick#${tickCount} cli=${state.cli}: blocked miss count=${state.blockedMissCount} ≥ ${BLOCKED_MISS_THRESHOLD} → working (dialog dismissed)`);
                clearScreenBlocked(state, "working", performance.now());
                correctedFromBlocked = true;
              } else {
                //                 console.log(`[agentStatus] tick#${tickCount} cli=${state.cli}: blocked miss count=${state.blockedMissCount} < ${BLOCKED_MISS_THRESHOLD} → staying blocked (redraw gap?)`);
              }
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
              //               console.log(`[agentStatus] tick#${tickCount} cli=${state.cli}: working but no spinner on screen → idle (echo correction)`);
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
                //                 console.log(`[agentStatus] tick#${tickCount} cli=${state.cli}: shell prompt on last line, resetting to unknown`);
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
          // Debug: log screen content every 10 ticks for CLI detection diagnosis
          if (tickCount % 10 === 0) {
            const nonEmpty = lines.filter((l) => l.trim().length > 0);
            const screenDump = nonEmpty.map((l, i) => `  [${i}] ${JSON.stringify(l)}`).join("\n");
            const msg = `tick#${tickCount} cli=unknown — screen (${nonEmpty.length} non-empty lines):\n${screenDump}`;
            // console.log(`[agentStatus] ${msg}`);  // too noisy — use logTerminalDebug only
            logTerminalDebug(sessionId, msg);
          }
          const detected = detectCli("", screenText);
          if (detected !== "unknown") {
            //             console.log(`[agentStatus] tick#${tickCount}: detected CLI=${detected} from screen`);
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
        // console.log(`[agentStatus] tick#${tickCount}: syncing React — status ${lastSyncedStatus}→${state.status} cli ${lastSyncedCli}→${state.cli}`);  // too noisy
        lastSyncedStatus = state.status;
        lastSyncedCli = state.cli;
        syncToReact();
      } else if (state.status === "blocked") {
        // Still blocked — question/options may have changed (multi-question
        // dialogs: OpenCode shows Q1, then Q2, etc. without leaving "blocked").
        // Re-extract question/options so the overlay updates.
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
      //       console.log(`[agentStatus] notifyAgentOutput: status ${prevStatus}→${entry.state.status} cli=${entry.state.cli}`);
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
      //       console.log(`[agentStatus] resetAgentStatus: status ${prevStatus}→${entry.state.status}`);
      entry.listener();
    }
  }
}
