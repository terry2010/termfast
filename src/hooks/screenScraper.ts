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
 * Extract tab info from multi-question dialog tab rows.
 *
 * Supports two formats:
 *
 * 1. OpenCode: "  ┃   编程语言   测试反馈   下一步   Confirm"
 *    - Tab labels separated by 2+ spaces
 *    - Last label is "Confirm"
 *    - Active tab has bright accent background color
 *
 * 2. Devin: "── 测试单选 · 测试多选 · 布尔确认 · 长标签测试 ──..."
 *    - Tab labels separated by " · " (space + middle dot + space)
 *    - Line starts with "──" and ends with "──"
 *    - Answered tabs have " ✓" appended
 *    - No separate "Confirm" tab (all tabs are question tabs)
 *    - Active tab detected by cell background color (if possible)
 *
 * @param term  the xterm.js Terminal instance
 * @returns `{ labels: string[], activeIndex: number, debug?: string }` or null if no tab row found.
 *   - `labels`: tab label strings
 *   - `activeIndex`: 0-based index of the active tab (-1 if detection failed)
 *   - `debug`: diagnostic info for debugging active tab detection
 */
export function extractTabInfo(term: Terminal): { labels: string[]; activeIndex: number; debug?: string } | null {
  const buffer = term.buffer.active;
  const height = buffer.length;
  const startLine = Math.max(0, height - MAX_LINES);

  for (let i = startLine; i < height; i++) {
    const line = buffer.getLine(i);
    if (!line) continue;
    const text = extractLineText(line);
    const trimmed = text.replace(/^\s*[┃│║]\s*/, "").trim();

    // Build a mapping from character index → cell index.
    // This is needed because CJK characters take 2 cells (width 2),
    // so text.indexOf() returns a character index that doesn't match
    // the cell index needed for line.getCell().
    const charToCell: number[] = [];
    let charIdx = 0;
    for (let x = 0; x < line.length; x++) {
      const c = line.getCell(x);
      if (!c) continue;
      if (c.getWidth() === 0) continue; // skip wide-char continuation
      charToCell[charIdx] = x;
      charIdx++;
    }

    // ── OpenCode format: "Confirm" + 2+ space-separated labels ──
    if (/\bConfirm\b/.test(trimmed) && /\s{2,}/.test(trimmed)) {
      const labels = trimmed.split(/\s{2,}/);
      if (labels.length < 2) continue;

      // Detect active tab by checking cell bg colors.
      let activeIndex = -1;
      let searchStart = 0;
      for (let j = 0; j < labels.length; j++) {
        const label = labels[j];
        const charPos = text.indexOf(label, searchStart);
        if (charPos < 0) {
          searchStart += label.length;
          continue;
        }
        const cellPos = charToCell[charPos] ?? charPos;
        const cell = line.getCell(cellPos);
        if (cell) {
          const bgMode = cell.getBgColorMode();
          const bgColor = cell.getBgColor();
          // xterm.js color mode: CM_RGB = 0x03000000
          if (bgMode === 0x03000000) {
            const r = (bgColor >> 16) & 0xff;
            const g = (bgColor >> 8) & 0xff;
            const b = bgColor & 0xff;
            if (r + g + b > 200) {
              activeIndex = j;
              break;
            }
          }
        }
        searchStart = charPos + label.length;
      }

      return { labels, activeIndex };
    }

    // ── Claude Code format: "←  ☐ label  ☐ label  ...  ✔ Submit  →" ──
    // Tab row wrapped with ← and → arrows. Tabs separated by 2+ spaces.
    // Each tab has a checkbox prefix: ☐ = unanswered, ✔ = answered.
    // Last entry is "Submit" button (always has ✔ prefix).
    if (/^[←]\s+☐.*✔\s*Submit\s*→/.test(trimmed)) {
      // Strip the leading "←" and trailing "→"
      const content = trimmed.replace(/^[←]\s+/, "").replace(/\s*→$/, "").trim();
      // Split by 2+ spaces
      const parts = content.split(/\s{2,}/);
      if (parts.length < 2) continue;
      // Extract labels by stripping ☐/✔ prefix
      const labels = parts.map((p) => p.replace(/^[☐✔]\s*/, "").trim());
      // Detect active tab by checking cell bg colors (bright = active)
      let activeIndex = -1;
      let searchStart = 0;
      for (let j = 0; j < labels.length; j++) {
        const label = labels[j];
        const charPos = text.indexOf(label, searchStart);
        if (charPos < 0) {
          searchStart += label.length;
          continue;
        }
        const cellPos = charToCell[charPos] ?? charPos;
        const cell = line.getCell(cellPos);
        if (cell) {
          const bgMode = cell.getBgColorMode();
          const bgColor = cell.getBgColor();
          if (bgMode === 0x03000000) {
            const r = (bgColor >> 16) & 0xff;
            const g = (bgColor >> 8) & 0xff;
            const b = bgColor & 0xff;
            if (r + g + b > 200) {
              activeIndex = j;
              break;
            }
          }
        }
        searchStart = charPos + label.length;
      }
      return { labels, activeIndex };
    }

    // ── Devin format: "── label · label · ... ──" ──
    // Line starts with "──", contains " · " separators, and ends with "──"
    if (/^──\s+.+\s·\s.+\s──/.test(trimmed)) {
      // Extract the content between the leading "──" and trailing "──"
      const content = trimmed.replace(/^──\s+/, "").replace(/\s──+$/, "").trim();
      // Split by " · " (space + middle dot + space)
      const labels = content.split(/\s·\s/);
      if (labels.length < 2) continue;

      // Debug: log the charToCell mapping for diagnosis
      const debugMap = labels.map((l, j) => {
        const labelForMatch = l.replace(/\s✓$/, "").trim();
        const charPos = text.indexOf(labelForMatch, j > 0 ? text.indexOf(labels[j-1].replace(/\s✓$/, "").trim()) + labels[j-1].length : 0);
        const cellPos = charPos >= 0 ? (charToCell[charPos] ?? -1) : -1;
        const cell = cellPos >= 0 ? line.getCell(cellPos) : null;
        const fgMode = cell?.getFgColorMode() ?? -1;
        const fgColor = cell?.getFgColor() ?? -1;
        const r = fgMode === 2 ? (fgColor >> 16) & 0xff : -1;
        const g = fgMode === 2 ? (fgColor >> 8) & 0xff : -1;
        const b = fgMode === 2 ? fgColor & 0xff : -1;
        return `[${j}] "${labelForMatch}" charPos=${charPos} cellPos=${cellPos} fgMode=${fgMode} rgb(${r},${g},${b}) sum=${r+g+b}`;
      });
      console.log(`[extractTabInfo:devin] ${debugMap.join(" | ")}`);

      // Detect active tab by checking cell FOREGROUND colors.
      // Devin uses foreground color to distinguish tabs:
      //   active tab:   RGB(94, 196, 255) — bright blue (R+G+B = 545)
      //   inactive tab: RGB(124, 124, 124) — gray (R+G+B = 372)
      let activeIndex = -1;
      let searchStart = 0;
      const debugInfo: string[] = [];
      for (let j = 0; j < labels.length; j++) {
        let label = labels[j];
        // Strip " ✓" suffix for matching (answered tabs have ✓ appended)
        const labelForMatch = label.replace(/\s✓$/, "").trim();
        const charPos = text.indexOf(labelForMatch, searchStart);
        if (charPos < 0) {
          searchStart += labelForMatch.length;
          continue;
        }
        // Convert character index to cell index
        const cellPos = charToCell[charPos] ?? charPos;
        const cell = line.getCell(cellPos);
        let fgMode = -1, fgColor = -1, r = -1, g = -1, b = -1;
        if (cell) {
          fgMode = cell.getFgColorMode();
          fgColor = cell.getFgColor();
          // xterm.js color mode constants:
          //   CM_DEFAULT = 0x00000000, CM_P16 = 0x01000000,
          //   CM_P256 = 0x02000000, CM_RGB = 0x03000000
          if (fgMode === 0x03000000) {
            r = (fgColor >> 16) & 0xff;
            g = (fgColor >> 8) & 0xff;
            b = fgColor & 0xff;
            // Active tab has bright blue fg (R+G+B > 400)
            // Inactive tabs are gray (R+G+B ≈ 372)
            if (r + g + b > 400) {
              activeIndex = j;
              debugInfo.push(`[${j}] "${labelForMatch}" ACTIVE rgb(${r},${g},${b}) sum=${r+g+b}`);
              break;
            }
          }
        }
        debugInfo.push(`[${j}] "${labelForMatch}" charPos=${charPos} cellPos=${cellPos} fgMode=${fgMode} rgb(${r},${g},${b}) sum=${r+g+b}`);
        searchStart = charPos + labelForMatch.length;
      }
      console.log(`[extractTabInfo:devin] text="${text.substring(0,80)}..." | ${debugInfo.join(" | ")}`);

      return { labels, activeIndex, debug: debugInfo.join(" | ") };
    }
  }

  return null;
}
