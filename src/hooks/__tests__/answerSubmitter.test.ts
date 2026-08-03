// Unit tests for answerSubmitter — per-CLI answer submission strategies
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { submitAnswer, toggleOpenCodeOption, submitOpenCodeMultiSelect, submitOpenCodeTextAnswer, submitOpenCodeConfirm, sendTextAnswerWithDelay, TEXT_ANSWER_DELAY_MS, TEXT_ANSWER_SUBMIT_DELAY_MS, toggleDevinOption, submitDevinMultiSelect, submitDevinConfirm, submitDevinTextAnswer, submitClaudeCodeConfirm, submitClaudeCodeTextAnswer, toggleClaudeCodeOption, submitClaudeCodeMultiSelect, navigatePrevQuestion, navigateNextQuestion, isClaudeCodePlanModeOption, buildClaudeCodePlanModeNavigate } from "../answerSubmitter";

describe("submitAnswer — Devin", () => {
  it("sends number + Enter for numbered option (permission dialog)", () => {
    expect(submitAnswer("devin", "1. Yes", 0)).toBe("1\r");
    expect(submitAnswer("devin", "2. No", 1)).toBe("2\r");
  });

  it("falls back to index+1 when no number prefix", () => {
    expect(submitAnswer("devin", "Yes", 0)).toBe("1\r");
    expect(submitAnswer("devin", "No", 1)).toBe("2\r");
  });

  it("sends 'e' for Other (type your own) option", () => {
    expect(submitAnswer("devin", "Other (type your own)", 4)).toBe("e");
  });
});

describe("toggleDevinOption — Devin multi-select toggle (relative navigation)", () => {
  it("sends Down + Space when target is below current position", () => {
    // currentPos=0, target=2: Down 2 + Space
    expect(toggleDevinOption("3. Python", 2, 0)).toBe("\x1b[B\x1b[B ");
  });

  it("sends Up + Space when target is above current position", () => {
    // currentPos=3, target=1: Up 2 + Space
    expect(toggleDevinOption("2. tmux", 1, 3)).toBe("\x1b[A\x1b[A ");
  });

  it("sends only Space when already at target", () => {
    // currentPos=2, target=2: just Space
    expect(toggleDevinOption("3. Other", 2, 2)).toBe(" ");
  });

  it("sends Down + Space for first toggle from pos 0", () => {
    // currentPos=0, target=1: Down 1 + Space
    expect(toggleDevinOption("2. TypeScript", 1, 0)).toBe("\x1b[B ");
  });

  it("defaults currentPos to 0 when not provided", () => {
    // No currentPos: defaults to 0, target=2: Down 2 + Space
    expect(toggleDevinOption("3. Other", 2)).toBe("\x1b[B\x1b[B ");
  });

  it("handles sequential toggles correctly (simulating cursor tracking)", () => {
    // Simulate: toggle 1, then 3, then 0 (with 5 options)
    // Toggle 1 from pos 0: Down 1 + Space, cursor -> 1
    expect(toggleDevinOption("2. TS", 1, 0)).toBe("\x1b[B ");
    // Toggle 3 from pos 1: Down 2 + Space, cursor -> 3
    expect(toggleDevinOption("4. Go", 3, 1)).toBe("\x1b[B\x1b[B ");
    // Toggle 0 from pos 3: Up 3 + Space, cursor -> 0
    expect(toggleDevinOption("1. Rust", 0, 3)).toBe("\x1b[A\x1b[A\x1b[A ");
  });
});

describe("submitDevinMultiSelect — Devin multi-select submit", () => {
  it("sends Enter to submit", () => {
    expect(submitDevinMultiSelect()).toBe("\r");
  });
});

describe("submitDevinConfirm — Devin multi-question confirm", () => {
  it("sends just Enter when already on last tab (no options)", () => {
    expect(submitDevinConfirm(false, 2, 3)).toBe("\r");
  });

  it("navigates with → arrows from tab 0 to last tab", () => {
    // 3 tabs, currently on tab 0, need 2 → presses + Enter
    expect(submitDevinConfirm(true, 0, 3)).toBe("\x1b[C\x1b[C\r");
  });

  it("navigates with → arrows from tab 1 to last tab", () => {
    // 3 tabs, currently on tab 1, need 1 → press + Enter
    expect(submitDevinConfirm(true, 1, 3)).toBe("\x1b[C\r");
  });

  it("sends Esc when on last tab (single-select, arrowsNeeded=0)", () => {
    // 3 tabs, currently on tab 2 (last), single-select → Esc (not Enter)
    expect(submitDevinConfirm(true, 2, 3)).toBe("\x1b");
  });

  it("wraps around with modulo when activeIndex > confirmIndex", () => {
    // 4 tabs, currently on tab 3, confirm is tab 3 → 0 arrows
    // Single-select (no isMultiSelect) → Esc
    expect(submitDevinConfirm(true, 3, 4)).toBe("\x1b");
  });

  it("sends Enter when on last tab (multi-select, arrowsNeeded=0)", () => {
    // 3 tabs, currently on tab 2 (last), multi-select → Enter (submit)
    expect(submitDevinConfirm(true, 2, 3, true)).toBe("\r");
  });

  it("sends Enter on the last single-select tab when an earlier answer exists", () => {
    expect(submitDevinConfirm(true, 3, 4, false, true)).toBe("\r");
  });

  it("falls back to Enter when totalTabs is 0", () => {
    expect(submitDevinConfirm(true, -1, 0)).toBe("\r");
  });
});

describe("submitDevinTextAnswer — Devin text answer", () => {
  it("sends number + 'e' for navigate, text + Enter for type (numbered option)", () => {
    const parts = submitDevinTextAnswer("5. Other (type your own)", "custom text");
    expect(parts.navigate).toBe("5e");
    expect(parts.type).toBe("\x15custom text\r");
  });

  it("sends relative Down navigation + 'e' when target is below current pos", () => {
    // "Other (type your own)" at index 4, current pos = 1
    const parts = submitDevinTextAnswer("Other (type your own)", "hello", false, 4, 5, 1);
    // Down 3 times (from pos 1 to pos 4) + 'e'
    expect(parts.navigate).toBe("\x1b[B\x1b[B\x1b[B" + "e");
    expect(parts.type).toBe("\x15hello\r");
  });

  it("sends relative Up navigation + 'e' when target is above current pos", () => {
    // "Other (type your own)" at index 0, current pos = 3
    const parts = submitDevinTextAnswer("Other (type your own)", "hello", false, 0, 5, 3);
    // Up 3 times (from pos 3 to pos 0) + 'e'
    expect(parts.navigate).toBe("\x1b[A\x1b[A\x1b[A" + "e");
    expect(parts.type).toBe("\x15hello\r");
  });

  it("sends only 'e' when already at target position", () => {
    const parts = submitDevinTextAnswer("Other (type your own)", "hello", false, 2, 5, 2);
    expect(parts.navigate).toBe("e");
    expect(parts.type).toBe("\x15hello\r");
  });

  it("defaults to currentPos=0 when not provided", () => {
    const parts = submitDevinTextAnswer("Other (type your own)", "hello", false, 2, 5);
    // Down 2 times (from pos 0 to pos 2) + 'e'
    expect(parts.navigate).toBe("\x1b[B\x1b[B" + "e");
    expect(parts.type).toBe("\x15hello\r");
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

  it("sends number key for numbered option (multi-question dialog)", () => {
    // "3. TypeScript" → send "3" (number key directly selects)
    expect(submitAnswer("claude-code", "3. TypeScript", 2)).toBe("3");
    expect(submitAnswer("claude-code", "1. Rust", 0)).toBe("1");
    expect(submitAnswer("claude-code", "6. Chat about this", 5)).toBe("6");
  });

  it("sends Down*index + Enter for Plan Mode options (no number keys)", () => {
    // Plan Mode dialog does NOT support number-key selection.
    // Option 1 (index 0): just Enter (default selected)
    expect(submitAnswer("claude-code", "1. Yes, and use auto mode", 0)).toBe("\r");
    // Option 2 (index 1): Down + Enter
    expect(submitAnswer("claude-code", "2. Yes, manually approve edits", 1)).toBe("\x1b[B\r");
    // Option 3 (index 2): Down*2 + Enter
    expect(submitAnswer("claude-code", "3. Tell Claude what to change", 2)).toBe("\x1b[B\x1b[B\r");
  });

  it("sends Down*index + Enter for non-numbered selection widget", () => {
    // No number prefix → fallback to arrow navigation
    expect(submitAnswer("claude-code", "Option C", 2)).toBe("\x1b[B\x1b[B\r");
  });
});

describe("submitClaudeCodeTextAnswer", () => {
  it("sends number key + text + single Enter for multi-question 'Type something.'", () => {
    const result = submitClaudeCodeTextAnswer("5. Type something.", "my custom answer");
    expect(result.navigate).toBe("5");
    expect(result.type).toBe("my custom answer");
    expect(result.submit).toBe("\r");
  });

  it("sends number key + text + double Enter for Plan Mode 'Tell Claude what to change'", () => {
    const result = submitClaudeCodeTextAnswer("3. Tell Claude what to change", "please refine the plan");
    expect(result.navigate).toBe("3");
    expect(result.type).toBe("please refine the plan");
    // Two Enters: first submits text into field, second approves the feedback
    expect(result.submit).toBe("\r\r");
  });

  it("defaults to number 5 when no number prefix", () => {
    const result = submitClaudeCodeTextAnswer("Type something.", "test");
    expect(result.navigate).toBe("5");
    expect(result.type).toBe("test");
    expect(result.submit).toBe("\r");
  });

  it("uses Down arrows (no Enter) for multi-select 'Type something' (index 4)", () => {
    const result = submitClaudeCodeTextAnswer("5. Type something", "my text", 4, true);
    // In multi-select, number key only toggles checkbox — need Down arrows
    // to navigate. When focused, TextInput auto-renders — no Enter needed.
    // Submit: Tab + Enter (sent separately with delay by sendTextAnswerWithDelay)
    expect(result.navigate).toBe("\x1b[B\x1b[B\x1b[B\x1b[B");
    expect(result.type).toBe("my text");
    expect(result.submit).toBe("\t\r");
  });

  it("uses empty navigate for multi-select when index is 0 (already focused)", () => {
    const result = submitClaudeCodeTextAnswer("1. Type something", "my text", 0, true);
    // Index 0 means option is already focused — no navigation needed
    expect(result.navigate).toBe("");
    expect(result.type).toBe("my text");
    expect(result.submit).toBe("\t\r");
  });
});

describe("toggleClaudeCodeOption", () => {
  it("sends number key for toggle", () => {
    expect(toggleClaudeCodeOption("1. 选项一")).toBe("1");
    expect(toggleClaudeCodeOption("3. 选项三")).toBe("3");
  });

  it("returns empty string when no number prefix", () => {
    expect(toggleClaudeCodeOption("选项一")).toBe("");
  });
});

describe("submitClaudeCodeMultiSelect", () => {
  it("sends Tab + Enter to navigate to Submit tab and confirm", () => {
    expect(submitClaudeCodeMultiSelect()).toBe("\t\r");
  });
});

describe("isClaudeCodePlanModeOption", () => {
  it("detects 'Yes, and use auto mode'", () => {
    expect(isClaudeCodePlanModeOption("1. Yes, and use auto mode")).toBe(true);
  });

  it("detects 'Yes, manually approve edits'", () => {
    expect(isClaudeCodePlanModeOption("2. Yes, manually approve edits")).toBe(true);
  });

  it("detects 'Tell Claude what to change'", () => {
    expect(isClaudeCodePlanModeOption("3. Tell Claude what to change")).toBe(true);
  });

  it("does not detect multi-question options", () => {
    expect(isClaudeCodePlanModeOption("1. Rust")).toBe(false);
    expect(isClaudeCodePlanModeOption("5. Type something.")).toBe(false);
  });
});

describe("buildClaudeCodePlanModeNavigate", () => {
  it("returns empty string for index 0 (default selected)", () => {
    expect(buildClaudeCodePlanModeNavigate(0)).toBe("");
  });

  it("returns one Down arrow for index 1", () => {
    expect(buildClaudeCodePlanModeNavigate(1)).toBe("\x1b[B");
  });

  it("returns two Down arrows for index 2", () => {
    expect(buildClaudeCodePlanModeNavigate(2)).toBe("\x1b[B\x1b[B");
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

  it("sends navigate, type, and submit as three separate sends for Claude Code", () => {
    const sends: Uint8Array[] = [];
    const send = (bytes: Uint8Array) => sends.push(bytes);
    const parts = submitClaudeCodeTextAnswer("4. Type something.", "my text");

    sendTextAnswerWithDelay(parts, send);

    // Immediately: navigate sent
    expect(sends).toHaveLength(1);
    expect(new TextDecoder().decode(sends[0])).toBe("4");

    // After TEXT_ANSWER_DELAY_MS: type sent (no Enter)
    vi.advanceTimersByTime(TEXT_ANSWER_DELAY_MS);
    expect(sends).toHaveLength(2);
    expect(new TextDecoder().decode(sends[1])).toBe("my text");

    // Before submit delay: still 2 sends
    vi.advanceTimersByTime(TEXT_ANSWER_SUBMIT_DELAY_MS - 1);
    expect(sends).toHaveLength(2);

    // After submit delay: Enter sent
    vi.advanceTimersByTime(1);
    expect(sends).toHaveLength(3);
    expect(new TextDecoder().decode(sends[2])).toBe("\r");
  });

  it("sends double Enter for Plan Mode feedback (submit \\r\\r sent char-by-char)", () => {
    const sends: Uint8Array[] = [];
    const send = (bytes: Uint8Array) => sends.push(bytes);
    const parts = submitClaudeCodeTextAnswer("3. Tell Claude what to change", "feedback");

    sendTextAnswerWithDelay(parts, send);

    expect(sends).toHaveLength(1);
    expect(new TextDecoder().decode(sends[0])).toBe("3");

    vi.advanceTimersByTime(TEXT_ANSWER_DELAY_MS);
    expect(sends).toHaveLength(2);
    expect(new TextDecoder().decode(sends[1])).toBe("feedback");

    // First Enter after submit delay
    vi.advanceTimersByTime(TEXT_ANSWER_SUBMIT_DELAY_MS);
    expect(sends).toHaveLength(3);
    expect(new TextDecoder().decode(sends[2])).toBe("\r");

    // Second Enter after another submit delay
    vi.advanceTimersByTime(TEXT_ANSWER_SUBMIT_DELAY_MS);
    expect(sends).toHaveLength(4);
    expect(new TextDecoder().decode(sends[3])).toBe("\r");
  });

  it("sends Tab and Enter separately for multi-select submit (\\t\\r)", () => {
    const sends: Uint8Array[] = [];
    const send = (bytes: Uint8Array) => sends.push(bytes);
    const parts = submitClaudeCodeTextAnswer("5. Type something", "my text", 4, true);

    sendTextAnswerWithDelay(parts, send);

    // Immediately: Down arrows sent (no Enter)
    expect(sends).toHaveLength(1);
    expect(new TextDecoder().decode(sends[0])).toBe("\x1b[B\x1b[B\x1b[B\x1b[B");

    // After delay: text sent
    vi.advanceTimersByTime(TEXT_ANSWER_DELAY_MS);
    expect(sends).toHaveLength(2);
    expect(new TextDecoder().decode(sends[1])).toBe("my text");

    // After submit delay: Tab sent (first char of submit)
    vi.advanceTimersByTime(TEXT_ANSWER_SUBMIT_DELAY_MS);
    expect(sends).toHaveLength(3);
    expect(new TextDecoder().decode(sends[2])).toBe("\t");

    // After another submit delay: Enter sent (second char of submit)
    vi.advanceTimersByTime(TEXT_ANSWER_SUBMIT_DELAY_MS);
    expect(sends).toHaveLength(4);
    expect(new TextDecoder().decode(sends[3])).toBe("\r");
  });

  it("cleanup function clears all timeouts (navigate + type + submit)", () => {
    const sends: Uint8Array[] = [];
    const send = (bytes: Uint8Array) => sends.push(bytes);
    const parts = submitClaudeCodeTextAnswer("4. Type something.", "my text");

    const cleanup = sendTextAnswerWithDelay(parts, send);
    expect(sends).toHaveLength(1); // navigate sent immediately

    cleanup(); // clear all timeouts

    vi.advanceTimersByTime(TEXT_ANSWER_DELAY_MS + TEXT_ANSWER_SUBMIT_DELAY_MS + 100);
    expect(sends).toHaveLength(1); // neither type nor submit sent
  });
});

// navigatePrevQuestion / navigateNextQuestion tests

describe("navigatePrevQuestion - multi-question tab navigation", () => {
  it("sends only Left arrow for Devin", () => {
    expect(navigatePrevQuestion("devin")).toBe("\x1b[D");
  });

  it("sends only Left arrow for OpenCode", () => {
    expect(navigatePrevQuestion("opencode")).toBe("\x1b[D");
  });

  it("sends only Left arrow for other CLIs", () => {
    expect(navigatePrevQuestion("claude-code")).toBe("\x1b[D");
    expect(navigatePrevQuestion("codex")).toBe("\x1b[D");
    expect(navigatePrevQuestion("unknown")).toBe("\x1b[D");
  });
});

describe("navigateNextQuestion - multi-question tab navigation", () => {
  it("sends only Right arrow for Devin", () => {
    expect(navigateNextQuestion("devin")).toBe("\x1b[C");
  });

  it("sends only Right arrow for OpenCode", () => {
    expect(navigateNextQuestion("opencode")).toBe("\x1b[C");
  });

  it("sends only Right arrow for other CLIs", () => {
    expect(navigateNextQuestion("claude-code")).toBe("\x1b[C");
    expect(navigateNextQuestion("codex")).toBe("\x1b[C");
    expect(navigateNextQuestion("unknown")).toBe("\x1b[C");
  });
});
