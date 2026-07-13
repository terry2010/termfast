// Log store tests — FP-7.1
import { describe, it, expect, beforeEach } from "vitest";
import { useLogStore } from "@/stores/logStore";
import type { LogEntry } from "@/types";

function mockEntry(id: string, overrides: Partial<LogEntry> = {}): LogEntry {
  return {
    id,
    timestamp: new Date().toISOString(),
    server_id: "srv_1",
    level: "info",
    category: "Connection",
    message: "test message",
    execution_id: null,
    command: null,
    exit_code: null,
    stdout: null,
    stderr: null,
    ...overrides,
  };
}

describe("logStore", () => {
  beforeEach(() => {
    useLogStore.setState({
      entries: [],
      filter_level: "all",
      filter_category: "all",
      filter_server_id: null,
      search_query: "",
      expanded: false,
    });
  });

  it("starts empty", () => {
    expect(useLogStore.getState().entries).toEqual([]);
  });

  it("addEntry adds to list", () => {
    useLogStore.getState().addEntry(mockEntry("1"));
    expect(useLogStore.getState().entries).toHaveLength(1);
  });

  it("clear empties the list", () => {
    useLogStore.getState().addEntry(mockEntry("1"));
    useLogStore.getState().clear();
    expect(useLogStore.getState().entries).toHaveLength(0);
  });

  it("filter by level", () => {
    useLogStore.getState().addEntry(mockEntry("1", { level: "info" }));
    useLogStore.getState().addEntry(mockEntry("2", { level: "error" }));
    useLogStore.getState().setFilterLevel("error");
    expect(useLogStore.getState().filteredEntries()).toHaveLength(1);
    expect(useLogStore.getState().filteredEntries()[0].id).toBe("2");
  });

  it("filter by category", () => {
    useLogStore.getState().addEntry(mockEntry("1", { category: "Connection" }));
    useLogStore.getState().addEntry(mockEntry("2", { category: "Trigger" }));
    useLogStore.getState().setFilterCategory("Trigger");
    expect(useLogStore.getState().filteredEntries()).toHaveLength(1);
  });

  it("filter by server", () => {
    useLogStore.getState().addEntry(mockEntry("1", { server_id: "srv_1" }));
    useLogStore.getState().addEntry(mockEntry("2", { server_id: "srv_2" }));
    useLogStore.getState().setFilterServer("srv_1");
    expect(useLogStore.getState().filteredEntries()).toHaveLength(1);
  });

  it("search query filters by message", () => {
    useLogStore.getState().addEntry(mockEntry("1", { message: "connection established" }));
    useLogStore.getState().addEntry(mockEntry("2", { message: "trigger fired" }));
    useLogStore.getState().setSearchQuery("trigger");
    expect(useLogStore.getState().filteredEntries()).toHaveLength(1);
    expect(useLogStore.getState().filteredEntries()[0].id).toBe("2");
  });
});
