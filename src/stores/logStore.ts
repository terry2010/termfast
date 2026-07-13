// Log store — Zustand
// §4.2 logStore: log entries + filter

import { create } from "zustand";
import type { LogEntry } from "@/types";

export type LogLevel = "all" | "info" | "warn" | "error";
export type LogCategory = "all" | "Connection" | "Trigger" | "Proxy" | "Config" | "Error" | "System";

interface LogStore {
  entries: LogEntry[];
  filter_level: LogLevel;
  filter_category: LogCategory;
  filter_server_id: string | null;
  search_query: string;
  expanded: boolean;

  addEntry: (entry: LogEntry) => void;
  setEntries: (entries: LogEntry[]) => void;
  clear: () => void;
  setFilterLevel: (level: LogLevel) => void;
  setFilterCategory: (category: LogCategory) => void;
  setFilterServer: (serverId: string | null) => void;
  setSearchQuery: (query: string) => void;
  setExpanded: (expanded: boolean) => void;

  filteredEntries: () => LogEntry[];
}

export const useLogStore = create<LogStore>((set, get) => ({
  entries: [],
  filter_level: "all",
  filter_category: "all",
  filter_server_id: null,
  search_query: "",
  expanded: false,

  addEntry: (entry) =>
    set((state) => ({ entries: [...state.entries, entry] })),

  setEntries: (entries) => set({ entries }),

  clear: () => set({ entries: [] }),

  setFilterLevel: (level) => set({ filter_level: level }),
  setFilterCategory: (category) => set({ filter_category: category }),
  setFilterServer: (serverId) => set({ filter_server_id: serverId }),
  setSearchQuery: (query) => set({ search_query: query }),
  setExpanded: (expanded) => set({ expanded }),

  filteredEntries: () => {
    const state = get();
    return state.entries.filter((entry) => {
      if (state.filter_level !== "all" && entry.level !== state.filter_level) {
        return false;
      }
      if (state.filter_category !== "all" && entry.category !== state.filter_category) {
        return false;
      }
      if (state.filter_server_id && entry.server_id !== state.filter_server_id) {
        return false;
      }
      if (state.search_query) {
        const q = state.search_query.toLowerCase();
        if (
          !entry.message.toLowerCase().includes(q) &&
          !(entry.command?.toLowerCase().includes(q) ?? false)
        ) {
          return false;
        }
      }
      return true;
    });
  },
}));
// test2 1783863643
