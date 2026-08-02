// Unit tests for AgentQuestionOverlay — question popup UI
import { describe, it, expect, vi } from "vitest";
import { render, fireEvent } from "@testing-library/react";
import { AgentQuestionOverlay } from "@/components/shared/AgentQuestionOverlay";

// Mock react-i18next
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe("AgentQuestionOverlay", () => {
  it("renders nothing when not visible", () => {
    const { container } = render(
      <AgentQuestionOverlay
        visible={false}
        status="blocked"
        cli="devin"
        question="Test?"
        options={["1. Yes", "2. No"]}
        blockedMessage={null}
        onAnswer={vi.fn()}
        onDismiss={vi.fn()}
      />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders nothing when status is not blocked", () => {
    const { container } = render(
      <AgentQuestionOverlay
        visible={true}
        status="working"
        cli="devin"
        question={null}
        options={null}
        blockedMessage={null}
        onAnswer={vi.fn()}
        onDismiss={vi.fn()}
      />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders question + options when blocked", () => {
    const { container, getByText } = render(
      <AgentQuestionOverlay
        visible={true}
        status="blocked"
        cli="devin"
        question="Do you want to continue?"
        options={["1. Yes", "2. No"]}
        blockedMessage={null}
        onAnswer={vi.fn()}
        onDismiss={vi.fn()}
      />,
    );
    expect(container.firstChild).toBeTruthy();
    expect(getByText("Do you want to continue?")).toBeTruthy();
    expect(getByText("1. Yes")).toBeTruthy();
    expect(getByText("2. No")).toBeTruthy();
  });

  it("shows CLI name in header", () => {
    const { getByText } = render(
      <AgentQuestionOverlay
        visible={true}
        status="blocked"
        cli="opencode"
        question="Permission required"
        options={["Allow", "Deny"]}
        blockedMessage={null}
        onAnswer={vi.fn()}
        onDismiss={vi.fn()}
      />,
    );
    expect(getByText(/OpenCode/)).toBeTruthy();
  });

  it("calls onAnswer when option is clicked", () => {
    const onAnswer = vi.fn();
    const { getByText } = render(
      <AgentQuestionOverlay
        visible={true}
        status="blocked"
        cli="devin"
        question="Continue?"
        options={["1. Yes", "2. No"]}
        blockedMessage={null}
        onAnswer={onAnswer}
        onDismiss={vi.fn()}
      />,
    );
    fireEvent.click(getByText("1. Yes"));
    expect(onAnswer).toHaveBeenCalledWith("1. Yes", 0);
  });

  it("calls onDismiss when × button is clicked", () => {
    const onDismiss = vi.fn();
    const { getByLabelText } = render(
      <AgentQuestionOverlay
        visible={true}
        status="blocked"
        cli="devin"
        question="Continue?"
        options={["1. Yes"]}
        blockedMessage={null}
        onAnswer={vi.fn()}
        onDismiss={onDismiss}
      />,
    );
    fireEvent.click(getByLabelText("dismiss"));
    expect(onDismiss).toHaveBeenCalled();
  });

  it("shows blockedMessage when question is null", () => {
    const { getByText } = render(
      <AgentQuestionOverlay
        visible={true}
        status="blocked"
        cli="devin"
        question={null}
        options={null}
        blockedMessage="Devin needs input"
        onAnswer={vi.fn()}
        onDismiss={vi.fn()}
      />,
    );
    expect(getByText("Devin needs input")).toBeTruthy();
  });

  it("shows go-to-terminal button when no options", () => {
    const { getByText } = render(
      <AgentQuestionOverlay
        visible={true}
        status="blocked"
        cli="devin"
        question="Continue?"
        options={null}
        blockedMessage={null}
        onAnswer={vi.fn()}
        onDismiss={vi.fn()}
      />,
    );
    expect(getByText("agent_go_to_terminal")).toBeTruthy();
  });
});
