// RemoteTerminalList — shows terminals on a remote desktop (FP-7)
// After connecting to a remote desktop, lists terminals and allows subscribing.

import { useEffect, useCallback, useState, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useRemoteDesktopStore } from "@/stores/remoteDesktopStore";
import { ipcInvoke, useTauriEvent } from "@/hooks/useIpc";
import { toast } from "sonner";
import { decodeBase64Json } from "@/lib/utils";

// Frame type constants (match remote_frame.rs)
const LIST_RESPONSE = 0x02;

interface RemoteTerminalListProps {
  pairingId: string;
  onTerminalSelect: (terminalId: number, name: string) => void;
}

interface TerminalInfo {
  terminal_id: number;
  name: string;
}

export function RemoteTerminalList({
  pairingId,
  onTerminalSelect,
}: RemoteTerminalListProps) {
  const { t } = useTranslation();
  const [terminals, setTerminals] = useState<TerminalInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const setActiveConnection = useRemoteDesktopStore(
    (s) => s.setActiveConnection
  );

  // Listen for LIST_RESPONSE frames from the remote desktop
  const pairingIdRef = useRef(pairingId);
  pairingIdRef.current = pairingId;
  useTauriEvent<{
    pairing_id: string;
    frame_type: number;
    terminal_id: number;
    data: string;
  }>("remote_client_frame", (event) => {
    if (event.pairing_id !== pairingIdRef.current) return;
    if (event.frame_type === LIST_RESPONSE) {
      try {
        const parsed = decodeBase64Json<any>(event.data);
        const terms = (parsed.terminals || []).map((t: any) => ({
          terminal_id: t.terminal_id,
          name: t.name || `Terminal #${t.terminal_id}`,
        }));
        setTerminals(terms);
        setLoading(false);
      } catch {
        // ignore parse errors
      }
    }
  });

  useEffect(() => {
    setActiveConnection(pairingId);
    setLoading(true);
    setTerminals([]);
    ipcInvoke<{ terminals: TerminalInfo[] }>("ipc_remote_client_list_terminals", {
      pairing_id: pairingId,
    })
      .then(() => {
        // Terminal list arrives via remote_client_frame event (LIST_RESPONSE)
        // Set a timeout to stop loading if no response
        setTimeout(() => setLoading(false), 5000);
      })
      .catch((e) => {
        toast.error(`Failed to list terminals: ${e?.message || e}`);
        setLoading(false);
      });
  }, [pairingId, setActiveConnection]);

  const handleSubscribe = useCallback(
    async (terminalId: number, name: string) => {
      try {
        await ipcInvoke("ipc_remote_client_subscribe", {
          pairing_id: pairingId,
          terminal_id: terminalId,
        });
        onTerminalSelect(terminalId, name);
      } catch (e: any) {
        toast.error(`Subscribe failed: ${e?.message || e}`);
      }
    },
    [pairingId, onTerminalSelect]
  );

  if (loading) {
    return <div className="p-4 text-sm text-gray-500">Loading terminals...</div>;
  }

  if (terminals.length === 0) {
    return (
      <div className="p-4 text-sm text-gray-500">
        {t("remote_desktop.no_terminals", "暂无终端")}
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-1 p-2">
      <div className="text-xs text-gray-500 mb-2 px-2">
        {t("remote_desktop.select_terminal", "选择终端")}
      </div>
      {terminals.map((term) => (
        <button
          key={term.terminal_id}
          onClick={() => handleSubscribe(term.terminal_id, term.name)}
          className="flex items-center gap-2 px-3 py-2 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-800 text-left text-sm transition-colors"
        >
          <span className="text-base">📊</span>
          <span className="truncate">{term.name || `Terminal #${term.terminal_id}`}</span>
        </button>
      ))}
    </div>
  );
}
