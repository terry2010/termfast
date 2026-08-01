// Server store — Zustand
// §4.2 serverStore: server list + status

import { create } from "zustand";
import type { ServerConfig, ServerStatus } from "@/types";

export interface ServerState extends ServerConfig {
  current_status: ServerStatus;
  current_ip: string | null;
  client_ip: string | null;
  connected_since: string | null;
  reconnect_count: number;
  max_attempts: number;
  proxy_running: boolean;
  active_channels: number;
  bytes_in: number;
  bytes_out: number;
  /** SSH auth banner (RFC4252 §5.4) — welcome message from server */
  auth_banner: string | null;
  /** Whether rz (ZMODEM upload) is available on the remote server */
  rz_available: boolean;
}

export interface TerminalTab {
  id: string;
  sessionId: string;
  label: string;
  defaultLabel: string;
  initialOutput: string;
  disconnected: boolean;
}

interface ServerStore {
  servers: ServerState[];
  selected_server_id: string | null;
  active_tabs: Record<string, string>;
  loading: boolean;
  terminal_tabs_by_server: Record<string, TerminalTab[]>;
  active_terminal_tab_by_server: Record<string, string>;

  setServers: (servers: ServerState[]) => void;
  updateServerStatus: (serverId: string, status: ServerStatus, ip?: string, clientIp?: string | null) => void;
  selectServer: (serverId: string | null) => void;
  setActiveTab: (tab: string) => void;
  getActiveTab: () => string;
  addServer: (server: ServerState) => void;
  removeServer: (serverId: string) => void;
  setLoading: (loading: boolean) => void;
  setProxyStatus: (serverId: string, running: boolean) => void;
  setAuthBanner: (serverId: string, banner: string | null) => void;
  setRzAvailable: (serverId: string, available: boolean) => void;
  addTerminalTab: (serverId: string, tab: TerminalTab) => void;
  removeTerminalTab: (serverId: string, tabId: string) => void;
  setTerminalTabsForServer: (serverId: string, tabs: TerminalTab[]) => void;
  setActiveTerminalTab: (serverId: string, tabId: string) => void;
  renameTerminalTab: (serverId: string, tabId: string, label: string) => void;
  setTerminalTabDisconnected: (serverId: string, sessionId: string) => void;
  clearTerminalTabs: (serverId: string) => void;
}

export const useServerStore = create<ServerStore>((set, get) => ({
  servers: [],
  selected_server_id: null,
  active_tabs: {},
  loading: false,
  terminal_tabs_by_server: {},
  active_terminal_tab_by_server: {},

  setServers: (servers) => set({ servers }),

  updateServerStatus: (serverId, status, ip, clientIp) =>
    set((state) => ({
      servers: state.servers.map((s) =>
        s.id === serverId
          ? {
              ...s,
              current_status: status,
              current_ip: ip ?? s.current_ip,
              client_ip: clientIp !== undefined ? clientIp : s.client_ip,
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

  setAuthBanner: (serverId, banner) =>
    set((state) => ({
      servers: state.servers.map((s) =>
        s.id === serverId ? { ...s, auth_banner: banner } : s
      ),
    })),

  setRzAvailable: (serverId, available) =>
    set((state) => ({
      servers: state.servers.map((s) =>
        s.id === serverId ? { ...s, rz_available: available } : s
      ),
    })),

  addTerminalTab: (serverId, tab) =>
    set((state) => ({
      terminal_tabs_by_server: {
        ...state.terminal_tabs_by_server,
        [serverId]: [...(state.terminal_tabs_by_server[serverId] || []), tab],
      },
      active_terminal_tab_by_server: {
        ...state.active_terminal_tab_by_server,
        [serverId]: tab.id,
      },
    })),

  removeTerminalTab: (serverId, tabId) =>
    set((state) => {
      const current = state.terminal_tabs_by_server[serverId] || [];
      const nextTabs = current.filter((t) => t.id !== tabId);
      const next = { ...state.terminal_tabs_by_server, [serverId]: nextTabs };
      const wasActive = state.active_terminal_tab_by_server[serverId] === tabId;
      return {
        terminal_tabs_by_server: next,
        active_terminal_tab_by_server: wasActive
          ? { ...state.active_terminal_tab_by_server, [serverId]: "overview" }
          : state.active_terminal_tab_by_server,
      };
    }),

  setTerminalTabsForServer: (serverId, tabs) =>
    set((state) => ({
      terminal_tabs_by_server: { ...state.terminal_tabs_by_server, [serverId]: tabs },
    })),

  setActiveTerminalTab: (serverId, tabId) =>
    set((state) => ({
      active_terminal_tab_by_server: {
        ...state.active_terminal_tab_by_server,
        [serverId]: tabId,
      },
    })),

  renameTerminalTab: (serverId, tabId, label) =>
    set((state) => {
      const current = state.terminal_tabs_by_server[serverId] || [];
      return {
        terminal_tabs_by_server: {
          ...state.terminal_tabs_by_server,
          [serverId]: current.map((t) => (t.id === tabId ? { ...t, label } : t)),
        },
      };
    }),

  setTerminalTabDisconnected: (serverId, sessionId) =>
    set((state) => {
      const current = state.terminal_tabs_by_server[serverId] || [];
      return {
        terminal_tabs_by_server: {
          ...state.terminal_tabs_by_server,
          [serverId]: current.map((t) =>
            t.sessionId === sessionId ? { ...t, disconnected: true } : t
          ),
        },
      };
    }),

  clearTerminalTabs: (serverId) =>
    set((state) => {
      const nextTabs = { ...state.terminal_tabs_by_server };
      delete nextTabs[serverId];
      const nextActive = { ...state.active_terminal_tab_by_server };
      delete nextActive[serverId];
      return {
        terminal_tabs_by_server: nextTabs,
        active_terminal_tab_by_server: nextActive,
      };
    }),
}));
