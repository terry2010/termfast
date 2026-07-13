// PendingEventsBanner component tests
import { describe, it, expect, vi } from "vitest";
import { render } from "@testing-library/react";
import { PendingEventsBanner } from "@/components/shared/PendingEventsBanner";

vi.mock("@/hooks/useIpc", () => ({
  ipcInvoke: vi.fn().mockResolvedValue({ pending_events: [] }),
  useTauriEvent: vi.fn(() => ({ data: null, loading: false })),
}));

describe("PendingEventsBanner", () => {
  it("renders without crashing", () => {
    const { container } = render(<PendingEventsBanner />);
    expect(container).toBeTruthy();
  });
});
