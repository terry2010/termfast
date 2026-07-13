// AddServerDialog component tests
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { AddServerDialog } from "@/components/shared/AddServerDialog";

vi.mock("@/hooks/useIpc", () => ({
  ipcInvoke: vi.fn().mockResolvedValue("srv_new"),
}));

describe("AddServerDialog", () => {
  it("renders dialog with add server title", () => {
    render(<AddServerDialog onAdd={vi.fn()} onCancel={vi.fn()} />);
    // t("server.add") renders as heading
    const heading = screen.getByRole("heading");
    expect(heading).toBeInTheDocument();
  });

  it("has input fields for server config", () => {
    render(<AddServerDialog onAdd={vi.fn()} onCancel={vi.fn()} />);
    const inputs = screen.getAllByRole("textbox");
    expect(inputs.length).toBeGreaterThanOrEqual(3);
  });

  it("calls onCancel when cancel button clicked", () => {
    const onCancel = vi.fn();
    render(<AddServerDialog onAdd={vi.fn()} onCancel={onCancel} />);
    fireEvent.click(screen.getByText(/Cancel|取消/i));
    expect(onCancel).toHaveBeenCalled();
  });
});
