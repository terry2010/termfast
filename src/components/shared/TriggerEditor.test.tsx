// TriggerEditor component tests
import { describe, it, expect, vi } from "vitest";

vi.mock("@/hooks/useIpc", () => ({
  ipcInvoke: vi.fn().mockResolvedValue(null),
}));

vi.mock("@codemirror/state", () => ({
  EditorState: {
    create: vi.fn(() => ({})),
    tabSize: { of: vi.fn() },
  },
}));
vi.mock("@codemirror/view", () => {
  return {
    EditorView: class MockEditorView {
      dom = document.createElement("div");
      constructor() {}
      destroy() {}
    },
    keymap: { of: vi.fn() },
    lineNumbers: vi.fn(),
  };
});
vi.mock("@codemirror/commands", () => ({
  defaultKeymap: [],
  history: vi.fn(),
  historyKeymap: [],
}));
vi.mock("@codemirror/theme-one-dark", () => ({
  oneDark: {},
}));
vi.mock("@codemirror/language", () => ({
  StreamLanguage: { define: vi.fn() },
}));
vi.mock("@codemirror/legacy-modes/mode/shell", () => ({
  shell: {},
}));

const { TriggerEditor } = await import("@/components/shared/TriggerEditor");
const { render } = await import("@testing-library/react");

describe("TriggerEditor", () => {
  it("renders without crashing for new trigger", () => {
    const { container } = render(
      <TriggerEditor serverId="s1" trigger={null} onClose={vi.fn()} />
    );
    expect(container).toBeTruthy();
  });
});
