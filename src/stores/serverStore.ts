// Server store — Zustand
// §4.2 serverStore: server list + status

import { create } from "zustand";
import type { ServerConfig, ServerStatus } from "@/types";

export interface ServerState extends ServerConfig {
  current_status: ServerStatus;
  current_ip: string | null;
  connected_since: string | null;
  reconnect_count: number;
  max_attempts: number;
  proxy_running: boolean;
  active_channels: number;
  bytes_in: number;
  bytes_out: number;
}

interface ServerStore {
  servers: ServerState[];
  selected_server_id: string | null;
  active_tabs: Record<string, string>;
  loading: boolean;

  setServers: (servers: ServerState[]) => void;
  updateServerStatus: (serverId: string, status: ServerStatus, ip?: string) => void;
  selectServer: (serverId: string | null) => void;
  setActiveTab: (tab: string) => void;
  getActiveTab: () => string;
  addServer: (server: ServerState) => void;
  removeServer: (serverId: string) => void;
  setLoading: (loading: boolean) => void;
  setProxyStatus: (serverId: string, running: boolean) => void;
}

export const useServerStore = create<ServerStore>((set, get) => ({
  servers: [],
  selected_server_id: null,
  active_tabs: {},
  loading: false,

  setServers: (servers) => set({ servers }),

  updateServerStatus: (serverId, status, ip) =>
    set((state) => ({
      servers: state.servers.map((s) =>
        s.id === serverId
          ? {
              ...s,
              current_status: status,
              current_ip: ip ?? s.current_ip,
              connected_since:
                status === "connected" ? new Date().toISOString() : null,
            }
          : s
      ),
    })),

  selectServer: (serverId) => set({ selected_server_id: serverId }),

  setActiveTab: (tab) =>
    set((state) => {
      const sid = state.selected_server_id;
      if (!sid) return {};
      return { active_tabs: { ...state.active_tabs, [sid]: tab } };
    }),

  getActiveTab: () => {
    const state = get();
    const sid = state.selected_server_id;
    if (!sid) return "connection";
    return state.active_tabs[sid] || "connection";
  },

  addServer: (server) =>
    set((state) => ({ servers: [...state.servers, server] })),

  removeServer: (serverId) =>
    set((state) => {
      const newTabs = { ...state.active_tabs };
      delete newTabs[serverId];
      return {
        servers: state.servers.filter((s) => s.id !== serverId),
        selected_server_id:
          state.selected_server_id === serverId ? null : state.selected_server_id,
        active_tabs: newTabs,
      };
    }),

  setLoading: (loading) => set({ loading }),

  setProxyStatus: (serverId, running) =>
    set((state) => ({
      servers: state.servers.map((s) =>
        s.id === serverId ? { ...s, proxy_running: running } : s
      ),
    })),
}));
