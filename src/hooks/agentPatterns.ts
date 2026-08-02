// agentPatterns — per-CLI regex patterns for status detection + question extraction
//
// Patterns are sourced from cli-agent-orchestrator (awslabs) production code
// and verified against live PTY captures. Each CLI has:
//   - statusPatterns: detect working/blocked/done/idle from screen content
//   - questionPatterns: extract question text + options when blocked
//
// All patterns operate on ANSI-stripped screen text (from screenScraper).

import type { CliType } from "./oscParser";
import type { AgentStatus } from "./agentStateMachine";

// ── ANSI stripping ──────────────────────────────────────────────────────────

/** Strip ANSI escape codes from text (SGR colors, cursor moves, etc.). */
export function stripAnsi(text: string): string {
  // SGR: \x1b[...m
  let result = text.replace(/\x1b\[[0-9;]*m/g, "");
  // CSI: \x1b[...<letter>
  result = result.replace(/\x1b\[[0-9;?]*[a-zA-Z]/g, "");
  // OSC: \x1b]...BEL or \x1b]...ESC \
  result = result.replace(/\x1b\][^\x07\x1b]*(\x07|\x1b\\)/g, "");
  // Other escapes
  result = result.replace(/\x1b[()][0-9a-zA-Z]/g, "");
  result = result.replace(/\x1b[=>]/g, "");
  result = result.replace(/\x1b/g, "");
  return result;
}

// ── Per-CLI status patterns ──────────────────────────────────────────────────

export interface StatusPattern {
  status: AgentStatus;
  pattern: RegExp;
  /** Priority — higher = checked first (e.g. blocked > working > idle). */
  priority: number;
}

export interface CliPatterns {
  /** Detect status from screen text. Checked in priority order. */
  statusPatterns: StatusPattern[];
  /** Extract question text when blocked (null if not extractable). */
  questionExtractor?: (screenText: string) => string | null;
  /** Extract answer options when blocked (null if not extractable). */
  optionsExtractor?: (screenText: string) => string[] | null;
  /** Detect if the current blocked dialog is multi-select (null if N/A). */
  multiSelectDetector?: (screenText: string) => boolean;
  /** Detect if the current blocked dialog is multi-question (has Confirm tab). */
  multiQuestionDetector?: (screenText: string) => boolean;
  /** Extract review answers from the Confirm tab (null if not on Confirm tab). */
  reviewAnswersExtractor?: (screenText: string) => string[] | null;
}

// ── Devin patterns ────────────────────────────────────────────────────────────
// Devin uses OSC 777/1337 for status (handled in oscParser), but screen scrape
// can extract question text + numbered options from the alt buffer.

const devinPatterns: CliPatterns = {
  statusPatterns: [
    // Devin's primary signals come from OSC, not screen. These are fallbacks.
    // Permission approval dialog: "1 Yes (Approve once)" + "↑↓ select · ↵ confirm"
    { status: "blocked", pattern: /\d+\s+Yes\s*\(Approve|↑↓\s+select.*↵\s+confirm/i, priority: 10 },
    // Question/selector dialog (ask_user_question): "↑↓ navigate · ↵ select" footer.
    // This covers all four variants:
    //   single-select single-question: "↑↓ navigate · ↵ select · e select+type · ? help me out · esc cancel"
    //   multi-select  single-question: "↑↓ navigate · ␣ toggle · ↵ select · e select+type · ? help me out · esc cancel"
    //   single-select multi-question:  "↑↓ navigate · ↵ select · e select+type · ←→ switch question · ? help me out · esc cancel"
    //   multi-select  multi-question:  "↑↓ navigate · ␣ toggle · ↵ select · e select+type · ←→ switch question · ? help me out · esc cancel"
    { status: "blocked", pattern: /↑↓\s+navigate.*↵\s+select.*esc\s+cancel/i, priority: 9 },
    // "Do you want to continue?" or "Press q to quit" patterns
    { status: "blocked", pattern: /Do you want to|Press . to|Would you like to/i, priority: 8 },
    // Devin working: Braille spinner + active verb (Thinking/Working/etc.) +
    // "(esc to interrupt)" footer. The Braille block (U+2800..U+28FF) is the
    // same range Claude Code and Codex use for their spinners (per agent-terminal
    // project's is_braille() detector). The "(esc to interrupt)" footer combined
    // with the Braille spinner is a reliable working indicator that
    // distinguishes real Devin work from user input echo (which has no spinner).
    // Example: "⠈⠉ Thinking · 2s (esc to interrupt)"
    { status: "working", pattern: /^\s*[\u2800-\u28FF]{1,4}\s+\w*ing\b[^\n]*\(esc\s+to\s+interrupt\)/im, priority: 7 },
    // Devin idle: ❭ followed by the placeholder text "Ask Devin to build features..."
    // When Devin is idle (waiting for user input), the input line shows this
    // fixed placeholder. When working, ❭ is followed by the user's message text.
    { status: "idle", pattern: /❭ Ask Devin to/i, priority: 3 },
  ],
  questionExtractor: (text) => {
    const lines = text.split("\n");
    // Question/selector dialog (ask_user_question):
    // The question text is on a line between the "──" separator and the
    // first numbered option. Format: "  这是第 1 个测试问题（单选）：..."
    const selectorIdx = lines.findIndex((l) => /↑↓\s+navigate.*esc\s+cancel/.test(l));
    if (selectorIdx >= 0) {
      // Find the first numbered option line above the footer.
      // Format: "  ❭ 1 确认型弹窗" or "  □ 1 多选问题" or "  · 1 Rust"
      const optionPattern = /^\s*[❭·□■]\s*(\d+)\s+(.+)/;
      let firstOptionIdx = -1;
      for (let i = 0; i < selectorIdx; i++) {
        if (optionPattern.test(lines[i])) {
          firstOptionIdx = i;
          break;
        }
      }
      if (firstOptionIdx >= 0) {
        // Walk upward from firstOptionIdx-1 to find the question text.
        // Skip empty lines, "──" separator lines, and description lines
        // (indented text under options). The question is the first
        // non-empty, non-separator line.
        for (let i = firstOptionIdx - 1; i >= 0; i--) {
          const trimmed = lines[i].trim();
          if (!trimmed) continue;
          // Skip "──" separator lines (box-drawing)
          if (/^[─━]+$/.test(trimmed)) continue;
          // Skip lines that are just box-drawing chars
          if (/^[┃│║┌┐└┘├┤┬┴┼─━]+$/.test(trimmed)) continue;
          return trimmed;
        }
      }
      return "Devin is asking a question";
    }
    // Devin permission dialog: the question is the command being approved
    // Look for "Running command" line or a line ending with ?
    for (const line of lines) {
      const trimmed = line.trim();
      if (trimmed.endsWith("?") && trimmed.length > 10) {
        return trimmed;
      }
    }
    // Look for "Running command" as the question context
    for (const line of lines) {
      const trimmed = line.trim();
      if (/Running command/i.test(trimmed)) {
        return "Approve command execution?";
      }
    }
    return null;
  },
  optionsExtractor: (text) => {
    const lines = text.split("\n");
    // Question/selector dialog (ask_user_question):
    // Options have format: "  ❭ 1 确认型弹窗" or "  □ 1 多选问题" or "  · 1 Rust"
    // Prefixes: "❭" = selected (single-select), "□" = unchecked (multi-select),
    //           "■" = checked (multi-select), "·" = unselected (single-select)
    // Also "  ·   Other (type your own)" — no number, special entry
    const selectorIdx = lines.findIndex((l) => /↑↓\s+navigate.*esc\s+cancel/.test(l));
    if (selectorIdx >= 0) {
      const options: string[] = [];
      const optionPattern = /^\s*[❭·□■]\s*(\d+)\s+(.+)/;
      const otherPattern = /^\s*[❭·□■]\s+(Other\s*\(type your own\))/i;
      for (let i = 0; i < selectorIdx; i++) {
        const m = lines[i].match(optionPattern);
        if (m) {
          options.push(`${m[1]}. ${m[2].trim()}`);
          continue;
        }
        const om = lines[i].match(otherPattern);
        if (om) {
          options.push(om[1].trim());
        }
      }
      if (options.length > 0) return options;
    }
    // Devin permission dialog: numbered options
    // "1 Yes  (Approve once)" — number + space (new permission dialog)
    // "1. Yes" — number + dot + space (older format)
    // Prefixes: "❭" = selected item, "·" = unselected item
    const options: string[] = [];
    const optionPattern = /^(?:[❭·]\s*)?(\d+)[.\s]+(.+)$/;
    for (const line of lines) {
      const m = line.trim().match(optionPattern);
      if (m) {
        options.push(`${m[1]}. ${m[2].trim()}`);
      }
    }
    return options.length > 0 ? options : null;
  },
  // Multi-select detection: "␣ toggle" in footer means multi-select.
  // Single-select footer has no "␣ toggle".
  multiSelectDetector: (text) => /↑↓\s+navigate.*␣\s+toggle.*esc\s+cancel/i.test(text),
  // Multi-question detection: "←→ switch question" in footer means multi-question.
  multiQuestionDetector: (text) => /↑↓\s+navigate.*←→\s+switch question.*esc\s+cancel/i.test(text),
};

// ── OpenCode patterns ──────────────────────────────────────────────────────────
// Verified from live PTY capture + cli-agent-orchestrator production code.

const opencodePatterns: CliPatterns = {
  statusPatterns: [
    // Permission dialog — highest priority
    { status: "blocked", pattern: /△\s+(?:Permission required|Always allow)\b/, priority: 10 },
    // Question/selector dialog: "↑↓ select  enter <verb>  esc dismiss" footer.
    // This appears when OpenCode asks the user a question with numbered options.
    // The verb after "enter" varies: "confirm" (single-select), "toggle"
    // (multi-select), "submit" (single-select variant). Match any word.
    // Distinct from permission dialog (which has "Allow once/Reject" buttons)
    // and from idle footer (which has "ctrl+p commands" but no "esc dismiss").
    { status: "blocked", pattern: /↑↓\s+select.*enter\s+\w+.*esc\s+dismiss/, priority: 9 },
    // Multi-question Confirm tab: "⇆ tab  enter submit  esc dismiss" footer.
    // The Confirm tab hides ↑↓ select (no options to navigate), but the dialog
    // is still blocked (user must press Enter to submit all answers).
    { status: "blocked", pattern: /⇆\s+tab.*enter\s+submit.*esc\s+dismiss/, priority: 9 },
    // Completion marker: "▣ Build · DeepSeek · 3.9s"
    { status: "done", pattern: /▣\s+\S+\s+·\s+.+?\s+·\s+(?:\d+m\s+)?\d+(?:\.\d+)?s/, priority: 8 },
    // Tool-call spinner: "⠋ Read <path>" etc. — only reliable working indicator
    { status: "working", pattern: /^\s+[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]\s+\S+/m, priority: 7 },
    // Idle footer (appears in both idle and working states, but when no spinner
    // is present, this indicates idle)
    { status: "idle", pattern: /ctrl\+p\s+commands/, priority: 5 },
    // NOTE: "esc interrupt" was removed as a working pattern — it's a static
    // footer that appears in both idle and working states, causing false
    // "working" detection. The spinner pattern above is the reliable indicator.
  ],
  questionExtractor: (text) => {
    const lines = text.split("\n");
    // Permission dialog: "△ Permission required" followed by tool description
    for (let i = 0; i < lines.length; i++) {
      if (/△\s+Permission required/.test(lines[i])) {
        // The next non-empty line is usually the tool/command description
        for (let j = i + 1; j < lines.length && j < i + 5; j++) {
          const trimmed = lines[j].trim();
          if (trimmed && !/^[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]/.test(trimmed)) {
            return `Permission required: ${trimmed}`;
          }
        }
        return "Permission required";
      }
    }
    // Question/selector dialog: find the question text above the numbered options.
    // The selector footer "↑↓ select ... esc dismiss" identifies the dialog.
    // The question is the first non-empty, non-box-drawing line above the
    // first numbered option (1. 2. 3. ...).
    const selectorIdx = lines.findIndex((l) => /↑↓\s+select.*esc\s+dismiss/.test(l));
    if (selectorIdx >= 0) {
      // Find ALL numbered option lines above the footer.
      // Format: "  ┃  1. Rust" (box-drawing char + spaces + number + text)
      const optionIdxs: number[] = [];
      for (let i = 0; i < selectorIdx; i++) {
        if (/^\s*[┃│║]?\s*\d+\.\s+\S/.test(lines[i])) {
          optionIdxs.push(i);
        }
      }
      if (optionIdxs.length > 0) {
        const firstOptionIdx = optionIdxs[0];
        // Walk upward from firstOptionIdx-1 to find the question text
        for (let i = firstOptionIdx - 1; i >= 0; i--) {
          const trimmed = lines[i].trim();
          // Strip leading box-drawing chars for content check
          const content = trimmed.replace(/^[┃│║]\s*/, "").trim();
          // Skip empty lines, box-drawing-only lines, and tab-label lines
          if (content &&
              !/^[┃│║┌┐└┘├┤┬┴┼─━]+$/.test(trimmed) &&
              !/^\s*$/.test(content)) {
            return content;
          }
        }
      }
      return "OpenCode is asking a question";
    }
    return null;
  },
  optionsExtractor: (text) => {
    const lines = text.split("\n");
    // Permission dialog: extract actual button names from the footer line.
    // Footer format: "Allow once   Allow always   Reject   ctrl+f fullscreen  ⇆ select  enter confirm"
    for (const line of lines) {
      if (/△\s+Permission required/.test(line)) {
        // Find the footer line with the buttons (contains "Allow" and "Reject")
        for (const fl of lines) {
          if (/Allow\s+once.*Allow\s+always.*Reject/.test(fl)) {
            // Extract button names: "Allow once", "Allow always", "Reject"
            const buttons = fl.match(/Allow\s+once|Allow\s+always|Reject/g);
            if (buttons && buttons.length > 0) return buttons;
          }
        }
        // Fallback if footer format changes
        return ["Allow once", "Allow always", "Reject"];
      }
    }
    // Question/selector dialog: extract numbered options (1. Rust  2. Python  ...)
    // Format: "  ┃  1. Rust" (single-select) or "  ┃  1. [ ] 单选" (multi-select)
    // Allow box-drawing chars before the number, strip [ ]/[✓] checkbox prefix
    const selectorIdx = lines.findIndex((l) => /↑↓\s+select.*esc\s+dismiss/.test(l));
    if (selectorIdx >= 0) {
      const options: string[] = [];
      for (let i = 0; i < selectorIdx; i++) {
        const m = lines[i].match(/^\s*[┃│║]?\s*(\d+)\.\s+(.+)$/);
        if (m) {
          // Strip [ ] or [✓] checkbox prefix from multi-select options
          const label = m[2].replace(/^\[[\s✓]\]\s*/, "").trim();
          options.push(`${m[1]}. ${label}`);
        }
      }
      if (options.length > 0) return options;
    }
    return null;
  },
  // Multi-select detection: "enter toggle" in footer means multi-select.
  // "enter confirm" or "enter submit" means single-select.
  multiSelectDetector: (text) => /↑↓\s+select.*enter\s+toggle.*esc\s+dismiss/.test(text),
  // Multi-question detection: tab row with "Confirm" at the end.
  // Format: "  ┃   编程语言   测试反馈   下一步   Confirm"
  // Single-question dialogs don't have a tab row.
  multiQuestionDetector: (text) => {
    const lines = text.split("\n");
    return lines.some((l) => {
      // Tab row has multiple tab labels ending with "Confirm"
      const trimmed = l.replace(/^[┃│║]\s*/, "").trim();
      return /\bConfirm\b/.test(trimmed) && /\s{2,}/.test(trimmed) && trimmed.split(/\s{2,}/).length >= 2;
    });
  },
  // Review answers extraction: on the Confirm tab, OpenCode shows a "Review"
  // header followed by lines like "弹窗功能: 多选, 自定义输入, 问题跳过".
  // Extract each "label: values" line as a review answer entry.
  reviewAnswersExtractor: (text) => {
    const lines = text.split("\n");
    // Must have a "Review" header line (identifies the Confirm tab content)
    const reviewIdx = lines.findIndex((l) => l.replace(/^\s*[┃│║]\s*/, "").trim() === "Review");
    if (reviewIdx < 0) return null;
    // Collect "label: values" lines after the Review header.
    // Skip empty lines (answers are separated by blank lines).
    // Stop at footer or tab row.
    const answers: string[] = [];
    for (let i = reviewIdx + 1; i < lines.length; i++) {
      const trimmed = lines[i].replace(/^\s*[┃│║]\s*/, "").trim();
      // Stop at footer or tab row
      if (/⇆\s+tab|enter\s+submit|enter\s+confirm|esc\s+dismiss/.test(trimmed)) break;
      if (/\bConfirm\b/.test(trimmed) && /\s{2,}/.test(trimmed)) break;
      // Skip empty lines
      if (!trimmed) continue;
      // Match "label: values" format (colon separator)
      if (/^[^:]+:\s*.+/.test(trimmed)) {
        answers.push(trimmed);
      }
    }
    return answers.length > 0 ? answers : null;
  },
};

// === SECTION 1 END ===

// ── Claude Code patterns ──────────────────────────────────────────────────────
// Sourced from cli-agent-orchestrator production code.
// Claude Code uses Ink TUI with Braille spinner characters.

const claudeCodePatterns: CliPatterns = {
  statusPatterns: [
    // Selection widget footer — blocked (user must choose)
    { status: "blocked", pattern: /↑\/↓\s+to\s+navigate/, priority: 10 },
    // Plan approval prompt
    { status: "blocked", pattern: /Would you like to proceed\?/, priority: 9 },
    // Workspace trust dialog
    { status: "blocked", pattern: /Yes,\s+I\s+trust\s+this\s+folder/, priority: 9 },
    // Bypass permissions confirmation
    { status: "blocked", pattern: /Yes,\s+I\s+accept/, priority: 9 },
    // Processing spinner: "✻ Cultivating…" etc.
    { status: "working", pattern: /[✶✢✽✻✳·*][^\n]*…/, priority: 8 },
    // Completion summary: "✻ Cultivated for 12s"
    { status: "done", pattern: /[✶✢✽✻✳][^\n…]*\bfor\s+\d+(?:\.\d+)?\s*s\b/, priority: 7 },
    // Idle prompt: ">" or "❯" followed by space
    { status: "idle", pattern: /[>❯][\s\xa0]/, priority: 5 },
  ],
  questionExtractor: (text) => {
    const lines = text.split("\n");
    // Plan approval
    for (const line of lines) {
      if (/Would you like to proceed\?/.test(line)) {
        return "Would you like to proceed?";
      }
    }
    // Trust dialog
    for (const line of lines) {
      if (/Yes,\s+I\s+trust\s+this\s+folder/.test(line)) {
        return "Do you trust this folder?";
      }
    }
    // Selection widget — find the question text above the "↑/↓ to navigate" footer
    for (let i = 0; i < lines.length; i++) {
      if (/↑\/↓\s+to\s+navigate/.test(lines[i])) {
        // Look backwards for the question text
        for (let j = i - 1; j >= 0 && j >= i - 10; j--) {
          const trimmed = lines[j].trim();
          if (trimmed && !/^[✶✢✽✻✳·*]/.test(trimmed) && !/^[>❯]/.test(trimmed)) {
            return trimmed;
          }
        }
        return "Select an option";
      }
    }
    return null;
  },
  optionsExtractor: (text) => {
    const lines = text.split("\n");
    // Plan approval: "Yes" / "No"
    for (const line of lines) {
      if (/Would you like to proceed\?/.test(line)) {
        return ["Yes", "No"];
      }
    }
    // Trust dialog
    for (const line of lines) {
      if (/Yes,\s+I\s+trust\s+this\s+folder/.test(line)) {
        return ["Yes, I trust this folder", "No, I don't trust this folder"];
      }
    }
    // Selection widget — options are listed above the "↑/↓ to navigate" footer
    for (let i = 0; i < lines.length; i++) {
      if (/↑\/↓\s+to\s+navigate/.test(lines[i])) {
        const options: string[] = [];
        // Look backwards for option lines (often indented with ○ or ●)
        for (let j = i - 1; j >= 0 && j >= i - 15; j--) {
          const trimmed = lines[j].trim();
          if (/^[○●]\s+/.test(trimmed)) {
            options.unshift(trimmed.replace(/^[○●]\s+/, ""));
          } else if (options.length > 0) {
            break; // Found options, stop when hitting non-option line
          }
        }
        return options.length > 0 ? options : null;
      }
    }
    return null;
  },
};

// ── Codex patterns ────────────────────────────────────────────────────────────
// Sourced from cli-agent-orchestrator production code.
// Codex uses "Approve/Allow ... y/n" prompts and TUI footer indicators.

const codexPatterns: CliPatterns = {
  statusPatterns: [
    // Approval prompt — blocked
    { status: "blocked", pattern: /^(?:Approve|Allow)\b.*\b(?:y\/n|yes\/no|yes|no)\b/im, priority: 10 },
    // Trust prompt v1
    { status: "blocked", pattern: /allow\s+Codex\s+to\s+work\s+in\s+this\s+folder/i, priority: 10 },
    // Trust prompt v2
    { status: "blocked", pattern: /Do you trust the contents of this directory\?/i, priority: 10 },
    // TUI progress spinner: "• Working (0s • esc to interrupt)"
    // This is the ONLY reliable working indicator for Codex. The prefix text
    // varies ("• Thinking (2s ...)", "• Starting script creation (10s ...)")
    // but the "(Ns • esc to interrupt)" format is consistent.
    // Reference: cli-agent-orchestrator codex.py TUI_PROGRESS_PATTERN.
    { status: "working", pattern: /•.*\(\d+s\s*•\s*esc\s+to\s+interrupt\)/, priority: 8 },
    // NOTE: A keyword-based pattern (\b(?:thinking|working|running|...)\b) was
    // removed because it caused false positives — it matches "running" in
    // "You are running Codex in..." (startup banner), "thinking" in user input
    // ("I'm thinking about..."), and stale keywords in previous AI output.
    // The reference project (cli-agent-orchestrator codex.py line 660-661)
    // explicitly documents this issue and does NOT use keyword matching for
    // status detection. The TUI spinner pattern above is sufficient.
    // Idle prompt: "❯" or "›" or "codex>" (empty prompt only — text after
    // the prompt means the user is typing, not idle)
    { status: "idle", pattern: /^\s*(?:❯|›|codex>)\s*$/m, priority: 5 },
  ],
  questionExtractor: (text) => {
    const lines = text.split("\n");
    // Approval prompt
    for (const line of lines) {
      const m = line.match(/^(?:Approve|Allow)\b.*\b(?:y\/n|yes\/no)\b/i);
      if (m) return line.trim();
    }
    // Trust prompt
    for (const line of lines) {
      if (/allow\s+Codex\s+to\s+work\s+in\s+this\s+folder/i.test(line)) {
        return "Allow Codex to work in this folder?";
      }
      if (/Do you trust the contents of this directory\?/i.test(line)) {
        return "Do you trust the contents of this directory?";
      }
    }
    return null;
  },
  optionsExtractor: (text) => {
    const lines = text.split("\n");
    for (const line of lines) {
      if (/^(?:Approve|Allow)\b.*\b(?:y\/n|yes\/no)\b/i.test(line)) {
        return ["Yes (y)", "No (n)"];
      }
    }
    for (const line of lines) {
      if (/allow\s+Codex\s+to\s+work\s+in\s+this\s+folder/i.test(line) ||
          /Do you trust the contents of this directory\?/i.test(line)) {
        return ["Yes", "No"];
      }
    }
    return null;
  },
};

// ── Pattern registry ──────────────────────────────────────────────────────────

const PATTERNS: Partial<Record<CliType, CliPatterns>> = {
  devin: devinPatterns,
  opencode: opencodePatterns,
  "claude-code": claudeCodePatterns,
  codex: codexPatterns,
};

/**
 * Get the patterns for a specific CLI type.
 * @returns patterns or null if no patterns registered for this CLI.
 */
export function getPatterns(cli: CliType): CliPatterns | null {
  return PATTERNS[cli] ?? null;
}

/**
 * Detect status from screen text using CLI-specific patterns.
 *
 * @param cli    the CLI type
 * @param screenText  ANSI-stripped screen text (from screenScraper)
 * @returns detected status, or null if no pattern matched.
 */
export function detectStatusFromScreen(cli: CliType, screenText: string): AgentStatus | null {
  const patterns = PATTERNS[cli];
  if (!patterns) return null;

  // Sort by priority (highest first)
  const sorted = [...patterns.statusPatterns].sort((a, b) => b.priority - a.priority);

  for (const { status, pattern } of sorted) {
    if (pattern.test(screenText)) {
      return status;
    }
  }

  return null;
}

/**
 * Extract question text from screen when blocked.
 * @returns question text or null.
 */
export function extractQuestion(cli: CliType, screenText: string): string | null {
  const patterns = PATTERNS[cli];
  if (!patterns?.questionExtractor) return null;
  return patterns.questionExtractor(screenText);
}

/**
 * Extract answer options from screen when blocked.
 * @returns array of option strings, or null if not extractable.
 */
export function extractOptions(cli: CliType, screenText: string): string[] | null {
  const patterns = PATTERNS[cli];
  if (!patterns?.optionsExtractor) return null;
  return patterns.optionsExtractor(screenText);
}

/**
 * Detect if the current blocked dialog is multi-select.
 * @returns true if multi-select, false if single-select or N/A.
 */
export function detectMultiSelect(cli: CliType, screenText: string): boolean {
  const patterns = PATTERNS[cli];
  if (!patterns?.multiSelectDetector) return false;
  return patterns.multiSelectDetector(screenText);
}

/**
 * Detect if the current blocked dialog is multi-question (has Confirm tab).
 * @returns true if multi-question, false if single-question or N/A.
 */
export function detectMultiQuestion(cli: CliType, screenText: string): boolean {
  const patterns = PATTERNS[cli];
  if (!patterns?.multiQuestionDetector) return false;
  return patterns.multiQuestionDetector(screenText);
}

/**
 * Extract review answers from the Confirm tab (multi-question dialogs).
 * @returns array of "label: values" strings, or null if not on Confirm tab.
 */
export function extractReviewAnswers(cli: CliType, screenText: string): string[] | null {
  const patterns = PATTERNS[cli];
  if (!patterns?.reviewAnswersExtractor) return null;
  return patterns.reviewAnswersExtractor(screenText);
}
// === SECTION 2 END ===
