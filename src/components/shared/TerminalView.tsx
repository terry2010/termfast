// TerminalView — xterm.js wrapper for interactive SSH terminal
// Connects to a backend PTY session via IPC
// Supports ZMODEM (rz/sz) file transfers via zmodem.js-ex
// Supports trzsz (trz/tsz) file transfers via trzsz.js (tmux-compatible)

import { useEffect, useRef, useState, useCallback } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { SearchAddon } from "@xterm/addon-search";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { SerializeAddon } from "@xterm/addon-serialize";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { save as saveDialog, open as openDialog } from "@tauri-apps/plugin-dialog";
import { open, remove, copyFile, readFile, writeFile, BaseDirectory, type FileHandle } from "@tauri-apps/plugin-fs";
import { TrzszFilter } from "trzsz";
import { ipcInvoke } from "@/hooks/useIpc";
import { useAgentStatus, notifyAgentOutput, resetAgentStatus } from "@/hooks/useAgentStatus";
import type { AgentStatus } from "@/hooks/agentStateMachine";
import { shouldResetOverlay } from "@/hooks/overlayReset";
import { getBehavior, executeSteps, type BehaviorContext } from "@/hooks/cliBehavior";
import { scrapeScreen, joinLines, getCursorLine, getCursorOptionIndex } from "@/hooks/screenScraper";
import { prepareScreenText } from "@/hooks/cliDetector";
import { extractQuestion, extractOptions, extractCursorIndex, detectMultiSelect } from "@/hooks/agentPatterns";
import type { CliType } from "@/hooks/oscParser";
import { AgentQuestionOverlay } from "@/components/shared/AgentQuestionOverlay";
import {
  initTerminalLog,
  logTerminalInput,
  logTerminalOutput,
  flushTerminalLog,
  closeTerminalLog,
} from "@/hooks/terminalLogger";
import { Channel } from "@tauri-apps/api/core";
import { useConfigStore } from "@/stores/configStore";
import { useServerStore } from "@/stores/serverStore";
import { getTerminalTheme } from "@/lib/terminalThemes";
import { Sentry as ZmodemSentry, type ZmodemDetection, type ZmodemSession, type ZmodemTransfer } from "zmodem.js-ex";
import * as ZmodemLib from "zmodem.js-ex";
import "@xterm/xterm/css/xterm.css";

// === ZMODEM library patches ===
// zmodem.js-ex (v3.0.0) has several bugs that prevent rz/sz from working
// with lrzsz. We monkey-patch the library at runtime to fix them:
//
// 1. Sentry only detects ZRQINIT (type 0 = sz/download), not ZRINIT
//    (type 1 = rz/upload). The COMMON_ZM_HEX_START constant includes the
//    type byte '0', so rz's ZRINIT is never detected and no session is
//    created — the ZRINIT bytes go to the terminal as garble.
//
// 2. Session.Send._stop_keepalive has a typo: it sets `_keep_alive_promise`
//    (with extra underscore) instead of `_keepalive_promise`. This means
//    the keepalive promise is never cleared, so _start_keepalive refuses to
//    start a new timer (it checks `if (!this._keepalive_promise)`). More
//    critically, the pending keepalive's .then() can still fire after
//    _stop_keepalive is called, overwriting _next_header_handler with a
//    ZACK handler at an unexpected time (e.g. during send_offer).
//
// 3. _consume_header throws on ANY unhandled header, crashing the entire
//    session. Some headers can arrive unexpectedly as PTY echo (ZRINIT) or
//    retransmits (ZRQINIT) and should be silently ignored.

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const ZmodemAny = (ZmodemLib as any).default || (ZmodemLib as any);

// Patch 1: Sentry._parse — use 4-byte prefix (** ZDLE B) instead of 5-byte
// (** ZDLE B 0) so both ZRQINIT (download) and ZRINIT (upload) are detected.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const SentryProto: any = ZmodemAny.Sentry?.prototype;
if (SentryProto && !SentryProto._patched_parse) {
  SentryProto._parse = function (array_like: any) {
    var cache = this._cache;
    cache.push.apply(cache, array_like);
    // ** ZDLE B — common prefix for all hex headers (ZRQINIT='0', ZRINIT='1')
    var COMMON_PREFIX = [42, 42, 24, 66];
    while (true) {
      var at = ZmodemAny.ZMLIB.find_subarray(cache, COMMON_PREFIX);
      if (at === -1) break;
      cache.splice(0, at);
      var zsession;
      try { zsession = ZmodemAny.Session.parse(cache); } catch (e) { /* ignore */ }
      if (!zsession) break;
      if (cache.length === 1 && cache[0] === ZmodemAny.ZMLIB.XON) cache.shift();
      return cache.length ? null : zsession;
    }
    cache.splice(21); // MAX_ZM_HEX_START_LENGTH
    return null;
  };
  SentryProto._patched_parse = true;
}

// Patch 2: Completely replace _start_keepalive and _stop_keepalive.
// The original code has a typo: _stop_keepalive sets _keep_alive_promise
// (extra underscore) instead of _keepalive_promise, so the promise is never
// cleared. Worse, the .then() callback unconditionally sends ZSINIT and
// restarts the timer — there is no "stopped" check. This means even after
// _stop_keepalive clears the timeout, if the .then() already fired (race),
// it will send ZSINIT and start a new timer, creating an infinite loop of
// ZSINIT packets after the session ends.
// We add a _keepalive_stopped flag checked both in _start_keepalive AND
// in the .then() callback to fully suppress keepalive after session end.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const SendSessionProto: any = ZmodemAny.Session?.Send?.prototype;
if (SendSessionProto && !SendSessionProto._patched_stop_keepalive) {
  SendSessionProto._start_keepalive = function () {
    if (this._keepalive_stopped) return;
    if (!this._keepalive_promise) {
      var sess = this;
      this._keepalive_promise = new Promise(function (resolve) {
        sess._keepalive_timeout = setTimeout(resolve, 5000);
      }).then(function () {
        // Check if session ended while we were waiting
        if (sess._keepalive_stopped) {
          sess._keepalive_promise = null;
          return;
        }
        sess._next_header_handler = {
          ZACK: function () { sess._got_ZSINIT_ZACK = true; },
        };
        sess._send_ZSINIT();
        sess._keepalive_promise = null;
        sess._start_keepalive();
      });
    }
  };
  SendSessionProto._stop_keepalive = function () {
    this._keepalive_stopped = true;
    if (this._keepalive_timeout) {
      clearTimeout(this._keepalive_timeout);
    }
    this._keepalive_promise = null;
  };
  SendSessionProto._patched_stop_keepalive = true;
}

// Patch 3: _consume_header — NEVER throw on unhandled headers.
// Instead, silently skip ALL unhandled headers (ZDATA, ZEOF, ZACK, ZSKIP,
// ZRINIT, ZRQINIT, etc.). This is critical because:
// - PTY echo causes our sent ZMODEM bytes to come back and be parsed
// - Timing issues can cause headers to arrive before handlers are set
// - Throwing causes abort → Sentry clears _zsession → re-parse of echoed
//   protocol bytes → spurious session → crash loop
// By skipping silently, the session state machine stays alive and can
// recover when the expected header arrives (or the peer retransmits).
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const SessionProto: any = ZmodemAny.Session?.prototype;
if (SessionProto && !SessionProto._patched_consume_header) {
  SessionProto._consume_header = function (new_header: { NAME: string }) {
    this._on_receive(new_header);
    if (!this._next_header_handler) {
      // No handler set yet — skip silently
      return;
    }
    var handler = this._next_header_handler[new_header.NAME];
    if (!handler) {
      // Unhandled header — skip silently, keep existing handler
      // so the expected header can still be processed later
      return;
    }
    this._next_header_handler = null;
    handler.call(this, new_header);
  };
  SessionProto._patched_consume_header = true;
}

// Patch 4: consume — catch errors but DON'T abort. Just log and continue.
// Aborting causes the Sentry to clear _zsession, which leads to re-parsing
// of echoed protocol bytes and spurious session creation.
if (SessionProto && !SessionProto._patched_consume) {
  var originalConsume = SessionProto.consume;
  SessionProto.consume = function (octets: number[]) {
    try {
      return originalConsume.call(this, octets);
    } catch (e) {
      // Log but don't abort — the session may recover on subsequent chunks
      console.error("[ZMODEM] session consume error (recovered):", e);
    }
  };
  SessionProto._patched_consume = true;
}

// Patch 5: close() — stop keepalive BEFORE sending ZFIN to prevent
// the keepalive .then() from overwriting the ZFIN handler.
// NB: Session.Send has its own close() that overrides Session.close(),
// so we must patch Session.Send.prototype.close directly.
const SendProtoClose: any = ZmodemAny.Session?.Send?.prototype;
if (SendProtoClose && !SendProtoClose._patched_close) {
  const origSendClose = SendProtoClose.close;
  SendProtoClose.close = function () {
    // Stop keepalive first — prevents race where keepalive .then()
    // overwrites _next_header_handler after close() sets { ZFIN }
    if (typeof this._stop_keepalive === "function") {
      this._stop_keepalive();
    }
    return origSendClose.call(this);
  };
  SendProtoClose._patched_close = true;
}
// Also patch Session.prototype.close for Receive sessions
if (SessionProto && !SessionProto._patched_close) {
  const origClose = SessionProto.close;
  SessionProto.close = function () {
    if (typeof this._stop_keepalive === "function") {
      this._stop_keepalive();
    }
    return origClose.call(this);
  };
  SessionProto._patched_close = true;
}

// Patch 6: Session.Receive._consume_first — don't throw if OO is missing
// after ZFIN. Some lrzsz/sz implementations send ZFIN then exit without
// sending OO. If we throw, the Sentry re-parses the remaining input and
// creates a spurious second Receive session. Treat the remaining bytes as
// trailing and end the session cleanly.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const ReceiveSessionProto: any = ZmodemAny.Session?.Receive?.prototype;
if (ReceiveSessionProto && !ReceiveSessionProto._patched_consume_first) {
  const origConsumeFirst = ReceiveSessionProto._consume_first;
  ReceiveSessionProto._consume_first = function () {
    if (this._got_ZFIN) {
      if (this._input_buffer.length < 2) return;
      const OO = [79, 79];
      if (ZmodemAny.ZMLIB.find_subarray(this._input_buffer, OO) === 0) {
        // OO found at start of remaining input — trim it from the trailing
        // bytes and end the session.
        this._bytes_after_OO = this._bytes_being_consumed.slice(0);
        if (this._bytes_after_OO[0] === OO[0] && this._bytes_after_OO[1] === OO[1]) {
          this._bytes_after_OO.splice(0, OO.length);
        } else if (this._bytes_after_OO[0] === OO[1]) {
          this._bytes_after_OO.splice(0, 1);
        }
        this._on_session_end();
        return;
      }
      // OO missing — just end the session, trailing bytes will be written
      // by the Sentry's to_terminal.
      this._bytes_after_OO = this._bytes_being_consumed.slice(0);
      this._on_session_end();
      return;
    }
    return origConsumeFirst.call(this);
  };
  ReceiveSessionProto._patched_consume_first = true;
}

// Patch 7: Session.Receive._consume_ZFIN — guard against sending ZFIN twice.
// _consume_first may be called with a second ZFIN if the peer retransmits or
// if an echoed ZFIN is fed back. Only send one ZFIN response.
if (ReceiveSessionProto && !ReceiveSessionProto._patched_consume_ZFIN) {
  const origConsumeZFIN = ReceiveSessionProto._consume_ZFIN;
  ReceiveSessionProto._consume_ZFIN = function () {
    if (this._got_ZFIN) return;
    return origConsumeZFIN.call(this);
  };
  ReceiveSessionProto._patched_consume_ZFIN = true;
}
// === End ZMODEM library patches ===

interface TerminalViewProps {
  sessionId: string;
  serverId: string;
  active: boolean;
  initialOutput?: string;
  rzAvailable?: boolean;
  /** Terminal tab ID (for updating agentStatus in the store). */
  tabId?: string;
  /** Tmux session name if this terminal is inside a tmux session. */
  tmuxSessionName?: string;
}

// Module-level snapshot cache: key = serverId, value = serialized terminal
// Preserved across unmount/remount (e.g. reconnect) so history isn't lost.
const snapshotCache = new Map<string, string>();

// Module-level WebGL context counter — browsers limit ~16 concurrent contexts.
// We only create a WebGL context if we're under the limit; otherwise DOM render.
const MAX_WEBGL_CONTEXTS = 8;
let activeWebglCount = 0;

// Module-level callback registry for binary terminal output.
// Key = session_id, value = callback that receives raw Uint8Array.
// TerminalView registers on mount; ServerDetail's Channel.onmessage dispatches here.
const terminalOutputCallbacks = new Map<string, (data: Uint8Array, isStderr: boolean) => void>();

/** Register a callback for binary terminal output (called by TerminalView on mount) */
export function registerTerminalOutput(sessionId: string, cb: (data: Uint8Array, isStderr: boolean) => void) {
  terminalOutputCallbacks.set(sessionId, cb);
}

/** Unregister a callback (called by TerminalView on unmount) */
export function unregisterTerminalOutput(sessionId: string) {
  terminalOutputCallbacks.delete(sessionId);
}

/** Dispatch raw binary terminal output to the registered callback (called by Channel.onmessage) */
export function dispatchTerminalOutput(sessionId: string, data: Uint8Array, isStderr: boolean) {
  const cb = terminalOutputCallbacks.get(sessionId);
  if (cb) cb(data, isStderr);
}

// Decode base64 to Uint8Array (used for initial_output only — small one-time payload)
function base64ToBytes(b64: string): Uint8Array {
  const binary = atob(b64);
  const arr = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    arr[i] = binary.charCodeAt(i);
  }
  return arr;
}

// Convert an array of byte values to a string for xterm.js
function octetsToString(octets: number[]): string {
  let str = "";
  for (let i = 0; i < octets.length; i++) {
    str += String.fromCharCode(octets[i]);
  }
  return str;
}

// Strip leading ZMODEM hex headers (e.g. ZFIN echoes) so the terminal
// doesn't display protocol frames as text. Hex headers start with ** ZDLE B
// and end with \r\n (optionally followed by XON 0x11).
function stripLeadingZmodemHeaders(octets: number[]): number[] {
  let i = 0;
  while (i < octets.length) {
    if (octets[i] === 0x2a && octets[i + 1] === 0x2a) {
      // hex header: skip to the first \r\n
      const cr = octets.indexOf(0x0d, i);
      if (cr === -1 || cr + 1 >= octets.length || octets[cr + 1] !== 0x0a) {
        break;
      }
      i = cr + 2;
      if (octets[i] === 0x11) i++; // XON
      continue;
    }
    if (octets[i] === 0x0d && octets[i + 1] === 0x0a) {
      i += 2;
      continue;
    }
    break;
  }
  return octets.slice(i);
}

export function TerminalView({ sessionId, serverId, active, initialOutput, rzAvailable, tabId, tmuxSessionName }: TerminalViewProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const searchRef = useRef<SearchAddon | null>(null);
  const serializeRef = useRef<SerializeAddon | null>(null);
  const webglRef = useRef<WebglAddon | null>(null);
  const sessionIdRef = useRef(sessionId);
  sessionIdRef.current = sessionId;
  // Track Devin multi-select cursor position for relative arrow navigation.
  // Reset to 0 when entering a new question or switching tabs.
  const devinCursorPosRef = useRef(0);

  // AI CLI agent status — binds OSC handlers + output monitoring to the terminal
  const [agentTerm, setAgentTerm] = useState<Terminal | null>(null);
  const { status: agentStatus, cli: agentCli, blockedMessage: agentBlockedMessage, question: agentQuestion, options: agentOptions, isMultiSelect: agentIsMultiSelect, isMultiQuestion: agentIsMultiQuestion, activeTabIndex: agentActiveTabIndex, totalTabs: agentTotalTabs, reviewAnswers: agentReviewAnswers, otherExpanded: agentOtherEditing, cursorIndex: agentCursorIndex } = useAgentStatus(agentTerm, sessionId);
  const setTerminalTabAgentStatus = useServerStore((s) => s.setTerminalTabAgentStatus);
  const [agentOverlayDismissed, setAgentOverlayDismissed] = useState(false);
  // Tracks whether a navigation (prev/next question) is in progress,
  // to disable buttons while keystrokes are being sent.
  const [agentNavigating, setAgentNavigating] = useState(false);
  // Ref mirror of agentNavigating for use inside async closures without
  // re-creating the callback on every state change.
  const agentNavigatingRef = useRef(false);
  useEffect(() => { agentNavigatingRef.current = agentNavigating; }, [agentNavigating]);
  // Track which option the user selected per tab (tabIndex → optionIndex).
  // Shared with AgentQuestionOverlay so TerminalView can verify cursor
  // position after navigation and correct if needed.
  const answeredTabsRef = useRef<Map<number, number>>(new Map());

  // Sync agent status to tab store (for tab label rendering)
  useEffect(() => {
    if (tabId && serverId) {
      setTerminalTabAgentStatus(serverId, tabId, agentStatus);
    }
  }, [agentStatus, tabId, serverId, setTerminalTabAgentStatus]);

  // Reset overlay dismissed state when status changes from blocked to non-blocked,
  // OR when a new question appears while still blocked (multi-question dialogs:
  // OpenCode shows question 1, then question 2, etc. without leaving "blocked").
  const agentStatusRef = useRef<AgentStatus>("unknown");
  const agentQuestionRef = useRef<string | null>(null);
  useEffect(() => {
    if (shouldResetOverlay(agentStatusRef.current, agentStatus, agentQuestionRef.current, agentQuestion)) {
      setAgentOverlayDismissed(false);
      devinCursorPosRef.current = 0;
    }
    agentStatusRef.current = agentStatus;
    agentQuestionRef.current = agentQuestion;
  }, [agentStatus, agentQuestion]);

  // Handle answer submission — send keystrokes to the PTY via the backend
  // In multi-question mode, DON'T dismiss — Devin/OpenCode auto-advances to next
  // question tab and the overlay should stay for the next question.
  const handleAgentAnswer = useCallback((option: string, index: number) => {
    if (!termRef.current || agentCli === "unknown") return;
    const ctx: BehaviorContext = {
      options: agentOptions,
      isMultiSelect: agentIsMultiSelect,
      isMultiQuestion: agentIsMultiQuestion,
      activeTabIndex: agentActiveTabIndex,
      totalTabs: agentTotalTabs,
      cursorPos: devinCursorPosRef.current,
      otherEditing: agentOtherEditing,
    };
    const result = getBehavior(agentCli).answer(option, index, ctx);
    executeSteps(result.steps, (bytes) => {
      logTerminalInput(sessionIdRef.current, bytes);
      sendToBackendRef.current(bytes);
    });
    if (result.dismiss) setAgentOverlayDismissed(true);
  }, [agentCli, agentOptions, agentIsMultiSelect, agentIsMultiQuestion, agentActiveTabIndex, agentTotalTabs, agentOtherEditing]);

  // Handle multi-select toggle — send toggle keystrokes but DON'T dismiss
  const handleAgentToggle = useCallback((option: string, index: number) => {
    if (!termRef.current || agentCli === "unknown") return;
    const ctx: BehaviorContext = {
      options: agentOptions,
      isMultiSelect: agentIsMultiSelect,
      isMultiQuestion: agentIsMultiQuestion,
      activeTabIndex: agentActiveTabIndex,
      totalTabs: agentTotalTabs,
      cursorPos: devinCursorPosRef.current,
      otherEditing: agentOtherEditing,
    };
    const result = getBehavior(agentCli).toggle(option, index, ctx);
    executeSteps(result.steps, (bytes) => {
      logTerminalInput(sessionIdRef.current, bytes);
      sendToBackendRef.current(bytes);
    });
    if (result.newCursorPos !== undefined) devinCursorPosRef.current = result.newCursorPos;
    // Don't dismiss — user may want to toggle more options
  }, [agentCli, agentOptions, agentIsMultiSelect, agentIsMultiQuestion, agentActiveTabIndex, agentTotalTabs, agentOtherEditing]);

  // Handle multi-select submit — send confirm keystrokes, then dismiss
  const handleAgentSubmitMultiSelect = useCallback(() => {
    if (!termRef.current || agentCli === "unknown") return;
    const ctx: BehaviorContext = {
      options: agentOptions,
      isMultiSelect: agentIsMultiSelect,
      isMultiQuestion: agentIsMultiQuestion,
      activeTabIndex: agentActiveTabIndex,
      totalTabs: agentTotalTabs,
      cursorPos: devinCursorPosRef.current,
      otherEditing: agentOtherEditing,
    };
    const result = getBehavior(agentCli).submitMultiSelect(ctx);
    executeSteps(result.steps, (bytes) => {
      logTerminalInput(sessionIdRef.current, bytes);
      sendToBackendRef.current(bytes);
    });
    setAgentOverlayDismissed(true);
  }, [agentCli, agentOptions, agentIsMultiSelect, agentIsMultiQuestion, agentActiveTabIndex, agentTotalTabs, agentOtherEditing]);

  // Handle text answer (Type your own answer) — navigate + type + submit
  // Split into two sends: first navigate to option + Enter (enter text mode),
  // then after 300ms delay, type the text + Enter (submit).
  // The delay is needed because the CLI needs time to redraw the text input
  // after Enter — if we send text immediately, it gets lost.
  const handleAgentTextAnswer = useCallback((option: string, text: string, index: number, hasExistingText?: boolean) => {
    if (!termRef.current || agentCli === "unknown") return;
    // For multi-select mode, devinCursorPosRef is unreliable (no ❭ marker).
    // Use getCursorOptionIndex to find the actual cursor position from the
    // terminal buffer, so arrow navigation in submitDevinTextAnswer computes
    // the correct distance.
    let cursorPos = devinCursorPosRef.current;
    if (agentIsMultiSelect && termRef.current) {
      const actualPos = getCursorOptionIndex(termRef.current);
      if (actualPos !== null) cursorPos = actualPos;
    }
    const ctx: BehaviorContext = {
      options: agentOptions,
      isMultiSelect: agentIsMultiSelect,
      isMultiQuestion: agentIsMultiQuestion,
      activeTabIndex: agentActiveTabIndex,
      totalTabs: agentTotalTabs,
      cursorPos,
      otherEditing: agentOtherEditing,
      hasExistingText,
    };
    const result = getBehavior(agentCli).textAnswer(option, text, index, ctx);
    executeSteps(result.steps, (bytes) => {
      logTerminalInput(sessionIdRef.current, bytes);
      sendToBackendRef.current(bytes);
    });
    if (result.newCursorPos !== undefined) devinCursorPosRef.current = result.newCursorPos;
    if (result.dismiss) setAgentOverlayDismissed(true);
  }, [agentCli, agentOptions, agentIsMultiSelect, agentIsMultiQuestion, agentActiveTabIndex, agentTotalTabs, agentOtherEditing]);

  // Handle text cancel — send Escape to exit CLI's text editing mode
  // (e.g. Devin's 'e' select+type mode). Without this, the CLI stays in
  // text editing mode and subsequent arrow keys move the text cursor
  // instead of switching questions.
  const handleAgentTextCancel = useCallback(() => {
    if (!termRef.current || agentCli === "unknown") return;
    const ctx: BehaviorContext = {
      options: agentOptions,
      isMultiSelect: agentIsMultiSelect,
      isMultiQuestion: agentIsMultiQuestion,
      activeTabIndex: agentActiveTabIndex,
      totalTabs: agentTotalTabs,
      cursorPos: devinCursorPosRef.current,
      otherEditing: agentOtherEditing,
    };
    const result = getBehavior(agentCli).textCancel(ctx);
    executeSteps(result.steps, (bytes) => {
      logTerminalInput(sessionIdRef.current, bytes);
      sendToBackendRef.current(bytes);
    });
  }, [agentCli, agentOptions, agentIsMultiSelect, agentIsMultiQuestion, agentActiveTabIndex, agentTotalTabs, agentOtherEditing]);

  // ── Closed-loop navigation for prev/next question ──────────────────────
  // Instead of blindly sending keystrokes and hoping for the best, this
  // function verifies each step's effect by scraping the screen, and
  // corrects the cursor position if it doesn't match the user's previous
  // selection.
  //
  // Steps:
  // 1. If in Other editing mode: send Up, poll until cursor leaves Other
  // 2. Send Left/Right, poll until question text changes (tab switched)
  // 3. Check cursorIndex vs answeredTabsRef; if mismatch, send Up/Down to correct
  // 4. Wait 0.5s, verify again (guard against server latency)
  const navigateWithVerify = useCallback(async (
    direction: "prev" | "next",
  ) => {
    const term = termRef.current;
    if (!term || agentCli === "unknown") return;

    const encoder = new TextEncoder();
    const send = (data: string) => {
      const bytes = encoder.encode(data);
      logTerminalInput(sessionIdRef.current, bytes);
      sendToBackendRef.current(bytes);
    };

    // Helper: scrape screen and extract current state
    const getScreenState = () => {
      const lines = scrapeScreen(term);
      const text = prepareScreenText(joinLines(lines));
      const question = extractQuestion(agentCli as CliType, text);
      const options = extractOptions(agentCli as CliType, text);
      const isMultiSel = detectMultiSelect(agentCli as CliType, text);
      const isMultiQ = agentIsMultiQuestion;
      const behavior = getBehavior(agentCli as CliType);
      // detectOtherExpanded checks for ❭ on Other line (single-select only).
      // For multi-select, use cursor position: if the cursor is on a line
      // starting with └ (the text input line under Other), we're in editing mode.
      let otherEditing = behavior.detectOtherExpanded(text, isMultiSel, isMultiQ);
      if (!otherEditing && isMultiSel) {
        const cursorLine = getCursorLine(term);
        if (cursorLine && /^\s*└/.test(cursorLine.text)) {
          otherEditing = true;
        }
      }
      const cursorIdx = !isMultiSel ? extractCursorIndex(agentCli as CliType, text) : null;
      return { question, options, cursorIdx, otherEditing };
    };

    // Helper: poll until condition is met or timeout
    const pollUntil = async <T,>(
      fn: () => T | null,
      isValid: (result: T) => boolean,
      timeoutMs = 3000,
      intervalMs = 100,
    ): Promise<T | null> => {
      const start = Date.now();
      while (Date.now() - start < timeoutMs) {
        await new Promise<void>((r) => setTimeout(r, intervalMs));
        const result = fn();
        if (result !== null && isValid(result)) return result;
      }
      return null;
    };

    // Step 1: If in Other editing mode, send Up and verify exit.
    // detectOtherExpanded via getCursorLine checks if cursor is on └ line
    // (multi-select) or ❭ on Other line (single-select).
    if (getScreenState().otherEditing) {
      send("\x1b[A");
      // Wait for editing mode to exit (otherEditing becomes false)
      await pollUntil(
        () => getScreenState().otherEditing,
        (otherEditing) => otherEditing === false,
        3000, 100,
      );
    }

    // Step 2: Send Left/Right and verify tab switch (question text changed).
    // If tab didn't switch, we may still be in editing mode (detection missed).
    // Send Up to exit editing, then retry Left/Right.
    const beforeQuestion = getScreenState().question;
    send(direction === "prev" ? "\x1b[D" : "\x1b[C");
    let switchedState = await pollUntil(
      () => getScreenState(),
      (state) => state.question !== null && state.question !== beforeQuestion,
      1000, 100,
    );
    if (!switchedState) {
      // Tab didn't switch — likely still in text editing mode. Send Up to exit,
      // then retry Left/Right.
      send("\x1b[A");
      await new Promise<void>((r) => setTimeout(r, 200));
      send(direction === "prev" ? "\x1b[D" : "\x1b[C");
      switchedState = await pollUntil(
        () => getScreenState(),
        (state) => state.question !== null && state.question !== beforeQuestion,
      );
    }
    if (!switchedState) return; // timeout — give up

    // Step 3: Check cursorIndex vs expected (user's previous selection)
    const targetTab = direction === "prev" ? agentActiveTabIndex - 1 : agentActiveTabIndex + 1;
    const expectedIdx = answeredTabsRef.current.get(targetTab);

    const correctCursor = async (): Promise<boolean> => {
      const state = getScreenState();
      if (expectedIdx === undefined || state.cursorIdx === null) return true;
      if (state.cursorIdx === expectedIdx) return true;

      const optionsCount = state.options ? state.options.length : 0;
      if (optionsCount === 0) return false;

      const currentIdx = state.cursorIdx;
      const diff = expectedIdx - currentIdx;
      if (diff > 0) {
        // Need to go Down
        for (let i = 0; i < diff; i++) {
          send("\x1b[B");
          const expected = currentIdx + i + 1;
          const result = await pollUntil(
            () => getScreenState().cursorIdx,
            (idx) => idx === expected,
            1000, 50,
          );
          if (!result) break;
        }
      } else if (diff < 0) {
        // Need to go Up
        for (let i = 0; i < -diff; i++) {
          send("\x1b[A");
          const expected = currentIdx - i - 1;
          const result = await pollUntil(
            () => getScreenState().cursorIdx,
            (idx) => idx === expected,
            1000, 50,
          );
          if (!result) break;
        }
      }
      // Verify final position
      const finalState = getScreenState();
      return finalState.cursorIdx === expectedIdx;
    };

    await correctCursor();

    // Step 4: Enable buttons immediately after initial verification succeeds.
    // Then wait 0.5s and verify again (guard against server latency).
    // If the second check finds a mismatch, re-disable buttons, correct, then
    // re-enable.
    // Use a flag to detect if user started a new navigation during the 0.5s
    // wait — if so, skip the recheck to avoid interfering with the new nav.
    setAgentNavigating(false);
    await new Promise<void>((r) => setTimeout(r, 500));
    // If user started a new navigation during the wait, skip recheck
    if (agentNavigatingRef.current) return;
    const stateBeforeRecheck = getScreenState();
    if (expectedIdx !== undefined && stateBeforeRecheck.cursorIdx !== null &&
        stateBeforeRecheck.cursorIdx !== expectedIdx) {
      // Mismatch detected — re-disable, correct, re-enable
      setAgentNavigating(true);
      await correctCursor();
      setAgentNavigating(false);
    }
  }, [agentCli, agentOptions, agentActiveTabIndex, agentIsMultiQuestion, answeredTabsRef]);

  // Handle navigate to previous question tab (multi-question mode)
  // Uses closed-loop navigation: sends keystrokes one at a time, verifies
  // each step's effect by scraping the screen, and corrects cursor position
  // if it doesn't match the user's previous selection.
  const handleAgentPrevQuestion = useCallback(() => {
    if (!termRef.current || agentCli === "unknown" || agentNavigating) return;
    setAgentNavigating(true);
    navigateWithVerify("prev").finally(() => {
      setAgentNavigating(false);
    });
  }, [agentCli, agentNavigating, navigateWithVerify]);

  // Handle navigate to next question tab (multi-question mode)
  const handleAgentNextQuestion = useCallback(() => {
    if (!termRef.current || agentCli === "unknown" || agentNavigating) return;
    setAgentNavigating(true);
    navigateWithVerify("next").finally(() => {
      setAgentNavigating(false);
    });
  }, [agentCli, agentNavigating, navigateWithVerify]);

  // Handle confirm in multi-question mode.
  // Navigates to the last tab (submit/confirm), then Enter to submit.
  const handleAgentConfirm = useCallback((hasAnswers = false) => {
    if (!termRef.current || agentCli === "unknown") return;
    const ctx: BehaviorContext = {
      options: agentOptions,
      isMultiSelect: agentIsMultiSelect,
      isMultiQuestion: agentIsMultiQuestion,
      activeTabIndex: agentActiveTabIndex,
      totalTabs: agentTotalTabs,
      cursorPos: devinCursorPosRef.current,
      otherEditing: agentOtherEditing,
    };
    const result = getBehavior(agentCli).confirm(hasAnswers, ctx);
    executeSteps(result.steps, (bytes) => {
      logTerminalInput(sessionIdRef.current, bytes);
      sendToBackendRef.current(bytes);
    });
    setAgentOverlayDismissed(true);
  }, [agentCli, agentOptions, agentIsMultiSelect, agentIsMultiQuestion, agentActiveTabIndex, agentTotalTabs, agentOtherEditing]);

  // Terminal appearance from config
  const config = useConfigStore((s) => s.config);
  const terminalTheme = config?.general.terminal_theme ?? "catppuccin-mocha";
  const terminalFontSize = config?.general.terminal_font_size ?? 13;
  const terminalFontFamily = config?.general.terminal_font_family ?? "'Menlo', 'Monaco', 'Courier New', monospace";

  const [zmodemProgress, setZmodemProgress] = useState<{
    active: boolean;
    isUpload: boolean;
    filename: string;
    received: number;
    total: number;
    percent: number;
  } | null>(null);
  const zmodemCancelRef = useRef<() => void>(() => {});
  const [showSearch, setShowSearch] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const searchInputRef = useRef<HTMLInputElement>(null);
  const [dragOver, setDragOver] = useState(false);
  const [showTmuxZmodemTip, setShowTmuxZmodemTip] = useState<null | "sz" | "rz">(null);
  const rzAvailableRef = useRef(rzAvailable);
  rzAvailableRef.current = rzAvailable;
  const tmuxSessionNameRef = useRef(tmuxSessionName);
  tmuxSessionNameRef.current = tmuxSessionName;
  const activeRef = useRef(active);
  activeRef.current = active;
  const sendToBackendRef = useRef<(bytes: Uint8Array | number[], waitForSend?: boolean) => Promise<void>>(() => Promise.resolve());

  useEffect(() => {
    if (!containerRef.current) return;

    const term = new Terminal({
      cursorBlink: true,
      fontSize: terminalFontSize,
      fontFamily: terminalFontFamily,
      theme: getTerminalTheme(terminalTheme).theme,
      allowProposedApi: true,
    });
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(containerRef.current);
    fitAddon.fit();

    // P0: WebGL2 is lazy-loaded only for the active tab (see useEffect [active]).
    // This prevents WebGL context exhaustion when many terminals are open.

    // P2: Unicode 11 for correct CJK/emoji wide character handling
    try {
      const unicode11Addon = new Unicode11Addon();
      term.loadAddon(unicode11Addon);
      term.unicode.activeVersion = "11";
    } catch { /* addon load failure is non-fatal */ }

    // P2: Clickable URLs in terminal output
    try {
      term.loadAddon(new WebLinksAddon());
    } catch { /* addon load failure is non-fatal */ }

    // P2: Search (Ctrl+F)
    const searchAddon = new SearchAddon();
    term.loadAddon(searchAddon);

    // P2: Serialize (for tab switch / reconnect buffer restore)
    const serializeAddon = new SerializeAddon();
    term.loadAddon(serializeAddon);

    // ── macOS WKWebView IME composition fix ───────────────────────────
    // On macOS Tauri (WKWebView), xterm.js's composition handling is broken:
    // 1. Chinese IME composition text may not be correctly sent to the PTY
    // 2. keyCode=229 events bypass xterm's normal key handling
    // 3. beforeinput events for punctuation/symbols are swallowed
    //
    // Fix: attach compositionend + beforeinput listeners to xterm's helper
    // textarea, forwarding composed text directly to the PTY via TextEncoder.
    // A compositionActive flag prevents double-send when xterm's own handler
    // also processes the event.
    const imeDisposers: (() => void)[] = [];
    const helperTextarea = term.textarea;
    if (helperTextarea) {
      const imeEncoder = new TextEncoder();
      let compositionActive = false;

      // Track composition state to avoid double-sending
      const onCompositionStart = () => { compositionActive = true; };
      const onCompositionUpdate = () => { compositionActive = true; };

      // compositionend: IME finished composing (user selected a candidate)
      // Forward the composed text directly to PTY, bypassing xterm's handler
      // which may drop or garble it on WKWebView.
      const onCompositionEnd = (e: CompositionEvent) => {
        compositionActive = false;
        if (e.data && e.data.length > 0) {
          const bytes = imeEncoder.encode(e.data);
          sendToBackendRef.current(bytes);
          // Clear textarea to prevent xterm's diff-based handler from re-sending
          helperTextarea.value = "";
        }
      };

      // beforeinput: catches punctuation/symbol input that WKWebView delivers
      // via this event instead of xterm's normal key path (macOS Chinese IME).
      // Only forward short symbol-only input (not regular text composition).
      const onBeforeInput = (e: InputEvent) => {
        if (e.isComposing || compositionActive) return;
        const data = e.data;
        if (!data || data.length === 0) return;
        // Only handle if it looks like IME-delivered symbol/punctuation
        // (not regular ASCII typing which xterm handles fine)
        if (data.length <= 4 && !/^[a-zA-Z0-9\s]+$/.test(data)) {
          const bytes = imeEncoder.encode(data);
          sendToBackendRef.current(bytes);
          e.preventDefault(); // prevent xterm's broken handler from also processing
        }
      };

      helperTextarea.addEventListener("compositionstart", onCompositionStart);
      helperTextarea.addEventListener("compositionupdate", onCompositionUpdate);
      helperTextarea.addEventListener("compositionend", onCompositionEnd);
      helperTextarea.addEventListener("beforeinput", onBeforeInput);
      imeDisposers.push(() => {
        helperTextarea.removeEventListener("compositionstart", onCompositionStart);
        helperTextarea.removeEventListener("compositionupdate", onCompositionUpdate);
        helperTextarea.removeEventListener("compositionend", onCompositionEnd);
        helperTextarea.removeEventListener("beforeinput", onBeforeInput);
      });
    }

    termRef.current = term;
    fitRef.current = fitAddon;
    searchRef.current = searchAddon;
    serializeRef.current = serializeAddon;
    // Expose terminal instance to useAgentStatus (registers OSC handlers)
    setAgentTerm(term);

    // Restore saved snapshot (from previous unmount, e.g. reconnect) if available.
    // Falls back to initialOutput (MOTD/prompt from backend) for new sessions.
    const savedSnapshot = snapshotCache.get(serverId);
    if (savedSnapshot) {
      term.write(savedSnapshot);
      snapshotCache.delete(serverId);
    } else if (initialOutput) {
      const bytes = base64ToBytes(initialOutput);
      term.write(bytes);
    }

    // Send initial resize to backend
    ipcInvoke("ipc_terminal_resize", {
      session_id: sessionIdRef.current,
      cols: term.cols,
      rows: term.rows,
    }).catch(() => {});

    // Helper: send raw bytes to backend (binary, no base64).
    // For ZMODEM uploads, wait_for_send=true provides backpressure by blocking
    // until SSH write is confirmed. For normal keystrokes, wait_for_send is
    // omitted (defaults to false) so typing stays responsive.
    const sendToBackend = (bytes: Uint8Array | number[], waitForSend = false): Promise<void> => {
      const arr = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
      return ipcInvoke("ipc_terminal_input", {
        session_id: sessionIdRef.current,
        data: arr,
        wait_for_send: waitForSend,
      }).then(() => {}).catch(() => {});
    };
    sendToBackendRef.current = sendToBackend;

    // --- trzsz File System Access API polyfill for Tauri WebView ---
    // trzsz.js in browser mode uses window.showDirectoryPicker() and
    // window.showOpenFilePicker() with FileSystemDirectoryHandle/FileHandle
    // to read/write files. Tauri WebView doesn't support these, so we
    // polyfill them using Tauri's file APIs.
    if (typeof (window as any).showDirectoryPicker === "undefined") {
      (window as any).showDirectoryPicker = async () => {
        const selected = await openDialog({ directory: true });
        if (!selected) throw { name: "AbortError" };
        const dirPath = Array.isArray(selected) ? selected[0] : selected;
        return makeDirHandle(dirPath);
      };
    }
    if (typeof (window as any).showOpenFilePicker === "undefined") {
      (window as any).showOpenFilePicker = async () => {
        const selected = await openDialog({ multiple: true });
        if (!selected) throw { name: "AbortError" };
        const paths = Array.isArray(selected) ? selected : [selected];
        return paths.map((p) => makeFileHandle(p));
      };
    }
    function makeFileHandle(filePath: string) {
      const name = filePath.split("/").pop() || filePath;
      return {
        kind: "file",
        name,
        _path: filePath,
        async getFile() {
          const data = await readFile(filePath);
          return new File([data], name);
        },
        async createWritable() {
          let written = false;
          return {
            async write(data: any) {
              const bytes = data instanceof Uint8Array ? data
                : data instanceof ArrayBuffer ? new Uint8Array(data)
                : new TextEncoder().encode(String(data));
              if (!written) { await writeFile(filePath, bytes, { create: true }); written = true; }
              else { await writeFile(filePath, bytes, { append: true }); }
            },
            async close() {},
            abort() {},
          };
        },
      };
    }
    function makeDirHandle(dirPath: string): any {
      return {
        kind: "directory",
        name: dirPath.split("/").pop() || dirPath,
        _path: dirPath,
        async getFileHandle(name: string) {
          return makeFileHandle(dirPath + "/" + name);
        },
        async getDirectoryHandle(name: string) {
          return makeDirHandle(dirPath + "/" + name);
        },
        async *values() { return; },
        async *keys() { return; },
      };
    }

    // --- trzsz filter (trz/tsz, tmux-compatible) ---
    // Intercepts trz/tsz protocol frames in the terminal stream.
    // Non-trzsz data is passed through to ZMODEM Sentry (which handles rz/sz).
    // trzsz works inside tmux, unlike ZMODEM (rz/sz).
    // Note: writeToTerminal is set after ZMODEM Sentry is created below,
    // so it can feed into the Sentry's consume pipeline.
    let trzszWriteToTerminal: (output: string | ArrayBuffer | Uint8Array | Blob) => void = (output) => {
      // Before ZMODEM Sentry is set up, write directly to terminal
      if (typeof output === "string") {
        term.write(output);
      } else if (output instanceof Uint8Array) {
        term.write(output);
      } else if (output instanceof ArrayBuffer) {
        term.write(new Uint8Array(output));
      }
      // Blob — not expected in Tauri, skip
    };
    const trzszFilter = new TrzszFilter({
      writeToTerminal: (output) => trzszWriteToTerminal(output),
      sendToServer: (input) => {
        if (typeof input === "string") {
          sendToBackend(new TextEncoder().encode(input));
        } else {
          sendToBackend(input);
        }
      },
      chooseSendFiles: async (directory) => {
        // If files were drag-dropped, use those paths instead of file picker
        if (pendingDropPaths && pendingDropPaths.length > 0) {
          const paths = pendingDropPaths;
          pendingDropPaths = null;
          return paths;
        }
        const selected = await openDialog({
          multiple: true,
          directory: directory ?? false,
        });
        if (!selected) return undefined;
        return Array.isArray(selected) ? selected : [selected];
      },
      chooseSaveDirectory: async () => {
        const selected = await openDialog({ directory: true });
        if (!selected) return undefined;
        return selected;
      },
      terminalColumns: term.cols,
    });

    // File paths from drag-drop (when rz is available, used instead of file picker)
    let pendingDropPaths: string[] | null = null;

    // --- ZMODEM Sentry ---
    // Intercepts ZMODEM frames in the terminal output stream. Non-ZMODEM
    // data is passed to the terminal; ZMODEM sessions trigger file transfer.
    let zmodemSession: ZmodemSession | null = null;
    // Cooldown timestamp: after a session ends, ignore new detections for
    // a few seconds to prevent spurious sessions from echoed ZMODEM bytes.
    let zmodemCooldownUntil = 0;
    // Ending flag: set true when the upload/download is wrapping up.
    // Blocks the sender callback so no ZSINIT keepalive or other protocol
    // bytes escape to the PTY after the session is done.
    let zmodemEnding = false;
    // Tracks the most recent sender promise (from the zmodem.js-ex sender
    // callback).  The upload loop awaits this before sending the next chunk,
    // creating backpressure so progress doesn't race ahead of actual SSH
    // transmission.
    let lastSenderPromise: Promise<void> | null = null;
    // Track the last active session type so to_terminal can suppress garbage
    // during Receive (download) sessions but still write trailing shell output.
    let lastZmodemSessionType: string | null = null;

    // Force-clear the Sentry's internal session state so it stops
    // feeding data to a dead session and stops creating spurious sessions
    // from echoed ZMODEM protocol bytes.
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const clearSentryState = () => {
      const s = zsentry as any;
      if (s._zsession) {
        try { s._zsession.abort(); } catch (_) { /* already aborted */ }
        s._zsession = null;
      }
      s._parsed_session = null;
      s._cache = [];
    };

    const clearZmodemProgress = () => {
      setZmodemProgress(null);
      zmodemCancelRef.current = () => {};
    };

    // sz: remote is SENDING files to us (session.type === "receive").
    // Stream each file chunk to a temp file as it arrives (so progress is
    // written to disk in real time) and then copy the temp file to the user
    // chosen path once the transfer completes.
    function handleSzDownload(session: ZmodemSession) {
      session.on("offer", async (xfer: ZmodemTransfer) => {
        const details = xfer.get_details();

        // Use a temporary file in the system temp directory. We stream each
        // received payload here while the user is still picking the save path.
        const safeName = details.name.replace(/[\\/]/g, "_");
        const tempName = `zmodem-${Date.now()}-${safeName}`;
        const writeQueue: Uint8Array[] = [];
        let fileHandle: FileHandle | null = null;
        let writing = false;
        let receivedBytes = 0;
        let lastProgressPercent = -1;
        let lastProgressReceived = 0;

        const writeQueueDrain = async () => {
          if (writing || !fileHandle) return;
          writing = true;
          while (writeQueue.length) {
            const data = writeQueue.shift()!;
            await fileHandle.write(data);
          }
          writing = false;
        };

        // Open the temp file asynchronously. The first payloads may arrive
        // before the file is open, so we queue them and drain once ready.
        open(tempName, {
          write: true,
          create: true,
          append: true,
          baseDir: BaseDirectory.Temp,
        })
          .then((fh) => {
            fileHandle = fh;
            writeQueueDrain();
          })
          .catch((e) => console.error("[ZMODEM] open temp file failed:", e));

        // Show progress UI immediately.
        setZmodemProgress({
          active: true,
          isUpload: false,
          filename: details.name,
          received: 0,
          total: details.size || 0,
          percent: 0,
        });

        // Cancel support for this download.
        let cancelled = false;
        let cancelReject: (e: Error) => void;
        const cancelPromise = new Promise<never>((_, reject) => {
          cancelReject = reject;
          zmodemCancelRef.current = () => {
            if (cancelled) return;
            cancelled = true;
            console.log("[ZMODEM] cancel download clicked");
            clearZmodemProgress();
            cleanupSession(session, true);
            reject(new Error("ZMODEM cancelled by user"));
          };
        });

        // accept() must be called synchronously to set up the ZDATA handler.
        // Pass an on_input callback so payloads are written to disk as they
        // arrive, instead of being spooled in memory until the end.
        const acceptPromise = (xfer as any).accept({
          on_input: (payload: number[]) => {
            receivedBytes += payload.length;
            writeQueue.push(new Uint8Array(payload));
            writeQueueDrain();

            const totalSize = details.size;
            if (totalSize && totalSize > 0) {
              // Cap at 99% while data is still streaming; the final 1% is shown
              // only after the file is fully written to the chosen destination.
              const percent = Math.min(99, Math.floor((receivedBytes / totalSize) * 100));
              if (percent > lastProgressPercent) {
                lastProgressPercent = percent;
                setZmodemProgress({
                  active: true,
                  isUpload: false,
                  filename: details.name,
                  received: receivedBytes,
                  total: totalSize,
                  percent,
                });
                if (percent % 5 === 0) {
                  console.log(`[ZMODEM] download progress: ${percent}% (${receivedBytes}/${totalSize})`);
                }
              }
            } else {
              // sz didn't send total file size — throttle UI updates to ~5% growth
              const shouldUpdate = lastProgressReceived === 0 || receivedBytes >= lastProgressReceived * 1.05;
              if (shouldUpdate) {
                lastProgressReceived = receivedBytes;
                setZmodemProgress({
                  active: true,
                  isUpload: false,
                  filename: details.name,
                  received: receivedBytes,
                  total: 0,
                  percent: 0,
                });
                console.log(`[ZMODEM] download progress: ${receivedBytes} bytes`);
              }
            }
          },
        });

        // Show the save dialog in parallel. The transfer is already writing
        // chunks to the temp file.
        let completed = false;
        try {
          const path = await Promise.race([
            saveDialog({ defaultPath: details.name }),
            cancelPromise,
          ]);
          if (!path) {
            console.log("[ZMODEM] save dialog cancelled, skipping file");
            xfer.skip();
            return;
          }

          await Promise.race([acceptPromise, cancelPromise]);
          await writeQueueDrain();
          await (fileHandle as any)?.close();
          fileHandle = null;

          await copyFile(tempName, path, { fromPathBaseDir: BaseDirectory.Temp });
          completed = true;
          console.log("[ZMODEM] file saved to:", path);

          // Show 100% only after the file is actually written to the destination.
          const totalSize = details.size || 0;
          setZmodemProgress({
            active: true,
            isUpload: false,
            filename: details.name,
            received: totalSize,
            total: totalSize,
            percent: 100,
          });
          setTimeout(() => clearZmodemProgress(), 1200);
        } catch (e: unknown) {
          if (cancelled) {
            console.log("[ZMODEM] download cancelled by user");
          } else {
            console.error("[ZMODEM] accept failed:", e);
            try { xfer.skip(); } catch (_) {}
          }
        } finally {
          try { await (fileHandle as any)?.close(); } catch (_) {}
          try { await remove(tempName, { baseDir: BaseDirectory.Temp }); } catch (_) {}
          if (!completed) clearZmodemProgress();
        }
      });
      session.on("session_end", () => {
        cleanupSession(session, false);
      });
      session.start().catch((e: unknown) => {
        console.error("[ZMODEM] session start failed:", e);
        cleanupSession(session);
      });
    }

    // Centralised cleanup for both upload and download sessions.
    const cleanupSession = (sess: ZmodemSession, clearProgress = true) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const sessAny = sess as any;
      sessAny._keepalive_stopped = true;
      if (sessAny._keepalive_timeout) {
        clearTimeout(sessAny._keepalive_timeout);
        sessAny._keepalive_timeout = null;
      }
      sessAny._keepalive_promise = null;
      sessAny._sender = function () { /* dead session */ };
      zmodemEnding = false;
      lastSenderPromise = null;
      zmodemSession = null;
      zmodemCooldownUntil = Date.now() + 10000;
      clearSentryState();
      if (clearProgress) clearZmodemProgress();
    };

    // rz: remote is RECEIVING files from us (session.type === "send").
    // If pendingDropPaths is set (from drag-drop), read those files directly.
    // Otherwise show a file picker and send selected files via ZMODEM.
    function handleRzUpload(session: ZmodemSession) {
      console.log("[ZMODEM] handleRzUpload called, pendingDropPaths:", pendingDropPaths);
      // If files were drag-dropped, use those paths instead of the file picker
      if (pendingDropPaths && pendingDropPaths.length > 0) {
        const dropPaths = pendingDropPaths;
        pendingDropPaths = null;
        console.log("[ZMODEM] using drag-drop paths:", dropPaths);
        (async () => {
          try {
            for (const filePath of dropPaths) {
              const fileName = filePath.split("/").pop() || filePath.split("\\").pop() || filePath;
              console.log("[ZMODEM] reading file:", filePath);
              const fileData = await readFile(filePath);
              console.log(`[ZMODEM] rz: sending drag-dropped file ${fileName} (${fileData.byteLength} bytes)`);
              const xfer = await session.send_offer({
                name: fileName,
                size: fileData.byteLength,
                mtime: new Date(),
                files_remaining: dropPaths.length - dropPaths.indexOf(filePath),
                bytes_remaining: 0,
              });
              console.log("[ZMODEM] rz: offer resolved, xfer=", !!xfer);
              if (!xfer) {
                console.log("[ZMODEM] rz: receiver skipped file");
                continue;
              }
              // The receiver may ask to resume from a non-zero offset (ZRPOS).
              const startOffset = (xfer as any).get_offset() as number;
              console.log(`[ZMODEM] rz: startOffset=${startOffset}, size=${fileData.byteLength}`);
              (session as any)._file_offset = startOffset;

              const total = Math.max(0, fileData.byteLength - startOffset);
              setZmodemProgress({
                active: true,
                isUpload: true,
                filename: fileName,
                received: 0,
                total,
                percent: 0,
              });
              let uploadCancelled = false;
              zmodemCancelRef.current = () => {
                if (uploadCancelled) return;
                uploadCancelled = true;
                try { (session as any).abort(); } catch (_) {}
                clearZmodemProgress();
              };
              const chunkSize = 8192;
              let lastUploadPercent = -1;
              for (let offset = startOffset; offset < fileData.byteLength; offset += chunkSize) {
                const chunk = fileData.subarray(offset, Math.min(offset + chunkSize, fileData.byteLength));
                xfer.send(chunk);
                if (lastSenderPromise) {
                  await lastSenderPromise;
                  lastSenderPromise = null;
                }
                const currentOffset = (xfer as any).get_offset() as number;
                const sent = currentOffset - startOffset;
                const percent = Math.min(99, Math.floor((sent / total) * 100));
                if (percent > lastUploadPercent) {
                  lastUploadPercent = percent;
                  setZmodemProgress({
                    active: true,
                    isUpload: true,
                    filename: fileName,
                    received: sent,
                    total,
                    percent,
                  });
                }
              }
              if (lastSenderPromise) {
                await lastSenderPromise;
                lastSenderPromise = null;
              }
              await xfer.end(new Uint8Array(0));
              setZmodemProgress({
                active: true,
                isUpload: true,
                filename: fileName,
                received: total,
                total,
                percent: 100,
              });
            }
            console.log("[ZMODEM] rz: closing session (drag-drop)");
            const closePromise = session.close();
            zmodemEnding = true;
            await closePromise;
            setTimeout(() => clearZmodemProgress(), 1200);
          } catch (e) {
            console.error("[ZMODEM] rz: drag-drop upload failed:", e);
            zmodemEnding = true;
            try { session.abort(); } catch (_) {}
            clearZmodemProgress();
          }
          cleanupSession(session);
        })();
        return;
      }

      const input = document.createElement("input");
      input.type = "file";
      input.multiple = true;
      input.style.display = "none";
      document.body.appendChild(input);

      input.onchange = async () => {
        document.body.removeChild(input);
        const files = input.files;
        if (!files || files.length === 0) {
          console.log("[ZMODEM] rz: no files selected, aborting");
          zmodemEnding = true;
          try { session.abort(); } catch (_) { /* ignore */ }
          cleanupSession(session);
          return;
        }
        console.log("[ZMODEM] rz: selected", files.length, "file(s)");
        try {
          for (let i = 0; i < files.length; i++) {
            const file = files[i];
            console.log(`[ZMODEM] rz: sending offer for ${file.name} (${file.size} bytes)`);
            const xfer = await session.send_offer({
              name: file.name,
              size: file.size,
              mtime: new Date(file.lastModified),
              files_remaining: files.length - i,
              bytes_remaining: 0,
            });
            console.log("[ZMODEM] rz: offer resolved, xfer=", !!xfer);
            if (!xfer) {
              console.log("[ZMODEM] rz: receiver skipped file");
              continue;
            }

            // The receiver may ask to resume from a non-zero offset (ZRPOS).
            // get_offset() returns the requested start; we must start reading
            // from there and sync the session's file offset so ZDATA/ZEOF
            // headers use the same offset.
            const startOffset = (xfer as any).get_offset() as number;
            console.log(`[ZMODEM] rz: startOffset=${startOffset}, file.size=${file.size}`);
            (session as any)._file_offset = startOffset;

            const chunkSize = 8192;
            let lastUploadPercent = -1;
            const total = Math.max(0, file.size - startOffset);
            setZmodemProgress({
              active: true,
              isUpload: true,
              filename: file.name,
              received: 0,
              total,
              percent: 0,
            });

            // Cancel support for this upload.
            let uploadCancelled = false;
            zmodemCancelRef.current = () => {
              if (uploadCancelled) return;
              uploadCancelled = true;
              console.log("[ZMODEM] cancel upload clicked");
              try { (session as any).abort(); } catch (_) {}
              clearZmodemProgress();
            };

            if (total <= 0) {
              // Remote already has the whole file (or more). Just close it.
              console.log("[ZMODEM] rz: remote already has file, ending immediately");
            } else {
              for (let offset = startOffset; offset < file.size; offset += chunkSize) {
                const slice = file.slice(offset, Math.min(offset + chunkSize, file.size));
                const buf = await slice.arrayBuffer();
                xfer.send(new Uint8Array(buf));

                // Wait for the previous chunk to be actually sent over SSH
                // before proceeding.  Without this, xfer.send() queues data
                // into the sender callback (which is fire-and-forget from
                // zmodem.js-ex's perspective), get_offset() races ahead, and
                // the progress bar shows 99% while the server has only
                // received a fraction of the file.
                if (lastSenderPromise) {
                  await lastSenderPromise;
                  lastSenderPromise = null;
                }

                const currentOffset = (xfer as any).get_offset() as number;
                const received = currentOffset - startOffset;
                // Cap at 99% while data is still streaming; the final 1% is
                // shown only after the peer has acknowledged the end of file.
                const percent = Math.min(99, Math.floor((received / total) * 100));
                if (percent > lastUploadPercent) {
                  lastUploadPercent = percent;
                  setZmodemProgress({
                    active: true,
                    isUpload: true,
                    filename: file.name,
                    received,
                    total,
                    percent,
                  });
                }
              }
              // Wait for the final chunk to be sent before ending the file.
              if (lastSenderPromise) {
                await lastSenderPromise;
                lastSenderPromise = null;
              }
            }
            console.log(`[ZMODEM] rz: sent ${(xfer as any).get_offset()} bytes, ending file`);
            await xfer.end(new Uint8Array(0));
            console.log("[ZMODEM] rz: file end confirmed");

            // Show 100% after the receiver has acknowledged the end of file.
            setZmodemProgress({
              active: true,
              isUpload: true,
              filename: file.name,
              received: total,
              total,
              percent: 100,
            });
          }
          // All files sent — wind down the session.
          console.log("[ZMODEM] rz: closing session");
          // close() sends ZFIN synchronously, then returns a promise that
          // resolves when the peer's ZFIN arrives. We capture the promise
          // first so ZFIN is sent while zmodemEnding is still false.
          const closePromise = session.close();
          // Now block all further sends (keepalive ZSINIT, etc.) while we
          // wait for the peer's ZFIN response.
          zmodemEnding = true;
          // Race with a timeout so we never hang forever if the peer
          // never responds.
          await Promise.race([
            closePromise,
            new Promise<never>((_, reject) =>
              setTimeout(() => reject(new Error("ZMODEM close timeout")), 10000),
            ),
          ]).catch((e) => {
            console.warn("[ZMODEM] rz: close failed/timed out:", e);
            try { session.abort(); } catch (_) { /* ignore */ }
          });
          console.log("[ZMODEM] rz: session closed, clearing state");
          cleanupSession(session);
        } catch (e) {
          console.error("[ZMODEM] rz: upload failed:", e);
          zmodemEnding = true;
          try { session.abort(); } catch (_) { /* ignore */ }
          cleanupSession(session);
        }
      };
      input.click();
    }

    const zsentry = new ZmodemSentry({
      // to_terminal: only write during active ZMODEM session.
      // When no session is active, the binary output callback writes
      // raw data directly (avoiding double-write). During a session,
      // to_terminal handles non-ZMODEM "garbage" data (e.g. trailing
      // shell output after transfer), filtering out ZMODEM protocol bytes.
      to_terminal: (octets: number[]) => {
        if (octets.length === 0) return;
        if (zmodemSession) {
          // During downloads (receive), the Sentry may emit file data as "garbage"
          // if the parser loses sync. Never write that to the terminal.
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          if ((zmodemSession as any).type === "receive") return;
          // Send (upload) sessions: filter out ZMODEM protocol bytes.
          if (octets.length >= 2) {
            const b0 = octets[0], b1 = octets[1];
            if (b0 === 0x2a && b1 === 0x2a) return; // hex header **
            if (b0 === 0x18) return;                 // ZDLE
            if (b0 === 0x2a && b1 === 0x18) return;  // ZPAD ZDLE
          }
          term.write(octetsToString(octets));
          return;
        }
        // No active session. If a session just ended, to_terminal is called
        // with trailing shell output. For receive (download), write it; for
        // send (upload), the remote is already back at shell, so suppress it.
        if (Date.now() < zmodemCooldownUntil && lastZmodemSessionType === "receive") {
          const clean = stripLeadingZmodemHeaders(octets);
          if (clean.length) term.write(octetsToString(clean));
        }
      },
      sender: (octets: number[]) => {
        // Block all sends when no session is active, during cooldown, or
        // when the session is ending. This prevents keepalive ZSINIT packets
        // from being sent after the session has ended.
        if (!zmodemSession || zmodemEnding || Date.now() < zmodemCooldownUntil) {
          console.log("[ZMODEM] sender BLOCKED:", "session=", !!zmodemSession, "ending=", zmodemEnding, "cooldown=", Date.now() < zmodemCooldownUntil, "len=", octets.length);
          return;
        }
        // Store the promise so the upload loop can await it for backpressure.
        // zmodem.js-ex calls sender synchronously, so we can't await here,
        // but the upload loop awaits lastSenderPromise before sending the
        // next chunk.
        lastSenderPromise = sendToBackend(octets, true);
      },
      on_detect: (detection: ZmodemDetection) => {
        console.log("[ZMODEM] on_detect triggered");
        // Cooldown: after a session ends, ignore spurious detections from
        // echoed ZMODEM protocol bytes that are still in the PTY buffer.
        if (Date.now() < zmodemCooldownUntil) {
          console.log("[ZMODEM] detection during cooldown, denying");
          try { detection.deny(); } catch (_) { /* ignore */ }
          return;
        }
        const session = detection.confirm();
        // The original library has a broken keepalive: the .then() callback
        // unconditionally sends ZSINIT, restarts the timer, and overwrites
        // _next_header_handler (which races with the ZFIN handler and prevents
        // close() from resolving). The keepalive is only needed to keep lrzsz
        // alive while the user is picking a file; we disable the keepalive
        // timer entirely but keep _send_ZSINIT working — it's needed by
        // _ensure_receiver_escapes_ctrl_chars() during send_offer().
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const sessAny = session as any;
        sessAny._start_keepalive = function () { /* disabled */ };
        sessAny._stop_keepalive = function () { /* disabled */ };
        // Stop any keepalive that was already scheduled by the original
        // _start_keepalive called during set_sender.
        sessAny._keepalive_stopped = true;
        if (sessAny._keepalive_timeout) {
          clearTimeout(sessAny._keepalive_timeout);
          sessAny._keepalive_timeout = null;
        }
        sessAny._keepalive_promise = null;
        console.log("[ZMODEM] session created, type=", session.type, "keepalive disabled");
        zmodemEnding = false;
        zmodemSession = session;
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        lastZmodemSessionType = (session as any).type;
        // session.type === "receive" → we are receiving (remote ran sz) → download
        // session.type === "send"    → we are sending (remote ran rz) → upload
        if (session.type === "receive") {
          handleSzDownload(session);
        } else {
          handleRzUpload(session);
        }
      },
      on_retract: () => {},
    });

    // Connect trzszWriteToTerminal to the ZMODEM Sentry pipeline.
    // trzsz's writeToTerminal output (non-trzsz data) feeds into the ZMODEM
    // Sentry for rz/sz detection. This way both trzsz and ZMODEM work.
    trzszWriteToTerminal = (output) => {
      let rawBytes: Uint8Array;
      if (typeof output === "string") {
        rawBytes = new TextEncoder().encode(output);
      } else if (output instanceof ArrayBuffer) {
        rawBytes = new Uint8Array(output);
      } else if (output instanceof Uint8Array) {
        rawBytes = output;
      } else {
        // Blob — not expected in Tauri, skip
        return;
      }
      if (rawBytes.length === 0) return;

      const hadSession = !!zmodemSession;
      const inCooldown = Date.now() < zmodemCooldownUntil;

      if (inCooldown && !zmodemSession) {
        const clean = stripLeadingZmodemHeaders(Array.from(rawBytes));
        const cleanBytes = clean.length ? new Uint8Array(clean) : new Uint8Array(0);
        pendingChunks.push(cleanBytes);
        if (!flushScheduled) {
          flushScheduled = true;
          requestAnimationFrame(flushPending);
        }
        return;
      }

      try {
        zsentry.consume(rawBytes);
      } catch (e) {
        console.error("[ZMODEM] sentry consume error:", e);
      }
      if (!hadSession && !zmodemSession) {
        pendingChunks.push(rawBytes);
        if (!flushScheduled) {
          flushScheduled = true;
          requestAnimationFrame(flushPending);
        }
      }
      if (hadSession && !zmodemSession) {
        zmodemCooldownUntil = Date.now() + 10000;
        clearSentryState();
      }
    };

    // Ctrl+Shift+F → search in terminal (avoids Cmd+F intercepted by macOS WebKit)
    term.attachCustomKeyEventHandler((e) => {
      if (e.type === "keydown" && e.ctrlKey && e.shiftKey && (e.key === "f" || e.key === "F" || e.code === "KeyF")) {
        setShowSearch(true);
        return false;
      }
      return true;
    });

    // User input → backend (UTF-8 encoded for binary safety)
    // Use TextEncoder for correct multi-byte UTF-8 (Chinese, Japanese, Korean, emoji)
    const utf8Encoder = new TextEncoder();
    // Track current input line for sz/rz detection in tmux
    let inputLineBuffer = "";
    const inputDisposable = term.onData((data) => {
      // Detect Enter key (\r) — check if current line is sz/rz in tmux
      if (data === "\r" && tmuxSessionNameRef.current) {
        const trimmed = inputLineBuffer.trim();
        // Match "sz" or "rz" optionally followed by arguments
        const cmdMatch = trimmed.match(/^(sz|rz)\b/);
        if (cmdMatch) {
          const cmd = cmdMatch[1] as "sz" | "rz";
          setShowTmuxZmodemTip(cmd);
          inputLineBuffer = "";
          return; // Block the Enter key
        }
        inputLineBuffer = "";
      } else if (data === "\r") {
        inputLineBuffer = "";
      } else if (data === "\x7f" || data === "\b") {
        // Backspace
        inputLineBuffer = inputLineBuffer.slice(0, -1);
      } else if (data === "\x03") {
        // Ctrl+C — clear line
        inputLineBuffer = "";
      } else if (data.length === 1 && data.charCodeAt(0) >= 32) {
        // Regular printable char
        inputLineBuffer += data;
      } else if (data.length > 1) {
        // Paste or multi-char input — append (may contain command)
        inputLineBuffer += data;
      }
      logTerminalInput(sessionIdRef.current, utf8Encoder.encode(data));
      // Pass through trzsz filter (handles trz/tsz input interception)
      trzszFilter.processTerminalInput(data);
    });
    // Binary input (e.g. paste of non-text data) — also through trzsz filter
    const binaryInputDisposable = term.onBinary((data) => {
      trzszFilter.processBinaryInput(data);
    });

    // Resize → backend
    const resizeDisposable = term.onResize(({ cols, rows }) => {
      ipcInvoke("ipc_terminal_resize", {
        session_id: sessionIdRef.current,
        cols,
        rows,
      }).catch(() => {});
      // Update trzsz filter's terminal columns for progress bar rendering
      trzszFilter.setTerminalColumns(cols);
    });

    // Listen for terminal output events from backend (base64-encoded)
    // When no ZMODEM session is active: feed to Sentry AND write raw data.
    // When a session IS active: only feed to Sentry; to_terminal handles output.
    // During cooldown: skip Sentry entirely, write raw data directly.
    //
    // Normal shell output is batched via requestAnimationFrame to avoid
    // flooding the main thread with high-frequency term.write() calls when
    // the server outputs large amounts of data (e.g. cat a huge file).
    // ZMODEM data is NOT batched — it needs immediate processing.
    let pendingChunks: Uint8Array[] = [];
    let flushScheduled = false;

    const flushPending = () => {
      flushScheduled = false;
      if (pendingChunks.length === 0) return;
      if (pendingChunks.length === 1) {
        term.write(pendingChunks[0]);
      } else {
        // Merge all pending chunks into one write
        const total = pendingChunks.reduce((s, c) => s + c.length, 0);
        const merged = new Uint8Array(total);
        let offset = 0;
        for (const chunk of pendingChunks) {
          merged.set(chunk, offset);
          offset += chunk.length;
        }
        term.write(merged);
      }
      pendingChunks = [];
      // Notify agent state machine that PTY output was received (drives "working" inference)
      notifyAgentOutput(sessionIdRef.current);
    };

    // Binary terminal output: register a callback for raw bytes from the Channel.
    // The Channel is created in ServerDetail.tsx and dispatches via dispatchTerminalOutput.
    const handleBinaryOutput = (rawBytes: Uint8Array, _isStderr: boolean) => {
      // Log raw PTY output to disk (for debugging AI CLI status detection)
      logTerminalOutput(sessionIdRef.current, rawBytes);

      // First pass through trzsz filter (handles trz/tsz, tmux-compatible).
      // Non-trzsz data is forwarded to trzszWriteToTerminal, which feeds
      // into the ZMODEM Sentry pipeline (handles rz/sz).
      trzszFilter.processServerOutput(rawBytes);
    };
    registerTerminalOutput(sessionId, handleBinaryOutput);

    // If initialOutput is empty (e.g. terminal was opened by a remote desktop
    // while user was on a different page), fetch history from the backend to
    // recover the output that was streamed before this TerminalView mounted.
    if (!initialOutput && sessionId) {
      ipcInvoke("ipc_terminal_get_history", { session_id: sessionId })
        .then((history: unknown) => {
          if (history instanceof ArrayBuffer) {
            const bytes = new Uint8Array(history);
            if (bytes.length > 0) {
              term.write(bytes);
            }
          } else if (history instanceof Uint8Array) {
            if (history.length > 0) {
              term.write(history);
            }
          } else if (Array.isArray(history)) {
            const bytes = new Uint8Array(history as number[]);
            if (bytes.length > 0) {
              term.write(bytes);
            }
          }
        })
        .catch(() => {
          // History fetch failed (session may not exist) — silently ignore
        });
    }

    // Initialize terminal I/O log (writes to AppLocalData/termfast-logs/)
    // Only when developer terminal logging is enabled in config
    const config = useConfigStore.getState().config;
    if (config?.general?.dev_terminal_log) {
      initTerminalLog(sessionId).catch(() => {});
    }

    // Listen for terminal closed event
    let unlistenClosed: UnlistenFn | undefined;
    listen<{ sessionId: string }>("terminal:closed", (event) => {
      if (event.payload.sessionId === sessionIdRef.current) {
        term.write("\r\n[Connection closed]\r\n");
        // Reset agent status — the AI CLI process has exited, so the tab
        // should stop showing the working spinner / blocked indicator.
        resetAgentStatus(sessionIdRef.current);
      }
    }).then((fn) => { unlistenClosed = fn; });

    // Listen for file drag-drop events from Tauri
    // Use a cancelled flag to handle the async listen() Promise race:
    // in React StrictMode, cleanup may run before the Promise resolves,
    // leaving the unlistenFn as undefined and leaking the listener.
    let dragCancelled = false;
    let unlistenDragEnter: UnlistenFn | undefined;
    let unlistenDragLeave: UnlistenFn | undefined;
    let unlistenFileDrop: UnlistenFn | undefined;
    listen<string[]>("file-drag-enter", () => {
      if (dragCancelled) return;
      if (activeRef.current && rzAvailableRef.current) setDragOver(true);
    }).then((fn) => {
      if (dragCancelled) { fn(); } else { unlistenDragEnter = fn; }
    });
    listen("file-drag-leave", () => {
      if (dragCancelled) return;
      setDragOver(false);
    }).then((fn) => {
      if (dragCancelled) { fn(); } else { unlistenDragLeave = fn; }
    });
    listen<string[]>("file-drop", (event) => {
      if (dragCancelled) return;
      setDragOver(false);
      if (!activeRef.current) return;
      const paths = event.payload;
      if (!paths || paths.length === 0) return;
      const encoder = new TextEncoder();
      if (rzAvailableRef.current) {
        // rz available: type "rz" command to trigger ZMODEM upload
        const bytes = encoder.encode("rz\n");
        sendToBackendRef.current(bytes);
        // ZMODEM sentry will detect the rz session and call handleRzUpload,
        // but we need to feed it the dropped file paths. Store them so
        // handleRzUpload can use them instead of the file picker.
        pendingDropPaths = paths;
      } else {
        // Try trzsz upload (works with tmux). Type "trz" command, then
        // pass file paths to trzszFilter.uploadFiles when it prompts.
        const bytes = encoder.encode("trz\n");
        sendToBackendRef.current(bytes);
        // Store paths for trzsz chooseSendFiles callback
        pendingDropPaths = paths;
      }
    }).then((fn) => {
      if (dragCancelled) { fn(); } else { unlistenFileDrop = fn; }
    });

    // Window resize handler — skip when container is hidden (0 dimensions)
    const handleResize = () => {
      if (!containerRef.current) return;
      const rect = containerRef.current.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) return;
      try { fitAddon.fit(); } catch { /* container not visible */ }
    };
    window.addEventListener("resize", handleResize);
    const resizeObserver = new ResizeObserver(() => handleResize());
    resizeObserver.observe(containerRef.current);

    term.focus();

    return () => {
      // Flush any pending chunks before disposing
      flushPending();
      // Flush + close terminal I/O log
      flushTerminalLog(sessionIdRef.current);
      closeTerminalLog(sessionIdRef.current).catch(() => {});
      // Save terminal snapshot for reconnect history restore
      if (serializeRef.current) {
        try {
          const snapshot = serializeRef.current.serialize();
          if (snapshot && snapshot.length > 0) {
            snapshotCache.set(serverId, snapshot);
          }
        } catch { /* serialize failure is non-fatal */ }
      }
      // Dispose WebGL addon explicitly to release GPU context
      if (webglRef.current) {
        try { webglRef.current.dispose(); } catch { /* already disposed */ }
        webglRef.current = null;
        activeWebglCount--;
      }
      inputDisposable.dispose();
      binaryInputDisposable.dispose();
      resizeDisposable.dispose();
      // Dispose IME composition fix listeners
      for (const d of imeDisposers) { try { d(); } catch { /* ignore */ } }
      unregisterTerminalOutput(sessionId);
      if (unlistenClosed) unlistenClosed();
      dragCancelled = true;
      if (unlistenDragEnter) unlistenDragEnter();
      if (unlistenDragLeave) unlistenDragLeave();
      if (unlistenFileDrop) unlistenFileDrop();
      window.removeEventListener("resize", handleResize);
      resizeObserver.disconnect();
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
      searchRef.current = null;
      serializeRef.current = null;
      webglRef.current = null;
      // Clear agent status monitoring
      setAgentTerm(null);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);

  // Live-update terminal appearance when config changes (no remount needed)
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.fontSize = terminalFontSize;
    term.options.fontFamily = terminalFontFamily;
    term.options.theme = getTerminalTheme(terminalTheme).theme;
    // Re-fit in case font size changed the cell dimensions
    fitRef.current?.fit();
  }, [terminalTheme, terminalFontSize, terminalFontFamily]);

  // Re-fit, focus, and manage WebGL when tab becomes active/inactive
  useEffect(() => {
    if (!termRef.current || !fitRef.current || !containerRef.current) return;

    if (active) {
      // Tab became active: lazy-load WebGL renderer for acceleration
      // Only create if under the global context limit
      if (!webglRef.current && activeWebglCount < MAX_WEBGL_CONTEXTS) {
        try {
          const webglAddon = new WebglAddon();
          webglAddon.onContextLoss(() => {
            webglAddon.dispose();
            if (webglRef.current === webglAddon) {
              webglRef.current = null;
              activeWebglCount--;
            }
          });
          termRef.current.loadAddon(webglAddon);
          webglRef.current = webglAddon;
          activeWebglCount++;
        } catch { /* WebGL not available — DOM renderer is fine */ }
      }
      // Delay fit() to next frame so the container has correct dimensions
      requestAnimationFrame(() => {
        if (!containerRef.current || !termRef.current || !fitRef.current) return;
        const rect = containerRef.current.getBoundingClientRect();
        if (rect.width === 0 || rect.height === 0) return;
        try {
          fitRef.current.fit();
          if (termRef.current.cols === 0 || termRef.current.rows === 0) return;
          ipcInvoke("ipc_terminal_resize", {
            session_id: sessionIdRef.current,
            cols: termRef.current.cols,
            rows: termRef.current.rows,
          }).catch(() => {});
          termRef.current.focus();
        } catch { /* ignore */ }
      });
    } else {
      // Tab became inactive: dispose WebGL to free GPU context
      if (webglRef.current) {
        try { webglRef.current.dispose(); } catch { /* already disposed */ }
        webglRef.current = null;
        activeWebglCount--;
      }
    }

    // Cleanup: dispose WebGL when this effect re-runs or component unmounts
    return () => {
      if (webglRef.current) {
        try { webglRef.current.dispose(); } catch { /* already disposed */ }
        webglRef.current = null;
        activeWebglCount--;
      }
    };
  }, [active]);

  // Focus search input when shown
  useEffect(() => {
    if (showSearch && searchInputRef.current) {
      searchInputRef.current.focus();
      searchInputRef.current.select();
    }
  }, [showSearch]);

  // Run search when query changes
  useEffect(() => {
    if (!showSearch || !searchRef.current) return;
    if (searchQuery.length > 0) {
      searchRef.current.findNext(searchQuery, { caseSensitive: false, wholeWord: false });
    }
  }, [searchQuery, showSearch]);

  const handleSearchKey = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      setShowSearch(false);
      setSearchQuery("");
      termRef.current?.focus();
    } else if (e.key === "Enter") {
      if (searchRef.current && searchQuery.length > 0) {
        if (e.shiftKey) {
          searchRef.current.findPrevious(searchQuery, { caseSensitive: false, wholeWord: false });
        } else {
          searchRef.current.findNext(searchQuery, { caseSensitive: false, wholeWord: false });
        }
      }
    }
  };

  const formatBytes = (bytes: number) => {
    if (bytes === 0) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB", "TB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return `${parseFloat((bytes / Math.pow(k, i)).toFixed(2))} ${sizes[i]}`;
  };

  return (
    <div className="relative w-full h-full bg-[#1e1e2e] overflow-hidden">
      <div
        ref={containerRef}
        className="w-full h-full"
      />
      <AgentQuestionOverlay
        visible={!agentOverlayDismissed}
        status={agentStatus}
        cli={agentCli}
        question={agentQuestion}
        options={agentOptions}
        isMultiSelect={agentIsMultiSelect}
        isMultiQuestion={agentIsMultiQuestion}
        activeTabIndex={agentActiveTabIndex}
        totalTabs={agentTotalTabs}
        reviewAnswers={agentReviewAnswers}
        blockedMessage={agentBlockedMessage}
        cursorIndex={agentCursorIndex}
        onAnswer={handleAgentAnswer}
        onToggle={handleAgentToggle}
        onSubmitMultiSelect={handleAgentSubmitMultiSelect}
        onTextAnswer={handleAgentTextAnswer}
        onTextCancel={handleAgentTextCancel}
        onPrevQuestion={handleAgentPrevQuestion}
        onNextQuestion={handleAgentNextQuestion}
        onConfirm={handleAgentConfirm}
        onDismiss={() => setAgentOverlayDismissed(true)}
        navigating={agentNavigating}
        answeredTabsRef={answeredTabsRef}
      />
      {showSearch && (
        <div className="absolute top-2 right-2 z-50 bg-slate-800/95 border border-slate-600 rounded-lg shadow-lg flex items-center gap-1 px-2 py-1.5">
          <input
            ref={searchInputRef}
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            onKeyDown={handleSearchKey}
            placeholder="搜索..."
            className="bg-transparent text-white text-sm outline-none w-40 placeholder-slate-500"
          />
          <kbd className="text-xs text-slate-500 px-1">↵下一个 ⇧↵上一个</kbd>
          <button
            type="button"
            onClick={() => { setShowSearch(false); setSearchQuery(""); termRef.current?.focus(); }}
            className="text-slate-400 hover:text-white px-1 text-sm"
          >
            ✕
          </button>
        </div>
      )}
      {zmodemProgress && zmodemProgress.active && (
        <div className="absolute top-4 left-4 right-4 z-50 bg-slate-800/95 border border-slate-600 text-white p-3 rounded-lg shadow-lg">
          <div className="flex justify-between text-sm mb-2">
            <span className="truncate pr-4">
              {zmodemProgress.isUpload ? "上传" : "下载"}: {zmodemProgress.filename}
            </span>
            <span className="shrink-0 font-mono">
              {zmodemProgress.percent}%
            </span>
          </div>
          <div className="w-full bg-slate-700 h-2 rounded-full overflow-hidden">
            <div
              className="bg-blue-500 h-2 rounded-full transition-all duration-100 ease-out"
              style={{ width: `${zmodemProgress.percent}%` }}
            />
          </div>
          <div className="flex justify-between items-center text-xs text-slate-400 mt-2 font-mono">
            <span>
              {formatBytes(zmodemProgress.received)} / {formatBytes(zmodemProgress.total)}
            </span>
            <button
              type="button"
              onClick={() => zmodemCancelRef.current()}
              className="ml-2 px-2 py-1 bg-red-600 hover:bg-red-500 text-white rounded text-xs"
            >
              取消
            </button>
          </div>
        </div>
      )}
      {showTmuxZmodemTip && (
        <div className="absolute top-4 left-4 right-4 z-50 bg-slate-800/95 border border-amber-500/50 text-white p-4 rounded-lg shadow-lg">
          <div className="flex items-start gap-3">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#f59e0b" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" className="shrink-0 mt-0.5">
              <path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/>
              <line x1="12" y1="9" x2="12" y2="13"/>
              <line x1="12" y1="17" x2="12.01" y2="17"/>
            </svg>
            <div className="flex-1 min-w-0">
              <p className="text-sm font-medium text-amber-400 mb-1">
                {showTmuxZmodemTip === "sz" ? "sz" : "rz"} 在 tmux 中不可用
              </p>
              <p className="text-xs text-slate-300 leading-relaxed">
                ZModem 协议（rz/sz）与 tmux 不兼容，tmux 会拦截二进制控制序列导致传输失败。
                推荐使用 <span className="text-blue-400 font-mono">trzsz</span>（trz/tsz）替代 — 它是 lrzsz 的现代替代品，完全兼容 tmux，还支持目录传输和断点续传。
              </p>
              <div className="mt-2 bg-slate-900/80 rounded px-3 py-2 font-mono text-xs text-slate-400">
                <div className="text-slate-500"># 在服务器上安装 trzsz：</div>
                <div className="text-green-400">pip3 install trzsz</div>
                <div className="text-slate-500 mt-1"># 上传文件（替代 rz）：</div>
                <div className="text-green-400">trz</div>
                <div className="text-slate-500 mt-1"># 下载文件（替代 sz）：</div>
                <div className="text-green-400">tsz &lt;文件名&gt;</div>
              </div>
            </div>
            <button
              type="button"
              onClick={() => setShowTmuxZmodemTip(null)}
              className="shrink-0 text-slate-400 hover:text-white text-lg leading-none"
            >
              ✕
            </button>
          </div>
        </div>
      )}
      {dragOver && (
        <div className="absolute top-4 left-4 right-4 z-40 bg-slate-800/95 border-2 border-dashed border-blue-500 text-white p-3 rounded-lg shadow-lg pointer-events-none">
          <div className="flex items-center justify-center gap-2 text-sm text-blue-400">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
              <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
              <polyline points="17 8 12 3 7 8" />
              <line x1="12" y1="3" x2="12" y2="15" />
            </svg>
            <span className="font-medium">拖入此处通过 rz 上传文件</span>
          </div>
        </div>
      )}
    </div>
  );
}
