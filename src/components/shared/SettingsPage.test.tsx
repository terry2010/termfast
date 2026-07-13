// SettingsPage component tests
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { SettingsPage } from "@/components/shared/SettingsPage";

const mockConfig = {
  version: 1,
  general: { language: "en", theme: "light", auto_start: false, minimize_to_tray: true, log_level: "info" },
  trigger_templates: [],
  servers: [],
};

vi.mock("@/hooks/useIpc", () => ({
  ipcInvoke: vi.fn().mockResolvedValue(null),
}));

vi.mock("@/stores/configStore", () => ({
  useConfigStore: vi.fn((selector) => {
    const state = {
      config: mockConfig,
      updateGeneral: vi.fn(),
      setConfig: vi.fn(),
    };
    return selector ? selector(state) : state;
  }),
}));

describe("SettingsPage", () => {
  it("renders without crashing", () => {
    const { container } = render(<SettingsPage onClose={vi.fn()} />);
    expect(container).toBeTruthy();
  });

  it("has close button", () => {
    render(<SettingsPage onClose={vi.fn()} />);
    const buttons = screen.getAllByRole("button");
    expect(buttons.length).toBeGreaterThan(0);
  });
});
