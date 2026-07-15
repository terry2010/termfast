// ServerList — left sidebar server list (§9.4 / FP-8.2)
// 5-state visualization, abnormal items pinned to top
// Features: inline proxy toggle, port copy chip, global health summary,
//   connect-all/disconnect-all/template-library/settings buttons, aria-live

import { useEffect, useState, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useServerStore } from "@/stores/serverStore";
import { useLogStore } from "@/stores/logStore";
import { ipcInvoke, formatIpcError, IpcErrorImpl } from "@/hooks/useIpc";
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
  collapsed = false,
  onToggleCollapse,
}: {
  onAddServer?: () => void;
  onOpenTemplates?: () => void;
  onOpenSettings?: () => void;
  collapsed?: boolean;
  onToggleCollapse?: () => void;
}) {
  const { t } = useTranslation();
  const servers = useServerStore((s) => s.servers);
  const selectedId = useServerStore((s) => s.selected_server_id);
  const selectServer = useServerStore((s) => s.selectServer);
  const [showAddDialog, setShowAddDialog] = useState(false);
  const [loading, setLoading] = useState(servers.length === 0);
  const [draggedId, setDraggedId] = useState<string | null>(null);
  const [dragOverId, setDragOverId] = useState<string | null>(null);
  const [dragOverPos, setDragOverPos] = useState<"before" | "after">("before");
  const [hoverExpand, setHoverExpand] = useState(false);

  // Load servers from daemon on mount. Guard against StrictMode/development
  // remounts that would otherwise reload the list and flash the sidebar.
  const initialLoadDone = useRef(false);
  useEffect(() => {
    if (initialLoadDone.current) return;
    initialLoadDone.current = true;
    if (servers.length === 0) {
      loadServers();
    }
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

  const handleContextMenu = useCallback(
    (e: React.MouseEvent, server: ServerState) => {
      const isConnected = server.current_status === "connected";
      const proxyEnabled = server.proxy_running;
      const socks5Port = server.proxy?.socks5_port || 1080;
      const httpPort = server.proxy?.http_port || 8080;
      const mixedPort = server.proxy?.mixed_port || 0;

      // Open a terminal: auto-connect if needed, then open terminal session and
      // select the server so ServerDetail shows the new terminal tab.
      // Flow: click → connecting → SSH connect + terminal open → tab created → connected
      const openTerminal = async () => {
        const store = useServerStore.getState();
        const serverId = server.id;
        const currentServer = store.servers.find((s) => s.id === serverId);
        if (!currentServer) return;
        const alreadyConnected = currentServer.current_status === "connected";

        // Select the server first so the detail panel is visible
        store.selectServer(serverId);

        // If not connected, connect first (status stays "connecting" until terminal is ready)
        if (!alreadyConnected) {
          store.updateServerStatus(serverId, "connecting");
          try {
            await ipcInvoke("ipc_connect_server", { serverId });
            // Don't set "connected" yet — wait until terminal is ready
          } catch (err: any) {
            const errMsg = formatIpcError(err);
            store.updateServerStatus(serverId, "offline");
            useLogStore.getState().addEntry({
              id: `ctx-conn-${Date.now()}-${Math.random().toString(36).slice(2)}`,
              timestamp: new Date().toISOString(),
              server_id: serverId,
              level: "error",
              category: "Connection",
              message: `Connection failed: ${errMsg}`,
              execution_id: null,
              command: null,
              exit_code: null,
              stdout: null,
              stderr: null,
            });
            toast.error(t("server.connect_failed"), { description: errMsg });
            if (err instanceof IpcErrorImpl && err.code === "CredentialNotFound") {
              window.dispatchEvent(
                new CustomEvent("edit-server", { detail: { serverId } })
              );
            }
            return;
          }
        }

        // SSH connected — now open terminal session
        try {
          const result = await ipcInvoke<{ session_id: string; initial_output: string }>(
            "ipc_terminal_open",
            { server_id: serverId, cols: 80, rows: 24 }
          );
          const sessionId = result.session_id;
          const initialOutput = result.initial_output || "";
          const tabId = `term:${sessionId}`;
          const currentTabs = store.terminal_tabs_by_server[serverId] || [];
          const defaultLabel = `${t("server.terminal")} ${currentTabs.length + 1}`;
          // Terminal ready — create tab and switch to it first
          store.addTerminalTab(serverId, { id: tabId, sessionId, label: defaultLabel, defaultLabel, initialOutput, disconnected: false });
          store.setActiveTerminalTab(serverId, tabId);
          // Wait for the tab to render before changing button status
          requestAnimationFrame(() => {
            store.updateServerStatus(serverId, "connected", currentServer.last_known_ip || undefined);
          });
        } catch (err) {
          const msg = formatIpcError(err);
          store.updateServerStatus(serverId, "offline");
          toast.error(t("server.terminal_open_failed"), { description: msg });
        }
      };

      const items: ContextMenuEntry[] = [
        {
          label: t("server.login_server"),
          icon: "⌨",
          onClick: () => { openTerminal(); },
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
        ...(mixedPort > 0
          ? [{
              label: t("server.copy_mixed", { port: mixedPort }),
              icon: "📋",
              onClick: () => {
                navigator.clipboard.writeText(`127.0.0.1:${mixedPort}`).catch(() => {});
              },
            }]
          : [
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
            ]
        ),
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

  const isHoverExpanded = collapsed && hoverExpand;
  const showFullContent = !collapsed || hoverExpand;

  return (
    <div
      className={`relative flex-shrink-0 ${collapsed ? "w-12" : "w-64"}`}
      role="navigation"
      aria-label={t("server.list")}
      onMouseEnter={() => setHoverExpand(true)}
      onMouseLeave={() => setHoverExpand(false)}
    >
      <div
        className={`flex flex-col h-full transition-none ${
          isHoverExpanded
            ? "absolute inset-y-0 left-0 w-64 z-50 bg-white dark:bg-gray-900 border-r border-gray-200 dark:border-gray-700 shadow-xl"
            : collapsed
              ? "w-12 border-r border-gray-200 dark:border-gray-700"
              : "w-64 border-r border-gray-200 dark:border-gray-700"
        }`}
      >
      {/* Collapse toggle + action buttons */}
      <div className={`p-3 border-b border-gray-200 dark:border-gray-700 ${showFullContent ? "flex items-center gap-2" : "flex flex-col items-center gap-2"}`}>
        {!showFullContent && (
          <button
            className="w-8 h-8 flex items-center justify-center rounded-lg bg-gray-100 dark:bg-gray-800 hover:bg-gray-200 dark:hover:bg-gray-700 text-gray-600 dark:text-gray-300 transition-colors"
            onClick={() => {
              setHoverExpand(false);
              onToggleCollapse?.();
            }}
            title={t("server.expand")}
            aria-label={t("server.expand")}
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <polyline points="13 17 18 12 13 7" />
              <polyline points="6 17 11 12 6 7" />
            </svg>
          </button>
        )}
        {showFullContent ? (
          <button
            className="flex items-center justify-center rounded-lg bg-blue-500 text-white hover:bg-blue-600 transition-colors shadow-sm px-3 py-1.5 text-xs font-medium flex-1"
            onClick={() => {
              if (collapsed) setHoverExpand(false);
              onAddServer ? onAddServer() : setShowAddDialog(true);
            }}
            title={t("server.add")}
            aria-label={t("server.add")}
          >
            + {t("server.add")}
          </button>
        ) : (
          <button
            className="w-8 h-8 flex items-center justify-center rounded-lg bg-blue-500 text-white hover:bg-blue-600 transition-colors shadow-sm"
            onClick={() => {
              if (collapsed) setHoverExpand(false);
              onAddServer ? onAddServer() : setShowAddDialog(true);
            }}
            title={t("server.add")}
            aria-label={t("server.add")}
          >
            +
          </button>
        )}
        <button
          className="w-8 h-8 flex items-center justify-center rounded-lg bg-gray-100 dark:bg-gray-800 hover:bg-gray-200 dark:hover:bg-gray-700 text-gray-600 dark:text-gray-300 transition-colors"
          onClick={() => {
            if (collapsed) setHoverExpand(false);
            onOpenTemplates?.();
          }}
          title={t("template.library")}
          aria-label={t("template.library")}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
            <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
          </svg>
        </button>
        <button
          className="w-8 h-8 flex items-center justify-center rounded-lg bg-gray-100 dark:bg-gray-800 hover:bg-gray-200 dark:hover:bg-gray-700 text-gray-600 dark:text-gray-300 transition-colors"
          onClick={() => {
            if (collapsed) setHoverExpand(false);
            onOpenSettings?.();
          }}
          title={t("settings.title")}
          aria-label={t("settings.title")}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round" strokeLinejoin="round">
            <circle cx="12" cy="12" r="3" />
            <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
          </svg>
        </button>
        {showFullContent && (
          <button
            className="w-8 h-8 flex items-center justify-center rounded-lg bg-gray-100 dark:bg-gray-800 hover:bg-gray-200 dark:hover:bg-gray-700 text-gray-600 dark:text-gray-300 transition-colors"
            onClick={() => {
              setHoverExpand(false);
              onToggleCollapse?.();
            }}
            title={collapsed ? t("server.expand") : t("server.collapse")}
            aria-label={collapsed ? t("server.expand") : t("server.collapse")}
          >
            {collapsed ? (
              // Sidebar is collapsed (currently hover-expanded) — show expand icon
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <polyline points="13 17 18 12 13 7" />
                <polyline points="6 17 11 12 6 7" />
              </svg>
            ) : (
              // Sidebar is expanded — show collapse icon
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <polyline points="11 17 6 12 11 7" />
                <polyline points="18 17 13 12 18 7" />
              </svg>
            )}
          </button>
        )}
      </div>

      {/* Server list */}
      <div
        className="flex-1 overflow-y-auto overflow-x-hidden"
        role="list"
        aria-label={t("server.list")}
      >
        {loading ? (
          <SkeletonList count={3} />
        ) : sorted.length === 0 ? (
          <button
            className={`w-full p-4 text-center text-sm text-gray-500 hover:text-blue-500 hover:bg-blue-50 dark:hover:bg-blue-900/20 transition-colors rounded-lg ${!showFullContent ? "hidden" : ""}`}
            onClick={() => {
              if (collapsed) setHoverExpand(false);
              onAddServer ? onAddServer() : setShowAddDialog(true);
            }}
            title={t("server.add")}
          >
            {t("server.add")}
          </button>
        ) : (
          sorted.map((server) => (
            <ServerListItem
              key={server.id}
              server={server}
              selected={server.id === selectedId}
              collapsed={!showFullContent}
              onSelect={() => selectServer(server.id)}
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
  collapsed = false,
  onSelect,
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
  collapsed?: boolean;
  onSelect: () => void;
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

  return (
    <div
      className={`group relative flex items-center gap-3 cursor-pointer transition-colors ${
        collapsed ? "justify-center px-2 py-3" : "px-4 py-3"
      } ${
        selected
          ? "bg-blue-50/80 dark:bg-blue-900/20"
          : "hover:bg-gray-50 dark:hover:bg-gray-800/60"
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
      title={collapsed ? server.name : undefined}
    >
      {selected && (
        <span className="absolute left-0 top-3 bottom-3 w-1 bg-blue-500 rounded-r-full" />
      )}
      <div
        className={`w-2.5 h-2.5 flex-shrink-0 rounded-full ${STATUS_COLORS[server.current_status]} ${STATUS_SHAPES[server.current_status]}`}
        aria-hidden
      />
      {!collapsed && (
        <>
          <div className="flex-1 min-w-0">
            <div className="text-sm font-semibold text-gray-900 dark:text-gray-100 truncate">{server.name}</div>
            <div className="text-xs text-gray-500 dark:text-gray-400 truncate">
              {server.ssh?.host || server.name}
            </div>
          </div>

          {/* Proxy port badge — only shown when proxy is running */}
          {server.proxy_running && (
            <span className="text-[10px] px-1.5 py-0.5 rounded-md font-mono bg-gray-100 dark:bg-gray-800 text-gray-500 dark:text-gray-400 flex-shrink-0">
              :{server.proxy.mixed_port > 0 ? server.proxy.mixed_port : server.proxy.socks5_port}
            </span>
          )}
        </>
      )}
    </div>
  );
}
