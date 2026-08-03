// Unit tests for AgentQuestionOverlay — question popup UI
import { describe, it, expect, vi } from "vitest";
import { render, fireEvent } from "@testing-library/react";
import { AgentQuestionOverlay } from "@/components/shared/AgentQuestionOverlay";

// Mock react-i18next
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

// Default props for all tests
const defaultProps = {
  visible: true,
  status: "blocked" as const,
  cli: "devin" as const,
  question: "Test?" as string | null,
  options: ["1. Yes", "2. No"] as string[] | null,
  isMultiSelect: false,
  isMultiQuestion: false,
  activeTabIndex: -1,
  totalTabs: 0,
  reviewAnswers: null as string[] | null,
  blockedMessage: null as string | null,
  cursorIndex: null as number | null,
  onAnswer: vi.fn(),
  onToggle: vi.fn(),
  onSubmitMultiSelect: vi.fn(),
  onTextAnswer: vi.fn(),
  onTextCancel: vi.fn(),
  onPrevQuestion: vi.fn(),
  onNextQuestion: vi.fn(),
  onConfirm: vi.fn(),
  onDismiss: vi.fn(),
};

describe("AgentQuestionOverlay", () => {
  it("renders nothing when not visible", () => {
    const { container } = render(
      <AgentQuestionOverlay {...defaultProps} visible={false} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders nothing when status is not blocked", () => {
    const { container } = render(
      <AgentQuestionOverlay {...defaultProps} status="working" question={null} options={null} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders question + options when blocked", () => {
    const { container, getByText } = render(
      <AgentQuestionOverlay {...defaultProps} question="Do you want to continue?" />,
    );
    expect(container.firstChild).toBeTruthy();
    expect(getByText("Do you want to continue?")).toBeTruthy();
    expect(getByText("1. Yes")).toBeTruthy();
    expect(getByText("2. No")).toBeTruthy();
  });

  it("shows CLI name in header", () => {
    const { getByText } = render(
      <AgentQuestionOverlay {...defaultProps} cli="opencode" question="Permission required" options={["Allow once", "Reject"]} />,
    );
    expect(getByText(/OpenCode/)).toBeTruthy();
  });

  it("calls onAnswer when option is clicked (single-select)", () => {
    const onAnswer = vi.fn();
    const { getByText } = render(
      <AgentQuestionOverlay {...defaultProps} question="Continue?" onAnswer={onAnswer} />,
    );
    fireEvent.click(getByText("1. Yes"));
    expect(onAnswer).toHaveBeenCalledWith("1. Yes", 0);
  });

  it("calls onDismiss when × button is clicked", () => {
    const onDismiss = vi.fn();
    const { getByLabelText } = render(
      <AgentQuestionOverlay {...defaultProps} question="Continue?" options={["1. Yes"]} onDismiss={onDismiss} />,
    );
    fireEvent.click(getByLabelText("dismiss"));
    expect(onDismiss).toHaveBeenCalled();
  });

  it("shows blockedMessage when question is null", () => {
    const { getByText } = render(
      <AgentQuestionOverlay {...defaultProps} question={null} options={null} blockedMessage="Devin needs input" />,
    );
    expect(getByText("Devin needs input")).toBeTruthy();
  });

  it("shows go-to-terminal button when no options", () => {
    const { getByText } = render(
      <AgentQuestionOverlay {...defaultProps} question="Continue?" options={null} />,
    );
    expect(getByText("server.agent_go_to_terminal")).toBeTruthy();
  });
});

// ── Multi-select mode tests ───────────────────────────────────────────

describe("AgentQuestionOverlay — multi-select", () => {
  it("shows checkboxes + Submit button in multi-select mode", () => {
    const { getByText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="opencode"
        question="Which fruits?"
        options={["1. Apple", "2. Banana", "3. Orange"]}
        isMultiSelect={true}
      />,
    );
    expect(getByText("1. Apple")).toBeTruthy();
    expect(getByText("2. Banana")).toBeTruthy();
    expect(getByText("server.agent_submit")).toBeTruthy();
  });

  it("calls onToggle (not onAnswer) when option clicked in multi-select", () => {
    const onToggle = vi.fn();
    const onAnswer = vi.fn();
    const { getByText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="opencode"
        question="Which fruits?"
        options={["1. Apple", "2. Banana"]}
        isMultiSelect={true}
        onToggle={onToggle}
        onAnswer={onAnswer}
      />,
    );
    fireEvent.click(getByText("1. Apple"));
    expect(onToggle).toHaveBeenCalledWith("1. Apple", 0);
    expect(onAnswer).not.toHaveBeenCalled();
  });

  it("calls onSubmitMultiSelect when Submit is clicked", () => {
    const onSubmitMultiSelect = vi.fn();
    const { getByText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="opencode"
        question="Which fruits?"
        options={["1. Apple", "2. Banana"]}
        isMultiSelect={true}
        onSubmitMultiSelect={onSubmitMultiSelect}
      />,
    );
    fireEvent.click(getByText("server.agent_submit"));
    expect(onSubmitMultiSelect).toHaveBeenCalled();
  });

  it("Devin: toggle preserves checked state across options prop changes (no [✔] sync)", () => {
    // Regression: the sync-from-screen effect was running for all CLIs,
    // but isCheckedOnScreen only matches Claude Code's [✓] markers.
    // Devin's options extractor strips ■/□ markers, so the effect cleared
    // the checked Set on every tick. This test verifies that after
    // toggling an option and re-rendering with a new options array
    // (simulating a screen-scrape tick), the checked state persists.
    const onToggle = vi.fn();
    const { container, getByText, rerender } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="devin"
        question="测试策略？"
        options={["1. 单元测试", "2. 集成测试", "3. E2E 测试"]}
        isMultiSelect={true}
        onToggle={onToggle}
      />,
    );
    // Click option 1 → toggle it on
    fireEvent.click(getByText("1. 单元测试"));
    expect(onToggle).toHaveBeenCalledWith("1. 单元测试", 0);
    // Re-render with a new options array (same content, new reference)
    // This simulates useAgentStatus creating a new state object each tick.
    rerender(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="devin"
        question="测试策略？"
        options={["1. 单元测试", "2. 集成测试", "3. E2E 测试"]}
        isMultiSelect={true}
        onToggle={onToggle}
      />,
    );
    // The checkbox SVG (checkmark path) should still be present for option 1.
    // If the sync effect wrongly cleared checked, the SVG would be gone.
    const svgs = container.querySelectorAll("svg");
    // At least one SVG (the checkmark) should be present
    expect(svgs.length).toBeGreaterThanOrEqual(1);
  });

  it("Claude Code: syncs checked state from [✔] markers in options", () => {
    // Claude Code includes [✔] (U+2714) markers in option labels.
    // The sync effect should parse these and set the checked Set accordingly.
    // stripCheckbox now correctly strips [✔] (U+2714) as well as [✓] (U+2713).
    const { container } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="claude-code"
        question="Which features?"
        options={["1. [✔] Feature A", "2. [ ] Feature B", "3. [✔] Feature C"]}
        isMultiSelect={true}
      />,
    );
    // 2 options have [✔] → 2 SVG checkmark icons should be rendered
    const svgs = container.querySelectorAll("svg");
    expect(svgs.length).toBe(2);
  });

  it("Claude Code: stripCheckbox removes [✔] (U+2714) from option labels", () => {
    // Regression: stripCheckbox used [✓] (U+2713) but Claude Code renders
    // [✔] (U+2714 HEAVY CHECK MARK). The old regex didn't strip [✔],
    // leaving the checkbox marker in the displayed option text.
    const { getByText, queryByText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="claude-code"
        question="Which features?"
        options={["1. [✔] Feature A", "2. [ ] Feature B"]}
        isMultiSelect={true}
      />,
    );
    // The option text should NOT contain "[✔]" — it should show "1. Feature A"
    expect(getByText("1. Feature A")).toBeTruthy();
    expect(queryByText("1. [✔] Feature A")).toBeFalsy();
    expect(getByText("2. Feature B")).toBeTruthy();
    expect(queryByText("2. [ ] Feature B")).toBeFalsy();
  });
});

// ── Type your own answer tests ────────────────────────────────────────

describe("AgentQuestionOverlay — type your own answer", () => {
  it("switches to text input mode when 'Type your own answer' is clicked", () => {
    const { getByText, getByPlaceholderText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="opencode"
        question="Pick one:"
        options={["1. Apple", "2. Type your own answer"]}
      />,
    );
    fireEvent.click(getByText("2. Type your own answer"));
    expect(getByPlaceholderText("server.agent_type_answer_placeholder")).toBeTruthy();
  });

  it("switches to text input mode when Devin 'Other (type your own)' is clicked", () => {
    const onAnswer = vi.fn();
    const { getByText, getByPlaceholderText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="devin"
        question="Pick one:"
        options={["1. Apple", "2. Banana", "Other (type your own)"]}
        onAnswer={onAnswer}
      />,
    );
    // Click "Other (type your own)" — should enter text mode, NOT call onAnswer
    fireEvent.click(getByText("Other (type your own)"));
    expect(getByPlaceholderText("server.agent_type_answer_placeholder")).toBeTruthy();
    expect(onAnswer).not.toHaveBeenCalled();
  });

  it("calls onTextAnswer when text is entered and Send is clicked", () => {
    const onTextAnswer = vi.fn();
    const { getByText, getByPlaceholderText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="opencode"
        question="Pick one:"
        options={["1. Apple", "2. Type your own answer"]}
        onTextAnswer={onTextAnswer}
      />,
    );
    fireEvent.click(getByText("2. Type your own answer"));
    const input = getByPlaceholderText("server.agent_type_answer_placeholder");
    fireEvent.change(input, { target: { value: "My custom answer" } });
    fireEvent.click(getByText("server.agent_type_answer_submit"));
    expect(onTextAnswer).toHaveBeenCalledWith("2. Type your own answer", "My custom answer", 1, false);
  });

  it("exits text mode when Cancel button is clicked", () => {
    const { getByText, getByPlaceholderText, queryByPlaceholderText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="opencode"
        question="Pick one:"
        options={["1. Apple", "2. Type your own answer"]}
      />,
    );
    fireEvent.click(getByText("2. Type your own answer"));
    expect(getByPlaceholderText("server.agent_type_answer_placeholder")).toBeTruthy();
    // Click the "Back to options" button (i18n key "server.agent_back_to_options" renders as "server.agent_back_to_options" in test mock)
    fireEvent.click(getByText("server.agent_back_to_options"));
    expect(queryByPlaceholderText("server.agent_type_answer_placeholder")).toBeNull();
    // Options should be visible again
    expect(getByText("1. Apple")).toBeTruthy();
  });

  it("calls onTextCancel when Cancel button is clicked (to exit CLI text mode)", () => {
    const onTextCancel = vi.fn();
    const { getByText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="devin"
        question="Pick one:"
        options={["1. Apple", "Other (type your own)"]}
        onTextCancel={onTextCancel}
      />,
    );
    fireEvent.click(getByText("Other (type your own)"));
    fireEvent.click(getByText("server.agent_back_to_options"));
    // onTextCancel should be called so the terminal sends Escape to exit
    // Devin's text editing mode
    expect(onTextCancel).toHaveBeenCalledTimes(1);
  });

  it("shows checkbox checked + typed text after multi-select text answer submit", () => {
    const onTextAnswer = vi.fn();
    const { getByText, getByPlaceholderText, container } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="opencode"
        question="Pick all:"
        options={["1. Apple", "2. Type your own answer"]}
        isMultiSelect={true}
        onTextAnswer={onTextAnswer}
      />,
    );
    // Click "Type your own answer" to enter text mode
    fireEvent.click(getByText("2. Type your own answer"));
    const input = getByPlaceholderText("server.agent_type_answer_placeholder");
    fireEvent.change(input, { target: { value: "my custom text" } });
    fireEvent.click(getByText("server.agent_type_answer_submit"));
    // onTextAnswer should be called
    expect(onTextAnswer).toHaveBeenCalledWith("2. Type your own answer", "my custom text", 1, false);
    // The checkbox for option 2 should be checked (blue bg)
    const buttons = container.querySelectorAll("button");
    const option2Btn = Array.from(buttons).find((b) => b.textContent?.includes("Type your own answer"));
    expect(option2Btn).toBeTruthy();
    const checkbox = option2Btn!.querySelector("span.inline-flex");
    expect(checkbox).toBeTruthy();
    expect(checkbox!.className).toContain("bg-blue-600");
    // The typed text should be displayed next to the option label
    expect(option2Btn!.textContent).toContain("my custom text");
  });

  it("pre-fills previous text when re-entering text mode in multi-select", () => {
    const { getByText, getByPlaceholderText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="opencode"
        question="Pick all:"
        options={["1. Apple", "2. Type your own answer"]}
        isMultiSelect={true}
      />,
    );
    // First: enter text mode and submit
    fireEvent.click(getByText("2. Type your own answer"));
    let input = getByPlaceholderText("server.agent_type_answer_placeholder");
    fireEvent.change(input, { target: { value: "first answer" } });
    fireEvent.click(getByText("server.agent_type_answer_submit"));
    // Second: re-enter text mode
    fireEvent.click(getByText("2. Type your own answer"));
    input = getByPlaceholderText("server.agent_type_answer_placeholder");
    // Should be pre-filled with the previous answer
    expect((input as HTMLInputElement).value).toBe("first answer");
  });
});

// ── Question change reset tests ───────────────────────────────────────

describe("AgentQuestionOverlay — question change reset", () => {
  it("resets text mode when question changes (new question in sequence)", () => {
    const { getByText, queryByPlaceholderText, rerender } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="opencode"
        question="Question 1?"
        options={["1. Apple", "2. Type your own answer"]}
      />,
    );
    // Enter text mode
    fireEvent.click(getByText("2. Type your own answer"));
    expect(queryByPlaceholderText("server.agent_type_answer_placeholder")).toBeTruthy();
    // Re-render with a new question
    rerender(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="opencode"
        question="Question 2?"
        options={["1. Banana", "2. Cherry"]}
      />,
    );
    // Text mode should be reset — options visible again
    expect(queryByPlaceholderText("server.agent_type_answer_placeholder")).toBeNull();
    expect(getByText("1. Banana")).toBeTruthy();
  });

  it("does NOT reset text mode when question becomes fallback (Devin screen redraw)", () => {
    // Devin scenario: user enters text mode → screen redraws → option numbers
    // disappear → questionExtractor returns "Devin is asking a question" (fallback)
    // → questionKey changes → but textMode should stay open.
    const { getByText, queryByPlaceholderText, rerender } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="devin"
        question="这是第 1 个问题：你选哪个？"
        options={["1. 选项 A", "2. 选项 B", "Other (type your own)"]}
      />,
    );
    // Enter text mode
    fireEvent.click(getByText("Other (type your own)"));
    expect(queryByPlaceholderText("server.agent_type_answer_placeholder")).toBeTruthy();
    // Re-render with fallback question (Devin screen redraw during text editing)
    rerender(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="devin"
        question="Devin is asking a question"
        options={["Other (type your own)"]}
      />,
    );
    // Text mode should NOT be reset — input still visible
    expect(queryByPlaceholderText("server.agent_type_answer_placeholder")).toBeTruthy();
  });

  it("does NOT reset text mode when question becomes empty (Devin screen redraw)", () => {
    const { getByText, queryByPlaceholderText, rerender } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="devin"
        question="Pick one:"
        options={["1. Apple", "Other (type your own)"]}
      />,
    );
    fireEvent.click(getByText("Other (type your own)"));
    expect(queryByPlaceholderText("server.agent_type_answer_placeholder")).toBeTruthy();
    // Re-render with empty question (extraction failed completely)
    rerender(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="devin"
        question={null}
        options={null}
      />,
    );
    // Text mode should NOT be reset — input still visible
    expect(queryByPlaceholderText("server.agent_type_answer_placeholder")).toBeTruthy();
  });

  it("resets checked state when question changes in multi-select", () => {
    const { getByText, rerender, container } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="opencode"
        question="Question 1?"
        options={["1. Apple", "2. Banana"]}
        isMultiSelect={true}
      />,
    );
    // Toggle first option
    fireEvent.click(getByText("1. Apple"));
    // Verify checkbox is checked (blue background)
    const checkbox = container.querySelector("button .bg-blue-600");
    expect(checkbox).toBeTruthy();
    // Re-render with new question
    rerender(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="opencode"
        question="Question 2?"
        options={["1. Cherry", "2. Grape"]}
        isMultiSelect={true}
      />,
    );
    // Checked state should be reset — no blue checkboxes
    const checkedBox = container.querySelector("button .bg-blue-600");
    expect(checkedBox).toBeNull();
  });

  // ── Multi-question mode tests ──────────────────────────────────────────
  it("shows Prev/Next/Confirm buttons in multi-question mode (last question)", () => {
    const { getByText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="opencode"
        question="你最喜欢哪种编程语言？"
        options={["1. Rust", "2. TypeScript", "3. Python"]}
        isMultiQuestion={true}
        activeTabIndex={2}
        totalTabs={4}
      />,
    );
    expect(getByText(/server.agent_prev_question/)).toBeTruthy();
    expect(getByText(/server.agent_next_question/)).toBeTruthy();
    expect(getByText(/server.agent_confirm/)).toBeTruthy();
  });

  it("does not show Confirm button when not on last question", () => {
    const { getByText, queryByText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="opencode"
        question="Q1?"
        options={["1. A", "2. B"]}
        isMultiQuestion={true}
        activeTabIndex={0}
        totalTabs={4}
      />,
    );
    // Prev/Next still shown, but Confirm NOT shown (not last question)
    expect(getByText(/server.agent_prev_question/)).toBeTruthy();
    expect(getByText(/server.agent_next_question/)).toBeTruthy();
    expect(queryByText(/server.agent_confirm/)).toBeNull();
  });

  it("Devin: shows Skip button (not Confirm) when on last question (single-select)", () => {
    // Devin has NO separate Confirm tab — all tabs are question tabs.
    // Last question = totalTabs - 1 (not totalTabs - 2 like OpenCode).
    // Single-select: shows "Skip (Esc)" instead of "Confirm" because
    // Enter would select the first option (cursor default).
    const { getByText, queryByText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="devin"
        question="这是最后一个问题"
        options={["1. 选项 A", "2. 选项 B"]}
        isMultiQuestion={true}
        activeTabIndex={3}
        totalTabs={4}
      />,
    );
    expect(getByText(/server.agent_prev_question/)).toBeTruthy();
    // Next button should be hidden on last question (prevent wrap)
    expect(queryByText(/server.agent_next_question/)).toBeNull();
    // Single-select: Skip button shown, Confirm NOT shown
    expect(getByText(/server.agent_skip/)).toBeTruthy();
    expect(queryByText(/server.agent_confirm/)).toBeNull();
  });

  it("Devin: shows disabled hint on last question after an earlier answer", () => {
    const onConfirm = vi.fn();
    const { getByText, queryByText, rerender, container } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="devin"
        question="Q2?"
        options={["1. A", "2. B"]}
        isMultiQuestion={true}
        activeTabIndex={1}
        totalTabs={4}
        onConfirm={onConfirm}
      />,
    );
    fireEvent.click(getByText("1. A"));
    rerender(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="devin"
        question="Q4?"
        options={["1. A", "2. B"]}
        isMultiQuestion={true}
        activeTabIndex={3}
        totalTabs={4}
        onConfirm={onConfirm}
      />,
    );
    // Should show disabled hint, not Confirm or Skip
    expect(getByText(/server.agent_select_to_submit/)).toBeTruthy();
    expect(queryByText(/server.agent_confirm/)).toBeNull();
    expect(queryByText(/server.agent_skip/)).toBeNull();
    // Should have cursor-not-allowed and title tooltip
    const hint = container.querySelector(".cursor-not-allowed");
    expect(hint).toBeTruthy();
    expect(hint?.getAttribute("title")).toBeTruthy();
  });

  it("Devin: does not show Confirm button when not on last question", () => {
    const { getByText, queryByText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="devin"
        question="Q1?"
        options={["1. A", "2. B"]}
        isMultiQuestion={true}
        activeTabIndex={0}
        totalTabs={4}
      />,
    );
    // First question (Devin): Prev hidden (prevent wrap), Next shown
    expect(queryByText(/server.agent_prev_question/)).toBeNull();
    expect(getByText(/server.agent_next_question/)).toBeTruthy();
    // activeTabIndex=0, totalTabs=4, lastQuestion=3 → NOT last question
    expect(queryByText(/server.agent_confirm/)).toBeNull();
  });

  it("Devin: shows both Prev and Next on middle question", () => {
    const { getByText, queryByText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="devin"
        question="Q2?"
        options={["1. A", "2. B"]}
        isMultiQuestion={true}
        activeTabIndex={1}
        totalTabs={4}
      />,
    );
    // Middle question: both Prev and Next shown
    expect(getByText(/server.agent_prev_question/)).toBeTruthy();
    expect(getByText(/server.agent_next_question/)).toBeTruthy();
    expect(queryByText(/server.agent_confirm/)).toBeNull();
  });

  it("does not show Prev/Next/Confirm buttons in single-question mode", () => {
    const { queryByText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="opencode"
        question="Continue?"
        options={["1. Yes", "2. No"]}
        isMultiQuestion={false}
      />,
    );
    expect(queryByText(/server.agent_prev_question/)).toBeNull();
    expect(queryByText(/server.agent_next_question/)).toBeNull();
    expect(queryByText(/server.agent_confirm/)).toBeNull();
  });

  it("calls onPrevQuestion when Prev button is clicked", () => {
    const onPrevQuestion = vi.fn();
    const { getByText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="opencode"
        question="Q1?"
        options={["1. A", "2. B"]}
        isMultiQuestion={true}
        activeTabIndex={0}
        totalTabs={4}
        onPrevQuestion={onPrevQuestion}
      />,
    );
    fireEvent.click(getByText(/server.agent_prev_question/));
    expect(onPrevQuestion).toHaveBeenCalledTimes(1);
  });

  it("calls onNextQuestion when Next button is clicked", () => {
    const onNextQuestion = vi.fn();
    const { getByText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="opencode"
        question="Q1?"
        options={["1. A", "2. B"]}
        isMultiQuestion={true}
        activeTabIndex={0}
        totalTabs={4}
        onNextQuestion={onNextQuestion}
      />,
    );
    fireEvent.click(getByText(/server.agent_next_question/));
    expect(onNextQuestion).toHaveBeenCalledTimes(1);
  });

  it("calls onConfirm when Confirm button is clicked (last question)", () => {
    const onConfirm = vi.fn();
    const { getByText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="opencode"
        question="Q1?"
        options={["1. A", "2. B"]}
        isMultiQuestion={true}
        activeTabIndex={2}
        totalTabs={4}
        onConfirm={onConfirm}
      />,
    );
    fireEvent.click(getByText(/server.agent_confirm/));
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("shows 'Back to options' button in text mode (not 'Cancel')", () => {
    const { getByText, queryByText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="opencode"
        question="Q1?"
        options={["1. A", "2. Type your own answer"]}
      />,
    );
    fireEvent.click(getByText("2. Type your own answer"));
    // Should show "Back to options" not "Cancel"
    expect(getByText("server.agent_back_to_options")).toBeTruthy();
    expect(queryByText("server.agent_cancel")).toBeNull();
  });

  it("shows Confirm button when no options + multi-question (Confirm tab)", () => {
    const onConfirm = vi.fn();
    const { getByText, queryByText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="opencode"
        question={null}
        options={null}
        isMultiQuestion={true}
        activeTabIndex={3}
        totalTabs={4}
        onConfirm={onConfirm}
      />,
    );
    // Should show review text + Confirm button, not "Go to terminal"
    expect(getByText("server.agent_review_answers")).toBeTruthy();
    expect(getByText(/server.agent_confirm/)).toBeTruthy();
    expect(queryByText("server.agent_go_to_terminal")).toBeNull();
    // Clicking Confirm calls onConfirm
    fireEvent.click(getByText(/server.agent_confirm/));
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("shows Go to terminal when no options + single-question", () => {
    const { getByText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="opencode"
        question={null}
        options={null}
        isMultiQuestion={false}
      />,
    );
    expect(getByText("server.agent_go_to_terminal")).toBeTruthy();
  });

  it("shows Prev/Next/Confirm in multi-select + multi-question mode (last question)", () => {
    const onPrevQuestion = vi.fn();
    const onNextQuestion = vi.fn();
    const onConfirm = vi.fn();
    const { getByText, queryByText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="opencode"
        question="Which features?"
        options={["1. A", "2. B", "3. C"]}
        isMultiSelect={true}
        isMultiQuestion={true}
        activeTabIndex={2}
        totalTabs={4}
        onPrevQuestion={onPrevQuestion}
        onNextQuestion={onNextQuestion}
        onConfirm={onConfirm}
      />,
    );
    // Should show Prev/Next/Confirm, NOT Submit
    expect(getByText(/server.agent_prev_question/)).toBeTruthy();
    expect(getByText(/server.agent_next_question/)).toBeTruthy();
    expect(getByText(/server.agent_confirm/)).toBeTruthy();
    expect(queryByText("server.agent_submit")).toBeNull();
    // Clicking Confirm calls onConfirm
    fireEvent.click(getByText(/server.agent_confirm/));
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("hides Confirm in multi-select + multi-question when not last question", () => {
    const { getByText, queryByText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="opencode"
        question="Which features?"
        options={["1. A", "2. B"]}
        isMultiSelect={true}
        isMultiQuestion={true}
        activeTabIndex={0}
        totalTabs={4}
      />,
    );
    expect(getByText(/server.agent_prev_question/)).toBeTruthy();
    expect(getByText(/server.agent_next_question/)).toBeTruthy();
    expect(queryByText(/server.agent_confirm/)).toBeNull();
  });

  it("shows Submit (not Prev/Next/Confirm) in multi-select + single-question", () => {
    const { getByText, queryByText } = render(
      <AgentQuestionOverlay
        {...defaultProps}
        cli="opencode"
        question="Which features?"
        options={["1. A", "2. B"]}
        isMultiSelect={true}
        isMultiQuestion={false}
      />,
    );
    expect(getByText("server.agent_submit")).toBeTruthy();
    expect(queryByText(/server.agent_prev_question/)).toBeNull();
    expect(queryByText(/server.agent_confirm/)).toBeNull();
  });
});
