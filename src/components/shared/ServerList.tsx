// ServerList — left sidebar server list (§9.4 / FP-8.2)
// 5-state visualization, abnormal items pinned to top
// Features: inline proxy toggle, port copy chip, global health summary,
//   connect-all/disconnect-all/template-library/settings buttons, aria-live

import { useEffect, useState, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useServerStore } from "@/stores/serverStore";
import { useLogStore } from "@/stores/logStore";
import { ipcInvoke } from "@/hooks/useIpc";
import { AddServerDialog } from "@/components/shared/AddServerDialog";
import { SkeletonList } from "@/components/ui/Skeleton";
import { showContextMenu, type ContextMenuEntry } from "@/components/ui/ContextMenu";
import { toast } from "@/components/ui/toast";
import type { ServerStatus } from "@/types";
import type { ServerState } from "@/stores/serverStore";

const STATUS_COLORS: Record<ServerStatus, string> = {
  connected: "bg-status-connected",
  connecting: "bg-status-connecting",
  reconnecting: "bg-status-reconnecting",
  auth_failed: "bg-status-authfailed",
  disconnected: "bg-status-disconnected",
  offline: "bg-status-disconnected",
};

const STATUS_SHAPES: Record<ServerStatus, string> = {
  connected: "rounded-full",
  connecting: "rounded-r-full",
  reconnecting: "rounded-l-full",
  auth_failed: "rounded-none",
  disconnected: "rounded-full border-2 border-current bg-transparent",
  offline: "rounded-full border-2 border-current bg-transparent",
};

// === SECTION 1 END ===

export function ServerList({
  onAddServer,
  onOpenTemplates,
  onOpenSettings,
}: {
  onAddServer?: () => void;
  onOpenTemplates?: () => void;
  onOpenSettings?: () => void;
}) {
  const { t } = useTranslation();
  const servers = useServerStore((s) => s.servers);
  const selectedId = useServerStore((s) => s.selected_server_id);
  const selectServer = useServerStore((s) => s.selectServer);
  const [showAddDialog, setShowAddDialog] = useState(false);
  const [copiedPort, setCopiedPort] = useState<string | null>(null);
  const [loading, setLoading] = useState(servers.length === 0);
  const [draggedId, setDraggedId] = useState<string | null>(null);
  const [dragOverId, setDragOverId] = useState<string | null>(null);
  const [dragOverPos, setDragOverPos] = useState<"before" | "after">("before");

  // Load servers from daemon on mount
  useEffect(() => {
    loadServers();
  }, []);

  const loadServers = async () => {
    setLoading(true);
    try {
      const data = await ipcInvoke<{ servers: ServerState[] }>("ipc_list_servers");
      if (data?.servers && data.servers.length > 0) {
        useServerStore.setState({ servers: data.servers });
      }
    } catch (e) {
      console.error("load servers failed:", e);
    } finally {
      setLoading(false);
    }
  };

  const handleAddServer = async () => {
    setShowAddDialog(false);
    await loadServers();
  };

  const handleConnectAll = useCallback(async () => {
    for (const s of servers) {
      try {
        await ipcInvoke("ipc_connect_server", { serverId: s.id });
      } catch (e) {
        console.error(`connect ${s.id} failed:`, e);
      }
    }
  }, [servers]);

  const handleDisconnectAll = useCallback(async () => {
    for (const s of servers) {
      try {
        await ipcInvoke("ipc_disconnect_server", { serverId: s.id });
      } catch (e) {
        console.error(`disconnect ${s.id} failed:`, e);
      }
    }
  }, [servers]);

  const handleToggleProxy = useCallback(
    async (serverId: string, currentEnabled: boolean) => {
      try {
        await ipcInvoke("ipc_toggle_proxy", {
          serverId,
          enabled: !currentEnabled,
        });
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        toast.error(t("server.proxy_toggle_failed"), { description: msg });
      }
    },
    [t]
  );

  const handleCopyPort = useCallback(async (port: number, serverId: string) => {
    try {
      await navigator.clipboard.writeText(String(port));
      setCopiedPort(`${serverId}:${port}`);
      setTimeout(() => setCopiedPort(null), 1500);
    } catch (e) {
      console.error("copy port failed:", e);
    }
  }, []);

  const handleContextMenu = useCallback(
    (e: React.MouseEvent, server: ServerState) => {
      const isConnected = server.current_status === "connected";
      const proxyEnabled = server.proxy_running;
      const socks5Port = server.proxy?.socks5_port || 1080;
      const httpPort = server.proxy?.http_port || 8080;

      const items: ContextMenuEntry[] = [
        {
          label: isConnected ? t("server.disconnect") : t("server.connect"),
          icon: isConnected ? "⏹" : "▶",
          onClick: async () => {
            try {
              if (isConnected) {
                await ipcInvoke("ipc_disconnect_server", { server_id: server.id });
              } else {
                await ipcInvoke("ipc_connect_server", { server_id: server.id });
              }
            } catch (e) {
              const errMsg = e instanceof Error ? e.message : String(e);
              useLogStore.getState().addEntry({
                id: `ctx-conn-${Date.now()}-${Math.random().toString(36).slice(2)}`,
                timestamp: new Date().toISOString(),
                server_id: server.id,
                level: "error",
                category: "Connection",
                message: `Connection failed: ${errMsg}`,
                execution_id: null,
                command: null,
                exit_code: null,
                stdout: null,
                stderr: null,
              });
            }
          },
          disabled: server.current_status === "connecting" || server.current_status === "reconnecting",
        },
        {
          label: proxyEnabled ? t("server.stop_proxy") : t("server.start_proxy"),
          icon: proxyEnabled ? "■" : "▶",
          onClick: async () => {
            try {
              await ipcInvoke("ipc_toggle_proxy", {
                server_id: server.id,
                enabled: !proxyEnabled,
              });
            } catch (err) {
              const msg = err instanceof Error ? err.message : String(err);
              toast.error(t("server.proxy_toggle_failed"), { description: msg });
            }
          },
          disabled: !isConnected,
        },
        {
          label: t("server.set_system_proxy"),
          icon: "🌐",
          onClick: async () => {
            try {
              await ipcInvoke("ipc_set_system_proxy", { server_id: server.id });
            } catch (err) {
              const msg = err instanceof Error ? err.message : String(err);
              toast.error(t("server.set_system_proxy_failed"), { description: msg });
            }
          },
          disabled: !isConnected || !proxyEnabled,
        },
        { separator: true },
        {
          label: t("server.copy_socks5", { port: socks5Port }),
          icon: "📋",
          onClick: () => {
            navigator.clipboard.writeText(`127.0.0.1:${socks5Port}`).catch(() => {});
          },
        },
        {
          label: t("server.copy_http", { port: httpPort }),
          icon: "📋",
          onClick: () => {
            navigator.clipboard.writeText(`127.0.0.1:${httpPort}`).catch(() => {});
          },
        },
        { separator: true },
        {
          label: t("server.edit"),
          icon: "✎",
          onClick: () => {
            selectServer(server.id);
            window.dispatchEvent(
              new CustomEvent("edit-server", { detail: { serverId: server.id } })
            );
          },
        },
        {
          label: t("server.delete_title"),
          icon: "✕",
          danger: true,
          onClick: () => {
            selectServer(server.id);
            window.dispatchEvent(
              new CustomEvent("delete-server", {
                detail: { serverId: server.id, serverName: server.name },
              })
            );
          },
        },
      ];

      showContextMenu(e, items);
    },
    [t, selectServer]
  );

  // Sort: abnormal servers pinned to top, rest keep config order
  const abnormalStatuses: ServerStatus[] = ["auth_failed", "reconnecting", "offline"];
  const sorted = [...servers].sort((a, b) => {
    const aAbnormal = abnormalStatuses.includes(a.current_status);
    const bAbnormal = abnormalStatuses.includes(b.current_status);
    if (aAbnormal && !bAbnormal) return -1;
    if (!aAbnormal && bAbnormal) return 1;
    return 0; // keep original order within same group
  });

  // Global health summary
  const connectedCount = servers.filter((s) => s.current_status === "connected").length;
  const abnormalCount = servers.filter((s) => abnormalStatuses.includes(s.current_status)).length;
  const totalActiveClients = servers.filter((s) => s.proxy_running).reduce((sum, s) => sum + (s.active_channels || 0), 0);
  const totalBytesIn = servers.filter((s) => s.proxy_running).reduce((sum, s) => sum + (s.bytes_in || 0), 0);
  const totalBytesOut = servers.filter((s) => s.proxy_running).reduce((sum, s) => sum + (s.bytes_out || 0), 0);

  // Calculate speed from byte delta
  const speedRef = useRef({ in: 0, out: 0, time: Date.now(), speedIn: 0, speedOut: 0 });
  const [, forceUpdate] = useState(0);
  useEffect(() => {
    if (totalActiveClients === 0) {
      speedRef.current = { in: 0, out: 0, time: Date.now(), speedIn: 0, speedOut: 0 };
      return;
    }
    const now = Date.now();
    const dt = (now - speedRef.current.time) / 1000;
    if (dt > 0.5) {
      const dIn = totalBytesIn - speedRef.current.in;
      const dOut = totalBytesOut - speedRef.current.out;
      speedRef.current = {
        in: totalBytesIn,
        out: totalBytesOut,
        time: now,
        speedIn: Math.max(0, dIn / dt),
        speedOut: Math.max(0, dOut / dt),
      };
    }
  }, [totalBytesIn, totalBytesOut, totalActiveClients]);

  // Periodic refresh to update speed display even when bytes don't change
  useEffect(() => {
    if (totalActiveClients === 0) return;
    const id = setInterval(() => forceUpdate((v) => v + 1), 1000);
    return () => clearInterval(id);
  }, [totalActiveClients]);

  const fmtSpeed = (bytesPerSec: number) => {
    if (bytesPerSec < 1024) return `${bytesPerSec.toFixed(0)} B/s`;
    if (bytesPerSec < 1024 * 1024) return `${(bytesPerSec / 1024).toFixed(1)} KB/s`;
    return `${(bytesPerSec / 1024 / 1024).toFixed(1)} MB/s`;
  };

  return (
    <div
      className="w-64 border-r border-gray-200 dark:border-gray-700 flex flex-col"
      role="navigation"
      aria-label={t("server.list")}
    >
      <div className="p-2 border-b border-gray-200 dark:border-gray-700 space-y-1">
        <button
          className="w-full px-3 py-1.5 text-sm rounded bg-blue-500 text-white hover:bg-blue-600"
          onClick={() => (onAddServer ? onAddServer() : setShowAddDialog(true))}
        >
          + {t("server.add")}
        </button>
        <div className="flex gap-1">
          <button
            className="flex-1 px-2 py-1 text-xs rounded bg-gray-100 dark:bg-gray-800 hover:bg-gray-200 dark:hover:bg-gray-700"
            onClick={handleConnectAll}
            title={t("server.connectAll")}
          >
            {t("server.connectAll")}
          </button>
          <button
            className="flex-1 px-2 py-1 text-xs rounded bg-gray-100 dark:bg-gray-800 hover:bg-gray-200 dark:hover:bg-gray-700"
            onClick={handleDisconnectAll}
            title={t("server.disconnectAll")}
          >
            {t("server.disconnectAll")}
          </button>
        </div>
        <div className="flex gap-1">
          <button
            className="flex-1 px-2 py-1 text-xs rounded bg-gray-100 dark:bg-gray-800 hover:bg-gray-200 dark:hover:bg-gray-700"
            onClick={onOpenTemplates}
            title={t("template.library")}
          >
            {t("template.library")}
          </button>
          <button
            className="flex-1 px-2 py-1 text-xs rounded bg-gray-100 dark:bg-gray-800 hover:bg-gray-200 dark:hover:bg-gray-700"
            onClick={onOpenSettings}
            title={t("settings.title")}
          >
            {t("settings.title")}
          </button>
        </div>
      </div>

      {/* Global health summary */}
      {servers.length > 0 && (
        <div
          className="px-3 py-1.5 text-xs text-gray-600 dark:text-gray-400 border-b border-gray-200 dark:border-gray-700"
          aria-live="polite"
          aria-atomic="true"
        >
          {connectedCount} {t("server.connected")} / {abnormalCount} {t("server.abnormal")}
          {totalActiveClients > 0 && (speedRef.current.speedIn > 0 || speedRef.current.speedOut > 0) && (
            <span className="ml-2 text-blue-500">
              ↑ {fmtSpeed(speedRef.current.speedIn)} ↓ {fmtSpeed(speedRef.current.speedOut)}
            </span>
          )}
        </div>
      )}

      <div
        className="flex-1 overflow-y-auto"
        role="list"
        aria-label={t("server.list")}
      >
        {loading ? (
          <SkeletonList count={3} />
        ) : sorted.length === 0 ? (
          <div className="p-4 text-center text-sm text-gray-500">
            {t("server.add")}
          </div>
        ) : (
          sorted.map((server) => (
            <ServerListItem
              key={server.id}
              server={server}
              selected={server.id === selectedId}
              onSelect={() => selectServer(server.id)}
              onToggleProxy={() =>
                handleToggleProxy(server.id, server.proxy_running)
              }
              onCopyPort={() =>
                handleCopyPort(server.proxy?.socks5_port || 1080, server.id)
              }
              copiedPort={copiedPort === `${server.id}:${server.proxy?.socks5_port || 1080}`}
              onContextMenu={(e) => handleContextMenu(e, server)}
              draggable
              isDragged={draggedId === server.id}
              isDragOver={dragOverId === server.id}
              dragOverPos={dragOverId === server.id ? dragOverPos : undefined}
              onDragStart={(e) => {
                e.dataTransfer.effectAllowed = "move";
                setDraggedId(server.id);
              }}
              onDragOver={(e) => {
                e.preventDefault();
                e.dataTransfer.dropEffect = "move";
                if (server.id !== draggedId) {
                  const rect = e.currentTarget.getBoundingClientRect();
                  const isAfter = e.clientY > rect.top + rect.height / 2;
                  setDragOverId(server.id);
                  setDragOverPos(isAfter ? "after" : "before");
                }
              }}
              onDragLeave={() => {
                setDragOverId(null);
              }}
              onDrop={(e) => {
                e.preventDefault();
                if (draggedId && draggedId !== server.id) {
                  const ids = servers.map((s) => s.id);
                  const fromIdx = ids.indexOf(draggedId);
                  const toIdx = ids.indexOf(server.id);
                  if (fromIdx >= 0 && toIdx >= 0) {
                    ids.splice(fromIdx, 1);
                    // Adjust target index if removing from before
                    let insertIdx = toIdx;
                    if (fromIdx < toIdx) insertIdx = toIdx - 1;
                    if (dragOverPos === "after") insertIdx += 1;
                    ids.splice(insertIdx, 0, draggedId);
                    // Optimistic update
                    const reordered = ids
                      .map((id) => servers.find((s) => s.id === id)!)
                      .filter(Boolean);
                    useServerStore.setState({ servers: reordered });
                    // Persist to backend
                    ipcInvoke("ipc_reorder_servers", { serverIds: ids }).catch(() => {});
                  }
                }
                setDraggedId(null);
                setDragOverId(null);
              }}
              onDragEnd={() => {
                setDraggedId(null);
                setDragOverId(null);
              }}
            />
          ))
        )}
      </div>
      {showAddDialog && (
        <AddServerDialog
          onAdd={handleAddServer}
          onCancel={() => setShowAddDialog(false)}
        />
      )}
    </div>
  );
}

// === SECTION 2 END ===

function ServerListItem({
  server,
  selected,
  onSelect,
  onToggleProxy,
  onCopyPort,
  copiedPort,
  onContextMenu,
  draggable,
  onDragStart,
  onDragOver,
  onDragLeave,
  onDrop,
  onDragEnd,
  isDragged,
  isDragOver,
  dragOverPos,
}: {
  server: ServerState;
  selected: boolean;
  onSelect: () => void;
  onToggleProxy: () => void;
  onCopyPort: () => void;
  copiedPort: boolean;
  onContextMenu: (e: React.MouseEvent) => void;
  draggable?: boolean;
  onDragStart?: (e: React.DragEvent) => void;
  onDragOver?: (e: React.DragEvent) => void;
  onDragLeave?: (e: React.DragEvent) => void;
  onDrop?: (e: React.DragEvent) => void;
  onDragEnd?: (e: React.DragEvent) => void;
  isDragged?: boolean;
  isDragOver?: boolean;
  dragOverPos?: "before" | "after";
}) {
  const { t } = useTranslation();
  const socks5Port = server.proxy?.socks5_port || 1080;

  return (
    <div
      className={`flex items-center gap-2 px-3 py-2 cursor-pointer hover:bg-gray-100 dark:hover:bg-gray-800 ${
        selected ? "bg-blue-50 dark:bg-blue-900/30" : ""
      } ${isDragged ? "opacity-40" : ""} ${
        isDragOver && dragOverPos === "before" ? "border-t-2 border-blue-400" : ""
      } ${isDragOver && dragOverPos === "after" ? "border-b-2 border-blue-400" : ""}`}
      onClick={onSelect}
      onContextMenu={onContextMenu}
      role="listitem"
      tabIndex={0}
      draggable={draggable}
      onDragStart={onDragStart}
      onDragOver={onDragOver}
      onDragLeave={onDragLeave}
      onDrop={onDrop}
      onDragEnd={onDragEnd}
      aria-label={`${server.name} ${t(`server.status.${server.current_status}`)}`}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onSelect();
        }
      }}
    >
      <div
        className={`w-3 h-3 ${STATUS_COLORS[server.current_status]} ${STATUS_SHAPES[server.current_status]}`}
        aria-hidden
      />
      <div className="flex-1 min-w-0">
        <div className="text-sm font-medium truncate">{server.name}</div>
        <div className="text-xs text-gray-500 truncate">
          {server.ssh?.host || server.name}
        </div>
      </div>

      {/* Firewall not configured badge (§9.4) */}
      {!server.suppress_firewall_badge && server.current_status === "connected" && (
        <span
          className="text-xs px-1 py-0.5 rounded bg-yellow-100 dark:bg-yellow-900/50 text-yellow-700 dark:text-yellow-400"
          title={t("server.firewallNotConfigured")}
        >
          FW
        </span>
      )}

      {/* Port copy chip (U8) */}
      <button
        className="text-xs px-1.5 py-0.5 rounded bg-gray-100 dark:bg-gray-800 hover:bg-blue-100 dark:hover:bg-blue-900 text-gray-600 dark:text-gray-400"
        onClick={(e) => {
          e.stopPropagation();
          onCopyPort();
        }}
        title={copiedPort ? t("common.copied") : t("server.copyPort")}
        aria-label={`${t("server.copyPort")} ${socks5Port}`}
      >
        {copiedPort ? "✓" : `:${socks5Port}`}
      </button>

      {/* Inline proxy toggle (U6) */}
      <button
        className={`w-8 h-4 rounded-full transition-colors relative ${
          server.proxy_running
            ? "bg-blue-500"
            : "bg-gray-300 dark:bg-gray-600"
        }`}
        onClick={(e) => {
          e.stopPropagation();
          onToggleProxy();
        }}
        title={server.proxy_running ? t("proxy.on") : t("proxy.off")}
        aria-label={t("proxy.toggle")}
        aria-pressed={server.proxy_running}
      >
        <span
          className={`absolute top-0.5 w-3 h-3 rounded-full bg-white transition-transform ${
            server.proxy_running ? "left-4" : "left-0.5"
          }`}
        />
      </button>
    </div>
  );
}
