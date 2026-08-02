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
//   cell.getBgColor()   — background color (packed RGB or palette index)
//   cell.getBgColorMode() — 0=default, 1=palette, 2=RGB
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

/**
 * Extract tab info from the OpenCode multi-question dialog's tab row.
 *
 * The tab row looks like: "  ┃   编程语言   测试反馈   下一步   Confirm"
 * The active tab has a bright accent background color (e.g., RGB 157,124,216),
 * while inactive tabs have a dark panel background (e.g., RGB 20,20,20).
 *
 * @param term  the xterm.js Terminal instance
 * @returns `{ labels: string[], activeIndex: number }` or null if no tab row found.
 *   - `labels`: tab label strings (including "Confirm" as the last one)
 *   - `activeIndex`: 0-based index of the active tab (-1 if detection failed)
 */
export function extractTabInfo(term: Terminal): { labels: string[]; activeIndex: number } | null {
  const buffer = term.buffer.active;
  const height = buffer.length;
  const startLine = Math.max(0, height - MAX_LINES);

  for (let i = startLine; i < height; i++) {
    const line = buffer.getLine(i);
    if (!line) continue;
    const text = extractLineText(line);
    const trimmed = text.replace(/^\s*[┃│║]\s*/, "").trim();

    // Check if this is the tab row: has "Confirm" + 2+ space-separated labels
    if (!/\bConfirm\b/.test(trimmed) || !/\s{2,}/.test(trimmed)) continue;
    const labels = trimmed.split(/\s{2,}/);
    if (labels.length < 2) continue;

    // Found the tab row. Detect the active tab by checking cell bg colors.
    // The active tab has a bright accent bg; inactive tabs have dark panel bg.
    let activeIndex = -1;
    let searchStart = 0;
    for (let j = 0; j < labels.length; j++) {
      const label = labels[j];
      const pos = text.indexOf(label, searchStart);
      if (pos < 0) {
        searchStart += label.length;
        continue;
      }
      // Check the bg color of the first cell of this label
      const cell = line.getCell(pos);
      if (cell) {
        const bgMode = cell.getBgColorMode();
        const bgColor = cell.getBgColor();
        if (bgMode === 2) {
          // RGB mode: unpack r, g, b
          const r = (bgColor >> 16) & 0xff;
          const g = (bgColor >> 8) & 0xff;
          const b = bgColor & 0xff;
          // Active tab has bright accent bg (R+G+B > 200)
          if (r + g + b > 200) {
            activeIndex = j;
            break;
          }
        }
      }
      searchStart = pos + label.length;
    }

    return { labels, activeIndex };
  }

  return null;
}
