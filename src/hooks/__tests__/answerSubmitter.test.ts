// Unit tests for answerSubmitter — per-CLI answer submission strategies
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { submitAnswer, toggleOpenCodeOption, submitOpenCodeMultiSelect, submitOpenCodeTextAnswer, submitOpenCodeConfirm, sendTextAnswerWithDelay, TEXT_ANSWER_DELAY_MS } from "../answerSubmitter";

describe("submitAnswer — Devin", () => {
  it("sends number + Enter for numbered option", () => {
    expect(submitAnswer("devin", "1. Yes", 0)).toBe("1\r");
    expect(submitAnswer("devin", "2. No", 1)).toBe("2\r");
  });

  it("falls back to index+1 when no number prefix", () => {
    expect(submitAnswer("devin", "Yes", 0)).toBe("1\r");
    expect(submitAnswer("devin", "No", 1)).toBe("2\r");
  });
});

describe("submitAnswer — OpenCode permission dialog", () => {
  // Permission dialog cycles through all buttons with Tab to reset focus
  // (mouse hover can change focus), then Tab (index) more times + Enter.
  it("cycles 3 tabs + Enter for Allow once (first button, 3 options)", () => {
    expect(submitAnswer("opencode", "Allow once", 0, 3)).toBe("\t\t\t\r");
  });

  it("cycles 3 tabs + 1 tab + Enter for Allow always (second button, 3 options)", () => {
    expect(submitAnswer("opencode", "Allow always", 1, 3)).toBe("\t\t\t\t\r");
  });

  it("cycles 3 tabs + 2 tabs + Enter for Reject (third button, 3 options)", () => {
    expect(submitAnswer("opencode", "Reject", 2, 3)).toBe("\t\t\t\t\t\r");
  });

  it("sends just Enter for Allow once when optionCount not provided", () => {
    expect(submitAnswer("opencode", "Allow once", 0)).toBe("\r");
  });

  it("sends Tab+Enter for Allow always when optionCount not provided", () => {
    expect(submitAnswer("opencode", "Allow always", 1)).toBe("\t\r");
  });
});

describe("submitAnswer — OpenCode selector dialog (number keys)", () => {
  // OpenCode binds number keys 1-9 to directly select an option.
  // This avoids circular ↑↓ navigation issues in multi-question dialogs.
  it("sends '1' for option 1", () => {
    expect(submitAnswer("opencode", "1. Rust", 0, 5)).toBe("1");
  });

  it("sends '2' for option 2", () => {
    expect(submitAnswer("opencode", "2. TypeScript", 1, 5)).toBe("2");
  });

  it("sends '3' for option 3", () => {
    expect(submitAnswer("opencode", "3. Python", 2, 5)).toBe("3");
  });

  it("sends '5' for option 5 (Type your own answer)", () => {
    expect(submitAnswer("opencode", "5. Type your own answer", 4, 5)).toBe("5");
  });

  it("works without optionCount (number key doesn't need it)", () => {
    expect(submitAnswer("opencode", "1. Rust", 0)).toBe("1");
    expect(submitAnswer("opencode", "3. TypeScript", 2)).toBe("3");
  });
});

describe("toggleOpenCodeOption — multi-select toggle (number keys)", () => {
  it("sends '1' for option 1", () => {
    expect(toggleOpenCodeOption("1. Apple", 5)).toBe("1");
  });

  it("sends '2' for option 2", () => {
    expect(toggleOpenCodeOption("2. Banana", 5)).toBe("2");
  });

  it("sends '3' for option 3", () => {
    expect(toggleOpenCodeOption("3. Orange", 5)).toBe("3");
  });

  it("sends '1' for option 1 (default count)", () => {
    expect(toggleOpenCodeOption("1. Apple")).toBe("1");
  });
});

describe("submitOpenCodeMultiSelect — multi-select submit", () => {
  it("sends Tab+Enter to confirm", () => {
    expect(submitOpenCodeMultiSelect()).toBe("\t\r");
  });
});

describe("submitOpenCodeConfirm — multi-question confirm navigation", () => {
  it("sends just Enter when no options (already on Confirm tab)", () => {
    expect(submitOpenCodeConfirm(false, 3, 4)).toBe("\r");
  });

  it("sends → × 3 + Enter from tab 0 with 4 tabs (3 questions + Confirm)", () => {
    // confirmIndex = 3, currentTab = 0, arrowsNeeded = (3-0+4)%4 = 3
    expect(submitOpenCodeConfirm(true, 0, 4)).toBe("\x1b[C\x1b[C\x1b[C\r");
  });

  it("sends → × 2 + Enter from tab 1 with 4 tabs", () => {
    // confirmIndex = 3, currentTab = 1, arrowsNeeded = (3-1+4)%4 = 2
    expect(submitOpenCodeConfirm(true, 1, 4)).toBe("\x1b[C\x1b[C\r");
  });

  it("sends → × 1 + Enter from tab 2 with 4 tabs", () => {
    // confirmIndex = 3, currentTab = 2, arrowsNeeded = (3-2+4)%4 = 1
    expect(submitOpenCodeConfirm(true, 2, 4)).toBe("\x1b[C\r");
  });

  it("sends → × 0 + Enter from tab 3 (Confirm) with 4 tabs", () => {
    // confirmIndex = 3, currentTab = 3, arrowsNeeded = (3-3+4)%4 = 0
    expect(submitOpenCodeConfirm(true, 3, 4)).toBe("\r");
  });

  it("sends → × 2 + Enter from tab 0 with 3 tabs (2 questions + Confirm)", () => {
    // confirmIndex = 2, currentTab = 0, arrowsNeeded = (2-0+3)%3 = 2
    expect(submitOpenCodeConfirm(true, 0, 3)).toBe("\x1b[C\x1b[C\r");
  });

  it("falls back to tab 0 when activeIndex is -1 (detection failed)", () => {
    // activeIndex = -1 → currentTab = 0, arrowsNeeded = (3-0+4)%4 = 3
    expect(submitOpenCodeConfirm(true, -1, 4)).toBe("\x1b[C\x1b[C\x1b[C\r");
  });

  it("falls back to Tab+Enter when totalTabs is 0 (tab detection failed)", () => {
    expect(submitOpenCodeConfirm(true, 0, 0)).toBe("\t\r");
    expect(submitOpenCodeConfirm(true, -1, 0)).toBe("\t\r");
  });

  it("falls back to Tab+Enter when totalTabs is negative", () => {
    expect(submitOpenCodeConfirm(true, 0, -1)).toBe("\t\r");
  });
});

describe("submitOpenCodeTextAnswer — single-select (default)", () => {
  it("sends number key to enter text mode, then types text + Enter", () => {
    const result = submitOpenCodeTextAnswer("5. Type your own answer", "hello");
    expect(result.navigate).toBe("5");
    expect(result.type).toBe("hello\r");
  });

  it("sends '4' for option 4", () => {
    const result = submitOpenCodeTextAnswer("4. Type your own answer", "custom");
    expect(result.navigate).toBe("4");
    expect(result.type).toBe("custom\r");
  });

  it("handles option without number prefix", () => {
    const result = submitOpenCodeTextAnswer("Type your own answer", "test");
    expect(result.navigate).toBe("");
    expect(result.type).toBe("test\r");
  });
});

describe("submitOpenCodeTextAnswer — multi-select", () => {
  it("sends number key to enter text mode, then types text + Enter", () => {
    const result = submitOpenCodeTextAnswer("5. Type your own answer", "hello", true, 5);
    expect(result.navigate).toBe("5");
    expect(result.type).toBe("hello\r");
  });

  it("sends '4' for option 4 (4 options)", () => {
    const result = submitOpenCodeTextAnswer("4. Type your own answer", "test", true, 4);
    expect(result.navigate).toBe("4");
    expect(result.type).toBe("test\r");
  });

  it("handles option without number prefix in multi-select", () => {
    const result = submitOpenCodeTextAnswer("Type your own answer", "test", true);
    expect(result.navigate).toBe("");
    expect(result.type).toBe("test\r");
  });
});

describe("submitAnswer — Claude Code", () => {
  it("sends Enter for Yes (default)", () => {
    expect(submitAnswer("claude-code", "Yes", 0)).toBe("\r");
  });

  it("sends Down+Enter for No", () => {
    expect(submitAnswer("claude-code", "No", 1)).toBe("\x1b[B\r");
  });

  it("sends Down*index + Enter for selection widget", () => {
    // Index 2 = press Down twice + Enter
    expect(submitAnswer("claude-code", "Option C", 2)).toBe("\x1b[B\x1b[B\r");
  });
});

describe("submitAnswer — Codex", () => {
  it("sends y+Enter for Yes", () => {
    expect(submitAnswer("codex", "Yes (y)", 0)).toBe("y\r");
  });

  it("sends n+Enter for No", () => {
    expect(submitAnswer("codex", "No (n)", 1)).toBe("n\r");
  });

  it("sends Enter for trust prompt", () => {
    expect(submitAnswer("codex", "Yes, I trust this folder", 0)).toBe("\r");
  });
});

describe("submitAnswer — fallback", () => {
  it("sends option text + Enter for unknown CLI", () => {
    expect(submitAnswer("unknown", "Yes", 0)).toBe("Yes\r");
  });
});

describe("sendTextAnswerWithDelay", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("sends navigate immediately and type after delay", () => {
    const sends: Uint8Array[] = [];
    const send = (bytes: Uint8Array) => sends.push(bytes);
    const parts = { navigate: "5", type: "hello\r" };

    sendTextAnswerWithDelay(parts, send);

    // Immediately: only navigate sent
    expect(sends).toHaveLength(1);
    expect(new TextDecoder().decode(sends[0])).toBe("5");

    // Before delay: still only 1 send
    vi.advanceTimersByTime(TEXT_ANSWER_DELAY_MS - 1);
    expect(sends).toHaveLength(1);

    // After delay: type sent
    vi.advanceTimersByTime(1);
    expect(sends).toHaveLength(2);
    expect(new TextDecoder().decode(sends[1])).toBe("hello\r");
  });

  it("does not send navigate if empty, but still sends type after delay", () => {
    const sends: Uint8Array[] = [];
    const send = (bytes: Uint8Array) => sends.push(bytes);
    const parts = { navigate: "", type: "test\r" };

    sendTextAnswerWithDelay(parts, send);

    // No navigate sent
    expect(sends).toHaveLength(0);

    vi.advanceTimersByTime(TEXT_ANSWER_DELAY_MS);
    expect(sends).toHaveLength(1);
    expect(new TextDecoder().decode(sends[0])).toBe("test\r");
  });

  it("cleanup function clears the timeout", () => {
    const sends: Uint8Array[] = [];
    const send = (bytes: Uint8Array) => sends.push(bytes);
    const parts = { navigate: "5", type: "hello\r" };

    const cleanup = sendTextAnswerWithDelay(parts, send);
    expect(sends).toHaveLength(1); // navigate sent immediately

    cleanup(); // clear timeout

    vi.advanceTimersByTime(TEXT_ANSWER_DELAY_MS * 2);
    expect(sends).toHaveLength(1); // type NOT sent (cleanup cleared it)
  });

  it("sends correct bytes for text answer (number key navigate)", () => {
    const sends: Uint8Array[] = [];
    const send = (bytes: Uint8Array) => sends.push(bytes);
    const parts = submitOpenCodeTextAnswer("5. Type your own answer", "hello", true, 5);

    sendTextAnswerWithDelay(parts, send);

    expect(sends).toHaveLength(1);
    expect(new TextDecoder().decode(sends[0])).toBe("5");

    vi.advanceTimersByTime(TEXT_ANSWER_DELAY_MS);
    expect(sends).toHaveLength(2);
    expect(new TextDecoder().decode(sends[1])).toBe("hello\r");
  });
});
