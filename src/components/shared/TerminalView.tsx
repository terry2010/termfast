// TerminalView — xterm.js wrapper for interactive SSH terminal
// Connects to a backend PTY session via IPC
// Supports ZMODEM (rz/sz) file transfers via zmodem.js-ex

import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { writeFile } from "@tauri-apps/plugin-fs";
import { ipcInvoke } from "@/hooks/useIpc";
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
}

// Encode bytes to base64
function bytesToBase64(bytes: Uint8Array | number[]): string {
  const arr = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  let binary = "";
  for (let i = 0; i < arr.length; i++) {
    binary += String.fromCharCode(arr[i]);
  }
  return btoa(binary);
}

// Decode base64 to Uint8Array
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

// Save received file payloads to disk via Tauri save dialog
async function saveToDisk(payloads: Uint8Array[], filename: string) {
  console.log("[ZMODEM] saveToDisk called, payloads:", payloads.length, "filename:", filename);
  const path = await saveDialog({ defaultPath: filename });
  if (!path) {
    console.log("[ZMODEM] save dialog cancelled");
    return;
  }
  console.log("[ZMODEM] save path:", path);
  // Concatenate payloads into a single Uint8Array
  const totalLen = payloads.reduce((s, p) => s + p.length, 0);
  console.log("[ZMODEM] total bytes to write:", totalLen);
  if (totalLen === 0) {
    console.error("[ZMODEM] no data received!");
    return;
  }
  const buf = new Uint8Array(totalLen);
  let off = 0;
  for (const p of payloads) { buf.set(p, off); off += p.length; }
  try {
    await writeFile(path, buf);
    console.log("[ZMODEM] file written successfully:", path);
  } catch (e) {
    console.error("[ZMODEM] writeFile failed:", e);
  }
}

export function TerminalView({ sessionId, serverId, active, initialOutput }: TerminalViewProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const sessionIdRef = useRef(sessionId);
  sessionIdRef.current = sessionId;

  useEffect(() => {
    if (!containerRef.current) return;

    const term = new Terminal({
      cursorBlink: true,
      fontSize: 13,
      fontFamily: "'Menlo', 'Monaco', 'Courier New', monospace",
      theme: {
        background: "#1e1e2e",
        foreground: "#cdd6f4",
        cursor: "#f5e0dc",
      },
      allowProposedApi: true,
    });
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(containerRef.current);
    fitAddon.fit();

    termRef.current = term;
    fitRef.current = fitAddon;

    // Write initial output (MOTD/prompt) captured at terminal open time.
    // The backend sends this as base64 to preserve binary data.
    if (initialOutput) {
      const bytes = base64ToBytes(initialOutput);
      term.write(bytes);
    }

    // Send initial resize to backend
    ipcInvoke("ipc_terminal_resize", {
      session_id: sessionIdRef.current,
      cols: term.cols,
      rows: term.rows,
    }).catch(() => {});

    // Helper: send raw bytes to backend (base64-encoded for binary safety)
    const sendToBackend = (bytes: Uint8Array | number[]) => {
      ipcInvoke("ipc_terminal_input", {
        session_id: sessionIdRef.current,
        data: bytesToBase64(bytes),
      }).catch(() => {});
    };

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

    // sz: remote is SENDING files to us (session.type === "receive").
    // Accept each file offer and save to disk via Tauri save dialog.
    // accept() must be called synchronously to set up the ZDATA handler
    // before ZDATA arrives; the save dialog runs in parallel.
    function handleSzDownload(session: ZmodemSession) {
      session.on("offer", (xfer: ZmodemTransfer) => {
        const details = xfer.get_details();
        // Start receiving immediately (sets ZDATA handler, sends ZRPOS)
        const acceptPromise = xfer.accept();
        // Ask user where to save — runs in parallel with data reception
        acceptPromise.then((payloads: Uint8Array[]) => {
          saveToDisk(payloads, details.name).catch((e: unknown) => {
            console.error("[ZMODEM] save failed:", e);
          });
        }).catch((e: unknown) => {
          console.error("[ZMODEM] accept failed:", e);
          xfer.skip();
        });
      });
      session.on("session_end", () => {
        cleanupSession(session);
      });
      session.start().catch((e: unknown) => {
        console.error("[ZMODEM] session start failed:", e);
        cleanupSession(session);
      });
    }

    // Centralised cleanup for both upload and download sessions.
    const cleanupSession = (sess: ZmodemSession) => {
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
      zmodemSession = null;
      zmodemCooldownUntil = Date.now() + 10000;
      clearSentryState();
    };

    // rz: remote is RECEIVING files from us (session.type === "send").
    // Show a file picker and send selected files via ZMODEM.
    function handleRzUpload(session: ZmodemSession) {
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
            const chunkSize = 8192;
            let sent = 0;
            for (let offset = 0; offset < file.size; offset += chunkSize) {
              const slice = file.slice(offset, Math.min(offset + chunkSize, file.size));
              const buf = await slice.arrayBuffer();
              xfer.send(new Uint8Array(buf));
              sent += buf.byteLength;
            }
            console.log(`[ZMODEM] rz: sent ${sent} bytes, ending file`);
            await xfer.end(new Uint8Array(0));
            console.log("[ZMODEM] rz: file end confirmed");
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
      // When no session is active, the terminal:output listener writes
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
        console.log("[ZMODEM] sender sending", octets.length, "bytes, first4:", octets.slice(0, 4).join(","));
        sendToBackend(octets);
      },
      on_detect: (detection: ZmodemDetection) => {
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
        // alive while the user is picking a file; we disable it entirely on the
        // session instance so no ZSINIT packets can ever be sent.
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const sessAny = session as any;
        sessAny._start_keepalive = function () { /* disabled */ };
        sessAny._stop_keepalive = function () { /* disabled */ };
        sessAny._send_ZSINIT = function () { /* disabled */ };
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

    // User input → backend (base64 encoded for binary safety)
    const inputDisposable = term.onData((data) => {
      const bytes = new Uint8Array(data.length);
      for (let i = 0; i < data.length; i++) bytes[i] = data.charCodeAt(i);
      sendToBackend(bytes);
    });

    // Resize → backend
    const resizeDisposable = term.onResize(({ cols, rows }) => {
      ipcInvoke("ipc_terminal_resize", {
        session_id: sessionIdRef.current,
        cols,
        rows,
      }).catch(() => {});
    });

    // Listen for terminal output events from backend (base64-encoded)
    // When no ZMODEM session is active: feed to Sentry AND write raw data.
    // When a session IS active: only feed to Sentry; to_terminal handles output.
    // During cooldown: skip Sentry entirely, write raw data directly.
    let unlistenOutput: UnlistenFn | undefined;
    listen<{ sessionId: string; data: string; stderr: boolean }>(
      "terminal:output",
      (event) => {
        if (event.payload.sessionId === sessionIdRef.current) {
          const rawBytes = base64ToBytes(event.payload.data);
          const hadSession = !!zmodemSession;
          const inCooldown = Date.now() < zmodemCooldownUntil;

          if (inCooldown && !zmodemSession) {
            // Cooldown after session end: write raw data, skip Sentry
            // to prevent spurious session creation from echoed ZMODEM bytes.
            // Strip any leading ZMODEM headers (e.g. echoed ZFIN) so they
            // don't appear as shell commands on the terminal.
            const clean = stripLeadingZmodemHeaders(Array.from(rawBytes));
            term.write(clean.length ? new Uint8Array(clean) : new Uint8Array(0));
            return;
          }

          try {
            zsentry.consume(rawBytes);
          } catch (e) {
            console.error("[ZMODEM] sentry consume error:", e);
          }
          // No session before or after → normal shell output, write directly
          if (!hadSession && !zmodemSession) {
            term.write(rawBytes);
          }
          // Session ended during this chunk — clear Sentry, set cooldown, and
          // let to_terminal handle the trailing bytes. Do NOT write rawBytes
          // here because it may contain ZMODEM protocol frames (ZFIN, etc.).
          if (hadSession && !zmodemSession) {
            zmodemCooldownUntil = Date.now() + 10000;
            clearSentryState();
          }
        }
      }
    ).then((fn) => { unlistenOutput = fn; });

    // Listen for terminal closed event
    let unlistenClosed: UnlistenFn | undefined;
    listen<{ sessionId: string }>("terminal:closed", (event) => {
      if (event.payload.sessionId === sessionIdRef.current) {
        term.write("\r\n[Connection closed]\r\n");
      }
    }).then((fn) => { unlistenClosed = fn; });

    // Window resize handler
    const handleResize = () => {
      try { fitAddon.fit(); } catch { /* container not visible */ }
    };
    window.addEventListener("resize", handleResize);
    const resizeObserver = new ResizeObserver(() => handleResize());
    resizeObserver.observe(containerRef.current);

    term.focus();

    return () => {
      inputDisposable.dispose();
      resizeDisposable.dispose();
      if (unlistenOutput) unlistenOutput();
      if (unlistenClosed) unlistenClosed();
      window.removeEventListener("resize", handleResize);
      resizeObserver.disconnect();
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);

  // Re-fit and focus when tab becomes active
  useEffect(() => {
    if (!active || !termRef.current || !fitRef.current) return;
    try {
      fitRef.current.fit();
      ipcInvoke("ipc_terminal_resize", {
        session_id: sessionIdRef.current,
        cols: termRef.current.cols,
        rows: termRef.current.rows,
      }).catch(() => {});
      termRef.current.focus();
    } catch { /* ignore */ }
  }, [active]);

  return (
    <div
      ref={containerRef}
      className="w-full h-full bg-[#1e1e2e] overflow-hidden"
    />
  );
}
