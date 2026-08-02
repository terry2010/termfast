// Unit tests for screenScraper — extractTabInfo (active tab detection)
import { describe, it, expect } from "vitest";
import { extractTabInfo } from "../screenScraper";
import type { Terminal, IBufferLine, IBufferCell } from "@xterm/xterm";

// ── Mock IBufferCell ──────────────────────────────────────────────────────
interface MockCellConfig {
  char: string;
  width?: number;
  bgMode?: number;
  bgColor?: number;
  fgMode?: number;
  fgColor?: number;
}

function createMockCell(config: MockCellConfig): IBufferCell {
  return {
    getWidth: () => config.width ?? 1,
    getChars: () => config.char,
    getCode: () => config.char.charCodeAt(0) || 0,
    getFgColor: () => config.fgColor ?? 0,
    getBgColor: () => config.bgColor ?? 0,
    getFgColorMode: () => config.fgMode ?? 0,
    getBgColorMode: () => config.bgMode ?? 0,
    isBold: () => false,
    isItalic: () => false,
    isDim: () => false,
    isUnderline: () => false,
    isBlink: () => false,
    isInverse: () => false,
    isInvisible: () => false,
    isStrikethrough: () => false,
  } as unknown as IBufferCell;
}

// ── Mock IBufferLine ──────────────────────────────────────────────────────
// Builds a line from a string + optional per-position color overrides.
// `bgOverrides` maps character index → { mode, color } for background.
// `fgOverrides` maps character index → { mode, color } for foreground.
// CJK characters are simulated as width=2 with a continuation cell (width=0),
// matching real xterm.js behavior.
function createMockLine(
  text: string,
  bgOverrides?: Map<number, { mode: number; color: number }>,
  fgOverrides?: Map<number, { mode: number; color: number }>,
): IBufferLine {
  const cells: IBufferCell[] = [];
  for (let i = 0; i < text.length; i++) {
    const bg = bgOverrides?.get(i);
    const fg = fgOverrides?.get(i);
    const isWide = /[\u3000-\u9fff\uf900-\ufaff\u{1f000}-\u{1ffff}]/u.test(text[i]);
    cells.push(
      createMockCell({
        char: text[i],
        width: isWide ? 2 : 1,
        bgMode: bg?.mode ?? 0,
        bgColor: bg?.color ?? 0,
        fgMode: fg?.mode ?? 0,
        fgColor: fg?.color ?? 0,
      }),
    );
    if (isWide) {
      // Add continuation cell (width=0) for wide characters
      cells.push(createMockCell({ char: "", width: 0 }));
    }
  }
  return {
    length: cells.length,
    getCell: (x: number) => (x >= 0 && x < cells.length ? cells[x] : undefined),
    isWrapped: false,
  } as unknown as IBufferLine;
}

// ── Mock Terminal ─────────────────────────────────────────────────────────
function createMockTerminal(lines: IBufferLine[]): Terminal {
  const buffer = {
    active: {
      length: lines.length,
      getLine: (i: number) => (i >= 0 && i < lines.length ? lines[i] : undefined),
    },
  };
  return {
    buffer,
  } as unknown as Terminal;
}

// ── Color constants ───────────────────────────────────────────────────────
// Accent color: RGB (157, 124, 216) — active tab background (OpenCode)
const ACCENT_BG_MODE = 0x03000000;
const ACCENT_BG_COLOR = (157 << 16) | (124 << 8) | 216; // 10302616

// Panel background: RGB (20, 20, 20) — inactive tab background (OpenCode)
const PANEL_BG_MODE = 0x03000000;
const PANEL_BG_COLOR = (20 << 16) | (20 << 8) | 20; // 1310740

// Devin active tab foreground: RGB (94, 196, 255) — bright blue
const DEVIN_ACTIVE_FG_MODE = 0x03000000;
const DEVIN_ACTIVE_FG_COLOR = (94 << 16) | (196 << 8) | 255; // 6188031

// Devin inactive tab foreground: RGB (124, 124, 124) — gray
const DEVIN_INACTIVE_FG_MODE = 0x03000000;
const DEVIN_INACTIVE_FG_COLOR = (124 << 16) | (124 << 8) | 124; // 8157436

// ── Tests ─────────────────────────────────────────────────────────────────

describe("extractTabInfo", () => {
  it("returns null when no tab row (no Confirm)", () => {
    const lines = [
      createMockLine("  ┃  Some content"),
      createMockLine("  ┃  1. Rust"),
      createMockLine("  ┃  ↑↓ select  enter confirm  esc dismiss"),
    ];
    const term = createMockTerminal(lines);
    expect(extractTabInfo(term)).toBeNull();
  });

  it("returns null when no tab row (Confirm but no 2+ spaces)", () => {
    const lines = [createMockLine("  ┃  Confirm")];
    const term = createMockTerminal(lines);
    expect(extractTabInfo(term)).toBeNull();
  });

  it("returns labels and activeIndex=0 when first tab is active", () => {
    // Tab row: "  ┃   编程语言   测试反馈   下一步   Confirm"
    // Active tab: "编程语言" (position 6, after "  ┃   ")
    const tabRow = "  ┃   编程语言   测试反馈   下一步   Confirm";
    const bgOverrides = new Map<number, { mode: number; color: number }>();
    // "编程语言" (pos 6-9): accent bg (active)
    for (let i = 6; i <= 9; i++) {
      bgOverrides.set(i, { mode: ACCENT_BG_MODE, color: ACCENT_BG_COLOR });
    }
    // "测试反馈" (pos 13-16): panel bg
    for (let i = 13; i <= 16; i++) {
      bgOverrides.set(i, { mode: PANEL_BG_MODE, color: PANEL_BG_COLOR });
    }
    // "下一步" (pos 20-22): panel bg
    for (let i = 20; i <= 22; i++) {
      bgOverrides.set(i, { mode: PANEL_BG_MODE, color: PANEL_BG_COLOR });
    }
    // "Confirm" (pos 26-32): panel bg
    for (let i = 26; i <= 32; i++) {
      bgOverrides.set(i, { mode: PANEL_BG_MODE, color: PANEL_BG_COLOR });
    }

    const lines = [createMockLine(tabRow, bgOverrides)];
    const term = createMockTerminal(lines);
    const result = extractTabInfo(term);
    expect(result).not.toBeNull();
    expect(result!.labels).toEqual(["编程语言", "测试反馈", "下一步", "Confirm"]);
    expect(result!.activeIndex).toBe(0);
  });

  it("returns labels and activeIndex=1 when second tab is active", () => {
    const tabRow = "  ┃   编程语言   测试反馈   下一步   Confirm";
    const bgOverrides = new Map<number, { mode: number; color: number }>();
    // "编程语言" (pos 6-9): panel bg
    for (let i = 6; i <= 9; i++) {
      bgOverrides.set(i, { mode: PANEL_BG_MODE, color: PANEL_BG_COLOR });
    }
    // "测试反馈" (pos 13-16): accent bg (active)
    for (let i = 13; i <= 16; i++) {
      bgOverrides.set(i, { mode: ACCENT_BG_MODE, color: ACCENT_BG_COLOR });
    }
    // "下一步" (pos 20-22): panel bg
    for (let i = 20; i <= 22; i++) {
      bgOverrides.set(i, { mode: PANEL_BG_MODE, color: PANEL_BG_COLOR });
    }
    // "Confirm" (pos 26-32): panel bg
    for (let i = 26; i <= 32; i++) {
      bgOverrides.set(i, { mode: PANEL_BG_MODE, color: PANEL_BG_COLOR });
    }

    const lines = [createMockLine(tabRow, bgOverrides)];
    const term = createMockTerminal(lines);
    const result = extractTabInfo(term);
    expect(result).not.toBeNull();
    expect(result!.labels).toEqual(["编程语言", "测试反馈", "下一步", "Confirm"]);
    expect(result!.activeIndex).toBe(1);
  });

  it("returns activeIndex=3 when Confirm tab is active", () => {
    const tabRow = "  ┃   编程语言   测试反馈   下一步   Confirm";
    const bgOverrides = new Map<number, { mode: number; color: number }>();
    // All tabs panel bg except Confirm (pos 26-32): accent bg
    for (let i = 6; i <= 9; i++) bgOverrides.set(i, { mode: PANEL_BG_MODE, color: PANEL_BG_COLOR });
    for (let i = 13; i <= 16; i++) bgOverrides.set(i, { mode: PANEL_BG_MODE, color: PANEL_BG_COLOR });
    for (let i = 20; i <= 22; i++) bgOverrides.set(i, { mode: PANEL_BG_MODE, color: PANEL_BG_COLOR });
    for (let i = 26; i <= 32; i++) bgOverrides.set(i, { mode: ACCENT_BG_MODE, color: ACCENT_BG_COLOR });

    const lines = [createMockLine(tabRow, bgOverrides)];
    const term = createMockTerminal(lines);
    const result = extractTabInfo(term);
    expect(result).not.toBeNull();
    expect(result!.activeIndex).toBe(3);
  });

  it("returns activeIndex=-1 when color detection fails (all default bg)", () => {
    const tabRow = "  ┃   编程语言   测试反馈   下一步   Confirm";
    // No bg overrides — all cells have default bg (mode 0)
    const lines = [createMockLine(tabRow)];
    const term = createMockTerminal(lines);
    const result = extractTabInfo(term);
    expect(result).not.toBeNull();
    expect(result!.labels).toEqual(["编程语言", "测试反馈", "下一步", "Confirm"]);
    expect(result!.activeIndex).toBe(-1);
  });

  it("returns labels with 2 tabs (1 question + Confirm)", () => {
    const tabRow = "  ┃   问题1   Confirm";
    const bgOverrides = new Map<number, { mode: number; color: number }>();
    // "问题1" at pos 6-8: accent bg (active)
    for (let i = 6; i <= 8; i++) {
      bgOverrides.set(i, { mode: ACCENT_BG_MODE, color: ACCENT_BG_COLOR });
    }
    // "Confirm" at pos 12-18: panel bg
    for (let i = 12; i <= 18; i++) {
      bgOverrides.set(i, { mode: PANEL_BG_MODE, color: PANEL_BG_COLOR });
    }

    const lines = [createMockLine(tabRow, bgOverrides)];
    const term = createMockTerminal(lines);
    const result = extractTabInfo(term);
    expect(result).not.toBeNull();
    expect(result!.labels).toEqual(["问题1", "Confirm"]);
    expect(result!.activeIndex).toBe(0);
  });

  it("finds tab row among multiple lines", () => {
    const lines = [
      createMockLine("  ┃  Some content"),
      createMockLine("  ┃   编程语言   测试反馈   Confirm"),
      createMockLine("  ┃  1. Rust"),
      createMockLine("  ┃  ↑↓ select  enter confirm  esc dismiss"),
    ];
    // Tab row: "  ┃   编程语言   测试反馈   Confirm"
    // "编程语言" at pos 6-9: accent bg (active)
    // "测试反馈" at pos 13-16: panel bg
    // "Confirm" at pos 20-26: panel bg
    const bgOverrides = new Map<number, { mode: number; color: number }>();
    for (let i = 6; i <= 9; i++) {
      bgOverrides.set(i, { mode: ACCENT_BG_MODE, color: ACCENT_BG_COLOR });
    }
    for (let i = 13; i <= 16; i++) {
      bgOverrides.set(i, { mode: PANEL_BG_MODE, color: PANEL_BG_COLOR });
    }
    for (let i = 20; i <= 26; i++) {
      bgOverrides.set(i, { mode: PANEL_BG_MODE, color: PANEL_BG_COLOR });
    }
    lines[1] = createMockLine("  ┃   编程语言   测试反馈   Confirm", bgOverrides);

    const term = createMockTerminal(lines);
    const result = extractTabInfo(term);
    expect(result).not.toBeNull();
    expect(result!.labels).toEqual(["编程语言", "测试反馈", "Confirm"]);
    expect(result!.activeIndex).toBe(0);
  });
});

// ── Devin tab row tests ───────────────────────────────────────────────────

describe("extractTabInfo — Devin format", () => {
  it("returns labels for Devin tab row (── label · label ──)", () => {
    // Devin tab row: "── 测试单选 · 测试多选 · 布尔确认 · 长标签测试 ──..."
    const tabRow = "── 测试单选 · 测试多选 · 布尔确认 · 长标签测试 ────────────────────────────────────────────────────────────────────────────────────────";
    const lines = [createMockLine(tabRow)];
    const term = createMockTerminal(lines);
    const result = extractTabInfo(term);
    expect(result).not.toBeNull();
    expect(result!.labels).toEqual(["测试单选", "测试多选", "布尔确认", "长标签测试"]);
  });

  it("returns activeIndex=0 when first tab has bright blue fg", () => {
    const tabRow = "── 测试单选 · 测试多选 · 布尔确认 ────────────────────────────────────────────────────────────────────────────────────────";
    // "测试单选" starts at pos 3 (after "── ")
    // Devin uses FOREGROUND color: active=RGB(94,196,255), inactive=RGB(124,124,124)
    const fgOverrides = new Map<number, { mode: number; color: number }>();
    for (let i = 3; i <= 6; i++) {
      fgOverrides.set(i, { mode: DEVIN_ACTIVE_FG_MODE, color: DEVIN_ACTIVE_FG_COLOR });
    }
    const lines = [createMockLine(tabRow, undefined, fgOverrides)];
    const term = createMockTerminal(lines);
    const result = extractTabInfo(term);
    expect(result).not.toBeNull();
    expect(result!.labels).toEqual(["测试单选", "测试多选", "布尔确认"]);
    expect(result!.activeIndex).toBe(0);
  });

  it("returns activeIndex=1 when second tab has bright blue fg", () => {
    const tabRow = "── 测试单选 · 测试多选 · 布尔确认 ────────────────────────────────────────────────────────────────────────────────────────";
    // "测试多选" starts at pos 9 (after "── 测试单选 · ")
    const fgOverrides = new Map<number, { mode: number; color: number }>();
    for (let i = 9; i <= 12; i++) {
      fgOverrides.set(i, { mode: DEVIN_ACTIVE_FG_MODE, color: DEVIN_ACTIVE_FG_COLOR });
    }
    const lines = [createMockLine(tabRow, undefined, fgOverrides)];
    const term = createMockTerminal(lines);
    const result = extractTabInfo(term);
    expect(result).not.toBeNull();
    expect(result!.activeIndex).toBe(1);
  });

  it("returns activeIndex=-1 when all tabs are gray (no active)", () => {
    const tabRow = "── 测试单选 · 测试多选 · 布尔确认 ────────────────────────────────────────────────────────────────────────────────────────";
    // All tabs gray fg (inactive) — no bright blue
    const fgOverrides = new Map<number, { mode: number; color: number }>();
    for (let i = 3; i <= 6; i++) {
      fgOverrides.set(i, { mode: DEVIN_INACTIVE_FG_MODE, color: DEVIN_INACTIVE_FG_COLOR });
    }
    for (let i = 9; i <= 12; i++) {
      fgOverrides.set(i, { mode: DEVIN_INACTIVE_FG_MODE, color: DEVIN_INACTIVE_FG_COLOR });
    }
    for (let i = 15; i <= 18; i++) {
      fgOverrides.set(i, { mode: DEVIN_INACTIVE_FG_MODE, color: DEVIN_INACTIVE_FG_COLOR });
    }
    const lines = [createMockLine(tabRow, undefined, fgOverrides)];
    const term = createMockTerminal(lines);
    const result = extractTabInfo(term);
    expect(result).not.toBeNull();
    expect(result!.labels).toEqual(["测试单选", "测试多选", "布尔确认"]);
    expect(result!.activeIndex).toBe(-1);
  });

  it("returns activeIndex=-1 when color detection fails (no fg color)", () => {
    const tabRow = "── 测试单选 · 测试多选 · 布尔确认 ────────────────────────────────────────────────────────────────────────────────────────";
    const lines = [createMockLine(tabRow)];
    const term = createMockTerminal(lines);
    const result = extractTabInfo(term);
    expect(result).not.toBeNull();
    expect(result!.labels).toEqual(["测试单选", "测试多选", "布尔确认"]);
    expect(result!.activeIndex).toBe(-1);
  });

  it("handles answered tabs with ✓ suffix", () => {
    // Tab row with ✓ on first tab: "── 测试单选 ✓ · 测试多选 · 布尔确认 ──..."
    const tabRow = "── 测试单选 ✓ · 测试多选 · 布尔确认 ────────────────────────────────────────────────────────────────────────────────────────";
    // "测试多选" is active (bright blue fg)
    // "测试单选" starts at pos 3, "测试多选" starts at pos 12 (after "── 测试单选 ✓ · ")
    const fgOverrides = new Map<number, { mode: number; color: number }>();
    for (let i = 12; i <= 15; i++) {
      fgOverrides.set(i, { mode: DEVIN_ACTIVE_FG_MODE, color: DEVIN_ACTIVE_FG_COLOR });
    }
    const lines = [createMockLine(tabRow, undefined, fgOverrides)];
    const term = createMockTerminal(lines);
    const result = extractTabInfo(term);
    expect(result).not.toBeNull();
    expect(result!.labels).toEqual(["测试单选 ✓", "测试多选", "布尔确认"]);
    expect(result!.activeIndex).toBe(1);
  });

  it("returns null for non-tab lines with ──", () => {
    // Just a separator line, not a tab row
    const lines = [createMockLine("─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────")];
    const term = createMockTerminal(lines);
    expect(extractTabInfo(term)).toBeNull();
  });

  it("returns null for Devin tab row with only 1 label", () => {
    const lines = [createMockLine("── 单标签 ────────────────────────────────────────────────────────────────────────────────────────")];
    const term = createMockTerminal(lines);
    expect(extractTabInfo(term)).toBeNull();
  });
});
