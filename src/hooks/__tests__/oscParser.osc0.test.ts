// Unit tests for OSC 0 title parsing + updated parseOsc dispatcher
import { describe, it, expect } from "vitest";
import { parseOsc0, parseOsc } from "../oscParser";

describe("parseOsc0", () => {
  it("detects OpenCode title", () => {
    const result = parseOsc0("OpenCode");
    expect(result).toEqual({ kind: "title", cli: "opencode", title: "OpenCode" });
  });

  it("detects OpenCode OC | task title", () => {
    const result = parseOsc0("OC | Create test.txt");
    expect(result).toEqual({ kind: "title", cli: "opencode", title: "OC | Create test.txt" });
  });

  it("detects Codex Action Required title", () => {
    const result = parseOsc0("Action Required");
    expect(result).toEqual({ kind: "title", cli: "codex", title: "Action Required" });
  });

  it("detects Codex in title", () => {
    const result = parseOsc0("codex - working");
    expect(result?.cli).toBe("codex");
  });

  it("detects Claude Code in title", () => {
    const result = parseOsc0("claude code - thinking");
    expect(result?.cli).toBe("claude-code");
  });

  it("detects Devin in title", () => {
    const result = parseOsc0("Devin - working");
    expect(result?.cli).toBe("devin");
  });

  it("detects Devin in lowercase title (devin: workspace)", () => {
    // Devin CLI v3000+ emits lowercase "devin: <workspace>" as OSC 0 title
    const result = parseOsc0("devin: termfast");
    expect(result).toEqual({ kind: "title", cli: "devin", title: "devin: termfast" });
  });

  it("returns null for non-CLI title", () => {
    expect(parseOsc0("bash")).toBeNull();
    expect(parseOsc0("zsh")).toBeNull();
  });

  it("returns null for empty title", () => {
    expect(parseOsc0("")).toBeNull();
    expect(parseOsc0("   ")).toBeNull();
  });
});

describe("parseOsc dispatcher with OSC 0", () => {
  it("dispatches 0 to parseOsc0", () => {
    const result = parseOsc(0, "OpenCode");
    expect(result).toEqual({ kind: "title", cli: "opencode", title: "OpenCode" });
  });

  it("still dispatches 777 correctly", () => {
    const result = parseOsc(777, "notify;Devin;Devin needs input");
    expect(result).toEqual({ kind: "notify", cli: "devin", message: "Devin needs input", done: false });
  });

  it("still dispatches 1337 correctly", () => {
    const result = parseOsc(1337, "devin-idle=true");
    expect(result).toEqual({ kind: "done", cli: "devin" });
  });
});
