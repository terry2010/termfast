// Unit tests for cliDetector — CLI type detection from title + screen
import { describe, it, expect } from "vitest";
import { detectCliFromTitle, detectCliFromScreen, detectCli, detectStatus, prepareScreenText } from "../cliDetector";

describe("detectCliFromTitle", () => {
  it("detects OpenCode", () => {
    expect(detectCliFromTitle("OpenCode")).toBe("opencode");
  });

  it("detects OpenCode with task", () => {
    expect(detectCliFromTitle("OC | Create file")).toBe("opencode");
  });

  it("detects Codex Action Required", () => {
    expect(detectCliFromTitle("Action Required")).toBe("codex");
  });

  it("detects Claude Code", () => {
    expect(detectCliFromTitle("claude code")).toBe("claude-code");
  });

  it("detects Devin", () => {
    expect(detectCliFromTitle("Devin - working")).toBe("devin");
  });

  it("detects Devin from lowercase title (devin: workspace)", () => {
    // Devin CLI v3000+ emits lowercase "devin: <workspace>" as OSC 0 title
    expect(detectCliFromTitle("devin: termfast")).toBe("devin");
  });

  it("returns unknown for non-CLI titles", () => {
    expect(detectCliFromTitle("bash")).toBe("unknown");
    expect(detectCliFromTitle("")).toBe("unknown");
  });
});

describe("detectCliFromScreen", () => {
  it("detects Devin from 'Devin CLI' banner", () => {
    const screen = "⠀⣴⣾⣶⡄⠀⠀⠀⠀\n⠀⠛⠿⠟⠻⣶⣾⣶⡄  Devin CLI\n⠀⣤⣶⣦⣴⠿⢿⠿⠃  v3000.3.27\n❭ ";
    expect(detectCliFromScreen(screen)).toBe("devin");
  });

  it("detects OpenCode from footer", () => {
    const screen = "some content\nesc interrupt  tab agents  ctrl+p commands";
    expect(detectCliFromScreen(screen)).toBe("opencode");
  });

  it("detects OpenCode from logo", () => {
    const screen = "█▀▀█ █▀▀█ █▀▀█";
    expect(detectCliFromScreen(screen)).toBe("opencode");
  });

  it("detects OpenCode from permission dialog", () => {
    const screen = "△ Permission required\nbash test.sh";
    expect(detectCliFromScreen(screen)).toBe("opencode");
  });

  it("detects OpenCode from selector dialog footer", () => {
    const screen = "  ┃  1. Rust\n  ┃  2. Python\n  ┃  ⇆ tab  ↑↓ select  enter confirm  esc dismiss";
    expect(detectCliFromScreen(screen)).toBe("opencode");
  });

  it("detects OpenCode from multi-select footer (enter toggle)", () => {
    const screen = "  ┃  1. [ ] 单选\n  ┃  2. [ ] 多选\n  ┃  ⇆ tab  ↑↓ select  enter toggle  esc dismiss";
    expect(detectCliFromScreen(screen)).toBe("opencode");
  });

  it("detects OpenCode from single-select footer (enter submit)", () => {
    const screen = "  ┃  1. 单选\n  ┃  2. 多选\n  ┃  ↑↓ select  enter submit  esc dismiss";
    expect(detectCliFromScreen(screen)).toBe("opencode");
  });

  it("detects Codex from progress spinner", () => {
    const screen = "• Working (0s • esc to interrupt)";
    expect(detectCliFromScreen(screen)).toBe("codex");
  });

  it("detects Codex from codex> prompt", () => {
    const screen = "some text\ncodex>\n";
    expect(detectCliFromScreen(screen)).toBe("codex");
  });

  it("returns unknown for generic shell output", () => {
    expect(detectCliFromScreen("$ ls\nfile.txt")).toBe("unknown");
  });
});

describe("detectCli (combined)", () => {
  it("prefers title detection", () => {
    expect(detectCli("OpenCode", "random screen content")).toBe("opencode");
  });

  it("falls back to screen detection", () => {
    expect(detectCli("", "esc interrupt  ctrl+p commands")).toBe("opencode");
  });

  it("returns unknown when neither matches", () => {
    expect(detectCli("bash", "$ ls")).toBe("unknown");
  });
});

describe("detectStatus", () => {
  it("detects OpenCode working from spinner", () => {
    const screen = prepareScreenText("  ⠋ Read src/main.ts\nesc interrupt  tab agents");
    expect(detectStatus("opencode", screen)).toBe("working");
  });

  it("detects OpenCode working from 'esc interrupt' footer (no spinner)", () => {
    // "esc interrupt" only appears when session status is not "idle" (working).
    // This catches the gap between Thinking spinner frames and text streaming.
    const screen = prepareScreenText("some content\nesc interrupt  tab agents  ctrl+p commands");
    expect(detectStatus("opencode", screen)).toBe("working");
  });

  it("detects OpenCode idle from footer (no esc interrupt)", () => {
    const screen = prepareScreenText("some content\n/path  ctrl+p commands  • OpenCode 1.18.11");
    expect(detectStatus("opencode", screen)).toBe("idle");
  });

  it("detects OpenCode blocked from Permission required", () => {
    const screen = prepareScreenText("△ Permission required\nbash test.sh");
    expect(detectStatus("opencode", screen)).toBe("blocked");
  });

  it("detects OpenCode blocked from selector dialog footer", () => {
    const screen = prepareScreenText(
      "  ┃  What framework?\n  ┃  1. React\n  ┃  2. Vue\n  ┃  ⇆ tab  ↑↓ select  enter confirm  esc dismiss"
    );
    expect(detectStatus("opencode", screen)).toBe("blocked");
  });

  it("detects OpenCode idle from ctrl+p commands", () => {
    const screen = prepareScreenText("ctrl+p commands\nsome content");
    expect(detectStatus("opencode", screen)).toBe("idle");
  });

  it("detects Claude Code blocked from ↑/↓ navigate", () => {
    const screen = prepareScreenText("Select an option\n↑/↓ to navigate");
    expect(detectStatus("claude-code", screen)).toBe("blocked");
  });

  it("detects Claude Code working from spinner", () => {
    const screen = prepareScreenText("✻ Cultivating…");
    expect(detectStatus("claude-code", screen)).toBe("working");
  });

  it("detects Codex blocked from Approve y/n", () => {
    const screen = prepareScreenText("Approve command? (y/n)");
    expect(detectStatus("codex", screen)).toBe("blocked");
  });

  it("returns null for unknown CLI", () => {
    expect(detectStatus("unknown", "anything")).toBeNull();
  });
});
