// TriggerList component tests
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render } from "@testing-library/react";

vi.mock("@/hooks/useIpc", () => ({
  ipcInvoke: vi.fn().mockResolvedValue(null),
}));

vi.mock("@/components/shared/TriggerEditor", () => ({
  TriggerEditor: () => null,
}));

const { TriggerList } = await import("@/components/shared/TriggerList");
const { useTriggerStore } = await import("@/stores/triggerStore");

const emptyTriggers: never[] = [];

beforeEach(() => {
  useTriggerStore.setState({
    serverTriggers: { s1: emptyTriggers },
    executing: {},
    templates: [],
  });
});

describe("TriggerList", () => {
  it("renders without crashing with no triggers", () => {
    const { container } = render(<TriggerList serverId="s1" />);
    expect(container).toBeTruthy();
  });
});
