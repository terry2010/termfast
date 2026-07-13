// Keyboard shortcuts tests — FP-7.4
import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook } from "@testing-library/react";
import { useKeyboardShortcuts } from "@/hooks/useKeyboardShortcuts";

describe("useKeyboardShortcuts", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("calls onEscape when Escape is pressed", () => {
    const handlers = {
      onSelectServer: vi.fn(),
      onAddServer: vi.fn(),
      onOpenSettings: vi.fn(),
      onFocusLogs: vi.fn(),
      onFocusLogSearch: vi.fn(),
      onToggleProxy: vi.fn(),
      onToggleTriggers: vi.fn(),
      onToggleConnection: vi.fn(),
      onToggleLogPanel: vi.fn(),
      onRefresh: vi.fn(),
      onEscape: vi.fn(),
    };

    renderHook(() => useKeyboardShortcuts(handlers));

    const event = new KeyboardEvent("keydown", { key: "Escape" });
    window.dispatchEvent(event);

    expect(handlers.onEscape).toHaveBeenCalledTimes(1);
  });

  it("calls onRefresh when F5 is pressed", () => {
    const handlers = {
      onSelectServer: vi.fn(),
      onAddServer: vi.fn(),
      onOpenSettings: vi.fn(),
      onFocusLogs: vi.fn(),
      onFocusLogSearch: vi.fn(),
      onToggleProxy: vi.fn(),
      onToggleTriggers: vi.fn(),
      onToggleConnection: vi.fn(),
      onToggleLogPanel: vi.fn(),
      onRefresh: vi.fn(),
      onEscape: vi.fn(),
    };

    renderHook(() => useKeyboardShortcuts(handlers));

    const event = new KeyboardEvent("keydown", { key: "F5" });
    window.dispatchEvent(event);

    expect(handlers.onRefresh).toHaveBeenCalledTimes(1);
  });

  it("calls onAddServer when Cmd+N is pressed", () => {
    const handlers = {
      onSelectServer: vi.fn(),
      onAddServer: vi.fn(),
      onOpenSettings: vi.fn(),
      onFocusLogs: vi.fn(),
      onFocusLogSearch: vi.fn(),
      onToggleProxy: vi.fn(),
      onToggleTriggers: vi.fn(),
      onToggleConnection: vi.fn(),
      onToggleLogPanel: vi.fn(),
      onRefresh: vi.fn(),
      onEscape: vi.fn(),
    };

    renderHook(() => useKeyboardShortcuts(handlers));

    const event = new KeyboardEvent("keydown", { key: "n", metaKey: true });
    window.dispatchEvent(event);

    expect(handlers.onAddServer).toHaveBeenCalledTimes(1);
  });

  it("calls onSelectServer with index for Cmd+1", () => {
    const handlers = {
      onSelectServer: vi.fn(),
      onAddServer: vi.fn(),
      onOpenSettings: vi.fn(),
      onFocusLogs: vi.fn(),
      onFocusLogSearch: vi.fn(),
      onToggleProxy: vi.fn(),
      onToggleTriggers: vi.fn(),
      onToggleConnection: vi.fn(),
      onToggleLogPanel: vi.fn(),
      onRefresh: vi.fn(),
      onEscape: vi.fn(),
    };

    renderHook(() => useKeyboardShortcuts(handlers));

    const event = new KeyboardEvent("keydown", { key: "3", metaKey: true });
    window.dispatchEvent(event);

    expect(handlers.onSelectServer).toHaveBeenCalledWith(2);
  });

  it("calls onOpenSettings when Cmd+, is pressed", () => {
    const handlers = {
      onSelectServer: vi.fn(),
      onAddServer: vi.fn(),
      onOpenSettings: vi.fn(),
      onFocusLogs: vi.fn(),
      onFocusLogSearch: vi.fn(),
      onToggleProxy: vi.fn(),
      onToggleTriggers: vi.fn(),
      onToggleConnection: vi.fn(),
      onToggleLogPanel: vi.fn(),
      onRefresh: vi.fn(),
      onEscape: vi.fn(),
    };

    renderHook(() => useKeyboardShortcuts(handlers));

    const event = new KeyboardEvent("keydown", { key: ",", metaKey: true });
    window.dispatchEvent(event);

    expect(handlers.onOpenSettings).toHaveBeenCalledTimes(1);
  });
});
