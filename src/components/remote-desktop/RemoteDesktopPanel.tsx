// RemoteDesktopPanel — main panel for remote desktop feature (FP-7)
// Shows: peer list → terminal list → terminal view

import { useState, useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useRemoteDesktopStore } from "@/stores/remoteDesktopStore";
import { ipcInvoke } from "@/hooks/useIpc";
import { RemoteDesktopList } from "./RemoteDesktopList";
import { RemoteTerminalList } from "./RemoteTerminalList";
import { RemoteTerminalView } from "./RemoteTerminalView";

type View =
  | { type: "peerList" }
  | { type: "terminalList"; pairingId: string }
  | { type: "terminalView"; pairingId: string; terminalId: number; terminalName: string };

/**
 * Clean up active remote desktop connection before closing the modal.
 * If there's an active connection, disconnect it and clear the store.
 * Errors are silently ignored (closing should not be blocked).
 */
export async function cleanupRemoteDesktopConnection(): Promise<void> {
  const store = useRemoteDesktopStore.getState();
  const activeConn = store.activeConnection;
  if (activeConn) {
    try {
      await ipcInvoke("ipc_remote_client_disconnect", {
        pairing_id: activeConn,
      });
    } catch {
      // Ignore disconnect errors on close
    }
    store.setActiveConnection(null);
  }
}

export function RemoteDesktopPanel() {
  const { t } = useTranslation();
  const [view, setView] = useState<View>({ type: "peerList" });
  const activeConnection = useRemoteDesktopStore((s) => s.activeConnection);
  const prevActiveRef = useRef<string | null>(null);

  // Auto-switch to terminalList when activeConnection becomes non-null
  // (i.e., connection succeeded). This bridges RemoteDesktopList → RemoteTerminalList.
  useEffect(() => {
    if (activeConnection && activeConnection !== prevActiveRef.current) {
      // Only auto-switch from peerList (user-initiated connect)
      setView((prev) => {
        if (prev.type === "peerList") {
          return { type: "terminalList", pairingId: activeConnection };
        }
        return prev;
      });
    }
    prevActiveRef.current = activeConnection;
  }, [activeConnection]);

  const handleTerminalSelect = useCallback(
    (terminalId: number, name: string) => {
      if (activeConnection) {
        setView({
          type: "terminalView",
          pairingId: activeConnection,
          terminalId,
          terminalName: name,
        });
      }
    },
    [activeConnection]
  );

  return (
    <div className="flex flex-col h-full">
      {/* Breadcrumb / header */}
      <div className="flex items-center gap-2 px-3 py-2 border-b border-gray-200 dark:border-gray-700 bg-white dark:bg-[#1E1E1E]">
        {view.type !== "peerList" && (
          <button
            onClick={() => setView({ type: "peerList" })}
            className="text-sm text-blue-600 hover:underline"
          >
            ← {t("remote_desktop.back", "返回")}
          </button>
        )}
        <span className="text-sm font-medium">
          {view.type === "peerList" && t("remote_desktop.title", "远程桌面")}
          {view.type === "terminalList" && t("remote_desktop.terminals", "终端列表")}
          {view.type === "terminalView" && t("remote_desktop.terminal", "终端")}
        </span>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-auto">
        {view.type === "peerList" && <RemoteDesktopList />}
        {view.type === "terminalList" && (
          <RemoteTerminalList
            pairingId={view.pairingId}
            onTerminalSelect={handleTerminalSelect}
          />
        )}
        {view.type === "terminalView" && (
          <RemoteTerminalView
            pairingId={view.pairingId}
            terminalId={view.terminalId}
            terminalName={view.terminalName}
          />
        )}
      </div>
    </div>
  );
}
