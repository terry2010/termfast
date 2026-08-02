// screenScraper — extract visible text from xterm.js terminal buffer
//
// Used by agent status detection to identify CLI states via screen content
// (e.g. "esc interrupt" = OpenCode working, "↑/↓ to navigate" = Claude Code
// blocked, "Approve ... y/n" = Codex blocked).
//
// xterm.js buffer API:
//   term.buffer.active  — the currently active buffer (normal or alternate)
//   buffer.length       — number of lines in the buffer
//   buffer.getLine(n)   — get IBufferLine at index n
//   line.cellCount      — number of cells in the line
//   line.getCell(x)     — get IBufferCell at position x
//   cell.getChars()     — the character(s) at this cell (may be empty for wide chars)
//   cell.getWidth()     — 0 for wide-char continuation, 1 for normal, 2 for wide
//
// We extract the visible viewport (not the entire scrollback) for efficiency
// and to match what the user actually sees.

import type { Terminal, IBufferLine } from "@xterm/xterm";

/** Maximum number of lines to scrape from the bottom of the viewport. */
const MAX_LINES = 50;

/**
 * Extract visible text lines from the terminal's active buffer.
 *
 * Returns an array of plain-text lines (no ANSI codes), trimmed of trailing
 * whitespace. Only the bottom MAX_LINES lines of the visible viewport are
 * returned — this is where status bars, prompts, and footers live.
 *
 * @param term  the xterm.js Terminal instance
 * @returns array of text lines (bottom of screen first in array index 0 = top)
 */
export function scrapeScreen(term: Terminal): string[] {
  const buffer = term.buffer.active;
  const height = buffer.length;
  const startLine = Math.max(0, height - MAX_LINES);
  const lines: string[] = [];

  for (let i = startLine; i < height; i++) {
    const line = buffer.getLine(i);
    if (!line) continue;
    const text = extractLineText(line);
    lines.push(text);
  }

  return lines;
}

/**
 * Extract text content from a single buffer line.
 * Handles wide characters (CJK, emoji) by skipping continuation cells.
 */
function extractLineText(line: IBufferLine): string {
  let text = "";
  const lineLength = line.length;

  for (let x = 0; x < lineLength; x++) {
    const cell = line.getCell(x);
    if (!cell) continue;
    // Skip wide-char continuation cells (width 0)
    if (cell.getWidth() === 0) continue;
    text += cell.getChars();
  }

  // Trim trailing whitespace (xterm pads lines with spaces)
  return text.replace(/\s+$/, "");
}

/**
 * Get the last N non-empty lines from the screen scrape.
 * Useful for footer/status bar detection where only the bottom matters.
 *
 * @param lines  full screen scrape (from scrapeScreen)
 * @param n      number of non-empty lines to return from the bottom
 * @returns array of the last N non-empty lines
 */
export function getBottomLines(lines: string[], n: number): string[] {
  const nonEmpty = lines.filter((l) => l.trim().length > 0);
  return nonEmpty.slice(-n);
}

/**
 * Join screen lines into a single string for regex matching.
 * Uses \n as line separator.
 */
export function joinLines(lines: string[]): string {
  return lines.join("\n");
}
