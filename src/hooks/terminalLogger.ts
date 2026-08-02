// terminalLogger — record raw PTY input/output to disk for debugging
//
// Writes a timestamped log of all terminal I/O (user keystrokes + server output)
// to a file in the app's local data directory. Each entry includes:
//   - timestamp (ISO 8601 + ms)
//   - direction (INPUT = user→server, OUTPUT = server→user)
//   - byte length
//   - hex dump (first 256 bytes) + UTF-8 text (if decodable)
//
// Log file path: {AppLocalData}/termfast-logs/terminal-{sessionId}-{timestamp}.log
//
// Usage:
//   import { terminalLogger } from "@/hooks/terminalLogger";
//   await terminalLogger.init(sessionId);
//   terminalLogger.logInput(sessionId, bytes);
//   terminalLogger.logOutput(sessionId, bytes);
//   terminalLogger.flush(sessionId);  // on unmount

import { open, mkdir, type FileHandle, BaseDirectory } from "@tauri-apps/plugin-fs";

interface LogState {
  handle: FileHandle;
  path: string;
  writeQueue: string[];
  flushTimer: number | null;
  closed: boolean;
}

const sessions = new Map<string, LogState>();

/** Buffer writes for this many ms before flushing to disk (batch small writes). */
const FLUSH_INTERVAL_MS = 200;

/** Max hex dump bytes per entry (keep logs readable). */
const MAX_HEX_BYTES = 256;

/**
 * Initialize a terminal log file for a session.
 * Creates the log file (truncates if exists) and writes a header.
 * Safe to call multiple times — re-init will close the old file and open a new one.
 */
export async function initTerminalLog(sessionId: string): Promise<string | null> {
  // Close existing log for this session
  closeTerminalLog(sessionId);

  const now = new Date();
  const ts = now.toISOString().replace(/[:.]/g, "-").slice(0, 23);
  const fileName = `terminal-${sessionId.slice(0, 12)}-${ts}.log`;
  const dir = "termfast-logs";

  try {
    // Ensure the log directory exists
    await mkdir(dir, { baseDir: BaseDirectory.AppLocalData, recursive: true });

    const handle = await open(`${dir}/${fileName}`, {
      write: true,
      create: true,
      truncate: true,
      baseDir: BaseDirectory.AppLocalData,
    });

    const header =
      `=== Terminal I/O Log ===\n` +
      `Session: ${sessionId}\n` +
      `Started: ${now.toISOString()}\n` +
      `Format: [ISO timestamp] DIRECTION len=N bytes=N\n` +
      `         HEX: xx xx xx ...\n` +
      `         TEXT: ...\n` +
      `\n`;

    await handle.write(new TextEncoder().encode(header));

    const state: LogState = {
      handle,
      path: `${dir}/${fileName}`,
      writeQueue: [],
      flushTimer: null,
      closed: false,
    };
    sessions.set(sessionId, state);

    console.log(`[terminalLogger] initialized: ${state.path}`);
    return state.path;
  } catch (e) {
    console.error(`[terminalLogger] init failed for ${sessionId}:`, e);
    return null;
  }
}

/** Convert bytes to a hex string (space-separated, max MAX_HEX_BYTES). */
function toHex(bytes: Uint8Array): string {
  const n = Math.min(bytes.length, MAX_HEX_BYTES);
  const parts: string[] = [];
  for (let i = 0; i < n; i++) {
    parts.push(bytes[i].toString(16).padStart(2, "0"));
  }
  if (bytes.length > MAX_HEX_BYTES) {
    parts.push(`... (${bytes.length - MAX_HEX_BYTES} more bytes)`);
  }
  return parts.join(" ");
}

/** Try to decode bytes as UTF-8 text; return null if not decodable (binary). */
function toText(bytes: Uint8Array): string | null {
  try {
    const decoded = new TextDecoder("utf-8", { fatal: false }).decode(bytes);
    // Replace control chars (except \n \r \t) with escape notation for readability
    return decoded.replace(/[\x00-\x08\x0B\x0C\x0E-\x1F\x7F]/g, (ch) => {
      const code = ch.charCodeAt(0);
      if (code === 0x1b) return "\\x1b"; // ESC — very common in terminal
      return `\\x${code.toString(16).padStart(2, "0")}`;
    });
  } catch {
    return null;
  }
}

/** Enqueue a log entry and schedule a flush. */
function enqueue(sessionId: string, entry: string): void {
  const state = sessions.get(sessionId);
  if (!state || state.closed) return;

  state.writeQueue.push(entry);

  if (state.flushTimer === null) {
    state.flushTimer = window.setTimeout(() => {
      flushTerminalLog(sessionId);
    }, FLUSH_INTERVAL_MS);
  }
}

/**
 * Log user input (keystrokes sent to the server).
 * @param sessionId  terminal session ID
 * @param bytes      raw bytes sent to the PTY
 */
export function logTerminalInput(sessionId: string, bytes: Uint8Array): void {
  if (bytes.length === 0) return;
  const ts = new Date().toISOString();
  const hex = toHex(bytes);
  const text = toText(bytes) ?? "(binary)";
  const entry =
    `[${ts}] INPUT  len=${bytes.length} bytes=${bytes.length}\n` +
    `         HEX: ${hex}\n` +
    `         TEXT: ${text}\n`;
  enqueue(sessionId, entry);
}

/**
 * Log server output (data received from the PTY).
 * @param sessionId  terminal session ID
 * @param bytes      raw bytes received from the PTY
 */
export function logTerminalOutput(sessionId: string, bytes: Uint8Array): void {
  if (bytes.length === 0) return;
  const ts = new Date().toISOString();
  const hex = toHex(bytes);
  const text = toText(bytes) ?? "(binary)";
  const entry =
    `[${ts}] OUTPUT len=${bytes.length} bytes=${bytes.length}\n` +
    `         HEX: ${hex}\n` +
    `         TEXT: ${text}\n`;
  enqueue(sessionId, entry);
}

/** Flush queued log entries to disk. */
export function flushTerminalLog(sessionId: string): void {
  const state = sessions.get(sessionId);
  if (!state || state.closed) return;

  if (state.flushTimer !== null) {
    clearTimeout(state.flushTimer);
    state.flushTimer = null;
  }

  if (state.writeQueue.length === 0) return;

  const data = state.writeQueue.join("");
  state.writeQueue = [];

  state.handle.write(new TextEncoder().encode(data)).catch((e) => {
    console.error(`[terminalLogger] write failed for ${sessionId}:`, e);
  });
}

/** Close the log file for a session (flush + close handle). */
export async function closeTerminalLog(sessionId: string): Promise<void> {
  const state = sessions.get(sessionId);
  if (!state) return;

  flushTerminalLog(sessionId);

  state.closed = true;
  try {
    await state.handle.close();
    console.log(`[terminalLogger] closed: ${state.path}`);
  } catch (e) {
    console.error(`[terminalLogger] close failed for ${sessionId}:`, e);
  }
  sessions.delete(sessionId);
}

/**
 * Log a debug message to the terminal log file (for agentStatus diagnostics).
 * This is separate from console.log — it writes to the same file as I/O logs
 * so screen scrape results can be correlated with raw PTY output.
 */
export function logTerminalDebug(sessionId: string, message: string): void {
  const ts = new Date().toISOString();
  const entry = `[${ts}] DEBUG  ${message}\n`;
  enqueue(sessionId, entry);
}

// === SECTION 1 END ===
