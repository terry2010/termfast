// Keyboard shortcuts — FP-7.4 / §9.8
//
// Cmd/Ctrl+1...9: switch server
// Cmd/Ctrl+0: 10th server
// Cmd/Ctrl+N: add server
// Cmd/Ctrl+,: settings
// Cmd/Ctrl+L: focus logs
// Cmd/Ctrl+Shift+L: focus log search
// Cmd/Ctrl+Shift+P: toggle proxy
// Cmd/Ctrl+Shift+T: pause/resume all triggers
// Cmd/Ctrl+Shift+Space: toggle connection (with confirm)
// Cmd/Ctrl+E: collapse/expand logs
// Cmd/Ctrl+B: toggle sidebar
// Cmd/Ctrl+Shift+R / F5: refresh status
// Cmd/Ctrl+Q: quit app (macOS standard)
// Esc: close panel/cancel/close dialog

import { useEffect } from "react";

export interface KeyboardShortcutHandlers {
  onSelectServer: (index: number) => void;
  onAddServer: () => void;
  onOpenSettings: () => void;
  onFocusLogs: () => void;
  onFocusLogSearch: () => void;
  onToggleProxy: () => void;
  onToggleTriggers: () => void;
  onToggleConnection: () => void;
  onToggleLogPanel: () => void;
  onToggleSidebar: () => void;
  onRefresh: () => void;
  onEscape: () => void;
  onQuit?: () => void;
}

export function useKeyboardShortcuts(handlers: KeyboardShortcutHandlers): void {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      if (!mod && e.key !== "Escape" && e.key !== "F5") return;

      // Escape (no modifier)
      if (e.key === "Escape" && !mod) {
        handlers.onEscape();
        return;
      }

      // F5 or Cmd/Ctrl+Shift+R: refresh
      if (e.key === "F5" || (mod && e.shiftKey && e.key === "R")) {
        e.preventDefault();
        handlers.onRefresh();
        return;
      }

      if (!mod) return;

      // Cmd/Ctrl+Shift+... combinations
      if (e.shiftKey) {
        switch (e.key.toLowerCase()) {
          case "l":
            e.preventDefault();
            handlers.onFocusLogSearch();
            return;
          case "p":
            e.preventDefault();
            handlers.onToggleProxy();
            return;
          case "t":
            e.preventDefault();
            handlers.onToggleTriggers();
            return;
          case " ":
            e.preventDefault();
            handlers.onToggleConnection();
            return;
        }
        return;
      }

      // Cmd/Ctrl+... without shift
      switch (e.key) {
        case "1": case "2": case "3": case "4": case "5":
        case "6": case "7": case "8": case "9":
          e.preventDefault();
          handlers.onSelectServer(parseInt(e.key) - 1);
          return;
        case "0":
          e.preventDefault();
          handlers.onSelectServer(9);
          return;
        case "n":
          e.preventDefault();
          handlers.onAddServer();
          return;
        case ",":
          e.preventDefault();
          handlers.onOpenSettings();
          return;
        case "l":
          e.preventDefault();
          handlers.onFocusLogs();
          return;
        case "e":
          e.preventDefault();
          handlers.onToggleLogPanel();
          return;
        case "b":
          e.preventDefault();
          handlers.onToggleSidebar();
          return;
        case "q":
          e.preventDefault();
          handlers.onQuit?.();
          return;
      }
    };

    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [handlers]);
}
