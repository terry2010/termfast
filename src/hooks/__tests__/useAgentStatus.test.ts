// Unit tests for useAgentStatus — React hook binding xterm.js to state machine
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useAgentStatus, notifyAgentOutput, resetAgentStatus } from "../useAgentStatus";

// ── Mock Terminal ────────────────────────────────────────────────────────────
// Minimal stub of xterm.js Terminal that captures OSC handler registrations
// and allows tests to fire them manually.

interface OscHandler {
  ident: number;
  callback: (data: string) => boolean | Promise<boolean>;
}

function createMockTerminal(screenLines: string[] = []) {
  const oscHandlers: OscHandler[] = [];
  const bellHandlers: Array<() => void> = [];
  // Mock buffer: returns lines that tests can set for screen scraping
  let _screenLines = screenLines;
  const mockLine = (text: string) => ({
    isWrapped: false,
    length: text.length,
    getCell: (x: number) => {
      if (x >= text.length) return undefined;
      const ch = text[x];
      return {
        getChars: () => ch,
        getWidth: () => 1,
        getCode: () => ch.charCodeAt(0),
        getFgColor: () => 0,
        getBgColor: () => 0,
        getFgColorMode: () => 0,
        getBgColorMode: () => 0,
        isBold: () => false,
        isItalic: () => false,
        isDim: () => false,
        isUnderline: () => false,
        isBlink: () => false,
        isInverse: () => false,
        isInvisible: () => false,
        isStrikethrough: () => false,
      };
    },
  });
  const bufferActive = {
    get length() { return _screenLines.length; },
    getLine: (i: number) => {
      if (i < 0 || i >= _screenLines.length) return undefined;
      return mockLine(_screenLines[i]);
    },
  };
  return {
    parser: {
      registerOscHandler: vi.fn((ident: number, callback: (data: string) => boolean | Promise<boolean>) => {
        const handler: OscHandler = { ident, callback };
        oscHandlers.push(handler);
        return { dispose: () => {
          const idx = oscHandlers.indexOf(handler);
          if (idx >= 0) oscHandlers.splice(idx, 1);
        }};
      }),
    },
    buffer: {
      active: bufferActive,
    },
    onBell: vi.fn((cb: () => void) => {
      bellHandlers.push(cb);
      return { dispose: () => {
        const idx = bellHandlers.indexOf(cb);
        if (idx >= 0) bellHandlers.splice(idx, 1);
      }};
    }),
    // Helper to fire an OSC handler by ident
    _fireOsc(ident: number, data: string): void {
      const handler = oscHandlers.find((h) => h.ident === ident);
      if (handler) handler.callback(data);
    },
    _fireBell(): void {
      for (const cb of bellHandlers) cb();
    },
    _oscHandlerCount: () => oscHandlers.length,
    // Helper to update screen content for scrape tests
    _setScreenLines(lines: string[]): void {
      _screenLines = lines;
    },
  };
}

describe("useAgentStatus", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("starts with unknown status", () => {
    const term = createMockTerminal();
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));
    expect(result.current.status).toBe("unknown");
    expect(result.current.cli).toBe("unknown");
  });

  it("transitions to blocked when OSC 777 Devin notify fires", () => {
    const term = createMockTerminal();
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    act(() => {
      term._fireOsc(777, "notify;Devin;Devin needs input");
    });

    expect(result.current.status).toBe("blocked");
    expect(result.current.cli).toBe("devin");
    expect(result.current.blockedMessage).toBe("Devin needs input");
  });

  it("transitions to done when OSC 1337 devin-idle=true fires", () => {
    const term = createMockTerminal();
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // First detect Devin via OSC 0 title (sets cli without changing status)
    act(() => {
      term._fireOsc(0, "Devin - working");
    });
    expect(result.current.cli).toBe("devin");

    // Get to working via output (now that CLI is detected)
    act(() => {
      notifyAgentOutput("s1");
    });
    // Fast-forward past debounce
    act(() => {
      vi.advanceTimersByTime(600);
    });
    expect(result.current.status).toBe("working");

    // Now fire done signal
    act(() => {
      term._fireOsc(1337, "devin-idle=true");
    });
    expect(result.current.status).toBe("done");
  });

  it("decays done → idle after 5s via tick timer", () => {
    const term = createMockTerminal();
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // Get to done
    act(() => {
      term._fireOsc(1337, "devin-idle=true");
    });
    expect(result.current.status).toBe("done");

    // Fast-forward 5s + a bit
    act(() => {
      vi.advanceTimersByTime(5100);
    });
    expect(result.current.status).toBe("idle");
  });

  it("does NOT transition blocked → working on notifyAgentOutput (spinner)", () => {
    const term = createMockTerminal();
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // Get to blocked — "Devin needs input" is a blocked notification
    act(() => {
      term._fireOsc(777, "notify;Devin;Devin needs input");
    });
    expect(result.current.status).toBe("blocked");

    // PTY output during blocked (spinner animation) should NOT clear blocked
    act(() => {
      notifyAgentOutput("s1");
    });
    expect(result.current.status).toBe("blocked");
  });

  it("registers OSC 0, 9, 777, and 1337 handlers on mount", () => {
    const term = createMockTerminal();
    renderHook(() => useAgentStatus(term as any, "s1"));
    expect(term._oscHandlerCount()).toBe(4);
  });

  it("cleans up OSC handlers on unmount", () => {
    const term = createMockTerminal();
    const { unmount } = renderHook(() => useAgentStatus(term as any, "s1"));
    expect(term._oscHandlerCount()).toBe(4);

    unmount();
    expect(term._oscHandlerCount()).toBe(0);
  });

  it("handles null terminal gracefully", () => {
    const { result } = renderHook(() => useAgentStatus(null, "s1"));
    expect(result.current.status).toBe("unknown");
  });

  it("ignores OSC 777 with non-Devin title", () => {
    const term = createMockTerminal();
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    act(() => {
      term._fireOsc(777, "notify;OtherApp;Hello");
    });
    expect(result.current.status).toBe("unknown");
  });

  it("ignores devin-idle=false (informational only)", () => {
    const term = createMockTerminal();
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    act(() => {
      term._fireOsc(1337, "devin-idle=false");
    });
    expect(result.current.status).toBe("unknown");
  });

  it("transitions to done when OSC 777 'Devin finished' fires", () => {
    const term = createMockTerminal();
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // First detect Devin via OSC 0 title
    act(() => {
      term._fireOsc(0, "Devin - working");
    });
    expect(result.current.cli).toBe("devin");

    // Get to working
    act(() => {
      notifyAgentOutput("s1");
    });
    act(() => {
      vi.advanceTimersByTime(600);
    });
    expect(result.current.status).toBe("working");

    // Fire "Devin finished" notification
    act(() => {
      term._fireOsc(777, "notify;Devin;Devin finished");
    });
    expect(result.current.status).toBe("done");
  });

  it("transitions to done when OSC 9 'Devin finished' fires", () => {
    const term = createMockTerminal();
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // First detect Devin
    act(() => {
      term._fireOsc(0, "Devin - working");
    });

    // Get to working
    act(() => {
      notifyAgentOutput("s1");
    });
    act(() => {
      vi.advanceTimersByTime(600);
    });
    expect(result.current.status).toBe("working");

    // Fire OSC 9 "Devin finished"
    act(() => {
      term._fireOsc(9, "Devin finished");
    });
    expect(result.current.status).toBe("done");
  });

  it("transitions to blocked when OSC 9 'Devin needs input' fires", () => {
    const term = createMockTerminal();
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    act(() => {
      term._fireOsc(9, "Devin needs input");
    });
    expect(result.current.status).toBe("blocked");
    expect(result.current.cli).toBe("devin");
    expect(result.current.blockedMessage).toBe("Devin needs input");
  });

  it("transitions working → done after 60s of no output (fallback timeout)", () => {
    const term = createMockTerminal();
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // Detect Devin
    act(() => {
      term._fireOsc(0, "Devin - working");
    });

    // Get to working
    act(() => {
      notifyAgentOutput("s1");
    });
    act(() => {
      vi.advanceTimersByTime(600);
    });
    expect(result.current.status).toBe("working");

    // Advance to just past 60s timeout (600ms + 59400ms = 60000ms)
    // working→done fires at 60000ms (60000 - 0 = 60000 >= 60000)
    act(() => {
      vi.advanceTimersByTime(59400);
    });
    expect(result.current.status).toBe("done");
  });

  it("resetAgentStatus resets to unknown from blocked state", () => {
    const term = createMockTerminal();
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // Get to blocked
    act(() => {
      term._fireOsc(777, "notify;Devin;Devin needs input");
    });
    expect(result.current.status).toBe("blocked");
    expect(result.current.cli).toBe("devin");
    expect(result.current.blockedMessage).toBe("Devin needs input");

    // Reset (simulates terminal close)
    act(() => {
      resetAgentStatus("s1");
    });
    expect(result.current.status).toBe("unknown");
    expect(result.current.cli).toBe("unknown");
    expect(result.current.blockedMessage).toBeNull();
  });

  it("resetAgentStatus resets to unknown from working state", () => {
    const term = createMockTerminal();
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // Detect Devin and get to working
    act(() => {
      term._fireOsc(0, "Devin - working");
    });
    act(() => {
      notifyAgentOutput("s1");
    });
    act(() => {
      vi.advanceTimersByTime(600);
    });
    expect(result.current.status).toBe("working");

    // Reset
    act(() => {
      resetAgentStatus("s1");
    });
    expect(result.current.status).toBe("unknown");
    expect(result.current.cli).toBe("unknown");
  });

  it("resetAgentStatus is a no-op for unknown session", () => {
    // Should not throw
    act(() => {
      resetAgentStatus("nonexistent-session");
    });
  });

  it("BEL handler resets lastOutputAt when CLI detected and working", () => {
    const term = createMockTerminal();
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // Detect Devin and get to working
    act(() => {
      term._fireOsc(0, "Devin - working");
    });
    act(() => {
      notifyAgentOutput("s1");
    });
    act(() => {
      vi.advanceTimersByTime(600);
    });
    expect(result.current.status).toBe("working");

    // Fire BEL — should reset lastOutputAt, preventing premature working→done timeout
    act(() => {
      term._fireBell();
    });

    // Advance 2s — not enough for 60s timeout (BEL reset the timer)
    act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(result.current.status).toBe("working");
  });

  it("BEL handler is a no-op when CLI is unknown", () => {
    const term = createMockTerminal();
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    expect(result.current.status).toBe("unknown");
    expect(result.current.cli).toBe("unknown");

    // Fire BEL — should have no effect
    act(() => {
      term._fireBell();
    });
    expect(result.current.status).toBe("unknown");
  });

  it("BEL handler is a no-op when status is not working", () => {
    const term = createMockTerminal();
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // Get to blocked
    act(() => {
      term._fireOsc(777, "notify;Devin;Devin needs input");
    });
    expect(result.current.status).toBe("blocked");

    // Fire BEL — should not change blocked state
    act(() => {
      term._fireBell();
    });
    expect(result.current.status).toBe("blocked");
  });

  it("OSC-set blocked is NOT overridden by screen scrape (blockedFromOsc)", () => {
    // Devin sends OSC 777 "Devin needs input" → blocked.
    // Screen scrape runs on next tick but Devin's screen patterns only
    // cover permission dialogs, not "needs input" — so screenStatus is null.
    // Without blockedFromOsc, the tick would falsely transition blocked→working.
    // With blockedFromOsc, the blocked state is preserved.
    const term = createMockTerminal(["some Devin output", "no blocked pattern here"]);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // OSC 777 sets blocked (blockedFromOsc = true)
    act(() => {
      term._fireOsc(777, "notify;Devin;Devin needs input");
    });
    expect(result.current.status).toBe("blocked");

    // Advance tick — screen scrape runs but should NOT clear blocked
    act(() => {
      vi.advanceTimersByTime(600);
    });
    expect(result.current.status).toBe("blocked");
    expect(result.current.blockedMessage).toBe("Devin needs input");
  });

  it("screen-set blocked IS cleared by screen scrape (no blockedFromOsc)", () => {
    // OpenCode permission dialog detected by screen scrape → blocked.
    // When the dialog disappears, screen scrape should clear blocked → working.
    // This is the normal flow for screen-scraping-based CLIs.
    const term = createMockTerminal([
      "△ Permission required",
      "bash rm -rf /",
    ]);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // Tick detects OpenCode + blocked from screen
    act(() => {
      vi.advanceTimersByTime(600);
    });
    expect(result.current.cli).toBe("opencode");
    expect(result.current.status).toBe("blocked");

    // Screen changes — dialog dismissed
    act(() => {
      term._setScreenLines(["  ⠋ Read src/main.ts", "esc interrupt  ctrl+p commands"]);
    });
    act(() => {
      vi.advanceTimersByTime(600);
    });
    // Screen now shows working (spinner) — blocked should be cleared
    // Working is a definitive signal, so it clears blocked immediately (1 tick)
    expect(result.current.status).toBe("working");
  });

  it("blocked persists during alt-screen redraw gaps (idle flicker)", () => {
    // OpenCode's permission dialog flickers: the △ Permission required pattern
    // appears in one frame but is overwritten by the next redraw. The idle
    // footer (ctrl+p commands) is always visible, so detectStatus returns idle
    // during the gap. blockedMissCount should keep blocked status stable
    // until N consecutive idle detections confirm the dialog is truly gone.
    const term = createMockTerminal([
      "△ Permission required",
      "bash rm -rf /",
      "esc interrupt  ctrl+p commands",
    ]);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // Tick detects OpenCode + blocked from screen
    act(() => { vi.advanceTimersByTime(600); });
    expect(result.current.cli).toBe("opencode");
    expect(result.current.status).toBe("blocked");

    // Screen redraws — △ disappears but idle footer remains.
    // This is a redraw gap, not a real idle — blocked should persist.
    // Note: no "esc interrupt" here — that would indicate working, not idle.
    term._setScreenLines([
      "  ┃  Some AI output text",
      "  ┃  More output",
      "   /path  ctrl+p commands  • OpenCode 1.18.11",
    ]);
    // Tick 1: idle detected, missCount=1 < 3 → stay blocked
    act(() => { vi.advanceTimersByTime(600); });
    expect(result.current.status).toBe("blocked");

    // Tick 2: idle detected again, missCount=2 < 3 → stay blocked
    act(() => { vi.advanceTimersByTime(600); });
    expect(result.current.status).toBe("blocked");

    // △ reappears briefly (next permission dialog frame) → blocked confirmed
    term._setScreenLines([
      "△ Permission required",
      "← Access external directory ~/.config/opencode",
      "esc interrupt  ctrl+p commands",
    ]);
    act(() => { vi.advanceTimersByTime(600); });
    expect(result.current.status).toBe("blocked");
    // missCount should be reset when blocked is re-detected

    // Screen changes to working (spinner) — definitive signal, clears blocked
    term._setScreenLines(["  ⠋ Thinking", "esc interrupt  ctrl+p commands"]);
    act(() => { vi.advanceTimersByTime(600); });
    expect(result.current.status).toBe("working");
  });

  it("blocked clears after N consecutive idle detections (dialog truly dismissed)", () => {
    // When the permission dialog is truly gone (user clicked Allow in the TUI),
    // the screen shows idle footer without △. After BLOCKED_MISS_THRESHOLD
    // consecutive idle detections, blocked should clear to idle.
    const term = createMockTerminal([
      "△ Permission required",
      "bash rm -rf /",
      "esc interrupt  ctrl+p commands",
    ]);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    act(() => { vi.advanceTimersByTime(600); });
    expect(result.current.status).toBe("blocked");

    // Screen shows idle (no △, no esc interrupt, just footer) — dialog dismissed
    term._setScreenLines([
      "  ┃  AI output text",
      "   /path  ctrl+p commands  • OpenCode 1.18.11",
    ]);

    // 2 ticks: missCount=2 < 3 → still blocked
    act(() => { vi.advanceTimersByTime(1100); });
    expect(result.current.status).toBe("blocked");

    // 3rd tick: missCount=3 ≥ 3 → clear to idle
    act(() => { vi.advanceTimersByTime(600); });
    expect(result.current.status).toBe("idle");
  });

  it("blocked clears immediately when working spinner appears (definitive signal)", () => {
    // When the screen shows a working spinner while blocked, the CLI has
    // clearly resumed. No need to wait for missCount threshold.
    const term = createMockTerminal([
      "△ Permission required",
      "bash rm -rf /",
      "esc interrupt  ctrl+p commands",
    ]);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    act(() => { vi.advanceTimersByTime(600); });
    expect(result.current.status).toBe("blocked");

    // Screen shows working spinner — definitive signal
    term._setScreenLines(["  ⠋ Read src/main.ts", "esc interrupt  ctrl+p commands"]);
    act(() => { vi.advanceTimersByTime(600); });
    // Should clear blocked immediately (1 tick, not 3)
    expect(result.current.status).toBe("working");
  });

  it("detects otherExpanded in multiQuestion (single-select) mode with └ editing", () => {
    // Real-world screen: Devin multi-question dialog, user pressed 'e' on
    // "Other (type your own)" and typed "333". The "└ 333" line indicates
    // text editing mode. ←→ should move text cursor, not switch tabs.
    // otherExpanded must be true so prev/next buttons send Up first.
    const term = createMockTerminal([
      "── 工作模式 ✓ · 代码风格 ✓ · 提交粒度 · 测试要求 ──",
      "  代码风格上你更偏向哪种？",
      "  · 紧凑简洁",
      "      代码尽量紧凑，合并重复分支，减少嵌套",
      "  · 清晰可读优先",
      "      保留适度空行和注释，便于阅读",
      "  ❭ Other (type your own)",
      "    └ 333",
      "─────────────────────────────────────────────────────────────────",
      "↑↓ navigate · ↵ select · e select+type · ←→ switch question · ? help me out · esc cancel",
      "? Not ready to answer, help me out!",
    ]);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // Detect Devin via OSC 0
    act(() => {
      term._fireOsc(0, "Devin - working");
    });

    // Get to blocked via OSC 777
    act(() => {
      term._fireOsc(777, "notify;Devin;Devin needs input");
    });
    expect(result.current.status).toBe("blocked");

    // Advance tick to trigger screen scrape
    act(() => {
      vi.advanceTimersByTime(600);
    });

    // otherExpanded should be true because "└ 333" is present
    // This is multiQuestion=true, multiSelect=false
    expect(result.current.isMultiQuestion).toBe(true);
    expect(result.current.isMultiSelect).toBe(false);
    expect(result.current.otherExpanded).toBe(true);
  });

  it("does not set otherExpanded when └ is not present", () => {
    const term = createMockTerminal([
      "── 工作模式 ✓ · 代码风格 · 提交粒度 · 测试要求 ──",
      "  代码风格上你更偏向哪种？",
      "  ❭ 1 紧凑简洁",
      "      代码尽量紧凑，合并重复分支，减少嵌套",
      "  · 2 清晰可读优先",
      "      保留适度空行和注释，便于阅读",
      "  ·   Other (type your own)",
      "─────────────────────────────────────────────────────────────────",
      "↑↓ navigate · ↵ select · e select+type · ←→ switch question · ? help me out · esc cancel",
    ]);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    act(() => {
      term._fireOsc(0, "Devin - working");
    });
    act(() => {
      term._fireOsc(777, "notify;Devin;Devin needs input");
    });
    act(() => {
      vi.advanceTimersByTime(600);
    });

    expect(result.current.isMultiQuestion).toBe(true);
    expect(result.current.otherExpanded).toBe(false);
  });

  it("does not set otherExpanded when ❭ moved away from Other but └ text persists", () => {
    // Regression: user typed text into Other, then moved cursor to another option.
    // The "└ 1" line persists below Other showing previously typed text,
    // but ❭ is NOT on Other — so otherExpanded must be false.
    // If true, prev/next buttons would send Up arrow, moving the cursor
    // instead of switching tabs.
    const term = createMockTerminal([
      "──────────────",
      "  你希望使用哪种编程语言作为主要开发语言？",
      "  · 1 Rust",
      "      系统级语言，性能高",
      "  · 2 TypeScript",
      "      前端主流语言",
      "  ❭ 3 Python",
      "      脚本语言，开发快速",
      "  · 4 Other (type your own)",
      "      └ 1",
      "─────────────────────────────────────────────────────────────────",
      "↑↓ navigate · ↵ select · e select+type · ←→ switch question · ? help me out · esc cancel",
      "? Not ready to answer, help me out!",
    ]);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    act(() => {
      term._fireOsc(0, "Devin - working");
    });
    act(() => {
      term._fireOsc(777, "notify;Devin;Devin needs input");
    });
    act(() => {
      vi.advanceTimersByTime(600);
    });

    expect(result.current.isMultiQuestion).toBe(true);
    expect(result.current.otherExpanded).toBe(false);
  });
});

// ── OSC 0 title detection tests ──────────────────────────────────────────────

describe("useAgentStatus — OSC 0 CLI detection", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("detects OpenCode from OSC 0 title", () => {
    const term = createMockTerminal();
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    act(() => {
      term._fireOsc(0, "OpenCode");
    });
    expect(result.current.cli).toBe("opencode");
  });

  it("detects OpenCode from OC | task title", () => {
    const term = createMockTerminal();
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    act(() => {
      term._fireOsc(0, "OC | Create file");
    });
    expect(result.current.cli).toBe("opencode");
  });

  it("detects Codex from Action Required title", () => {
    const term = createMockTerminal();
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    act(() => {
      term._fireOsc(0, "Action Required");
    });
    expect(result.current.cli).toBe("codex");
    // Action Required title should also set blocked status
    expect(result.current.status).toBe("blocked");
  });

  it("detects Claude Code from title", () => {
    const term = createMockTerminal();
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    act(() => {
      term._fireOsc(0, "claude code - thinking");
    });
    expect(result.current.cli).toBe("claude-code");
  });

  it("detects Devin from title", () => {
    const term = createMockTerminal();
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    act(() => {
      term._fireOsc(0, "Devin - working");
    });
    expect(result.current.cli).toBe("devin");
  });

  it("does not change CLI type on non-CLI title", () => {
    const term = createMockTerminal();
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    act(() => {
      term._fireOsc(0, "bash");
    });
    expect(result.current.cli).toBe("unknown");
  });

  it("CLI type is sticky — does not un-detect on empty title", () => {
    const term = createMockTerminal();
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    act(() => {
      term._fireOsc(0, "OpenCode");
    });
    expect(result.current.cli).toBe("opencode");

    // Empty title should not un-detect
    act(() => {
      term._fireOsc(0, "");
    });
    expect(result.current.cli).toBe("opencode");
  });
});

// ── Screen scraping tests ────────────────────────────────────────────────────

describe("useAgentStatus — screen scraping for non-Devin CLIs", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("detects OpenCode working from spinner via tick", () => {
    const screen = [
      "  ⠋ Read src/main.ts",
      "esc interrupt  tab agents  ctrl+p commands",
    ];
    const term = createMockTerminal(screen);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // Advance tick to trigger screen scrape
    act(() => {
      vi.advanceTimersByTime(600);
    });

    expect(result.current.cli).toBe("opencode");
    expect(result.current.status).toBe("working");
  });

  it("detects OpenCode idle from footer (no esc interrupt) via tick", () => {
    const screen = [
      "Some content",
      "/path  ctrl+p commands  • OpenCode 1.18.11",
    ];
    const term = createMockTerminal(screen);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // Advance tick to trigger screen scrape
    act(() => {
      vi.advanceTimersByTime(600);
    });

    expect(result.current.cli).toBe("opencode");
    expect(result.current.status).toBe("idle");
  });

  it("detects OpenCode working from 'esc interrupt' footer via tick", () => {
    const screen = [
      "Some content",
      "esc interrupt  tab agents  ctrl+p commands",
    ];
    const term = createMockTerminal(screen);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    act(() => {
      vi.advanceTimersByTime(600);
    });

    expect(result.current.cli).toBe("opencode");
    expect(result.current.status).toBe("working");
  });

  it("detects OpenCode blocked from Permission required via tick", () => {
    const screen = [
      "△ Permission required",
      "bash rm -rf /",
    ];
    const term = createMockTerminal(screen);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    act(() => {
      vi.advanceTimersByTime(600);
    });

    expect(result.current.cli).toBe("opencode");
    expect(result.current.status).toBe("blocked");
  });

  it("detects Codex blocked from Approve y/n via tick", () => {
    const screen = [
      "Some output",
      "Approve command? (y/n)",
    ];
    const term = createMockTerminal(screen);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    act(() => {
      vi.advanceTimersByTime(600);
    });

    expect(result.current.cli).toBe("codex");
    expect(result.current.status).toBe("blocked");
  });

  it("Devin OSC signals take priority over screen scrape", () => {
    // Even if screen shows "esc interrupt" (OpenCode working pattern),
    // Devin's OSC 1337 done signal should take priority.
    // Note: screen scrape now runs for Devin too, but detectStatus("devin", ...)
    // only matches Devin patterns (not OpenCode's "esc interrupt"), so OSC signals
    // are not overridden by screen scrape.
    const screen = ["esc interrupt  ctrl+p commands"];
    const term = createMockTerminal(screen);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // First, detect as Devin via OSC 0
    act(() => {
      term._fireOsc(0, "Devin - working");
    });
    expect(result.current.cli).toBe("devin");

    // Send OSC 1337 done signal
    act(() => {
      term._fireOsc(1337, "devin-idle=true");
    });
    expect(result.current.status).toBe("done");

    // Advance tick — screen scrape runs for Devin, but "esc interrupt"
    // doesn't match any Devin pattern, so status stays done
    act(() => {
      vi.advanceTimersByTime(600);
    });
    // Status should remain done (Devin screen patterns don't match OpenCode footer)
    expect(result.current.status).toBe("done");
  });

  it("extracts question + options when blocked (OpenCode)", () => {
    const screen = [
      "Some content",
      "△ Permission required",
      "  # Shell command",
      "  Allow once   Allow always   Reject   ctrl+f fullscreen  ⇆ select  enter confirm",
    ];
    const term = createMockTerminal(screen);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    act(() => {
      vi.advanceTimersByTime(600);
    });

    expect(result.current.status).toBe("blocked");
    expect(result.current.cli).toBe("opencode");
    // Question should be extracted from screen
    expect(result.current.question).toContain("Permission required");
    // Options should be the actual OpenCode buttons
    expect(result.current.options).toEqual(["Allow once", "Allow always", "Reject"]);
  });

  it("re-extracts question/options when question changes while still blocked (multi-question)", () => {
    // OpenCode multi-question dialog: Q1 appears, user answers, Q2 appears
    // without leaving "blocked" status. The tick must re-extract question/options.
    const q1Screen = [
      "  ┃  → Asked 3 questions",
      "  ┃   编程语言   测试反馈   下一步   Confirm",
      "  ┃  你最喜欢哪种编程语言？",
      "  ┃  1. Rust",
      "  ┃  2. TypeScript",
      "  ┃  3. Python",
      "  ┃  4. Go",
      "  ┃  5. Type your own answer",
      "  ┃  ⇆ tab  ↑↓ select  enter confirm  esc dismiss",
    ];
    const term = createMockTerminal(q1Screen);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // Tick 1: detect OpenCode + blocked + Q1
    act(() => {
      vi.advanceTimersByTime(600);
    });
    expect(result.current.status).toBe("blocked");
    expect(result.current.cli).toBe("opencode");
    expect(result.current.question).toContain("编程语言");
    expect(result.current.options).toEqual(["1. Rust", "2. TypeScript", "3. Python", "4. Go", "5. Type your own answer"]);

    // Simulate user answering Q1 → OpenCode switches to Q2 (still blocked)
    const q2Screen = [
      "  ┃  → Asked 3 questions",
      "  ┃   编程语言   测试反馈   下一步   Confirm",
      "  ┃  你对这个测试弹窗的感觉如何？",
      "  ┃  1. 很好用",
      "  ┃  2. 一般般",
      "  ┃  3. 有改进空间",
      "  ┃  4. Type your own answer",
      "  ┃  ⇆ tab  ↑↓ select  enter confirm  esc dismiss",
    ];
    term._setScreenLines(q2Screen);

    // Tick 2: still blocked, but question changed → must re-extract
    act(() => {
      vi.advanceTimersByTime(600);
    });
    expect(result.current.status).toBe("blocked");
    expect(result.current.question).toContain("测试弹窗的感觉");
    expect(result.current.options).toEqual(["1. 很好用", "2. 一般般", "3. 有改进空间", "4. Type your own answer"]);
  });

  it("detects Devin permission dialog via screen scrape", () => {
    // Devin's new permission dialog (1-7 options) may not trigger OSC 777,
    // so screen scrape must detect it.
    const screen = [
      "❭ 每隔1秒说一次你好",
      "⏺ Running command",
      "  │ $ for i in $(seq 1 30); do echo hello; sleep 1; done",
      "❭ 1 Yes  (Approve once)",
      "· 2 Yes, allow `seq` commands",
      "· 3 No",
      "↑↓ select · ↵ confirm · esc cancel",
    ];
    const term = createMockTerminal(screen);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // First tick: detect Devin from "Devin CLI" or screen content
    // Since screen doesn't have "Devin CLI", we need OSC 0 to detect
    act(() => {
      term._fireOsc(0, "Devin - working");
    });
    expect(result.current.cli).toBe("devin");

    // Advance tick — screen scrape should detect permission dialog
    act(() => {
      vi.advanceTimersByTime(600);
    });

    expect(result.current.status).toBe("blocked");
    // Question should be extracted
    expect(result.current.question).toBeTruthy();
    // Options should include the 7 items
    expect(result.current.options).toBeTruthy();
    expect(result.current.options!.length).toBeGreaterThanOrEqual(3);
  });

  it("resets to unknown when user exits CLI and returns to shell prompt", () => {
    // Start with Devin screen visible
    const screen = ["Devin CLI v3000.3.27", "❭ some output"];
    const term = createMockTerminal(screen);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // Detect Devin via tick
    act(() => {
      vi.advanceTimersByTime(600);
    });
    expect(result.current.cli).toBe("devin");

    // User exits devin — screen now shows shell prompt
    term._setScreenLines(["terry@mac-mini ssh-proxy %"]);

    // Advance tick — should detect shell prompt and reset to unknown
    act(() => {
      vi.advanceTimersByTime(600);
    });
    expect(result.current.cli).toBe("unknown");
    expect(result.current.status).toBe("unknown");
  });

  it("does NOT reset to unknown when screen has lone >/$/% (not a shell prompt)", () => {
    // Devin is still running — screen has Devin output with a lone % char.
    // The old isShellPrompt would false-positive on this; the new one
    // requires user@host pattern and should NOT reset.
    const screen = ["Devin CLI v3000.3.27", "❭ some output", "%"];
    const term = createMockTerminal(screen);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // Detect Devin via tick
    act(() => {
      vi.advanceTimersByTime(600);
    });
    expect(result.current.cli).toBe("devin");

    // Advance more ticks — should NOT reset (no user@host shell prompt)
    act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(result.current.cli).toBe("devin");
  });

  it("resets to unknown when CLI exits but banner is still on screen", () => {
    // Devin exits — shell prompt appears on last line, but Devin's banner
    // is still visible above. The old code checked detectCli on full screen
    // which would return "devin" from the banner, preventing reset.
    // The new code checks only the last line for shell prompt.
    const screen = ["Devin CLI v3000.3.27", "❭ some old output", "terry@mac-mini ssh-proxy %"];
    const term = createMockTerminal(screen);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // Detect Devin via tick (banner is visible)
    act(() => {
      vi.advanceTimersByTime(600);
    });
    expect(result.current.cli).toBe("devin");

    // Advance tick — last line is shell prompt → should reset to unknown
    // even though Devin banner is still on screen
    act(() => {
      vi.advanceTimersByTime(600);
    });
    expect(result.current.cli).toBe("unknown");
    expect(result.current.status).toBe("unknown");
  });

  it("full Devin lifecycle: detect → idle → working → blocked → answer → working → idle → exit", () => {
    // Phase 1: Devin startup banner with idle placeholder
    // Real screen: ❭ followed by "Ask Devin to build features..."
    const term = createMockTerminal([
      "⣴⣾⣶⡄",
      "⠛⠿⠟⠻⣶⣾⣶⡄  Devin CLI",
      "⣤⣶⣦⣴⠿⢿⠿⠃  v3000.3.27",
      "⠻⢿⠿⠃",
      "────────────────────────────────",
      "❭ Ask Devin to build features, fix bugs, or work on your code",
      "────────────────────────────────",
      "GLM-5.2 High  Press opt+m to switch between available models",
    ]);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // Tick 1: detect Devin from screen, should be idle (placeholder text)
    act(() => { vi.advanceTimersByTime(600); });
    expect(result.current.cli).toBe("devin");
    // Screen shows idle placeholder → status should be idle
    // (not working, even though banner output triggered working briefly)
    expect(result.current.status).toBe("idle");

    // Phase 2: User submits a message — Devin starts working (spinner visible)
    term._setScreenLines([
      "⣴⣾⣶⡄",
      "⠛⠿⠟⠻⣶⣾⣶⡄  Devin CLI",
      "⣤⣶⣦⣴⠿⢿⠿⠃  v3000.3.27",
      "⠻⢿⠿⠃",
      "────────────────────────────────",
      "❭ what is 2+2?",
      "────────────────────────────────",
      "⠈⠉ Thinking · 2s (esc to interrupt)",
    ]);
    // Output received → working
    act(() => { notifyAgentOutput("s1"); });
    act(() => { vi.advanceTimersByTime(1000); });
    expect(result.current.status).toBe("working");

    // Phase 3: Permission dialog appears (blocked)
    term._setScreenLines([
      "❭ 1 Yes  (Approve once)",
      "· 2 No",
      "↑↓ select · ↵ confirm · esc cancel",
    ]);
    act(() => { vi.advanceTimersByTime(600); });
    expect(result.current.status).toBe("blocked");

    // Phase 4: User clicks Yes — dialog disappears, Devin resumes
    term._setScreenLines(["⏺ Running command", "  │ $ echo hello", "hello"]);
    // Need 3 consecutive ticks with non-blocked screen to clear blocked
    // (blockedMissCount threshold prevents flickering during alt-screen redraws)
    act(() => { vi.advanceTimersByTime(1600); });
    // Blocked but screen no longer shows blocked pattern → working
    expect(result.current.status).toBe("working");

    // Phase 5: Devin finishes — screen shows idle placeholder again
    term._setScreenLines([
      "⣴⣾⣶⡄",
      "⠛⠿⠟⠻⣶⣾⣶⡄  Devin CLI",
      "⣤⣶⣦⣴⠿⢿⠿⠃  v3000.3.27",
      "⠻⢿⠿⠃",
      "────────────────────────────────",
      "❭ Ask Devin to build features, fix bugs, or work on your code",
      "────────────────────────────────",
      "GLM-5.2 High  Type @ to mention files and add them as context",
    ]);
    act(() => { vi.advanceTimersByTime(600); });
    // Screen shows idle placeholder → idle
    expect(result.current.status).toBe("idle");

    // Phase 6: User exits Devin — shell prompt appears on last line
    term._setScreenLines(["terry@mac-mini ssh-proxy %"]);
    act(() => { vi.advanceTimersByTime(600); });
    expect(result.current.cli).toBe("unknown");
    expect(result.current.status).toBe("unknown");
  });

  it("OSC 'Devin finished' → done persists 5s even when screen shows idle placeholder", () => {
    // Bug: When Devin finishes, it emits OSC 777/9 "Devin finished" → status="done"
    // (blue dot). The screen immediately shows the idle placeholder "❭ Ask Devin to...".
    // Without the fix, the next tick's screen scrape detects "idle" and immediately
    // overrides "done" → "idle", so the user never sees the blue dot.
    // With the fix, "done" persists for 5s (DONE_TO_IDLE_MS) before decaying to "idle".
    const term = createMockTerminal([
      "⣴⣾⣶⡄",
      "⠛⠿⠟⠻⣶⣾⣶⡄  Devin CLI",
      "⣤⣶⣦⣴⠿⢿⠿⠃  v3000.3.27",
      "⠻⢿⠿⠃",
      "────────────────────────────────",
      "❭ what is 2+2?",
      "────────────────────────────────",
      "⠈⠉ Thinking · 2s (esc to interrupt)",
    ]);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // Detect Devin
    act(() => { term._fireOsc(0, "Devin - working"); });
    expect(result.current.cli).toBe("devin");

    // Get to working (screen shows user message + output, not idle placeholder)
    act(() => { notifyAgentOutput("s1"); });
    act(() => { vi.advanceTimersByTime(600); });
    expect(result.current.status).toBe("working");

    // Devin finishes — OSC 9 "Devin finished" → done (blue dot)
    act(() => { term._fireOsc(9, "Devin finished"); });
    expect(result.current.status).toBe("done");

    // Screen now shows idle placeholder (Devin shows it the moment it finishes)
    term._setScreenLines([
      "⣴⣾⣶⡄",
      "⠛⠿⠟⠻⣶⣾⣶⡄  Devin CLI",
      "⣤⣶⣦⣴⠿⢿⠿⠃  v3000.3.27",
      "⠻⢿⠿⠃",
      "────────────────────────────────",
      "❭ Ask Devin to build features, fix bugs, or work on your code",
      "────────────────────────────────",
    ]);

    // Tick fires — screen scrape detects "idle" but must NOT override "done"
    act(() => { vi.advanceTimersByTime(600); });
    expect(result.current.status).toBe("done"); // still done, not idle!

    // More ticks — still done (within 5s window)
    act(() => { vi.advanceTimersByTime(2000); });
    expect(result.current.status).toBe("done");

    // After 5s total since last output, done decays to idle via tick timer
    act(() => { vi.advanceTimersByTime(3000); }); // total 5600ms since done
    expect(result.current.status).toBe("idle");
  });

  it("OSC 1337 devin-idle=true → done persists 5s even when screen shows idle placeholder", () => {
    // Same bug scenario but with OSC 1337 (legacy done signal)
    const term = createMockTerminal([
      "❭ what is 2+2?",
      "⠈⠉ Thinking · 2s (esc to interrupt)",
    ]);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    act(() => { term._fireOsc(0, "Devin - working"); });
    act(() => { notifyAgentOutput("s1"); });
    act(() => { vi.advanceTimersByTime(600); });
    expect(result.current.status).toBe("working");

    // OSC 1337 done signal
    act(() => { term._fireOsc(1337, "devin-idle=true"); });
    expect(result.current.status).toBe("done");

    // Screen now shows idle placeholder — tick must not override done
    term._setScreenLines(["❭ Ask Devin to build features, fix bugs, or work on your code"]);
    act(() => { vi.advanceTimersByTime(600); });
    expect(result.current.status).toBe("done");

    // After 5s, decays to idle
    act(() => { vi.advanceTimersByTime(5000); });
    expect(result.current.status).toBe("idle");
  });

  it("user typing in Devin input should NOT show spin dot (echo correction)", () => {
    // Bug: When user types in Devin's input box, PTY echoes the chars back.
    // notifyOutput() treats the echo as "AI is working" → schedules debounced
    // "working" → 500ms later, spin dot shows. But Devin isn't working —
    // the user is just typing. The screen shows no Braille spinner.
    // Fix: tick's screen scrape returns null (no spinner) → corrects to idle.
    const term = createMockTerminal([
      "⣴⣾⣶⡄",
      "⠛⠿⠟⠻⣶⣾⣶⡄  Devin CLI",
      "⣤⣶⣦⣴⠿⢿⠿⠃  v3000.3.27",
      "⠻⢿⠿⠃",
      "────────────────────────────────",
      "❭ Ask Devin to build features, fix bugs, or work on your code",
      "────────────────────────────────",
    ]);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // Detect Devin, settle to idle
    act(() => { term._fireOsc(0, "Devin - idle"); });
    act(() => { vi.advanceTimersByTime(600); });
    expect(result.current.status).toBe("idle");

    // User types "what is 2+2?" — screen changes to show user input
    term._setScreenLines([
      "⣴⣾⣶⡄",
      "⠛⠿⠟⠻⣶⣾⣶⡄  Devin CLI",
      "⣤⣶⣦⣴⠿⢿⠿⠃  v3000.3.27",
      "⠻⢿⠿⠃",
      "────────────────────────────────",
      "❭ what is 2+2?",
      "────────────────────────────────",
    ]);

    // Simulate PTY echo: each keystroke produces output → notifyAgentOutput
    act(() => { notifyAgentOutput("s1"); }); // echo of 'w'
    act(() => { notifyAgentOutput("s1"); }); // echo of 'h'
    act(() => { notifyAgentOutput("s1"); }); // echo of 'a'
    act(() => { notifyAgentOutput("s1"); }); // echo of 't'

    // Advance past debounce (500ms) — notifyOutput scheduled "working"
    act(() => { vi.advanceTimersByTime(600); });

    // WITHOUT the fix: status would be "working" (spin dot) ❌
    // WITH the fix: screen shows no spinner → detectStatus returns null
    //   → echo correction transitions to idle ✓
    expect(result.current.status).toBe("idle");

    // Continue typing — still no spinner, still idle
    act(() => { notifyAgentOutput("s1"); });
    act(() => { vi.advanceTimersByTime(600); });
    expect(result.current.status).toBe("idle");
  });

  it("Devin working spinner on screen should show spin dot", () => {
    // When Devin IS working, the screen shows the Braille spinner.
    // detectStatus returns "working" → spin dot shows correctly.
    const term = createMockTerminal([
      "⣴⣾⣶⡄",
      "⠛⠿⠟⠻⣶⣾⣶⡄  Devin CLI",
      "⣤⣶⣦⣴⠿⢿⠿⠃  v3000.3.27",
      "⠻⢿⠿⠃",
      "────────────────────────────────",
      "❭ what is 2+2?",
      "────────────────────────────────",
      "⠈⠉ Thinking · 2s (esc to interrupt)",
    ]);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    // Detect Devin
    act(() => { term._fireOsc(0, "Devin - working"); });
    act(() => { notifyAgentOutput("s1"); });
    act(() => { vi.advanceTimersByTime(600); });

    // Screen shows Braille spinner → working ✓
    expect(result.current.status).toBe("working");

    // Spinner continues — still working
    term._setScreenLines([
      "❭ what is 2+2?",
      "⠈⠉ Thinking · 5s (esc to interrupt)",
    ]);
    act(() => { vi.advanceTimersByTime(600); });
    expect(result.current.status).toBe("working");
  });

  it("Devin working → spinner disappears → idle (echo correction doesn't block real transitions)", () => {
    // When Devin finishes, the spinner disappears and the idle placeholder
    // appears. The echo correction should NOT prevent this transition.
    const term = createMockTerminal([
      "❭ what is 2+2?",
      "⠈⠉ Thinking · 2s (esc to interrupt)",
    ]);
    const { result } = renderHook(() => useAgentStatus(term as any, "s1"));

    act(() => { term._fireOsc(0, "Devin - working"); });
    act(() => { notifyAgentOutput("s1"); });
    act(() => { vi.advanceTimersByTime(600); });
    expect(result.current.status).toBe("working");

    // Devin finishes — spinner gone, idle placeholder appears
    term._setScreenLines([
      "❭ Ask Devin to build features, fix bugs, or work on your code",
    ]);
    act(() => { vi.advanceTimersByTime(600); });
    // detectStatus returns "idle" (not null) → applyScreenStatus("idle")
    // → transitions to idle correctly
    expect(result.current.status).toBe("idle");
  });
});
