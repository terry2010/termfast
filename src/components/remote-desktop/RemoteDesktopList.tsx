// RemoteDesktopList — sidebar component showing desktop-to-desktop peers (FP-7)
// Lists paired desktops with online status and connect button.

import { useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { useRemoteDesktopStore } from "@/stores/remoteDesktopStore";
import { ipcInvoke, useTauriEvent } from "@/hooks/useIpc";
import { toast } from "sonner";

interface RemoteClientStateEvent {
  pairing_id: string;
  connected: boolean;
}

export function RemoteDesktopList() {
  const { t } = useTranslation();
  const peers = useRemoteDesktopStore((s) => s.peers);
  const loadPeers = useRemoteDesktopStore((s) => s.loadPeers);
  const setPeerOnline = useRemoteDesktopStore((s) => s.setPeerOnline);
  const setActiveConnection = useRemoteDesktopStore(
    (s) => s.setActiveConnection
  );

  // Load peers on mount
  useEffect(() => {
    loadPeers();
  }, [loadPeers]);

  // Listen for connection state events
  const handleStateEvent = useCallback(
    (data: RemoteClientStateEvent) => {
      setPeerOnline(data.pairing_id, data.connected);
      if (data.connected) {
        setActiveConnection(data.pairing_id);
      }
    },
    [setPeerOnline, setActiveConnection]
  );
  useTauriEvent<RemoteClientStateEvent>("remote_client_state", handleStateEvent);

  const handleConnect = useCallback(
    async (pairingId: string) => {
      const peer = peers.find((p) => p.pairingId === pairingId);
      if (!peer) return;
      try {
        await ipcInvoke("ipc_remote_client_connect", {
          pairing_id: peer.pairingId,
          pairing_key_hex: peer.pairingKeyHex,
          pairing_jwt: peer.jwt,
          relay_url: peer.relayUrl,
        });
        toast.success(`Connecting to ${peer.peerName}...`);
      } catch (e: any) {
        toast.error(`Connection failed: ${e?.message || e}`);
      }
    },
    [peers]
  );

  const handleDisconnect = useCallback(
    async (pairingId: string) => {
      try {
        await ipcInvoke("ipc_remote_client_disconnect", { pairing_id: pairingId });
        setPeerOnline(pairingId, false);
        setActiveConnection(null);
      } catch (e: any) {
        toast.error(`Disconnect failed: ${e?.message || e}`);
      }
    },
    [setPeerOnline, setActiveConnection]
  );

  if (peers.length === 0) {
    return (
      <div className="p-4 text-sm text-gray-500 dark:text-gray-400">
        {t("remote_desktop.no_peers", "暂无桌面互配。请在手机端发起桌面互配。")}
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-2 p-2">
      {peers.map((peer) => (
        <div
          key={peer.pairingId}
          className="rounded-lg border border-gray-200 dark:border-gray-700 p-3 hover:bg-gray-50 dark:hover:bg-gray-800 transition-colors"
        >
          <div className="flex items-center gap-2">
            <span className="text-lg">🖥️</span>
            <div className="flex-1 min-w-0">
              <div className="font-medium text-sm truncate">
                {peer.peerName || peer.pairingId}
              </div>
              <div className="flex items-center gap-1 text-xs text-gray-500">
                <span
                  className={`inline-block w-2 h-2 rounded-full ${
                    peer.online ? "bg-green-500" : "bg-gray-400"
                  }`}
                />
                {peer.online
                  ? t("remote_desktop.online", "在线")
                  : t("remote_desktop.offline", "离线")}
                <span className="ml-1 text-gray-400">({peer.peerRole})</span>
              </div>
            </div>
          </div>
          <div className="mt-2 flex gap-2">
            {peer.online ? (
              <button
                onClick={() => handleDisconnect(peer.pairingId)}
                className="text-xs px-3 py-1 rounded bg-red-100 text-red-700 hover:bg-red-200 dark:bg-red-900/30 dark:text-red-400"
              >
                {t("remote_desktop.disconnect", "断开")}
              </button>
            ) : (
              <button
                onClick={() => handleConnect(peer.pairingId)}
                className="text-xs px-3 py-1 rounded bg-blue-100 text-blue-700 hover:bg-blue-200 dark:bg-blue-900/30 dark:text-blue-400"
              >
                {t("remote_desktop.connect", "连接")}
              </button>
            )}
          </div>
        </div>
      ))}
    </div>
  );
}
