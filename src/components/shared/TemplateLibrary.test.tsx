// TemplateLibrary component tests
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { TemplateLibrary } from "@/components/shared/TemplateLibrary";

vi.mock("@/hooks/useIpc", () => ({
  ipcInvoke: vi.fn().mockImplementation((cmd: string) => {
    if (cmd === "ipc_list_templates") return Promise.resolve([
      { id: "tmpl_1", name: "Firewalld IP Update", trigger_type: "OnIpChange", commands: ["firewall-cmd --add-source={{.NewIP}}"], timeout_secs: 30, template_hash: "abc" },
    ]);
    return Promise.resolve(null);
  }),
}));

describe("TemplateLibrary", () => {
  it("renders template library with title", () => {
    render(<TemplateLibrary onClose={vi.fn()} />);
    expect(screen.getByText(/Template|模板/i)).toBeInTheDocument();
  });

  it("has close button", () => {
    render(<TemplateLibrary onClose={vi.fn()} />);
    expect(screen.getByRole("button", { name: /close|关闭/i })).toBeInTheDocument();
  });
});
