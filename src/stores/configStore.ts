// Config store — Zustand
// §4.2 configStore: global config

import { create } from "zustand";
import type { Config, GeneralConfig } from "@/types";

interface ConfigStore {
  config: Config | null;
  loading: boolean;

  setConfig: (config: Config) => void;
  updateGeneral: (general: Partial<GeneralConfig>) => void;
  setLoading: (loading: boolean) => void;
}

export const useConfigStore = create<ConfigStore>((set) => ({
  config: null,
  loading: false,

  setConfig: (config) => set({ config }),

  updateGeneral: (general) =>
    set((state) => ({
      config: state.config
        ? {
            ...state.config,
            general: { ...state.config.general, ...general },
          }
        : null,
    })),

  setLoading: (loading) => set({ loading }),
}));
