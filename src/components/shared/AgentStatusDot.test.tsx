// Unit tests for AgentStatusDot — tab label status indicator
import { describe, it, expect } from "vitest";
import { render } from "@testing-library/react";
import { AgentStatusDot } from "@/components/shared/AgentStatusDot";

describe("AgentStatusDot", () => {
  it("renders nothing for unknown status", () => {
    const { container } = render(<AgentStatusDot status="unknown" />);
    expect(container.firstChild).toBeNull();
  });

  it("renders a yellow spinner for working", () => {
    const { container } = render(<AgentStatusDot status="working" />);
    const dot = container.firstChild as HTMLElement;
    expect(dot).toBeTruthy();
    expect(dot.className).toContain("animate-spin");
    expect(dot.className).toContain("border-yellow-400");
  });

  it("renders a red pulsing dot for blocked", () => {
    const { container } = render(<AgentStatusDot status="blocked" />);
    const dot = container.firstChild as HTMLElement;
    expect(dot).toBeTruthy();
    expect(dot.className).toContain("bg-red-500");
    expect(dot.className).toContain("animate-pulse");
  });

  it("renders a blue dot for done", () => {
    const { container } = render(<AgentStatusDot status="done" />);
    const dot = container.firstChild as HTMLElement;
    expect(dot).toBeTruthy();
    expect(dot.className).toContain("bg-blue-400");
    expect(dot.className).not.toContain("animate-pulse");
  });

  it("renders a gray dot for idle", () => {
    const { container } = render(<AgentStatusDot status="idle" />);
    const dot = container.firstChild as HTMLElement;
    expect(dot).toBeTruthy();
    expect(dot.className).toContain("bg-gray-400");
    expect(dot.className).not.toContain("animate-pulse");
  });

  it("applies custom size", () => {
    const { container } = render(<AgentStatusDot status="done" size={12} />);
    const dot = container.firstChild as HTMLElement;
    expect(dot.style.width).toBe("12px");
    expect(dot.style.height).toBe("12px");
  });

  it("sets aria-label for accessibility", () => {
    const { container } = render(<AgentStatusDot status="blocked" />);
    const dot = container.firstChild as HTMLElement;
    expect(dot.getAttribute("aria-label")).toBe("agent-status-blocked");
  });
});
