// ServerDetail — right panel showing selected server details (§9.4)
// Shows connection controls, proxy toggle, IP, and trigger status
// Tab-based UI: Connection / Proxy / Triggers / Auth (FP-8.3)

import { useState, useRef, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useServerStore, type TerminalTab } from "@/stores/serverStore";
import { useLogStore } from "@/stores/logStore";
import { ipcInvoke, formatIpcError, IpcErrorImpl } from "@/hooks/useIpc";
import { TriggerList } from "@/components/shared/TriggerList";
import { TerminalView } from "@/components/shared/TerminalView";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { showContextMenu, type ContextMenuEntry } from "@/components/ui/ContextMenu";
import { toast } from "@/components/ui/toast";

type Tab = "overview" | `term:${string}`;

const STATUS_COLORS: Record<string, string> = {
  connected: "bg-green-500",
  connecting: "bg-yellow-400",
  reconnecting: "bg-yellow-500",
  auth_failed: "bg-red-500",
  disconnected: "bg-gray-400",
  offline: "bg-gray-500",
};

export function ServerDetail() {
  const { t } = useTranslation();
  const selectedId = useServerStore((s) => s.selected_server_id);
  const servers = useServerStore((s) => s.servers);
  const updateServerStatus = useServerStore((s) => s.updateServerStatus);
  const setProxyStatus = useServerStore((s) => s.setProxyStatus);
  // Terminal tab state lives in the global store so it survives StrictMode
  // remounts in development and keeps tabs when switching servers.
  const addTerminalTab = useServerStore((s) => s.addTerminalTab);
  const removeTerminalTab = useServerStore((s) => s.removeTerminalTab);
  const setTerminalTabsForServer = useServerStore((s) => s.setTerminalTabsForServer);
  const setActiveTerminalTab = useServerStore((s) => s.setActiveTerminalTab);
  const renameTerminalTab = useServerStore((s) => s.renameTerminalTab);
  const setTerminalTabDisconnected = useServerStore((s) => s.setTerminalTabDisconnected);
  const clearTerminalTabs = useServerStore((s) => s.clearTerminalTabs);
  const terminalTabsByServer = useServerStore((s) => s.terminal_tabs_by_server);
  const activeTerminalTabByServer = useServerStore((s) => s.active_terminal_tab_by_server);
  const termTabs = terminalTabsByServer[selectedId || ""] || [];
  const activeTab: Tab = (activeTerminalTabByServer[selectedId || ""] as Tab) || "overview";
  const [connecting, setConnecting] = useState(false);
  const [testProxyUrl, setTestProxyUrl] = useState("");
  const [testProxyResult, setTestProxyResult] = useState<{
    success: boolean;
    exit_ip: string | null;
    latency_ms: number;
    error?: string;
  } | null>(null);
  const [testingProxy, setTestingProxy] = useState(false);
  const testProxyAbort = useRef<AbortController | null>(null);
  const [systemProxyEnabled, setSystemProxyEnabled] = useState(false);
  // Tab rename state: which tab id is being renamed, and the current edit text
  const [renamingTabId, setRenamingTabId] = useState<string | null>(null);
  const [renameText, setRenameText] = useState("");
  // Disconnect confirmation: shown when user clicks disconnect with active terminals
  const [showDisconnectConfirm, setShowDisconnectConfirm] = useState(false);
  // Ref to detect right-click on macOS "bottom-right corner" click mode
  // (where click fires before contextmenu with button=0, ctrlKey=false)
  const rightClickRef = useRef(false);
  // Drag-to-reorder state for terminal tabs (overview tab is not draggable)
  const [draggedTabId, setDraggedTabId] = useState<string | null>(null);
  const [dragOverTabId, setDragOverTabId] = useState<string | null>(null);

  const server = servers.find((s) => s.id === selectedId);
  const isConnected = server?.current_status === "connected";

  // Listen for terminal:closed events to mark tabs as disconnected.
  // Iterate all servers' tabs because the closed session may belong to a
  // server that is not currently selected.
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    listen<{ sessionId: string }>("terminal:closed", (event) => {
      const sid = event.payload.sessionId;
      const store = useServerStore.getState();
      for (const serverId of Object.keys(store.terminal_tabs_by_server)) {
        store.setTerminalTabDisconnected(serverId, sid);
      }
    }).then((fn) => { unlisten = fn; });
    return () => { if (unlisten) unlisten(); };
  }, []);

  // Open a new terminal session and add a tab for it.
  const handleOpenTerminal = useCallback(async () => {
    if (!server?.id) return;
    const serverId = server.id;
    // If not connected, auto-connect first
    if (!isConnected) {
      setConnecting(true);
      updateServerStatus(serverId, "connecting");
      try {
        await ipcInvoke("ipc_connect_server", { serverId });
        updateServerStatus(serverId, "connected", server.last_known_ip || undefined);
      } catch (e: any) {
        const errMsg = formatIpcError(e);
        updateServerStatus(serverId, "offline");
        useLogStore.getState().addEntry({
          id: `conn-err-${Date.now()}-${Math.random().toString(36).slice(2)}`,
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
        if (e instanceof IpcErrorImpl && e.code === "CredentialNotFound") {
          window.dispatchEvent(
            new CustomEvent("edit-server", { detail: { serverId } })
          );
        }
        return;
      } finally {
        setConnecting(false);
      }
    }
    // Now connected — open terminal session
    try {
      const result = await ipcInvoke<{ session_id: string; initial_output: string }>(
        "ipc_terminal_open",
        { server_id: serverId, cols: 80, rows: 24 }
      );
      const sessionId = result.session_id;
      const initialOutput = result.initial_output || "";
      const tabId: Tab = `term:${sessionId}`;
      const currentTabs = useServerStore.getState().terminal_tabs_by_server[serverId] || [];
      const defaultLabel = `${t("server.terminal")} ${currentTabs.length + 1}`;
      addTerminalTab(serverId, { id: tabId, sessionId, label: defaultLabel, defaultLabel, initialOutput, disconnected: false });
    } catch (e) {
      const msg = formatIpcError(e);
      toast.error(t("server.terminal_open_failed"), { description: msg });
    }
  }, [server?.id, server?.last_known_ip, isConnected, t, addTerminalTab]);

  // Open a terminal from the context menu. Uses the same logic as the login button.
  const openTerminalFromMenu = useCallback(async () => {
    const store = useServerStore.getState();
    const serverId = store.selected_server_id;
    if (!serverId) return;
    const currentServer = store.servers.find((s) => s.id === serverId);
    if (!currentServer) return;
    const alreadyConnected = currentServer.current_status === "connected";

    // If not connected, connect first
    if (!alreadyConnected) {
      setConnecting(true);
      store.updateServerStatus(serverId, "connecting");
      try {
        await ipcInvoke("ipc_connect_server", { serverId });
        store.updateServerStatus(serverId, "connected", currentServer.last_known_ip || undefined);
      } catch (e: any) {
        const errMsg = formatIpcError(e);
        store.updateServerStatus(serverId, "offline");
        useLogStore.getState().addEntry({
          id: `conn-err-${Date.now()}-${Math.random().toString(36).slice(2)}`,
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
        if (e instanceof IpcErrorImpl && e.code === "CredentialNotFound") {
          window.dispatchEvent(
            new CustomEvent("edit-server", { detail: { serverId } })
          );
        }
        return;
      } finally {
        setConnecting(false);
      }
    }

    // Open terminal session
    try {
      const result = await ipcInvoke<{ session_id: string; initial_output: string }>(
        "ipc_terminal_open",
        { server_id: serverId, cols: 80, rows: 24 }
      );
      const sessionId = result.session_id;
      const initialOutput = result.initial_output || "";
      const tabId: Tab = `term:${sessionId}`;
      const currentTabs = store.terminal_tabs_by_server[serverId] || [];
      const defaultLabel = `${t("server.terminal")} ${currentTabs.length + 1}`;
      store.addTerminalTab(serverId, { id: tabId, sessionId, label: defaultLabel, defaultLabel, initialOutput, disconnected: false });
    } catch (e) {
      const msg = formatIpcError(e);
      toast.error(t("server.terminal_open_failed"), { description: msg });
    }
  }, [t]);

  const handleCloseTerminal = useCallback(
    (tabId: string, e: React.MouseEvent) => {
      e.stopPropagation();
      const serverId = selectedId || "";
      const currentTabs = terminalTabsByServer[serverId] || [];
      const tab = currentTabs.find((tt) => tt.id === tabId);
      if (tab) {
        ipcInvoke("ipc_terminal_close", { session_id: tab.sessionId }).catch(() => {});
      }
      removeTerminalTab(serverId, tabId);
    },
    [selectedId, terminalTabsByServer, removeTerminalTab]
  );

  if (!server) {
    return (
      <div className="flex-1 flex items-center justify-center text-gray-500">
        {t("server.add")}
      </div>
    );
  }

  const handleConnect = async () => {
    if (!server.id) return;
    setConnecting(true);
    updateServerStatus(server.id, "connecting");
    try {
      await ipcInvoke("ipc_connect_server", { serverId: server.id });
      updateServerStatus(server.id, "connected", server.last_known_ip || undefined);
    } catch (e: any) {
      const errMsg = formatIpcError(e);
      updateServerStatus(server.id, "offline");
      useLogStore.getState().addEntry({
        id: `conn-err-${Date.now()}-${Math.random().toString(36).slice(2)}`,
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
      toast.error(t("server.connect_failed"), { description: errMsg });
      if (e instanceof IpcErrorImpl && e.code === "CredentialNotFound") {
        window.dispatchEvent(
          new CustomEvent("edit-server", { detail: { serverId: server.id } })
        );
      }
    } finally {
      setConnecting(false);
    }
  };

  const handleDisconnect = async () => {
    if (!server.id) return;
    // If there are active terminal sessions, confirm before disconnecting
    // (disconnecting will close all terminals)
    if (termTabs.length > 0) {
      setShowDisconnectConfirm(true);
      return;
    }
    doDisconnect();
  };

  const doDisconnect = async () => {
    if (!server.id) return;
    try {
      // Close all terminal sessions for this server
      for (const tt of termTabs) {
        ipcInvoke("ipc_terminal_close", { session_id: tt.sessionId }).catch(() => {});
      }
      clearTerminalTabs(server.id);
      await ipcInvoke("ipc_disconnect_server", { serverId: server.id });
      // Optimistic update — daemon event will confirm/refine this
      updateServerStatus(server.id, "disconnected");
    } catch (e) {
      const msg = formatIpcError(e);
      toast.error(t("server.disconnect_failed"), { description: msg });
    }
  };

  const handleRenameTab = (tabId: string, currentLabel: string) => {
    setRenamingTabId(tabId);
    setRenameText(currentLabel);
  };

  const commitRename = () => {
    if (!renamingTabId || !server.id) {
      setRenamingTabId(null);
      return;
    }
    const newLabel = renameText.trim();
    if (newLabel) {
      renameTerminalTab(server.id, renamingTabId, newLabel);
    }
    setRenamingTabId(null);
  };

  // Close a single terminal tab (no event — used by context menu)
  const closeTab = (tabId: string) => {
    const serverId = selectedId || "";
    const currentTabs = terminalTabsByServer[serverId] || [];
    const tab = currentTabs.find((tt) => tt.id === tabId);
    if (tab) {
      ipcInvoke("ipc_terminal_close", { session_id: tab.sessionId }).catch(() => {});
    }
    removeTerminalTab(serverId, tabId);
  };

  // Close all disconnected tabs
  const closeDisconnectedTabs = () => {
    const serverId = selectedId || "";
    const currentTabs = terminalTabsByServer[serverId] || [];
    for (const tt of currentTabs) {
      if (tt.disconnected) {
        ipcInvoke("ipc_terminal_close", { session_id: tt.sessionId }).catch(() => {});
      }
    }
    setTerminalTabsForServer(serverId, currentTabs.filter((tt) => !tt.disconnected));
  };

  // Close all tabs except the given one
  const closeOtherTabs = (keepTabId: string) => {
    const serverId = selectedId || "";
    const currentTabs = terminalTabsByServer[serverId] || [];
    for (const tt of currentTabs) {
      if (tt.id !== keepTabId) {
        ipcInvoke("ipc_terminal_close", { session_id: tt.sessionId }).catch(() => {});
      }
    }
    setTerminalTabsForServer(serverId, currentTabs.filter((tt) => tt.id === keepTabId));
    if (activeTerminalTabByServer[serverId] !== keepTabId) {
      setActiveTerminalTab(serverId, "overview");
    }
  };

  // Close all terminal tabs
  const closeAllTabs = () => {
    const serverId = selectedId || "";
    const currentTabs = terminalTabsByServer[serverId] || [];
    for (const tt of currentTabs) {
      ipcInvoke("ipc_terminal_close", { session_id: tt.sessionId }).catch(() => {});
    }
    setTerminalTabsForServer(serverId, []);
    setActiveTerminalTab(serverId, "overview");
  };

  // Reorder terminal tabs by moving draggedTabId to the position of targetTabId.
  // Overview tab is never part of the draggable set.
  const handleReorderTabs = (draggedId: string, targetId: string) => {
    if (draggedId === targetId) return;
    const serverId = selectedId || "";
    const currentTabs = terminalTabsByServer[serverId] || [];
    const draggedIndex = currentTabs.findIndex((tt) => tt.id === draggedId);
    const targetIndex = currentTabs.findIndex((tt) => tt.id === targetId);
    if (draggedIndex === -1 || targetIndex === -1) return;
    const next = [...currentTabs];
    const [moved] = next.splice(draggedIndex, 1);
    next.splice(targetIndex, 0, moved);
    setTerminalTabsForServer(serverId, next);
  };

  // Restore a tab's label to its default
  const restoreDefaultName = (tabId: string) => {
    const serverId = selectedId || "";
    const tab = terminalTabsByServer[serverId]?.find((tt) => tt.id === tabId);
    if (tab) {
      renameTerminalTab(serverId, tabId, tab.defaultLabel);
    }
  };

  // Context menu for the overview tab
  const handleOverviewContextMenu = (e: React.MouseEvent) => {
    const serverId = selectedId || "";
    const currentTabs = terminalTabsByServer[serverId] || [];
    const hasDisconnected = currentTabs.some((tt) => tt.disconnected);
    const proxyPort = server.proxy.mixed_port > 0 ? server.proxy.mixed_port : server.proxy.socks5_port;
    const items: ContextMenuEntry[] = [
      ...(isConnected
        ? [{ label: t("tab.disconnect"), onClick: () => handleDisconnect(), danger: true } as ContextMenuEntry]
        : [{ label: t("tab.connect"), onClick: () => handleConnect() } as ContextMenuEntry]),
      { label: t("tab.login_server"), onClick: () => openTerminalFromMenu() },
      { separator: true },
      { label: t("tab.close_disconnected_terminals"), onClick: () => closeDisconnectedTabs(), disabled: !hasDisconnected },
      { label: t("tab.close_all_terminals"), onClick: () => closeAllTabs(), disabled: currentTabs.length === 0 },
      { separator: true },
      ...(server.proxy_running
        ? [
            { label: t("tab.stop_proxy", { port: proxyPort }), onClick: () => handleToggleProxy() } as ContextMenuEntry,
            ...(systemProxyEnabled
              ? [{ label: t("tab.unset_system_proxy"), onClick: () => handleClearSystemProxy() } as ContextMenuEntry]
              : [{ label: t("tab.set_system_proxy"), onClick: () => handleSetSystemProxy() } as ContextMenuEntry]),
          ]
        : [{ label: t("tab.start_proxy", { port: proxyPort }), onClick: () => handleToggleProxy() } as ContextMenuEntry]),
    ];
    showContextMenu(e, items);
  };

  // Context menu for terminal tabs
  const handleTabContextMenu = (e: React.MouseEvent, tabId: string) => {
    const serverId = selectedId || "";
    const currentTabs = terminalTabsByServer[serverId] || [];
    const tab = currentTabs.find((tt) => tt.id === tabId);
    if (!tab) return;
    const hasDisconnected = currentTabs.some((tt) => tt.disconnected);
    const items: ContextMenuEntry[] = [
      { label: t("tab.rename"), onClick: () => handleRenameTab(tabId, tab.label) },
      { label: t("tab.restore_default_name"), onClick: () => restoreDefaultName(tabId), disabled: tab.label === tab.defaultLabel },
      { label: t("tab.reconnect"), onClick: () => handleConnect(), disabled: isConnected },
      { label: t("tab.disconnect"), onClick: () => handleDisconnect(), disabled: !isConnected, danger: true },
      { separator: true },
      { label: t("tab.close_session"), onClick: () => closeTab(tabId) },
      { label: t("tab.close_disconnected_sessions"), onClick: () => closeDisconnectedTabs(), disabled: !hasDisconnected },
      { label: t("tab.close_other_sessions"), onClick: () => closeOtherTabs(tabId), disabled: currentTabs.length <= 1 },
      { label: t("tab.close_all_sessions"), onClick: () => closeAllTabs(), disabled: currentTabs.length === 0 },
      { separator: true },
      { label: t("tab.new_clone_session"), onClick: () => openTerminalFromMenu() },
    ];
    showContextMenu(e, items);
  };

  const handleToggleProxy = async () => {
    if (!server.id) return;
    const newEnabled = !server.proxy_running;

    // If starting proxy and not connected, auto-connect first
    if (newEnabled && !isConnected) {
      setConnecting(true);
      updateServerStatus(server.id, "connecting");
      try {
        await ipcInvoke("ipc_connect_server", { serverId: server.id });
        updateServerStatus(server.id, "connected", server.last_known_ip || undefined);
      } catch (e: any) {
        const errMsg = formatIpcError(e);
        updateServerStatus(server.id, "offline");
        useLogStore.getState().addEntry({
          id: `conn-err-${Date.now()}-${Math.random().toString(36).slice(2)}`,
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
        toast.error(t("server.connect_failed"), { description: errMsg });
        if (e instanceof IpcErrorImpl && e.code === "CredentialNotFound") {
          window.dispatchEvent(
            new CustomEvent("edit-server", { detail: { serverId: server.id } })
          );
        }
        return;
      } finally {
        setConnecting(false);
      }
    }

    try {
      await ipcInvoke("ipc_toggle_proxy", {
        serverId: server.id,
        enabled: newEnabled,
      });
      setProxyStatus(server.id, newEnabled);
    } catch (e) {
      const errMsg = formatIpcError(e);
      useLogStore.getState().addEntry({
        id: `proxy-toggle-${Date.now()}-${Math.random().toString(36).slice(2)}`,
        timestamp: new Date().toISOString(),
        server_id: server.id,
        level: "error",
        category: "Proxy",
        message: `Proxy toggle failed: ${errMsg}`,
        execution_id: null,
        command: null,
        exit_code: null,
        stdout: null,
        stderr: null,
      });
      toast.error(t("server.proxy_toggle_failed"), { description: errMsg });
    }
  };

  const handleUpdateProxy = async (patch: { socks5_port?: number; http_port?: number; mixed_port?: number }) => {
    if (!server.id) return;
    try {
      await ipcInvoke("ipc_update_server", {
        server_id: server.id,
        ...patch,
      });
      // Update local store
      useServerStore.setState((s) => ({
        servers: s.servers.map((srv) =>
          srv.id === server.id
            ? { ...srv, proxy: { ...srv.proxy, ...patch } }
            : srv
        ),
      }));
    } catch (e) {
      const msg = formatIpcError(e);
      toast.error(t("server.proxy_update_failed"), { description: msg });
    }
  };

  const handleSetSystemProxy = async () => {
    if (!server.id) return;
    try {
      await ipcInvoke("ipc_set_system_proxy", { serverId: server.id });
      setSystemProxyEnabled(true);
      toast.success(t("server.set_system_proxy"));
    } catch (e) {
      const msg = formatIpcError(e);
      toast.error(t("server.set_system_proxy_failed"), { description: msg });
    }
  };

  const handleClearSystemProxy = async () => {
    try {
      await ipcInvoke("ipc_clear_system_proxy", {});
      setSystemProxyEnabled(false);
      toast.success(t("server.clear_system_proxy"));
    } catch (e) {
      const msg = formatIpcError(e);
      toast.error(t("server.clear_system_proxy_failed"), { description: msg });
    }
  };

  const handleTestProxy = async () => {
    if (!server.id) return;
    setTestingProxy(true);
    setTestProxyResult(null);
    const abort = new AbortController();
    testProxyAbort.current = abort;
    try {
      const result = await Promise.race([
        ipcInvoke<{
          success: boolean;
          exit_ip: string | null;
          latency_ms: number;
          error?: string;
        }>("ipc_test_proxy", {
          server_id: server.id,
          url: testProxyUrl || undefined,
        }),
        new Promise<never>((_, reject) => {
          abort.signal.addEventListener("abort", () =>
            reject(new Error(t("server.test_proxy_cancelled")))
          );
        }),
      ]);
      setTestProxyResult(result);
    } catch (e) {
      if (abort.signal.aborted) {
        // User cancelled — don't show error result
      } else {
        setTestProxyResult({
          success: false,
          exit_ip: null,
          latency_ms: 0,
          error: formatIpcError(e),
        });
      }
    } finally {
      setTestingProxy(false);
      testProxyAbort.current = null;
    }
  };

  const handleCancelTestProxy = () => {
    testProxyAbort.current?.abort();
  };

  const tabs: { key: Tab; label: string; disconnected: boolean }[] = [
    { key: "overview", label: t("server.overview"), disconnected: false },
    ...termTabs.map((tt) => ({ key: tt.id as Tab, label: tt.label, disconnected: tt.disconnected })),
  ];

  const statusColor = isConnected ? "text-green-500" : server.current_status === "auth_failed" || server.current_status === "offline" ? "text-red-500" : "text-gray-400";

  // When a terminal tab is active, remove all padding so the terminal fills
  // the panel edge-to-edge. When overview is active, keep the padded layout.
  const isTerminalActive = activeTab !== "overview";

  return (
    <div className={`flex-1 ${isTerminalActive ? "overflow-hidden flex flex-col" : "overflow-y-auto p-8"} bg-gray-50/50 dark:bg-gray-900/50`}>
      {/* Tab bar — overview + terminal tabs */}
      <div className={`flex gap-1 border-b border-gray-200 dark:border-gray-700 ${isTerminalActive ? "" : "mb-8"}`}>
        {tabs.map((tab) => {
          const isOverview = tab.key === "overview";
          const isDraggable = !isOverview;
          return (
          <div
            key={tab.key}
            draggable={isDraggable}
            onDragStart={(e) => {
              if (!isDraggable) return;
              setDraggedTabId(tab.key);
              e.dataTransfer.effectAllowed = "move";
              // Required for Firefox to start a drag
              e.dataTransfer.setData("text/plain", tab.key);
            }}
            onDragEnd={() => {
              setDraggedTabId(null);
              setDragOverTabId(null);
            }}
            onDragOver={(e) => {
              if (!isDraggable || !draggedTabId || draggedTabId === tab.key) return;
              e.preventDefault();
              e.dataTransfer.dropEffect = "move";
              setDragOverTabId(tab.key);
            }}
            onDragLeave={() => {
              if (dragOverTabId === tab.key) setDragOverTabId(null);
            }}
            onDrop={(e) => {
              if (!isDraggable || !draggedTabId) return;
              e.preventDefault();
              if (draggedTabId !== tab.key) {
                handleReorderTabs(draggedTabId, tab.key);
              }
              setDraggedTabId(null);
              setDragOverTabId(null);
            }}
            className={`flex items-center gap-1.5 px-4 py-2.5 text-sm font-medium rounded-t-lg transition-colors cursor-pointer ${
              activeTab === tab.key
                ? "bg-white dark:bg-gray-800 text-blue-600 dark:text-blue-400 border-b-2 border-blue-500"
                : "text-gray-500 hover:text-gray-700 dark:hover:text-gray-300"
            } ${tab.disconnected && !isOverview ? "opacity-50 italic" : ""} ${
              isDraggable ? "select-none" : ""
            } ${dragOverTabId === tab.key && draggedTabId && draggedTabId !== tab.key ? "border-l-2 border-blue-400" : ""} ${
              draggedTabId === tab.key ? "opacity-40" : ""
            }`}
            onClick={() => {
              rightClickRef.current = false;
              setActiveTerminalTab(server.id, tab.key);
            }}
            onDoubleClick={(e) => {
              if (tab.key !== "overview") {
                e.stopPropagation();
                handleRenameTab(tab.key, tab.label);
              }
            }}
            onContextMenu={(e) => {
              // Set flag so the ✕ button's delayed onClick can detect right-click
              rightClickRef.current = true;
              if (tab.key === "overview") {
                handleOverviewContextMenu(e);
              } else {
                handleTabContextMenu(e, tab.key);
              }
            }}
            title={tab.key !== "overview" ? t("server.double_click_to_rename") : undefined}
          >
            {renamingTabId === tab.key ? (
              <input
                className="text-sm bg-transparent border-b border-blue-500 outline-none text-blue-600 dark:text-blue-400 min-w-0 w-24"
                value={renameText}
                autoFocus
                onChange={(e) => setRenameText(e.target.value)}
                onBlur={commitRename}
                onKeyDown={(e) => {
                  if (e.key === "Enter") commitRename();
                  if (e.key === "Escape") setRenamingTabId(null);
                }}
                onClick={(e) => e.stopPropagation()}
              />
            ) : (
              <span>{tab.label}</span>
            )}
            {tab.key !== "overview" && renamingTabId !== tab.key && (
              <button
                className="ml-1 text-gray-400 hover:text-red-500 transition-colors text-xs leading-none"
                onClick={(e) => {
                  e.stopPropagation();
                  // Ignore Ctrl+click (macOS right-click)
                  if (e.ctrlKey || e.metaKey) return;
                  // Delay to check if contextmenu fires after click
                  // (macOS "bottom-right corner" right-click generates a regular
                  //  click event with button=0, ctrlKey=false before contextmenu)
                  setTimeout(() => {
                    if (rightClickRef.current) {
                      rightClickRef.current = false;
                      return;
                    }
                    closeTab(tab.key);
                  }, 100);
                }}
                title={t("common.close")}
              >
                ✕
              </button>
            )}
          </div>
          );
        })}
      </div>

      {/* Disconnect confirmation — shown when disconnecting with active terminals */}
      {showDisconnectConfirm && (
        <ConfirmDialog
          level="low"
          title={t("server.disconnect")}
          message={t("server.disconnect_with_terminals_confirm", { count: termTabs.length })}
          confirmLabel={t("server.disconnect")}
          onConfirm={() => {
            setShowDisconnectConfirm(false);
            doDisconnect();
          }}
          onCancel={() => setShowDisconnectConfirm(false)}
        />
      )}

      {activeTab === "overview" && (
        <div className="space-y-6 max-w-6xl">
          {/* Primary action cards */}
          <div className="grid grid-cols-1 md:grid-cols-2 gap-5">
            {/* Connection card */}
            <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 p-5 shadow-sm">
              <div className="flex items-center justify-between mb-5">
                <div className="min-w-0">
                  <div className="text-xs text-gray-500 uppercase tracking-wider font-medium">{t("server.node_name")}</div>
                  <div className="flex items-center gap-2 mt-1">
                    <span className="text-xl font-bold text-gray-900 dark:text-gray-100 truncate">{server.name}</span>
                    <span className={`px-2 py-0.5 rounded-full text-[10px] font-semibold ${statusColor} bg-gray-100 dark:bg-gray-800`}>
                      {t(`server.status.${server.current_status}`)}
                    </span>
                  </div>
                </div>
                <div className="flex items-center gap-2 flex-shrink-0">
                  {isConnected && (
                    <button
                      className="px-5 py-2.5 text-sm rounded-lg bg-red-500 text-white hover:bg-red-600 font-medium shadow-sm transition-colors"
                      onClick={handleDisconnect}
                    >
                      {t("server.disconnect")}
                    </button>
                  )}
                  <button
                    className="px-4 py-2.5 text-sm rounded-lg bg-green-500 text-white hover:bg-green-600 disabled:opacity-50 font-medium shadow-sm transition-colors"
                    onClick={handleOpenTerminal}
                    disabled={connecting}
                  >
                    {connecting ? t("server.status.connecting") : (termTabs.length === 0 ? t("server.connect_terminal") : t("server.login_server"))}
                  </button>
                </div>
              </div>
              <div className="grid grid-cols-2 gap-4 text-sm border-t border-gray-100 dark:border-gray-700 pt-5">
                <div>
                  <div className="text-xs text-gray-500 mb-1.5">{t("server.host")}</div>
                  <div className="font-mono text-sm text-gray-900 dark:text-gray-100 truncate">{server.ssh?.host || "?"}:{server.ssh?.port || "?"}</div>
                  <div className="text-xs text-gray-500 mt-3 mb-1">{t("server.ip_label")}</div>
                  <div className="font-mono text-sm text-gray-900 dark:text-gray-100 truncate">{server.client_ip || "—"}</div>
                </div>
                <div>
                  <div className="text-xs text-gray-500 mb-1.5">{t("server.auth_method")}</div>
                  <div className="text-sm text-gray-900 dark:text-gray-100">{server.ssh?.auth_method === "key" ? t("server.ssh_key") : t("server.password")}</div>
                </div>
              </div>
              {server.auth_banner && (
                <div className="mt-4 border-t border-gray-100 dark:border-gray-700 pt-4">
                  <div className="text-xs text-gray-500 mb-1.5">{t("server.welcome_message")}</div>
                  <pre className="font-mono text-xs text-gray-700 dark:text-gray-300 bg-gray-50 dark:bg-gray-900/50 rounded-lg p-3 overflow-x-auto whitespace-pre-wrap">{server.auth_banner}</pre>
                </div>
              )}
            </div>

            {/* Proxy card */}
            <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 p-5 shadow-sm flex flex-col">
              {/* Header: status + toggle */}
              <div className="flex items-center justify-between mb-5">
                <div>
                  <div className="text-xs text-gray-500 uppercase tracking-wider font-medium">{t("server.proxy")}</div>
                  <div className={`text-xl font-bold mt-1 ${server.proxy_running ? "text-green-500" : "text-gray-400"}`}>
                    {server.proxy_running ? t("proxy.on") : t("proxy.off")}
                  </div>
                </div>
                <button
                  className={`px-5 py-2.5 text-sm rounded-lg font-medium shadow-sm transition-colors ${
                    server.proxy_running
                      ? "bg-green-500 text-white hover:bg-green-600"
                      : "bg-gray-100 dark:bg-gray-700 text-gray-700 dark:text-gray-200 hover:bg-gray-200 dark:hover:bg-gray-600"
                  }`}
                  onClick={handleToggleProxy}
                  disabled={connecting && !server.proxy_running}
                >
                  {server.proxy_running ? t("server.stop_proxy") : t("server.start_proxy")}
                </button>
              </div>

              {/* Port configuration */}
              <div className="border-t border-gray-100 dark:border-gray-700 pt-5 mb-5">
                <div className="flex items-center gap-4 flex-wrap">
                  {server.proxy.mixed_port > 0 ? (
                    <div className="flex items-center gap-2">
                      <span className="text-xs text-gray-500 font-medium">Mixed</span>
                      <input
                        type="number"
                        className="appearance-none w-14 px-2 py-1.5 text-sm font-mono border border-gray-200 dark:border-gray-600 rounded-lg bg-transparent text-gray-900 dark:text-gray-100 focus:outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-100 dark:focus:ring-blue-900/30 [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none"
                        value={server.proxy.mixed_port}
                        onChange={(e) => handleUpdateProxy({ mixed_port: parseInt(e.target.value) || 0 })}
                        disabled={server.proxy_running}
                      />
                    </div>
                  ) : (
                    <>
                      <div className="flex items-center gap-2">
                        <span className="text-xs text-gray-500 font-medium">SOCKS5</span>
                        <input
                          type="number"
                          className="appearance-none w-14 px-2 py-1.5 text-sm font-mono border border-gray-200 dark:border-gray-600 rounded-lg bg-transparent text-gray-900 dark:text-gray-100 focus:outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-100 dark:focus:ring-blue-900/30 [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none"
                          value={server.proxy.socks5_port}
                          onChange={(e) => handleUpdateProxy({ socks5_port: parseInt(e.target.value) || 1080 })}
                          disabled={server.proxy_running}
                        />
                      </div>
                      <div className="flex items-center gap-2">
                        <span className="text-xs text-gray-500 font-medium">HTTP</span>
                        <input
                          type="number"
                          className="appearance-none w-14 px-2 py-1.5 text-sm font-mono border border-gray-200 dark:border-gray-600 rounded-lg bg-transparent text-gray-900 dark:text-gray-100 focus:outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-100 dark:focus:ring-blue-900/30 [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none"
                          value={server.proxy.http_port}
                          onChange={(e) => handleUpdateProxy({ http_port: parseInt(e.target.value) || 8080 })}
                          disabled={server.proxy_running}
                        />
                      </div>
                    </>
                  )}
                  <label className="flex items-center gap-1.5 text-xs text-gray-500 cursor-pointer select-none">
                    <input
                      type="checkbox"
                      checked={server.proxy.mixed_port > 0}
                      onChange={(e) => handleUpdateProxy({ mixed_port: e.target.checked ? (server.proxy.socks5_port || 1080) : 0 })}
                      disabled={server.proxy_running}
                      className="rounded"
                    />
                    {t("server.mixed_port")}
                  </label>
                  <label className={`inline-flex items-center gap-1.5 text-xs text-gray-500 cursor-pointer select-none ml-auto ${!server.proxy_running ? "opacity-50 pointer-events-none" : ""}`}>
                    <input
                      type="checkbox"
                      checked={systemProxyEnabled}
                      onChange={(e) => {
                        if (e.target.checked) handleSetSystemProxy();
                        else handleClearSystemProxy();
                      }}
                      disabled={!server.proxy_running}
                      className="rounded"
                    />
                    {t("server.set_system_proxy")}
                  </label>
                </div>
              </div>

              {/* Active clients indicator */}
              {server.proxy_running && server.active_channels > 0 && (
                <div className="text-xs text-green-600 dark:text-green-400 font-medium mb-4">
                  {server.active_channels} {t("server.active_clients")}
                </div>
              )}

              {/* Test proxy section */}
              <div className="border-t border-gray-100 dark:border-gray-700 pt-4 mt-auto">
                <div className="flex items-center gap-2">
                  <input
                    type="text"
                    className="flex-1 px-3 py-2 text-sm border border-gray-200 dark:border-gray-600 rounded-lg bg-transparent focus:outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-100 dark:focus:ring-blue-900/30"
                    placeholder={t("server.test_proxy_url_placeholder")}
                    value={testProxyUrl}
                    onChange={(e) => setTestProxyUrl(e.target.value)}
                    disabled={!server.proxy_running}
                  />
                  <button
                    className="px-4 py-2 text-sm rounded-lg bg-blue-500 text-white hover:bg-blue-600 disabled:opacity-50 transition-colors"
                    onClick={handleTestProxy}
                    disabled={!server.proxy_running || testingProxy}
                  >
                    {testingProxy ? t("common.testing") : t("server.test_proxy_btn")}
                  </button>
                  {testingProxy && (
                    <button
                      className="px-4 py-2 text-sm rounded-lg bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 transition-colors"
                      onClick={handleCancelTestProxy}
                    >
                      {t("common.cancel")}
                    </button>
                  )}
                </div>
                {testProxyResult && (
                  <div className={`mt-3 p-3 rounded-lg text-sm ${
                    testProxyResult.success
                      ? "bg-green-50 dark:bg-green-900/30 text-green-700 dark:text-green-400"
                      : "bg-red-50 dark:bg-red-900/30 text-red-700 dark:text-red-400"
                  }`}>
                    {testProxyResult.success ? (
                      <span>
                        {t("server.test_proxy_success", {
                          ip: testProxyResult.exit_ip,
                          latency: testProxyResult.latency_ms,
                        })}
                      </span>
                    ) : (
                      <span>
                        {t("server.test_proxy_failed")}
                        {testProxyResult.error ? `: ${testProxyResult.error}` : ""}
                      </span>
                    )}
                  </div>
                )}
              </div>
            </div>
          </div>

          {/* Triggers panel — full width */}
          <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 shadow-sm overflow-hidden">
            <div className="px-5 py-4 border-b border-gray-100 dark:border-gray-700 bg-gray-50/50 dark:bg-gray-800/50 flex items-center justify-between">
              <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100">{t("trigger.title")}</h3>
            </div>
            <div className="p-5">
              <TriggerList serverId={server.id} />
            </div>
          </div>
        </div>
      )}

      {/* Terminal tab content — all tabs kept mounted; hidden tabs use absolute
          positioning so xterm.js containers still have proper dimensions for fit() */}
      {termTabs.map((tt) => (
        <div
          key={tt.id}
          className={isTerminalActive ? "flex-1 min-h-0" : "h-[calc(100vh-200px)] min-h-[400px]"}
          style={
            activeTab === tt.id
              ? { position: "relative", visibility: "visible" }
              : { position: "absolute", left: "-9999px", top: 0, width: "100%", visibility: "hidden" }
          }
        >
          <TerminalView sessionId={tt.sessionId} serverId={server.id} active={activeTab === tt.id} initialOutput={tt.initialOutput} />
        </div>
      ))}
    </div>
  );
}
