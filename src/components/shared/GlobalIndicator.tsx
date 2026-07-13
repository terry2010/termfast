// GlobalIndicator — top bar showing global health (§9.4)
// Shows: [N connected / M abnormal] + hostkey warning + connect/disconnect all

import { useTranslation } from "react-i18next";
import { useServerStore } from "@/stores/serverStore";
import { ipcInvoke } from "@/hooks/useIpc";
import { useState, useEffect } from "react";
import { listen as tauriListen, type UnlistenFn } from "@tauri-apps/api/event";

// === SECTION 1 END ===

export function GlobalIndicator() {
  const { t } = useTranslation();
  const servers = useServerStore((s) => s.servers);
  const [hostkeyWarning, setHostkeyWarning] = useState<string | null>(null);

  // Listen for hostkey mismatch events
  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    tauriListen<{ server_id: string; expected: string; actual: string }>(
      "ssh:hostkey_mismatch",
      (e) => {
        setHostkeyWarning(
          t("server.hostkey_mismatch", { id: e.payload.server_id })
        );
        // Auto-clear after 30s
        setTimeout(() => setHostkeyWarning(null), 30000);
      }
    ).then((fn) => {
      unlisten = fn;
    });
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  const connected = servers.filter((s) => s.current_status === "connected").length;
  const abnormal = servers.filter(
    (s) =>
      s.current_status === "auth_failed" ||
      s.current_status === "reconnecting" ||
      s.current_status === "offline"
  ).length;

  const handleConnectAll = async () => {
    for (const s of servers) {
      try {
        await ipcInvoke("ipc_connect_server", { serverId: s.id });
      } catch {
        // ignore individual failures
      }
    }
  };

  const handleDisconnectAll = async () => {
    for (const s of servers) {
      try {
        await ipcInvoke("ipc_disconnect_server", { serverId: s.id });
      } catch {
        // ignore individual failures
      }
    }
  };

  return (
    <div className="flex items-center justify-between px-4 py-2 border-b border-gray-200 dark:border-gray-700">
      <div className="flex items-center gap-4">
        <span className="text-sm" aria-live="polite">
          <span className="text-status-connected font-medium">{connected}</span>
          {" "}{t("server.status.connected")}
          {abnormal > 0 && (
            <>
              {" / "}
              <span className="text-status-authfailed font-medium">{abnormal}</span>
              {" "}{t("common.warning")}
            </>
          )}
        </span>
        {hostkeyWarning && (
          <span
            className="text-sm text-red-600 dark:text-red-400 font-medium animate-pulse"
            role="alert"
          >
            ⚠ {hostkeyWarning}
          </span>
        )}
      </div>
      <div className="flex items-center gap-2">
        <button
          className="px-3 py-1 text-sm rounded hover:bg-gray-100 dark:hover:bg-gray-800"
          onClick={handleConnectAll}
        >
          {t("menu.connect_all")}
        </button>
        <button
          className="px-3 py-1 text-sm rounded hover:bg-gray-100 dark:hover:bg-gray-800"
          onClick={handleDisconnectAll}
        >
          {t("menu.disconnect_all")}
        </button>
      </div>
    </div>
  );
}

// === SECTION 2 END ===
