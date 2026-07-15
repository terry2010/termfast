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

  setTemplates: (templates: TriggerTemplate[]) => void;
  loadTemplates: () => Promise<void>;
  setServerTriggers: (serverId: string, triggers: TriggerInstance[]) => void;
  startExecution: (exec: TriggerExecution) => void;
  updateExecution: (
    executionId: string,
    update: Partial<TriggerExecution>,
  ) => void;
  finishExecution: (executionId: string) => void;
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

export const useTriggerStore = create<TriggerStore>((set) => ({
  templates: [],
  serverTriggers: {},
  executing: {},

  setTemplates: (templates) => set({ templates }),

  loadTemplates: async () => {
    try {
      const data = await ipcInvoke<{ templates: TriggerTemplate[] }>(
        "ipc_list_templates",
      );
      set({ templates: data?.templates || [] });
    } catch (e) {
      console.error("load templates failed:", e);
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
}));
