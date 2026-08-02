// Unit tests for agentPatterns — question + option extraction
import { describe, it, expect } from "vitest";
import { extractQuestion, extractOptions, detectStatusFromScreen, detectMultiSelect, detectMultiQuestion, extractReviewAnswers, stripAnsi } from "../agentPatterns";

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
    const screen = "Some output\nDo you want to continue?\n1. Yes\n2. No";
    expect(extractQuestion("devin", screen)).toBe("Do you want to continue?");
  });

  it("returns null when no question found", () => {
    expect(extractQuestion("devin", "just some output")).toBeNull();
  });
});

describe("extractOptions — Devin", () => {
  it("extracts numbered options (old format with dot)", () => {
    const screen = "Do you want to continue?\n1. Yes\n2. No\n3. Maybe";
    const options = extractOptions("devin", screen);
    expect(options).toEqual(["1. Yes", "2. No", "3. Maybe"]);
  });

  it("extracts numbered options (new permission dialog format)", () => {
    const screen = [
      "❭ 1 Yes  (Approve once)",
      "· 2 Yes, allow `seq` commands",
      "· 3 Yes, always allow `seq` commands in `ssh-proxy`",
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
      "3. Yes, always allow `seq` commands in `ssh-proxy`",
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
});

// ── OpenCode question/selector dialog tests ──────────────────────────────────

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

describe("extractQuestion — Codex", () => {
  it("extracts Approve prompt", () => {
    const screen = "Approve command? (y/n)";
    expect(extractQuestion("codex", screen)).toBe("Approve command? (y/n)");
  });

  it("extracts trust prompt v1", () => {
    const screen = "allow Codex to work in this folder";
    expect(extractQuestion("codex", screen)).toBe("Allow Codex to work in this folder?");
  });
});

describe("extractOptions — Codex", () => {
  it("returns Yes/No for y/n prompt", () => {
    const screen = "Approve command? (y/n)";
    expect(extractOptions("codex", screen)).toEqual(["Yes (y)", "No (n)"]);
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
