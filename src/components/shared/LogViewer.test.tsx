// LogViewer component tests
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { LogViewer } from "@/components/shared/LogViewer";
import { useLogStore } from "@/stores/logStore";

beforeEach(() => {
  useLogStore.setState({ entries: [], filter_level: "all", filter_category: "all", filter_server_id: null, search_query: "", expanded: false });
});

describe("LogViewer", () => {
  it("renders log viewer with title", () => {
    render(<LogViewer onClose={vi.fn()} />);
    expect(screen.getByText(/Logs|日志/i)).toBeInTheDocument();
  });

  it("has close button (×)", () => {
    render(<LogViewer onClose={vi.fn()} />);
    expect(screen.getByText("×")).toBeInTheDocument();
  });
});
