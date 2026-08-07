// Trigger store — Zustand
// §4.2 triggerStore: trigger templates + instances

import { create } from "zustand";
import { ipcInvoke } from "@/hooks/useIpc";
import type { TriggerTemplate, TriggerInstance } from "@/types";

interface TriggerStore {
  templates: TriggerTemplate[];
  /** Per-server trigger instances */
  serverTriggers: Record<string, TriggerInstance[]>;
  /** Currently executing triggers (for progress display) */
  executing: Record<string, TriggerExecution>;
  /** Per-server runtime overrides for exec_in_terminal (NOT persisted).
   * Keyed by serverId, then triggerId → override value.
   * When a new terminal opens, these are sent to the daemon as session overrides. */
  serverExecInTerminalOverrides: Record<string, Record<string, boolean>>;
  /** Per-server runtime overrides for bind_new_terminals (NOT persisted).
   * Keyed by serverId, then triggerId → override value.
   * Reset when the app closes; preserved across tab switches. */
  serverBindNewTerminalsOverrides: Record<string, Record<string, boolean>>;

  setTemplates: (templates: TriggerTemplate[]) => void;
  loadTemplates: () => Promise<void>;
  setServerTriggers: (serverId: string, triggers: TriggerInstance[]) => void;
  startExecution: (exec: TriggerExecution) => void;
  updateExecution: (
    executionId: string,
    update: Partial<TriggerExecution>,
  ) => void;
  finishExecution: (executionId: string) => void;
  /** Set a runtime override for exec_in_terminal (not persisted). */
  setExecInTerminalOverride: (serverId: string, triggerId: string, value: boolean) => void;
  /** Get the effective exec_in_terminal value (override or config). */
  getEffectiveExecInTerminal: (serverId: string, trigger: TriggerInstance) => boolean;
  /** Set a runtime override for bind_new_terminals (not persisted). */
  setBindNewTerminalsOverride: (serverId: string, triggerId: string, value: boolean) => void;
  /** Get the effective bind_new_terminals value (override or config). */
  getEffectiveBindNewTerminals: (serverId: string, trigger: TriggerInstance) => boolean;
}

export interface CommandResult {
  command: string;
  exit_code: number;
  stdout: string;
  stderr: string;
  success: boolean;
}

export interface TriggerExecution {
  execution_id: string;
  server_id: string;
  trigger_id: string;
  trigger_name: string;
  total_commands: number;
  executed_commands: number;
  current_command: string | null;
  success: boolean | null;
  results?: CommandResult[];
}

export const useTriggerStore = create<TriggerStore>((set, get) => ({
  templates: [],
  serverTriggers: {},
  executing: {},
  serverExecInTerminalOverrides: {},
  serverBindNewTerminalsOverrides: {},

  setTemplates: (templates) => set({ templates }),

  loadTemplates: async () => {
    try {
      const data = await ipcInvoke<{ templates: TriggerTemplate[] }>(
        "ipc_list_templates",
      );
      set({ templates: data?.templates || [] });
    } catch (e) {
      console.warn("load templates failed:", String(e));
    }
  },

  setServerTriggers: (serverId, triggers) =>
    set((state) => ({
      serverTriggers: { ...state.serverTriggers, [serverId]: triggers },
    })),

  startExecution: (exec) =>
    set((state) => ({
      executing: { ...state.executing, [exec.execution_id]: exec },
    })),

  updateExecution: (executionId, update) =>
    set((state) => ({
      executing: {
        ...state.executing,
        [executionId]: { ...state.executing[executionId], ...update },
      },
    })),

  finishExecution: (executionId) =>
    set((state) => {
      const { [executionId]: _, ...rest } = state.executing;
      return { executing: rest };
    }),

  setExecInTerminalOverride: (serverId, triggerId, value) =>
    set((state) => ({
      serverExecInTerminalOverrides: {
        ...state.serverExecInTerminalOverrides,
        [serverId]: {
          ...(state.serverExecInTerminalOverrides[serverId] || {}),
          [triggerId]: value,
        },
      },
    })),

  getEffectiveExecInTerminal: (serverId, trigger) => {
    const overrides = get().serverExecInTerminalOverrides[serverId];
    if (overrides && trigger.id in overrides) {
      return overrides[trigger.id];
    }
    return trigger.exec_in_terminal;
  },

  setBindNewTerminalsOverride: (serverId, triggerId, value) =>
    set((state) => ({
      serverBindNewTerminalsOverrides: {
        ...state.serverBindNewTerminalsOverrides,
        [serverId]: {
          ...(state.serverBindNewTerminalsOverrides[serverId] || {}),
          [triggerId]: value,
        },
      },
    })),

  getEffectiveBindNewTerminals: (serverId, trigger) => {
    const overrides = get().serverBindNewTerminalsOverrides[serverId];
    if (overrides && trigger.id in overrides) {
      return overrides[trigger.id];
    }
    return trigger.bind_new_terminals;
  },
}));
