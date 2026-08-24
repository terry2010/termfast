// Unit tests for agentPatterns — question + option extraction
import { describe, it, expect } from "vitest";
import { extractQuestion, extractOptions, detectStatusFromScreen, detectMultiSelect, detectMultiQuestion, extractReviewAnswers, extractCursorIndex, stripAnsi } from "../agentPatterns";

describe("stripAnsi", () => {
  it("removes SGR codes", () => {
    expect(stripAnsi("\x1b[38;2;255;255;255mHello\x1b[0m")).toBe("Hello");
  });

  it("removes CSI codes", () => {
    expect(stripAnsi("\x1b[2;1HHello")).toBe("Hello");
  });

  it("removes OSC sequences", () => {
    expect(stripAnsi("\x1b]0;Title\x07Hello")).toBe("Hello");
  });

  it("handles plain text", () => {
    expect(stripAnsi("Hello World")).toBe("Hello World");
  });
});

describe("extractQuestion — Devin", () => {
  it("extracts question ending with ?", () => {
    const screen = "Some output\nDo you want to continue?\n1. Yes\n2. No\n↑↓ select · ↵ confirm · esc cancel";
    expect(extractQuestion("devin", screen)).toBe("Do you want to continue?");
  });

  it("extracts action from ⏺ line in file-write permission dialog", () => {
    const screen = [
      "❭ create a test file in /tmp",
      " ⏺ Writing /tmp/test.txt",
      " └ 1 +  This is a test file.",
      "❭ 1 Yes  (Approve once)",
      "· 2 Yes, allow edits in /private/tmp",
      "· 3 Yes, always allow edits in /private/tmp",
      "· 4 No",
      "↑↓ select · ↵ confirm · esc cancel",
    ].join("\n");
    expect(extractQuestion("devin", screen)).toBe("Approve: Writing /tmp/test.txt?");
  });

  it("returns null when no question found", () => {
    expect(extractQuestion("devin", "just some output")).toBeNull();
  });
});

describe("extractOptions — Devin", () => {
  it("extracts numbered options (old format with dot)", () => {
    const screen = "Do you want to continue?\n1. Yes\n2. No\n3. Maybe\n↑↓ select · ↵ confirm · esc cancel";
    const options = extractOptions("devin", screen);
    expect(options).toEqual(["1. Yes", "2. No", "3. Maybe"]);
  });

  it("extracts numbered options (new permission dialog format)", () => {
    const screen = [
      "❭ 1 Yes  (Approve once)",
      "· 2 Yes, allow `seq` commands",
      "· 3 Yes, always allow `seq` commands in `termfast`",
      "· 4 Yes, always allow `seq` commands in all projects",
      "· 5 Edit command",
      "· 6 Describe change to command",
      "· 7 No",
      "↑↓ select · ↵ confirm · esc cancel",
    ].join("\n");
    const options = extractOptions("devin", screen);
    expect(options).toEqual([
      "1. Yes  (Approve once)",
      "2. Yes, allow `seq` commands",
      "3. Yes, always allow `seq` commands in `termfast`",
      "4. Yes, always allow `seq` commands in all projects",
      "5. Edit command",
      "6. Describe change to command",
      "7. No",
    ]);
  });

  it("returns null when no options found", () => {
    expect(extractOptions("devin", "just text")).toBeNull();
  });
});

describe("extractQuestion — OpenCode", () => {
  it("extracts permission required", () => {
    const screen = "△ Permission required\nbash rm -rf /";
    expect(extractQuestion("opencode", screen)).toBe("Permission required: bash rm -rf /");
  });

  it("returns Permission required when no detail", () => {
    const screen = "△ Permission required\n\n";
    expect(extractQuestion("opencode", screen)).toBe("Permission required");
  });

  it("extracts question text from multi-question dialog with path and description lines", () => {
    // Real screen capture: multi-question dialog with working-dir path line
    // and option description lines (后端/前端/脚本) between options and footer.
    const screen = [
      "  ┃",
      "  ┃   多选   Confirm",
      "  ┃",
      "  ┃  选择你喜欢的语言（可多选） (select all that apply)",
      "  ┃",
      "  ┃  1. [ ] Go",
      "  ┃     后端",
      "  ┃  2. [ ] TypeScript",
      "  ┃     前端",
      "  ┃  3. [ ] Python",
      "  ┃     脚本/后端",
      "  ┃  4. [ ] Type your own answer",
      "  ┃                                                                                              /Volumes/2t/code/termfast-all",
      "  ┃  ⇆ tab  ↑↓ select  enter toggle  esc dismiss",
      "  ┃                                                                                              • OpenCode 1.18.21",
    ].join("\n");
    expect(extractQuestion("opencode", screen)).toBe("选择你喜欢的语言（可多选） (select all that apply)");
  });

  it("extracts Always allow sub-state with patterns description", () => {
    // Sub-state after user clicks "Allow always" — shows patterns to confirm.
    const screen = [
      "  △ Always allow",
      "  This will allow the following patterns until OpenCode is restarted",
      "  - bash npm *",
      "  - bash npx *",
      "    Confirm   Cancel",
      "  ⇆ select  enter confirm",
    ].join("\n");
    expect(extractQuestion("opencode", screen)).toBe("Always allow: This will allow the following patterns until OpenCode is restarted");
  });

  it("extracts Reject permission sub-state with description", () => {
    // Sub-state after user clicks "Reject" (session has parentID).
    const screen = [
      "  △ Reject permission",
      "  Tell OpenCode what to do differently",
      "  [textarea]",
      "  enter confirm  esc cancel",
    ].join("\n");
    expect(extractQuestion("opencode", screen)).toBe("Reject permission: Tell OpenCode what to do differently");
  });
});

describe("extractOptions — OpenCode", () => {
  it("returns Allow once/Allow always/Reject for permission", () => {
    const screen = "△ Permission required\n  # Shell command\n  Allow once   Allow always   Reject   ctrl+f fullscreen  ⇆ select  enter confirm";
    expect(extractOptions("opencode", screen)).toEqual(["Allow once", "Allow always", "Reject"]);
  });

  it("returns fallback buttons when footer not found", () => {
    const screen = "△ Permission required\nbash test.sh";
    expect(extractOptions("opencode", screen)).toEqual(["Allow once", "Allow always", "Reject"]);
  });

  it("returns null when no permission dialog", () => {
    expect(extractOptions("opencode", "just working")).toBeNull();
  });

  it("returns Confirm/Cancel for Always allow sub-state", () => {
    const screen = [
      "  △ Always allow",
      "  This will allow the following patterns until OpenCode is restarted",
      "  - bash npm *",
      "    Confirm   Cancel",
      "  ⇆ select  enter confirm",
    ].join("\n");
    expect(extractOptions("opencode", screen)).toEqual(["Confirm", "Cancel"]);
  });

  it("returns null for Reject permission sub-state (textarea, no options)", () => {
    const screen = [
      "  △ Reject permission",
      "  Tell OpenCode what to do differently",
      "  [textarea]",
      "  enter confirm  esc cancel",
    ].join("\n");
    expect(extractOptions("opencode", screen)).toBeNull();
  });

  it("extracts numbered options from multi-question dialog with path and description lines", () => {
    // Real screen capture: path line and description lines between options
    // and footer must not break the upward scan.
    const screen = [
      "  ┃",
      "  ┃   多选   Confirm",
      "  ┃",
      "  ┃  选择你喜欢的语言（可多选） (select all that apply)",
      "  ┃",
      "  ┃  1. [ ] Go",
      "  ┃     后端",
      "  ┃  2. [ ] TypeScript",
      "  ┃     前端",
      "  ┃  3. [ ] Python",
      "  ┃     脚本/后端",
      "  ┃  4. [ ] Type your own answer",
      "  ┃                                                                                              /Volumes/2t/code/termfast-all",
      "  ┃  ⇆ tab  ↑↓ select  enter toggle  esc dismiss",
      "  ┃                                                                                              • OpenCode 1.18.21",
    ].join("\n");
    expect(extractOptions("opencode", screen)).toEqual([
      "1. Go",
      "2. TypeScript",
      "3. Python",
      "4. Type your own answer",
    ]);
  });
});

// ── OpenCode question/selector dialog tests ──────────────────────────────────

describe("detectStatusFromScreen — OpenCode permission sub-states", () => {
  it("detects blocked for Always allow sub-state", () => {
    const screen = [
      "  △ Always allow",
      "  This will allow the following patterns until OpenCode is restarted",
      "  - bash npm *",
      "    Confirm   Cancel",
      "  ⇆ select  enter confirm",
    ].join("\n");
    expect(detectStatusFromScreen("opencode", screen)).toBe("blocked");
  });

  it("detects blocked for Reject permission sub-state", () => {
    const screen = [
      "  △ Reject permission",
      "  Tell OpenCode what to do differently",
      "  [textarea]",
      "  enter confirm  esc cancel",
    ].join("\n");
    expect(detectStatusFromScreen("opencode", screen)).toBe("blocked");
  });
});

describe("detectStatusFromScreen — OpenCode selector dialog", () => {
  it("detects blocked for single-select footer (enter confirm)", () => {
    const screen = [
      "  ┃  如果只能用一种语言写一辈子代码，你选哪个？",
      "  ┃  1. Rust",
      "  ┃  2. Python",
      "  ┃  3. TypeScript",
      "  ┃  4. Type your own answer",
      "  ┃  ⇆ tab  ↑↓ select  enter confirm  esc dismiss",
    ].join("\n");
    expect(detectStatusFromScreen("opencode", screen)).toBe("blocked");
  });

  it("detects blocked for single-select footer (enter submit)", () => {
    const screen = [
      "  ┃  你想测试提问弹窗的什么功能？",
      "  ┃  1. 单选",
      "  ┃  2. 多选",
      "  ┃  3. 自定义输入",
      "  ┃  4. Type your own answer",
      "  ┃  ↑↓ select  enter submit  esc dismiss",
    ].join("\n");
    expect(detectStatusFromScreen("opencode", screen)).toBe("blocked");
  });

  it("detects blocked for multi-select footer (enter toggle)", () => {
    const screen = [
      "  ┃  你希望测试哪种类型的提问？ (select all that apply)",
      "  ┃  1. [ ] 单选",
      "  ┃  2. [ ] 多选",
      "  ┃  3. [✓] 自由输入",
      "  ┃  4. [ ] Type your own answer",
      "  ┃  ⇆ tab  ↑↓ select  enter toggle  esc dismiss",
    ].join("\n");
    expect(detectStatusFromScreen("opencode", screen)).toBe("blocked");
  });

  it("does NOT detect blocked for permission footer without esc dismiss", () => {
    // Permission dialog footer: "⇆ select  enter confirm" (no "esc dismiss")
    const screen = [
      "  ┃  △ Permission required",
      "  ┃   Allow once   Allow always   Reject",
      "  ┃  ⇆ select  enter confirm",
    ].join("\n");
    // Should still be blocked, but via the △ pattern (priority 10), not the selector pattern
    expect(detectStatusFromScreen("opencode", screen)).toBe("blocked");
  });

  it("does NOT detect blocked for idle footer with ctrl+p commands", () => {
    const screen = [
      "  ┃  Some AI output",
      "  ┃  esc interrupt  ctrl+p commands",
    ].join("\n");
    expect(detectStatusFromScreen("opencode", screen)).not.toBe("blocked");
  });

  it("detects blocked for multi-question Confirm tab (no ↑↓ select)", () => {
    // Confirm tab footer: "⇆ tab  enter submit  esc dismiss" — no ↑↓ select
    // because there are no options to navigate on the Confirm tab.
    const screen = [
      "  ┃  → Asked 3 questions",
      "  ┃   编程语言   测试反馈   下一步   Confirm",
      "  ┃  Review",
      "  ┃  编程语言: Rust",
      "  ┃  测试反馈: 很好用",
      "  ┃  ⇆ tab  enter submit  esc dismiss",
    ].join("\n");
    expect(detectStatusFromScreen("opencode", screen)).toBe("blocked");
  });
});

describe("detectStatusFromScreen — OpenCode working vs idle", () => {
  it("detects working from braille spinner + Thinking", () => {
    const screen = "  ┃  Some output\n  ⠋ Thinking\n  esc interrupt  ctrl+p commands";
    expect(detectStatusFromScreen("opencode", screen)).toBe("working");
  });

  it("detects working from 'esc interrupt' footer without spinner", () => {
    // When OpenCode is streaming text content (between Thinking steps),
    // there's no braille spinner on screen but "esc interrupt" is still shown.
    const screen = "  ┃  Some AI output text here\n  ⬝⬝⬝⬝⬝⬝⬝⬝  esc interrupt  ctrl+p commands";
    expect(detectStatusFromScreen("opencode", screen)).toBe("working");
  });

  it("detects idle when only ctrl+p commands (no esc interrupt)", () => {
    const screen = "  ┃  Done\n  /path  ctrl+p commands  • OpenCode 1.18.11";
    expect(detectStatusFromScreen("opencode", screen)).toBe("idle");
  });

  it("done pattern (priority 8) overrides esc interrupt (priority 6)", () => {
    const screen = "  ▣  Build · DeepSeek · 3.9s\n  esc interrupt  ctrl+p commands";
    expect(detectStatusFromScreen("opencode", screen)).toBe("done");
  });

  it("blocked pattern (priority 9) overrides esc interrupt (priority 6)", () => {
    const screen = "  △ Permission required\n  esc interrupt  ctrl+p commands";
    expect(detectStatusFromScreen("opencode", screen)).toBe("blocked");
  });
});

describe("extractQuestion — OpenCode selector dialog", () => {
  it("extracts question text above numbered options", () => {
    const screen = [
      "  ┃",
      "  ┃  如果只能用一种语言写一辈子代码，你选哪个？",
      "  ┃",
      "  ┃  1. Rust",
      "  ┃     性能与内存安全兼得，但学习曲线陡峭",
      "  ┃  2. Python",
      "  ┃  3. TypeScript",
      "  ┃  4. Type your own answer",
      "  ┃  ⇆ tab  ↑↓ select  enter confirm  esc dismiss",
    ].join("\n");
    expect(extractQuestion("opencode", screen)).toBe("如果只能用一种语言写一辈子代码，你选哪个？");
  });

  it("extracts question with box-drawing prefix on option lines", () => {
    const screen = [
      "  ┃  What is your preferred framework?",
      "  ┃  1. React",
      "  ┃  2. Vue",
      "  ┃  3. Svelte",
      "  ┃  ⇆ tab  ↑↓ select  enter confirm  esc dismiss",
    ].join("\n");
    expect(extractQuestion("opencode", screen)).toBe("What is your preferred framework?");
  });

  it("returns fallback when no question text found above options", () => {
    const screen = [
      "  ┃  1. Option A",
      "  ┃  2. Option B",
      "  ┃  ⇆ tab  ↑↓ select  enter confirm  esc dismiss",
    ].join("\n");
    expect(extractQuestion("opencode", screen)).toBe("OpenCode is asking a question");
  });

  it("returns null when no selector footer present", () => {
    const screen = "  ┃  Just some output\n  ┃  ctrl+p commands";
    expect(extractQuestion("opencode", screen)).toBeNull();
  });
});

describe("extractOptions — OpenCode selector dialog", () => {
  it("extracts numbered options from selector dialog", () => {
    const screen = [
      "  ┃  如果只能用一种语言写一辈子代码，你选哪个？",
      "  ┃  1. Rust",
      "  ┃     性能与内存安全兼得，但学习曲线陡峭",
      "  ┃  2. Python",
      "  ┃     简单通用，AI 时代生态最丰富",
      "  ┃  3. TypeScript",
      "  ┃     全栈通吃，前后端一鱼两吃",
      "  ┃  4. Type your own answer",
      "  ┃  ⇆ tab  ↑↓ select  enter confirm  esc dismiss",
    ].join("\n");
    expect(extractOptions("opencode", screen)).toEqual([
      "1. Rust",
      "2. Python",
      "3. TypeScript",
      "4. Type your own answer",
    ]);
  });

  it("extracts options with box-drawing prefix", () => {
    const screen = [
      "  ┃  1. React",
      "  ┃  2. Vue",
      "  ┃  3. Svelte",
      "  ┃  ⇆ tab  ↑↓ select  enter confirm  esc dismiss",
    ].join("\n");
    expect(extractOptions("opencode", screen)).toEqual([
      "1. React",
      "2. Vue",
      "3. Svelte",
    ]);
  });

  it("returns null when no selector footer present", () => {
    const screen = "  ┃  1. Some text\n  ┃  ctrl+p commands";
    expect(extractOptions("opencode", screen)).toBeNull();
  });

  it("returns null when no numbered options found above footer", () => {
    const screen = [
      "  ┃  Some output without options",
      "  ┃  ⇆ tab  ↑↓ select  enter confirm  esc dismiss",
    ].join("\n");
    expect(extractOptions("opencode", screen)).toBeNull();
  });

  it("strips [ ] and [✓] checkbox prefix from multi-select options", () => {
    const screen = [
      "  ┃  你希望测试哪种类型的提问？",
      "  ┃  1. [ ] 单选",
      "  ┃  2. [ ] 多选",
      "  ┃  3. [✓] 自由输入",
      "  ┃  4. [ ] Type your own answer",
      "  ┃  ⇆ tab  ↑↓ select  enter toggle  esc dismiss",
    ].join("\n");
    expect(extractOptions("opencode", screen)).toEqual([
      "1. 单选",
      "2. 多选",
      "3. 自由输入",
      "4. Type your own answer",
    ]);
  });
});

describe("detectMultiSelect — OpenCode", () => {
  it("returns true for multi-select footer (enter toggle)", () => {
    const screen = "  ┃  1. [ ] Apple\n  ┃  ⇆ tab  ↑↓ select  enter toggle  esc dismiss";
    expect(detectMultiSelect("opencode", screen)).toBe(true);
  });

  it("returns false for single-select footer (enter confirm)", () => {
    const screen = "  ┃  1. Apple\n  ┃  ⇆ tab  ↑↓ select  enter confirm  esc dismiss";
    expect(detectMultiSelect("opencode", screen)).toBe(false);
  });

  it("returns false for single-select footer (enter submit)", () => {
    const screen = "  ┃  1. Apple\n  ┃  ↑↓ select  enter submit  esc dismiss";
    expect(detectMultiSelect("opencode", screen)).toBe(false);
  });

  it("returns false for permission dialog (no selector footer)", () => {
    const screen = "△ Permission required\nAllow once   Allow always   Reject";
    expect(detectMultiSelect("opencode", screen)).toBe(false);
  });

  it("returns false for non-OpenCode CLI", () => {
    const screen = "Some question\n1. Yes\n2. No";
    expect(detectMultiSelect("devin", screen)).toBe(false);
  });
});

describe("detectMultiQuestion — OpenCode", () => {
  it("returns true for multi-question dialog with Confirm tab", () => {
    const screen = [
      "  ┃  → Asked 3 questions",
      "  ┃   编程语言   测试反馈   下一步   Confirm",
      "  ┃  你最喜欢哪种编程语言？",
      "  ┃  1. Rust",
      "  ┃  ⇆ tab  ↑↓ select  enter confirm  esc dismiss",
    ].join("\n");
    expect(detectMultiQuestion("opencode", screen)).toBe(true);
  });

  it("returns false for single-question dialog (no tab row)", () => {
    const screen = [
      "  ┃  你喜欢哪个？",
      "  ┃  1. Apple",
      "  ┃  2. Banana",
      "  ┃  ↑↓ select  enter submit  esc dismiss",
    ].join("\n");
    expect(detectMultiQuestion("opencode", screen)).toBe(false);
  });

  it("returns false for permission dialog (no tab row)", () => {
    const screen = "△ Permission required\nAllow once   Allow always   Reject";
    expect(detectMultiQuestion("opencode", screen)).toBe(false);
  });

  it("returns false for non-OpenCode CLI", () => {
    const screen = "Some question\n1. Yes\n2. No";
    expect(detectMultiQuestion("devin", screen)).toBe(false);
  });
});

describe("extractReviewAnswers — OpenCode Confirm tab", () => {
  it("extracts review answers from Confirm tab", () => {
    const screen = [
      "  ┃   弹窗功能   主题风格   交互体验   Confirm",
      "  ┃",
      "  ┃  Review",
      "  ┃",
      "  ┃  弹窗功能: 多选, 自定义输入, 问题跳过, 234234",
      "  ┃",
      "  ┃  主题风格: 跟随系统, 深色",
      "  ┃",
      "  ┃  交互体验: 待优化",
      "  ┃  ⇆ tab  enter submit  esc dismiss",
    ].join("\n");
    const answers = extractReviewAnswers("opencode", screen);
    expect(answers).not.toBeNull();
    expect(answers).toHaveLength(3);
    expect(answers![0]).toBe("弹窗功能: 多选, 自定义输入, 问题跳过, 234234");
    expect(answers![1]).toBe("主题风格: 跟随系统, 深色");
    expect(answers![2]).toBe("交互体验: 待优化");
  });

  it("returns null when not on Confirm tab (no Review header)", () => {
    const screen = [
      "  ┃   弹窗功能   主题风格   交互体验   Confirm",
      "  ┃  你最喜欢哪种编程语言？",
      "  ┃  1. Rust",
      "  ┃  ⇆ tab  ↑↓ select  enter confirm  esc dismiss",
    ].join("\n");
    expect(extractReviewAnswers("opencode", screen)).toBeNull();
  });

  it("returns null for non-OpenCode CLI", () => {
    const screen = "Review\nQuestion 1: Answer";
    expect(extractReviewAnswers("devin", screen)).toBeNull();
  });

  it("stops at footer line", () => {
    const screen = [
      "  ┃  Review",
      "  ┃  Q1: Answer 1",
      "  ┃  ⇆ tab  enter submit  esc dismiss",
      "  ┃  Q2: Answer 2",
    ].join("\n");
    const answers = extractReviewAnswers("opencode", screen);
    expect(answers).not.toBeNull();
    expect(answers).toHaveLength(1);
    expect(answers![0]).toBe("Q1: Answer 1");
  });

  it("returns null when Review header exists but no answer lines", () => {
    const screen = [
      "  ┃  Review",
      "  ┃  ⇆ tab  enter submit  esc dismiss",
    ].join("\n");
    expect(extractReviewAnswers("opencode", screen)).toBeNull();
  });
});

describe("extractQuestion — Claude Code", () => {
  it("extracts Would you like to proceed?", () => {
    const screen = "Some plan\nWould you like to proceed?";
    expect(extractQuestion("claude-code", screen)).toBe("Would you like to proceed?");
  });

  it("extracts trust dialog", () => {
    const screen = "Yes, I trust this folder";
    expect(extractQuestion("claude-code", screen)).toBe("Do you trust this folder?");
  });
});

describe("extractOptions — Claude Code", () => {
  it("returns Yes/No for plan approval", () => {
    const screen = "Would you like to proceed?";
    expect(extractOptions("claude-code", screen)).toEqual(["Yes", "No"]);
  });
});

describe("Claude Code v2.1 multi-question dialog", () => {
  // Screen capture from Claude Code v2.1.220 multi-question (4 questions, single-select)
  const multiQScreen = [
    " successfully\" || echo \"FCC server failed to start\")",
    "FCC server is running",
    "terry@mac-mini ~ % cd /Volumes/2t/code/termfast",
    "terry@mac-mini termfast % fcc-claude",
    "╭───ClaudeCodev2.1.220───╮",
    "│ Tips for getting started │",
    "│ Welcome back! │",
    "│ ▐▛███▜▌ │ What's new │",
    "│ ▝▜█████▛▘ │ Bug fixes and reliability improvements │",
    "│ ▘▘▝▝ │ Added Claude Opus 5 │",
    "│ Opus 5 (1M context) · API Usage Billing │",
    "│ /Volumes/2t/code/termfast │",
    "╰───────────────────────────────────────────────────────────╯",
    "❯ 我在测试claude 交互， 你创建一个弹窗， 4个问题， 单选",
    "  Thought for 3s (ctrl+o to expand)",
    "───────────────────────────────────────────────────────────",
    "←  ☐ 编程语言  ☐ 操作系统  ☐ 编辑器  ☐ 弹窗体验  ✔ Submit  →",
    "你最喜欢哪种编程语言？",
    "❯ 1. Rust",
    "   高性能、内存安全，适合系统级开发",
    "  2. Python",
    "   简洁易读，生态丰富，适合快速开发",
    "3.TypeScript",
    "类型安全的JavaScript超集，前端主流",
    "4.Go",
    "简单高效，并发能力强，适合后端服务",
    "5. Type something.",
    "───────────────────────────────────────────────────────────",
    "6. Chat about this",
    "Entertoselect·Tab/Arrowkeystonavigate·Esctocancel",
  ].join("\n");

  it("detects blocked status from multi-question footer", () => {
    const status = detectStatusFromScreen("claude-code", multiQScreen);
    expect(status).toBe("blocked");
  });

  it("detects multi-question from tab row with Submit", () => {
    expect(detectMultiQuestion("claude-code", multiQScreen)).toBe(true);
  });

  it("extracts question text from multi-question dialog", () => {
    const q = extractQuestion("claude-code", multiQScreen);
    expect(q).toBe("你最喜欢哪种编程语言？");
  });

  it("extracts question text when focused option is NOT the first (regression)", () => {
    // When user navigates to "Type something." (option 4), the ❯ marker
    // is on option 4, not option 1. The question extractor should still
    // find the FIRST option and walk up to the question text — not start
    // from the focused option and pick up a description line by mistake.
    const screen = [
      "←  ☐ 问题一  ☐ 问题二  ✔ Submit  →",
      "",
      "问题 1：测试授权弹窗——你想对第一个工具做何选择？",
      "",
      "  1. 选项 1：允许执行命令",
      "     模拟批准 Bash 工具调用",
      "  2. 选项 2：允许读取文件",
      "     模拟批准 Read 工具调用",
      "  3. 选项 3：拒绝授权",
      "     模拟用户拒绝本次授权",
      "❯ 4. Type something.",
      "─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────",
      "  5. Chat about this",
      "",
      "Enter to select · Tab/Arrow keys to navigate · ctrl+g to edit in Vim · Esc to cancel",
    ].join("\n");
    const q = extractQuestion("claude-code", screen);
    expect(q).toBe("问题 1：测试授权弹窗——你想对第一个工具做何选择？");
  });

  it("extracts all 6 options from multi-question dialog", () => {
    const opts = extractOptions("claude-code", multiQScreen);
    expect(opts).toEqual([
      "1. Rust",
      "2. Python",
      "3. TypeScript",
      "4. Go",
      "5. Type something.",
      "6. Chat about this",
    ]);
  });

  it("is not multi-select (single-select dialog)", () => {
    expect(detectMultiSelect("claude-code", multiQScreen)).toBe(false);
  });
});

describe("Claude Code AskUserQuestion — ↑/↓ navigate footer", () => {
  // AskUserQuestion dialog with 5 options, descriptions, separator, and
  // "Chat about this" below the separator. Footer uses ↑/↓ (not Tab/Arrow).
  const askUserScreen = [
    "✻ Crunched for 28s",
    "❯ 你激活的前两个选项还是yes",
    "  Thought for 48s (ctrl+o to expand)",
    "─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────",
    "Planning: /Users/terry/.claude/plans/modular-churning-pearl.md",
    "─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────",
    " ☐ 下一步",
    "ExitPlanMode 弹窗前两个选项都是「是」（CLI 渲染，我无法改）。接下来怎么继续测试「否」？",
    "❯ 1. 重新触发弹窗",
    "     再次调用 ExitPlanMode，让工具重新扫描弹窗的全部选项，看看「否」是否在后面的位置",
    "  2. 先退出 plan mode",
    "     结束本次弹窗测试，由你在工具侧调整选项提取逻辑后再测",
    "  3. 我描述弹窗结构",
    "     由你告诉我工具当前检测到的选项列表，我据此判断「否」在哪里、需要怎么选择",
    "  4. Type something.",
    "─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────",
    "  5. Chat about this",
    "Enter to select · ↑/↓ to navigate · Esc to cancel",
  ].join("\n");

  it("detects blocked status from ↑/↓ navigate footer", () => {
    const status = detectStatusFromScreen("claude-code", askUserScreen);
    expect(status).toBe("blocked");
  });

  it("extracts question text (not option 5)", () => {
    const q = extractQuestion("claude-code", askUserScreen);
    expect(q).toBe("ExitPlanMode 弹窗前两个选项都是「是」（CLI 渲染，我无法改）。接下来怎么继续测试「否」？");
  });

  it("extracts all 5 options crossing separator", () => {
    const opts = extractOptions("claude-code", askUserScreen);
    expect(opts).toEqual([
      "1. 重新触发弹窗",
      "2. 先退出 plan mode",
      "3. 我描述弹窗结构",
      "4. Type something.",
      "5. Chat about this",
    ]);
  });

  it("is not multi-select", () => {
    expect(detectMultiSelect("claude-code", askUserScreen)).toBe(false);
  });
});

describe("Claude Code AskUserQuestion — multi-select with ↑/↓ footer", () => {
  // Multi-select dialog with [ ] checkboxes, tab row (single question + Submit),
  // 2-space indent descriptions, separator, and "Chat about this" below.
  const multiSelectScreen = [
    "──────────────────────────────────────────────────────────────────────────────────────────",
    "←  ☐ 多选测试  ✔ Submit  →",
    "",
    "请多选若干选项，测试完成后我会告诉你选了哪几个（第几个）？",
    "",
    "❯ 1. [ ] 选项一",
    "  第一个选项（索引 1）",
    "  2. [ ] 选项二",
    "  第二个选项（索引 2）",
    "  3. [ ] 选项三",
    "  第三个选项（索引 3）",
    "  4. [ ] 选项四",
    "  第四个选项（索引 4）",
    "  5. [ ] Type something",
    "     Submit",
    "─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────",
    "  6. Chat about this",
    "",
    "Enter to select · ↑/↓ to navigate · Esc to cancel",
  ].join("\n");

  it("detects blocked status", () => {
    const status = detectStatusFromScreen("claude-code", multiSelectScreen);
    expect(status).toBe("blocked");
  });

  it("detects multi-select from [ ] checkboxes", () => {
    expect(detectMultiSelect("claude-code", multiSelectScreen)).toBe(true);
  });

  it("detects multi-select when ALL options are checked [✔] (regression)", () => {
    // Claude Code uses ✔ (U+2714 HEAVY CHECK MARK), not ✓ (U+2713).
    // When all options are checked, the screen only shows [✔] markers.
    // The detector must still recognize this as multi-select — otherwise
    // textAnswer would use the single-select submit path (just Enter,
    // no Tab), causing Enter to UNCHECK the option instead of submitting.
    const allCheckedScreen = [
      "←  ☒ 测试题A  ☒ 测试题B  ✔ Submit  →",
      "",
      "题目 A：可多选（测试题）",
      "",
      "  1. [✔] 甲",
      "  第 1 个可选",
      "  2. [✔] 乙",
      "  第 2 个可选",
      "  3. [✔] 丙",
      "  第 3 个可选",
      "  4. [✔] 丁",
      "  第 4 个可选",
      "❯ 5. [✔] 1111222",
      "     Next",
      "  6. Chat about this",
      "",
      "Enter to select · Tab/Arrow keys to navigate · ctrl+g to edit in Vim · Esc to cancel",
    ].join("\n");
    expect(detectMultiSelect("claude-code", allCheckedScreen)).toBe(true);
  });

  it("detects multi-question from tab row", () => {
    expect(detectMultiQuestion("claude-code", multiSelectScreen)).toBe(true);
  });

  it("extracts question text (not a description line)", () => {
    const q = extractQuestion("claude-code", multiSelectScreen);
    expect(q).toBe("请多选若干选项，测试完成后我会告诉你选了哪几个（第几个）？");
  });

  it("extracts all 6 options crossing separator and descriptions (with [✔] markers)", () => {
    const opts = extractOptions("claude-code", multiSelectScreen);
    expect(opts).toEqual([
      "1. [ ] 选项一",
      "2. [ ] 选项二",
      "3. [ ] 选项三",
      "4. [ ] 选项四",
      "5. [ ] Type something",
      "6. Chat about this",
    ]);
  });
});

describe("Claude Code multi-question — Submit tab", () => {
  // On the Submit tab, the footer may not include "Enter to select"
  // (there are no options to select). The tab row is still visible.
  // All question tabs are ☒ (answered), not ☐ (unanswered).
  const submitTabScreen = [
    "❯ 我在测试claude 交互",
    "  Thought for 3s (ctrl+o to expand)",
    "───────────────────────────────────────────────────────────",
    "←  ☒ 交互风格  ☒ 开发阶段  ☒ 改动方式  ☒ 后续动作  ✔ Submit  →",
    "Review your answers",
    "● 你希望这个弹窗的交互风格是哪种？",
    "  → 带预览对比",
    "● 这个项目（termfast）目前的开发阶段是？",
    "  → 打磨优化",
    "● 你希望我接下来如何处理代码改动？",
    "  → 严格评审",
    "● 弹窗测试完成后，还需要我做什么？",
    "  → 分析测试结果",
    "Ready to submit your answers?",
    "❯ 1. Submit answers",
    "  2. Cancel",
    "Enter to submit · Esc to cancel",
  ].join("\n");

  it("detects blocked status from tab row on Submit tab (all ☒)", () => {
    const status = detectStatusFromScreen("claude-code", submitTabScreen);
    expect(status).toBe("blocked");
  });

  it("detects multi-question on Submit tab (all ☒)", () => {
    expect(detectMultiQuestion("claude-code", submitTabScreen)).toBe(true);
  });

  it("extracts Submit answers and Cancel options on Submit tab", () => {
    const opts = extractOptions("claude-code", submitTabScreen);
    expect(opts).toEqual(["1. Submit answers", "2. Cancel"]);
  });

  it("extracts 'Ready to submit your answers?' as question on Submit tab", () => {
    const q = extractQuestion("claude-code", submitTabScreen);
    expect(q).toBe("Ready to submit your answers?");
  });
});

describe("Claude Code — permission dialog (v2.1+)", () => {
  // Permission dialog when Claude Code wants to run a command.
  // Footer: "Esc to cancel · Tab to amend · ctrl+e to explain"
  // After the extractLineText fix, spaces between cursor-positioned
  // words are now preserved (empty cells padded with spaces).
  const permScreen = [
    "⏺ Bash(echo \"授权弹窗测试\" > /tmp/claude-auth-test-2.txt)",
    "  ⎿  Waiting…",
    "───────────────────────────────────────────────────────────",
    " Bash command",
    " echo \"授权弹窗测试\" > /tmp/claude-auth-test-2.txt",
    "写入测试文件以触发授权弹窗",
    "Do you want to proceed?",
    "❯ 1. Yes",
    "  2. Yes, and always allow access to tmp/ from this project",
    "  3. No",
    "Esc to cancel · Tab to amend · ctrl+e to explain",
  ].join("\n");

  it("detects blocked status from permission footer", () => {
    const status = detectStatusFromScreen("claude-code", permScreen);
    expect(status).toBe("blocked");
  });

  it("extracts question from permission dialog", () => {
    const q = extractQuestion("claude-code", permScreen);
    expect(q).toBe("Do you want to proceed?");
  });

  it("extracts all 3 options from permission dialog", () => {
    const opts = extractOptions("claude-code", permScreen);
    expect(opts).toEqual([
      "1. Yes",
      "2. Yes, and always allow access to tmp/ from this project",
      "3. No",
    ]);
  });

  it("is not multi-select", () => {
    expect(detectMultiSelect("claude-code", permScreen)).toBe(false);
  });

  it("is not multi-question", () => {
    expect(detectMultiQuestion("claude-code", permScreen)).toBe(false);
  });
});

describe("Claude Code — Create file dialog (short footer)", () => {
  // Create file / Write dialog has a shorter footer:
  // "Esc to cancel · Tab to amend" (no "ctrl+e to explain")
  // Question: "Do you want to create test-file.txt?"
  const createScreen = [
    "⏺ Write(/tmp/test-file.txt)",
    "───────────────────────────────────────────────────────────",
    " Create file",
    " ../../../../tmp/test-file.txt",
    "╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌",
    "  1 # Test File",
    "  2",
    "╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌",
    " Do you want to create test-file.txt?",
    " ❯ 1. Yes",
    "   2. Yes, allow all edits in tmp/ during this session (shift+tab)",
    "   3. No",
    " Esc to cancel · Tab to amend",
  ].join("\n");

  it("detects blocked status from short permission footer", () => {
    const status = detectStatusFromScreen("claude-code", createScreen);
    expect(status).toBe("blocked");
  });

  it("extracts question from Create file dialog", () => {
    const q = extractQuestion("claude-code", createScreen);
    expect(q).toBe("Do you want to create test-file.txt?");
  });

  it("extracts all 3 options from Create file dialog", () => {
    const opts = extractOptions("claude-code", createScreen);
    expect(opts).toEqual([
      "1. Yes",
      "2. Yes, allow all edits in tmp/ during this session (shift+tab)",
      "3. No",
    ]);
  });

  it("is not multi-select", () => {
    expect(detectMultiSelect("claude-code", createScreen)).toBe(false);
  });

  it("is not multi-question", () => {
    expect(detectMultiQuestion("claude-code", createScreen)).toBe(false);
  });
});

describe("Claude Code — Plan Mode (ExitPlanMode) dialog", () => {
  // Plan Mode approval dialog when Claude Code finishes planning.
  // Question: "Claude has written up a plan and is ready to execute. Would you like to proceed?"
  // Options: "❯ 1. Yes, and use auto mode", "  2. Yes, manually approve edits",
  //          "  3. Tell Claude what to change"
  // Footer: "ctrl+g to edit in Vim · ~/.claude/plans/xxx.md"
  const planScreen = [
    " 为 TermFast 增加 Claude Code ExitPlanMode 弹窗检测",
    " Context",
    " TermFast 的 AI CLI 状态检测已覆盖 Claude Code 的权限弹窗和多问题向导。",
    " 目标",
    " 让 TermFast 在 Claude Code 处于 plan mode 审批状态时，检测到该弹窗并弹出 AnswerOverlay。",
    "╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌",
    "─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────",
    " Claude has written up a plan and is ready to execute. Would you like to proceed?",
    " ❯ 1. Yes, and use auto mode",
    "   2. Yes, manually approve edits",
    "   3. Tell Claude what to change",
    "      shift+tab to approve with this feedback",
    " ctrl+g to edit in Vim · ~/.claude/plans/agile-waddling-cocoa.md",
  ].join("\n");

  it("detects blocked status from Would you like to proceed", () => {
    const status = detectStatusFromScreen("claude-code", planScreen);
    expect(status).toBe("blocked");
  });

  it("extracts full question text including prefix", () => {
    const q = extractQuestion("claude-code", planScreen);
    expect(q).toBe("Claude has written up a plan and is ready to execute. Would you like to proceed?");
  });

  it("extracts all 3 numbered options from Plan Mode dialog", () => {
    const opts = extractOptions("claude-code", planScreen);
    expect(opts).toEqual([
      "1. Yes, and use auto mode",
      "2. Yes, manually approve edits",
      "3. Tell Claude what to change",
    ]);
  });

  it("is not multi-select", () => {
    expect(detectMultiSelect("claude-code", planScreen)).toBe(false);
  });

  it("is not multi-question", () => {
    expect(detectMultiQuestion("claude-code", planScreen)).toBe(false);
  });
});

describe("extractQuestion — Codex", () => {
  it("extracts Approve prompt", () => {
    const screen = "Approve command? (y/n)";
    expect(extractQuestion("codex", screen)).toBe("Approve command? (y/n)");
  });

  it("extracts trust prompt v1", () => {
    const screen = "allow Codex to work in this folder";
    expect(extractQuestion("codex", screen)).toBe("Allow Codex to work in this folder?");
  });

  it("extracts trust prompt v2", () => {
    const screen = "Do you trust the contents of this directory? Working with untrusted...";
    expect(extractQuestion("codex", screen)).toBe("Do you trust the contents of this directory?");
  });

  it("extracts new TUI exec approval title", () => {
    const screen = [
      "  Would you like to run the following command?",
      "",
      "  $ echo hello world",
      "",
      "› 1. Yes, proceed (y)",
      "  2. No, and tell Codex what to do differently (esc)",
      "  Press enter to confirm or esc to cancel",
    ].join("\n");
    expect(extractQuestion("codex", screen)).toBe("Would you like to run the following command?");
  });

  it("extracts new TUI patch approval title", () => {
    const screen = [
      "  Would you like to make the following edits?",
      "",
      "› 1. Yes, proceed (y)",
      "  2. Yes, and don't ask again for these files (a)",
      "  3. No, and tell Codex what to do differently (esc)",
      "  Press enter to confirm or esc to cancel",
    ].join("\n");
    expect(extractQuestion("codex", screen)).toBe("Would you like to make the following edits?");
  });

  it("extracts new TUI network approval title", () => {
    const screen = [
      '  Do you want to approve network access to "example.com"?',
      "",
      "› 1. Yes, just this once (y)",
      "  2. Yes, and allow this host for this conversation (a)",
      "  3. Yes, and allow this host in the future (p)",
      "  4. No, and tell Codex what to do differently (esc)",
      "  Press enter to confirm or esc to cancel",
    ].join("\n");
    expect(extractQuestion("codex", screen)).toBe('Do you want to approve network access to "example.com"?');
  });

  it("extracts new TUI permissions approval title", () => {
    const screen = [
      "  Would you like to grant these permissions?",
      "",
      "› 1. Yes, grant these permissions for this turn (y)",
      "  2. Yes, grant for this turn with strict auto review (r)",
      "  3. Yes, grant these permissions for this session (a)",
      "  4. No, continue without permissions (d)",
      "  Press enter to confirm or esc to cancel",
    ].join("\n");
    expect(extractQuestion("codex", screen)).toBe("Would you like to grant these permissions?");
  });
});

describe("extractOptions — Codex", () => {
  it("returns Yes/No for y/n prompt", () => {
    const screen = "Approve command? (y/n)";
    expect(extractOptions("codex", screen)).toEqual(["Yes (y)", "No (n)"]);
  });

  it("returns Yes/No for trust prompt", () => {
    const screen = "Do you trust the contents of this directory?";
    expect(extractOptions("codex", screen)).toEqual(["Yes, continue", "No, quit"]);
  });

  it("extracts new TUI exec approval options", () => {
    const screen = [
      "  Would you like to run the following command?",
      "",
      "  $ echo hello world",
      "",
      "› 1. Yes, proceed (y)",
      "  2. No, and tell Codex what to do differently (esc)",
      "  Press enter to confirm or esc to cancel",
    ].join("\n");
    expect(extractOptions("codex", screen)).toEqual([
      "1. Yes, proceed (y)",
      "2. No, and tell Codex what to do differently (esc)",
    ]);
  });

  it("ignores numbered lists in AI conversation history above approval title", () => {
    // Real-world screen: AI response contains "1. 直接回复在对话框..." etc.
    const screen = [
      "• 目前我知道的情况来说：",
      "  - 直接提问：就是我刚才那样在回复里发问题。",
      "  1. 直接回复在对话框",
      "  2. 写入仓库文档（docs/ 目录，不入库）",
      "  3. 其他（请直接输入）",
      "› 你激活 审批 弹窗",
      "• 好的，我来触发一次权限审批弹窗。",
      "• Running echo approval-test-123",
      "  Would you like to run the following command?",
      "  Environment: local",
      "  Reason: 这只是一个无害命令，用来触发系统权限审批弹窗。",
      "  $ echo approval-test-123",
      "› 1. Yes, proceed (y)",
      "  2. Yes, and don't ask again for commands that start with `echo approval-test-123` (p)",
      "  3. No, and tell Codex what to do differently (esc)",
      "  Press enter to confirm or esc to cancel",
    ].join("\n");
    expect(extractOptions("codex", screen)).toEqual([
      "1. Yes, proceed (y)",
      "2. Yes, and don't ask again for commands that start with `echo approval-test-123` (p)",
      "3. No, and tell Codex what to do differently (esc)",
    ]);
  });

  it("extracts new TUI patch approval options (3 options)", () => {
    const screen = [
      "  Would you like to make the following edits?",
      "",
      "› 1. Yes, proceed (y)",
      "  2. Yes, and don't ask again for these files (a)",
      "  3. No, and tell Codex what to do differently (esc)",
      "  Press enter to confirm or esc to cancel",
    ].join("\n");
    expect(extractOptions("codex", screen)).toEqual([
      "1. Yes, proceed (y)",
      "2. Yes, and don't ask again for these files (a)",
      "3. No, and tell Codex what to do differently (esc)",
    ]);
  });

  it("extracts new TUI network approval options (4 options)", () => {
    const screen = [
      '  Do you want to approve network access to "example.com"?',
      "",
      "› 1. Yes, just this once (y)",
      "  2. Yes, and allow this host for this conversation (a)",
      "  3. Yes, and allow this host in the future (p)",
      "  4. No, and tell Codex what to do differently (esc)",
      "  Press enter to confirm or esc to cancel",
    ].join("\n");
    expect(extractOptions("codex", screen)).toEqual([
      "1. Yes, just this once (y)",
      "2. Yes, and allow this host for this conversation (a)",
      "3. Yes, and allow this host in the future (p)",
      "4. No, and tell Codex what to do differently (esc)",
    ]);
  });

  it("extracts new TUI permissions approval options", () => {
    const screen = [
      "  Would you like to grant these permissions?",
      "",
      "› 1. Yes, grant these permissions for this turn (y)",
      "  2. Yes, grant for this turn with strict auto review (r)",
      "  3. Yes, grant these permissions for this session (a)",
      "  4. No, continue without permissions (d)",
      "  Press enter to confirm or esc to cancel",
    ].join("\n");
    expect(extractOptions("codex", screen)).toEqual([
      "1. Yes, grant these permissions for this turn (y)",
      "2. Yes, grant for this turn with strict auto review (r)",
      "3. Yes, grant these permissions for this session (a)",
      "4. No, continue without permissions (d)",
    ]);
  });
});

describe("detectStatusFromScreen — Codex new TUI", () => {
  it("detects blocked from new TUI footer", () => {
    const screen = [
      "  Would you like to run the following command?",
      "› 1. Yes, proceed (y)",
      "  2. No, and tell Codex what to do differently (esc)",
      "  Press enter to confirm or esc to cancel",
    ].join("\n");
    expect(detectStatusFromScreen("codex", screen)).toBe("blocked");
  });

  it("detects blocked from trust prompt footer", () => {
    const screen = [
      "  Do you trust the contents of this directory?",
      "› 1. Yes, continue",
      "  2. No, quit",
      "  Press enter to continue",
    ].join("\n");
    expect(detectStatusFromScreen("codex", screen)).toBe("blocked");
  });
});

describe("extractQuestion — Codex request_user_input", () => {
  it("extracts question from Plan mode dialog", () => {
    const screen = [
      "  Question 1/2 (2 unanswered)",
      "  你想测试哪类单选问题？",
      "  › 1. 功能偏好 (Recommended)  询问功能偏好，用来测试弹窗交互。",
      "    2. 技术选择                询问某个技术方案的选择。",
      "    3. None of the above       Optionally, add details in notes (tab).",
      "  tab to add notes | enter to submit answer | ←/→ to navigate questions | esc to interrupt",
    ].join("\n");
    expect(extractQuestion("codex", screen)).toBe("你想测试哪类单选问题？");
  });

  it("extracts question from single question dialog", () => {
    const screen = [
      "  Question 1/1 (1 unanswered)",
      "  What would you like to do next?",
      "",
      "    1. Discuss a code change (Recommended)  Walk through a plan and edit code together.",
      "    2. Run tests                            Pick a crate and run its tests.",
      "  › 4. Refactor                             Tighten structure and remove dead code.",
      "    5. Ship it                              Finalize and open a PR.",
      "",
      "  tab to add notes | enter to submit answer | esc to interrupt",
    ].join("\n");
    expect(extractQuestion("codex", screen)).toBe("What would you like to do next?");
  });

  it("extracts question from freeform-only dialog", () => {
    const screen = [
      "  Question 1/1 (1 unanswered)",
      "  Share details.",
      "",
      "  › Type your answer (optional)",
      "",
      "  enter to submit answer | esc to interrupt",
    ].join("\n");
    expect(extractQuestion("codex", screen)).toBe("Share details.");
  });
});

describe("extractOptions — Codex request_user_input", () => {
  it("extracts options with descriptions stripped", () => {
    const screen = [
      "  Question 1/2 (2 unanswered)",
      "  你想测试哪类单选问题？",
      "  › 1. 功能偏好 (Recommended)  询问功能偏好，用来测试弹窗交互。",
      "    2. 技术选择                询问某个技术方案的选择。",
      "    3. None of the above       Optionally, add details in notes (tab).",
      "  tab to add notes | enter to submit answer | ←/→ to navigate questions | esc to interrupt",
    ].join("\n");
    expect(extractOptions("codex", screen)).toEqual([
      "1. 功能偏好 (Recommended)",
      "2. 技术选择",
      "3. None of the above",
    ]);
  });

  it("extracts options from scrolling options dialog", () => {
    const screen = [
      "  Question 1/1 (1 unanswered)",
      "  What would you like to do next?",
      "",
      "    1. Discuss a code change (Recommended)  Walk through a plan and edit code together.",
      "    2. Run tests                            Pick a crate and run its tests.",
      "    3. Review a diff                        Summarize or review current changes.",
      "  › 4. Refactor                             Tighten structure and remove dead code.",
      "    5. Ship it                              Finalize and open a PR.",
      "",
      "  tab to add notes | enter to submit answer | esc to interrupt",
    ].join("\n");
    expect(extractOptions("codex", screen)).toEqual([
      "1. Discuss a code change (Recommended)",
      "2. Run tests",
      "3. Review a diff",
      "4. Refactor",
      "5. Ship it",
    ]);
  });

  it("returns null for freeform-only dialog (no options)", () => {
    const screen = [
      "  Question 1/1 (1 unanswered)",
      "  Share details.",
      "",
      "  › Type your answer (optional)",
      "",
      "  enter to submit answer | esc to interrupt",
    ].join("\n");
    expect(extractOptions("codex", screen)).toBeNull();
  });

  it("ignores numbered lists in AI conversation history above Question header", () => {
    // Real-world screen: AI response contains "1. 切换到 Plan mode..." etc.
    const screen = [
      "│ >_ OpenAI Codex (v0.146.0)                               │",
      "› 我在测试 codex 的交互",
      "• 当前会话处于 Default 模式，request_user_input 工具只在 Plan mode 下可用。",
      "  你可以：",
      "  1. 切换到 Plan mode（如果有切换入口），然后我再调用该工具；",
      "  2. 或者直接在这里用文字告诉我你的选择，我也能正常处理。",
      "  你更想用哪种方式继续？",
      "• Model changed to opencode/deepseek-v4-flash-free medium for Plan mode.",
      "› 我在测试 codex 的交互",
      "  Question 1/2 (2 unanswered)",
      "  这是第一个测试问题，请选择一个选项：",
      "  › 1. 选项 A             这是选项 A 的描述文字。",
      "    2. 选项 B             这是选项 B 的描述文字。",
      "    3. 选项 C             这是选项 C 的描述文字。",
      "    4. None of the above  Optionally, add details in notes (tab).",
      "  tab to add notes | enter to submit answer | ←/→ to navigate questions | esc to interrupt",
    ].join("\n");
    expect(extractOptions("codex", screen)).toEqual([
      "1. 选项 A",
      "2. 选项 B",
      "3. 选项 C",
      "4. None of the above",
    ]);
  });
});

describe("detectStatusFromScreen — Codex request_user_input", () => {
  it("detects blocked from Question N/M header", () => {
    const screen = [
      "  Question 1/2 (2 unanswered)",
      "  你想测试哪类单选问题？",
      "  › 1. 功能偏好 (Recommended)  询问功能偏好，用来测试弹窗交互。",
      "    2. 技术选择                询问某个技术方案的选择。",
      "  tab to add notes | enter to submit answer | ←/→ to navigate questions | esc to interrupt",
    ].join("\n");
    expect(detectStatusFromScreen("codex", screen)).toBe("blocked");
  });

  it("detects blocked from footer 'tab to add notes'", () => {
    const screen = [
      "  Question 1/1 (1 unanswered)",
      "  What would you like to do next?",
      "    1. Option 1  First choice.",
      "  › 2. Option 2  Second choice.",
      "  tab to add notes | enter to submit answer | esc to interrupt",
    ].join("\n");
    expect(detectStatusFromScreen("codex", screen)).toBe("blocked");
  });

  it("detects blocked from 'enter to submit all' (last question)", () => {
    const screen = [
      "  Question 2/2 (2 unanswered)",
      "  Share details.",
      "  › Type your answer (optional)",
      "  enter to submit all | ctrl + p / ctrl + n change question | esc to interrupt",
    ].join("\n");
    expect(detectStatusFromScreen("codex", screen)).toBe("blocked");
  });
});

describe("detectStatusFromScreen — priority ordering", () => {
  it("blocked has higher priority than working", () => {
    // OpenCode: both spinner (working) and "△ Permission required" (blocked) present
    const screen = "  ⠋ Read src/main.ts\n△ Permission required";
    expect(detectStatusFromScreen("opencode", screen)).toBe("blocked");
  });

  it("blocked has higher priority than idle", () => {
    // OpenCode: both "ctrl+p commands" (idle) and "△ Permission required" (blocked) present
    const screen = "ctrl+p commands\n△ Permission required";
    expect(detectStatusFromScreen("opencode", screen)).toBe("blocked");
  });
});

describe("detectStatusFromScreen — Devin permission dialog", () => {
  it("detects blocked from '1 Yes (Approve once)' pattern", () => {
    const screen = "❭ 1 Yes  (Approve once)\n· 2 No\n↑↓ select · ↵ confirm · esc cancel";
    expect(detectStatusFromScreen("devin", screen)).toBe("blocked");
  });

  it("detects blocked from '↑↓ select · ↵ confirm' footer", () => {
    const screen = "Some output\n↑↓ select · ↵ confirm · esc cancel";
    expect(detectStatusFromScreen("devin", screen)).toBe("blocked");
  });

  it("returns null for normal Devin output", () => {
    const screen = "Devin CLI v3000.3.27\n❭ some output";
    expect(detectStatusFromScreen("devin", screen)).toBeNull();
  });
});

describe("detectStatusFromScreen — Devin idle prompt", () => {
  it("detects idle from '❭ Ask Devin to build features' placeholder", () => {
    // Real Devin idle screen: ❭ followed by fixed placeholder text
    const screen = "Devin CLI v3000.3.27\n❭ Ask Devin to build features, fix bugs, or work on your code";
    expect(detectStatusFromScreen("devin", screen)).toBe("idle");
  });

  it("does NOT detect idle when ❭ has user input text", () => {
    // Real Devin working screen: ❭ followed by user's message
    const screen = "Devin CLI v3000.3.27\n❭ what is 2+2?";
    expect(detectStatusFromScreen("devin", screen)).toBeNull();
  });

  it("does NOT detect idle from lone ❭", () => {
    // Lone ❭ without placeholder text is NOT idle (could be working)
    const screen = "Devin CLI v3000.3.27\n❭ ";
    expect(detectStatusFromScreen("devin", screen)).toBeNull();
  });

  it("blocked has higher priority than idle", () => {
    const screen = "❭ Ask Devin to build features\n❭ 1 Yes  (Approve once)\n↑↓ select · ↵ confirm";
    expect(detectStatusFromScreen("devin", screen)).toBe("blocked");
  });
});

describe("detectStatusFromScreen — Devin working spinner", () => {
  it("detects working from '⠈⠉ Thinking · 2s (esc to interrupt)'", () => {
    // Real Devin working screen: Braille spinner + "Thinking" + timing + footer
    const screen = "Devin CLI v3000.3.27\n⠈⠉ Thinking · 2s (esc to interrupt)";
    expect(detectStatusFromScreen("devin", screen)).toBe("working");
  });

  it("detects working with different Braille spinner frames", () => {
    // Devin cycles through Braille spinner frames: ⠈⠉, ⠘⠋, ⠸⠙, etc.
    const screen = "⠘⠋ Thinking · 5s (esc to interrupt)";
    expect(detectStatusFromScreen("devin", screen)).toBe("working");
  });

  it("detects working with 'Working' verb (not just 'Thinking')", () => {
    const screen = "⠈⠉ Working · 3s (esc to interrupt)";
    expect(detectStatusFromScreen("devin", screen)).toBe("working");
  });

  it("detects working with 'Processing' verb", () => {
    const screen = "⠸⠙ Processing · 10s (esc to interrupt)";
    expect(detectStatusFromScreen("devin", screen)).toBe("working");
  });

  it("detects working with minutes in timing", () => {
    const screen = "⠈⠉ Thinking · 1m 23s (esc to interrupt)";
    expect(detectStatusFromScreen("devin", screen)).toBe("working");
  });

  it("does NOT detect working from user input (no Braille, no spinner)", () => {
    // User typing in Devin's input box: ❭ followed by user text, no spinner
    const screen = "Devin CLI v3000.3.27\n❭ what is 2+2?";
    expect(detectStatusFromScreen("devin", screen)).toBeNull();
  });

  it("does NOT detect working from idle placeholder", () => {
    const screen = "❭ Ask Devin to build features, fix bugs, or work on your code";
    expect(detectStatusFromScreen("devin", screen)).toBe("idle");
  });

  it("does NOT detect working from '(esc to interrupt)' without Braille spinner", () => {
    // The footer alone is not a working indicator (could be static chrome)
    const screen = "Some output\n(esc to interrupt)";
    expect(detectStatusFromScreen("devin", screen)).toBeNull();
  });

  it("does NOT detect working from Braille without -ing verb", () => {
    // Braille chars alone (e.g. in logo art) without a verb + footer
    const screen = "⣴⣾⣶⡄  Devin CLI\n⠛⠿⠟⠻⣶⣾⣶⡄  v3000.3.27";
    expect(detectStatusFromScreen("devin", screen)).toBeNull();
  });

  it("blocked has higher priority than working", () => {
    // Both spinner and permission dialog on screen → blocked wins
    const screen = "⠈⠉ Thinking · 2s (esc to interrupt)\n❭ 1 Yes  (Approve once)\n↑↓ select · ↵ confirm";
    expect(detectStatusFromScreen("devin", screen)).toBe("blocked");
  });
});

// ── Devin ask_user_question dialog tests ─────────────────────────────────────
// These tests verify detection of Devin's ask_user_question popup format:
//   "↑↓ navigate · ↵ select · e select+type · ? help me out · esc cancel"
// Four variants: single/multi-select × single/multi-question

describe("detectStatusFromScreen — Devin ask_user_question dialog", () => {
  it("detects blocked for single-select single-question footer", () => {
    const screen = "  ❭ 1 Rust\n  · 2 TypeScript\n↑↓ navigate · ↵ select · e select+type · ? help me out · esc cancel";
    expect(detectStatusFromScreen("devin", screen)).toBe("blocked");
  });

  it("detects blocked for multi-select single-question footer", () => {
    const screen = "  □ 1 VS Code\n  □ 2 tmux\n↑↓ navigate · ␣ toggle · ↵ select · e select+type · ? help me out · esc cancel";
    expect(detectStatusFromScreen("devin", screen)).toBe("blocked");
  });

  it("detects blocked for single-select multi-question footer", () => {
    const screen = "  ❭ 1 确认型弹窗\n  · 2 选择型弹窗\n↑↓ navigate · ↵ select · e select+type · ←→ switch question · ? help me out · esc cancel";
    expect(detectStatusFromScreen("devin", screen)).toBe("blocked");
  });

  it("detects blocked for multi-select multi-question footer", () => {
    const screen = "  □ 1 多选问题\n  □ 2 单选问题\n↑↓ navigate · ␣ toggle · ↵ select · e select+type · ←→ switch question · ? help me out · esc cancel";
    expect(detectStatusFromScreen("devin", screen)).toBe("blocked");
  });

  it("blocked (ask_user_question) has higher priority than idle", () => {
    const screen = "❭ Ask Devin to build features\n  ❭ 1 Rust\n↑↓ navigate · ↵ select · esc cancel";
    expect(detectStatusFromScreen("devin", screen)).toBe("blocked");
  });
});

describe("extractQuestion — Devin ask_user_question", () => {
  it("extracts question text above first numbered option", () => {
    const screen = [
      "── 编程语言 ────────────────────────────────────────────────────",
      "  这是一个单选单问题弹窗测试。你最喜欢哪种编程语言？",
      "  ❭ 1 Rust",
      "      系统级语言，内存安全，性能优秀",
      "  · 2 TypeScript",
      "─────────────────────────────────────────────────────────────",
      "↑↓ navigate · ↵ select · e select+type · ? help me out · esc cancel",
    ].join("\n");
    expect(extractQuestion("devin", screen)).toBe("这是一个单选单问题弹窗测试。你最喜欢哪种编程语言？");
  });

  it("extracts question text for multi-select dialog", () => {
    const screen = [
      "── 开发工具 ────────────────────────────────────────────────────",
      "  这是一个多选单问题弹窗测试。你日常开发中常用哪些工具？（可多选） (multi-select)",
      "  □ 1 VS Code",
      "      代码编辑器，轻量级、插件丰富",
      "  □ 2 tmux",
      "─────────────────────────────────────────────────────────────",
      "↑↓ navigate · ␣ toggle · ↵ select · e select+type · ? help me out · esc cancel",
    ].join("\n");
    expect(extractQuestion("devin", screen)).toBe("这是一个多选单问题弹窗测试。你日常开发中常用哪些工具？（可多选） (multi-select)");
  });

  it("extracts question text for multi-question dialog", () => {
    const screen = [
      "── 交互类型 · 弹窗特性 · UI 渲染 · 改进建议 ────────────────────",
      "  这是第 1 个测试问题（单选）：你希望测试哪种类型的弹窗交互？",
      "  ❭ 1 确认型弹窗",
      "      简单的 yes/no 确认场景",
      "  · 2 选择型弹窗",
      "─────────────────────────────────────────────────────────────",
      "↑↓ navigate · ↵ select · e select+type · ←→ switch question · ? help me out · esc cancel",
    ].join("\n");
    expect(extractQuestion("devin", screen)).toBe("这是第 1 个测试问题（单选）：你希望测试哪种类型的弹窗交互？");
  });

  it("returns fallback when question text not found", () => {
    const screen = "  ❭ 1 Rust\n↑↓ navigate · ↵ select · esc cancel";
    expect(extractQuestion("devin", screen)).toBe("Devin is asking a question");
  });
});

describe("extractOptions — Devin ask_user_question", () => {
  it("extracts options with ❭/· prefix (single-select)", () => {
    const screen = [
      "  ❭ 1 Rust",
      "      系统级语言，内存安全，性能优秀",
      "  · 2 TypeScript",
      "      前端主流，类型安全的 JavaScript 超集",
      "  · 3 Python",
      "  · 4 Go",
      "  ·   Other (type your own)",
      "↑↓ navigate · ↵ select · e select+type · ? help me out · esc cancel",
    ].join("\n");
    expect(extractOptions("devin", screen)).toEqual([
      "1. Rust",
      "2. TypeScript",
      "3. Python",
      "4. Go",
      "Other (type your own)",
    ]);
  });

  it("extracts options with □ prefix (multi-select)", () => {
    const screen = [
      "  □ 1 多选问题",
      "      测试 multi_select=true 时用户能否选多个选项",
      "  □ 2 单选问题",
      "  □ 3 Other 自定义",
      "  □ 4 跳过问题",
      "  □ 5 Other (type your own)",
      "↑↓ navigate · ␣ toggle · ↵ select · e select+type · ? help me out · esc cancel",
    ].join("\n");
    expect(extractOptions("devin", screen)).toEqual([
      "1. 多选问题",
      "2. 单选问题",
      "3. Other 自定义",
      "4. 跳过问题",
      "5. Other (type your own)",
    ]);
  });

  it("extracts options with ■ prefix (checked multi-select)", () => {
    const screen = [
      "  ■ 1 React",
      "  □ 2 Vue",
      "↑↓ navigate · ␣ toggle · ↵ select · e select+type · ? help me out · esc cancel",
    ].join("\n");
    expect(extractOptions("devin", screen)).toEqual(["1. React", "2. Vue"]);
  });
});

describe("detectMultiSelect — Devin", () => {
  it("returns true for multi-select footer (␣ toggle)", () => {
    const screen = "  □ 1 VS Code\n↑↓ navigate · ␣ toggle · ↵ select · e select+type · ? help me out · esc cancel";
    expect(detectMultiSelect("devin", screen)).toBe(true);
  });

  it("returns false for single-select footer (no ␣ toggle)", () => {
    const screen = "  ❭ 1 Rust\n↑↓ navigate · ↵ select · e select+type · ? help me out · esc cancel";
    expect(detectMultiSelect("devin", screen)).toBe(false);
  });

  it("returns true for multi-select multi-question footer", () => {
    const screen = "  □ 1 多选问题\n↑↓ navigate · ␣ toggle · ↵ select · e select+type · ←→ switch question · ? help me out · esc cancel";
    expect(detectMultiSelect("devin", screen)).toBe(true);
  });
});

describe("detectMultiQuestion — Devin", () => {
  it("returns true for multi-question footer (←→ switch question)", () => {
    const screen = "  ❭ 1 确认型弹窗\n↑↓ navigate · ↵ select · e select+type · ←→ switch question · ? help me out · esc cancel";
    expect(detectMultiQuestion("devin", screen)).toBe(true);
  });

  it("returns false for single-question footer (no ←→ switch question)", () => {
    const screen = "  ❭ 1 Rust\n↑↓ navigate · ↵ select · e select+type · ? help me out · esc cancel";
    expect(detectMultiQuestion("devin", screen)).toBe(false);
  });

  it("returns true for multi-select multi-question footer", () => {
    const screen = "  □ 1 多选问题\n↑↓ navigate · ␣ toggle · ↵ select · e select+type · ←→ switch question · ? help me out · esc cancel";
    expect(detectMultiQuestion("devin", screen)).toBe(true);
  });
});

describe("detectStatusFromScreen — Codex working (TUI spinner only)", () => {
  it("detects working from '• Working (0s • esc to interrupt)'", () => {
    const screen = "• Working (0s • esc to interrupt)";
    expect(detectStatusFromScreen("codex", screen)).toBe("working");
  });

  it("detects working from '• Thinking (2s • esc to interrupt)'", () => {
    const screen = "• Thinking (2s • esc to interrupt)";
    expect(detectStatusFromScreen("codex", screen)).toBe("working");
  });

  it("detects working with longer prefix text", () => {
    const screen = "• Starting script creation (10s • esc to interrupt)";
    expect(detectStatusFromScreen("codex", screen)).toBe("working");
  });

  it("does NOT detect working from 'running' in startup banner", () => {
    // Bug: keyword pattern matched "running" in "You are running Codex in..."
    const screen = "You are running Codex in /home/user/project\ncodex>";
    expect(detectStatusFromScreen("codex", screen)).toBe("idle");
  });

  it("does NOT detect working from 'thinking' in user input", () => {
    // Bug: keyword pattern matched "thinking" when user types "I'm thinking..."
    const screen = "❯ I'm thinking about this approach";
    expect(detectStatusFromScreen("codex", screen)).toBeNull();
  });

  it("does NOT detect working from 'analyzing' in stale AI output", () => {
    // Bug: keyword pattern matched "analyzing" in previous AI response text
    const screen = "I was analyzing the code structure\n❯ ";
    expect(detectStatusFromScreen("codex", screen)).toBe("idle");
  });

  it("does NOT detect working from 'processing' in generic text", () => {
    const screen = "processing your request...\n❯ ";
    expect(detectStatusFromScreen("codex", screen)).toBe("idle");
  });

  it("detects idle from empty '❯' prompt", () => {
    const screen = "some output\n❯";
    expect(detectStatusFromScreen("codex", screen)).toBe("idle");
  });

  it("detects idle from empty '›' prompt with whitespace", () => {
    const screen = "  ›  ";
    expect(detectStatusFromScreen("codex", screen)).toBe("idle");
  });

  it("does NOT detect idle when prompt has user text", () => {
    // "❯ what is 2+2?" — user is typing, not idle
    const screen = "❯ what is 2+2?";
    expect(detectStatusFromScreen("codex", screen)).toBeNull();
  });

  it("blocked has higher priority than working spinner", () => {
    const screen = "• Working (5s • esc to interrupt)\nApprove command? (y/n)";
    expect(detectStatusFromScreen("codex", screen)).toBe("blocked");
  });
});

// Regression tests for user input being mistaken for options.
// When Devin shows a multi-question dialog, the user's original input line
// (starting with ❭) appears ABOVE the dialog tab row. The extractors must
// bound their search to the dialog area only.
describe("Devin dialog boundary — user input above dialog", () => {
  // Screen with user input above the dialog (unnumbered options)
  const screenWithUserInput = [
    "❭ 我在测试devin 的交互， 你触发一个 单选多问题 弹窗",
    "",
    "  · IDE ✓ · Git 习惯 ✓ · 反馈方式 ────────────────────────────────",
    "  你最高效的工作时段是？",
    "  · 上午",
    "    上午精力充沛，适合写代码",
    "  · 下午",
    "    下午状态稳定，适合调试和重构",
    "  · 深夜",
    "    夜深人静，灵感最旺",
    "  ❭ Other (type your own)",
    "─────────────────────────────────────────────────────────────────",
    "↑↓ navigate · ↵ select · e select+type · ←→ switch question · ? help me out · esc cancel",
    "? Not ready to answer, help me out!",
  ].join("\n");

  it("extractQuestion does not pick up user input as question", () => {
    expect(extractQuestion("devin", screenWithUserInput)).toBe("你最高效的工作时段是？");
  });

  it("extractOptions does not include user input as first option", () => {
    const options = extractOptions("devin", screenWithUserInput);
    expect(options).not.toBeNull();
    expect(options!.length).toBe(4);
    expect(options![0]).toBe("上午");
    expect(options![1]).toBe("下午");
    expect(options![2]).toBe("深夜");
    expect(options![3]).toBe("Other (type your own)");
    // User input must NOT appear in options
    expect(options!.some((o) => o.includes("我在测试"))).toBe(false);
  });

  it("extractCursorIndex returns correct index for ❭ on Other", () => {
    expect(extractCursorIndex("devin", screenWithUserInput)).toBe(3);
  });

  // Screen with user input above the dialog (numbered options)
  const screenNumberedWithUserInput = [
    "❭ 我在测试devin 的交互",
    "",
    "─────────────────────────────────────────────────────────────────",
    "  你希望项目采用哪种构建工具？",
    "  · 1 Vite",
    "      快速的前端构建工具",
    "  · 2 Webpack",
    "  ❭ 3 esbuild",
    "      极速打包，配置简单",
    "  ·   Other (type your own)",
    "─────────────────────────────────────────────────────────────────",
    "↑↓ navigate · ↵ select · e select+type · ←→ switch question · ? help me out · esc cancel",
    "? Not ready to answer, help me out!",
  ].join("\n");

  it("extractOptions does not include user input (numbered options)", () => {
    const options = extractOptions("devin", screenNumberedWithUserInput);
    expect(options).not.toBeNull();
    expect(options!.length).toBe(4);
    expect(options![0]).toBe("1. Vite");
    expect(options![1]).toBe("2. Webpack");
    expect(options![2]).toBe("3. esbuild");
    expect(options![3]).toBe("Other (type your own)");
    expect(options!.some((o) => o.includes("我在测试"))).toBe(false);
  });

  it("extractCursorIndex returns correct index for numbered options", () => {
    expect(extractCursorIndex("devin", screenNumberedWithUserInput)).toBe(2);
  });

  it("extractQuestion does not pick up user input (numbered options)", () => {
    expect(extractQuestion("devin", screenNumberedWithUserInput)).toBe("你希望项目采用哪种构建工具？");
  });
});
