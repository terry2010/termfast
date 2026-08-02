// Unit tests for answerSubmitter — per-CLI answer submission strategies
import { describe, it, expect } from "vitest";
import { submitAnswer } from "../answerSubmitter";

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

describe("submitAnswer — OpenCode", () => {
  it("sends Enter for Allow (first button)", () => {
    expect(submitAnswer("opencode", "Allow", 0)).toBe("\r");
  });

  it("sends Tab+Enter for Deny (second button)", () => {
    expect(submitAnswer("opencode", "Deny", 1)).toBe("\t\r");
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
