// ServerDetail — right panel showing selected server details (§9.4)
// Shows connection controls, proxy toggle, IP, and trigger status
// Tab-based UI: Connection / Proxy / Triggers / Auth (FP-8.3)

import { useState, useRef, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useServerStore, type TerminalTab } from "@/stores/serverStore";
import { useLogStore } from "@/stores/logStore";
import { useConfigStore } from "@/stores/configStore";
import { ipcInvoke, formatIpcError, IpcErrorImpl } from "@/hooks/useIpc";
import { TriggerList } from "@/components/shared/TriggerList";
import { TerminalView } from "@/components/shared/TerminalView";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import {
  showContextMenu,
  type ContextMenuEntry,
} from "@/components/ui/ContextMenu";
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
  const setTerminalTabsForServer = useServerStore(
    (s) => s.setTerminalTabsForServer,
  );
  const setActiveTerminalTab = useServerStore((s) => s.setActiveTerminalTab);
  const renameTerminalTab = useServerStore((s) => s.renameTerminalTab);
  const setTerminalTabDisconnected = useServerStore(
    (s) => s.setTerminalTabDisconnected,
  );
  const clearTerminalTabs = useServerStore((s) => s.clearTerminalTabs);
  const terminalTabsByServer = useServerStore((s) => s.terminal_tabs_by_server);
  const activeTerminalTabByServer = useServerStore(
    (s) => s.active_terminal_tab_by_server,
  );
  const termTabs = terminalTabsByServer[selectedId || ""] || [];
  const activeTab: Tab =
    (activeTerminalTabByServer[selectedId || ""] as Tab) || "overview";
  // System proxy state is derived from the global config — this server is the
  // system proxy if config.general.system_proxy_server_id === selectedId.
  const config = useConfigStore((s) => s.config);
  const systemProxyEnabled =
    config?.general?.system_proxy_server_id === selectedId;
  const [testProxyUrl, setTestProxyUrl] = useState("");
  const [testProxyResult, setTestProxyResult] = useState<{
    success: boolean;
    exit_ip: string | null;
    latency_ms: number;
    error?: string;
  } | null>(null);
  const [testingProxy, setTestingProxy] = useState(false);
  const testProxyAbort = useRef<AbortController | null>(null);
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

  // Reset transient UI state when switching to a different server
  // (the component is reused across servers, so local state persists otherwise)
  useEffect(() => {
    setTestProxyResult(null);
    setTestingProxy(false);
    setShowDisconnectConfirm(false);
    setRenamingTabId(null);
    setDraggedTabId(null);
    setDragOverTabId(null);
  }, [selectedId]);

  const server = servers.find((s) => s.id === selectedId);
  const isConnected = server?.current_status === "connected";
  // "connecting" is derived from the server's current status in the store,
  // so it survives switching to another server and back.
  const connecting =
    server?.current_status === "connecting" ||
    server?.current_status === "reconnecting";

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
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  // Open a new terminal session and add a tab for it.
  // Flow: click → connecting → SSH connect + terminal open → tab created → connected
  const handleOpenTerminal = useCallback(async () => {
    if (!server?.id) return;
    const serverId = server.id;

    // If not connected, connect first (status stays "connecting" until terminal is ready)
    if (!isConnected) {
      updateServerStatus(serverId, "connecting");
      try {
        await ipcInvoke("ipc_connect_server", { serverId });
        // Don't set "connected" yet — wait until terminal is ready
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
            new CustomEvent("edit-server", { detail: { serverId } }),
          );
        }
        if (e instanceof IpcErrorImpl && e.code === "HostKeyMismatch") {
          // Parse "expected: SHA256:xxx, got: SHA256:yyy" from detail
          const detail = e.detail || "";
          const expectedMatch = detail.match(/expected:\s*(SHA256:\S+)/);
          const actualMatch = detail.match(/got:\s*(SHA256:\S+)/);
          window.dispatchEvent(
            new CustomEvent("hostkey-mismatch", {
              detail: {
                serverId,
                serverName: server?.name || serverId,
                expected: expectedMatch?.[1] || "unknown",
                actual: actualMatch?.[1] || "unknown",
              },
            }),
          );
        }
        return;
      }
    }
    // SSH connected — now open terminal session
    try {
      const result = await ipcInvoke<{
        session_id: string;
        initial_output: string;
      }>("ipc_terminal_open", { server_id: serverId, cols: 80, rows: 24 });
      const sessionId = result.session_id;
      const initialOutput = result.initial_output || "";
      const tabId: Tab = `term:${sessionId}`;
      const currentTabs =
        useServerStore.getState().terminal_tabs_by_server[serverId] || [];
      const defaultLabel = `${t("server.terminal")} ${currentTabs.length + 1}`;
      // Terminal ready — create tab and switch to it first
      addTerminalTab(serverId, {
        id: tabId,
        sessionId,
        label: defaultLabel,
        defaultLabel,
        initialOutput,
        disconnected: false,
      });
      setActiveTerminalTab(serverId, tabId);
      // Wait for the tab to render before changing button status
      requestAnimationFrame(() => {
        updateServerStatus(
          serverId,
          "connected",
          server.last_known_ip || undefined,
        );
      });
    } catch (e) {
      const msg = formatIpcError(e);
      updateServerStatus(serverId, "offline");
      toast.error(t("server.terminal_open_failed"), { description: msg });
    }
  }, [
    server?.id,
    server?.last_known_ip,
    isConnected,
    t,
    addTerminalTab,
    setActiveTerminalTab,
    updateServerStatus,
  ]);

  // Open a terminal from the context menu. Uses the same logic as the login button.
  const openTerminalFromMenu = useCallback(async () => {
    const store = useServerStore.getState();
    const serverId = store.selected_server_id;
    if (!serverId) return;
    const currentServer = store.servers.find((s) => s.id === serverId);
    if (!currentServer) return;
    const alreadyConnected = currentServer.current_status === "connected";

    // If not connected, connect first (status stays "connecting" until terminal is ready)
    if (!alreadyConnected) {
      store.updateServerStatus(serverId, "connecting");
      try {
        await ipcInvoke("ipc_connect_server", { serverId });
        // Don't set "connected" yet — wait until terminal is ready
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
            new CustomEvent("edit-server", { detail: { serverId } }),
          );
        }
        return;
      }
    }

    // SSH connected — now open terminal session
    try {
      const result = await ipcInvoke<{
        session_id: string;
        initial_output: string;
      }>("ipc_terminal_open", { server_id: serverId, cols: 80, rows: 24 });
      const sessionId = result.session_id;
      const initialOutput = result.initial_output || "";
      const tabId: Tab = `term:${sessionId}`;
      const currentTabs = store.terminal_tabs_by_server[serverId] || [];
      const defaultLabel = `${t("server.terminal")} ${currentTabs.length + 1}`;
      // Terminal ready — create tab and switch to it first
      store.addTerminalTab(serverId, {
        id: tabId,
        sessionId,
        label: defaultLabel,
        defaultLabel,
        initialOutput,
        disconnected: false,
      });
      store.setActiveTerminalTab(serverId, tabId);
      // Wait for the tab to render before changing button status
      requestAnimationFrame(() => {
        store.updateServerStatus(
          serverId,
          "connected",
          currentServer.last_known_ip || undefined,
        );
      });
    } catch (e) {
      const msg = formatIpcError(e);
      store.updateServerStatus(serverId, "offline");
      toast.error(t("server.terminal_open_failed"), { description: msg });
    }
  }, [t]);

  // After closing terminal tabs, check if the server has no remaining tabs
  // and no running proxy. If so, disconnect the SSH connection to avoid
  // leaving an idle connection to the server.
  const maybeDisconnectIfIdle = useCallback(
    (serverId: string, remainingTabCount: number) => {
      if (remainingTabCount > 0) return;
      const srv = useServerStore
        .getState()
        .servers.find((s) => s.id === serverId);
      if (!srv) return;
      if (srv.proxy_running) return;
      if (srv.current_status !== "connected") return;
      ipcInvoke("ipc_disconnect_server", { serverId }).catch(() => {});
      updateServerStatus(serverId, "disconnected");
    },
    [updateServerStatus],
  );

  const handleCloseTerminal = useCallback(
    (tabId: string, e: React.MouseEvent) => {
      e.stopPropagation();
      const serverId = selectedId || "";
      const currentTabs = terminalTabsByServer[serverId] || [];
      const tab = currentTabs.find((tt) => tt.id === tabId);
      if (tab) {
        if (tab.sessionId)
          ipcInvoke("ipc_terminal_close", { session_id: tab.sessionId }).catch(
            () => {},
          );
      }
      removeTerminalTab(serverId, tabId);
      maybeDisconnectIfIdle(serverId, currentTabs.length - 1);
    },
    [
      selectedId,
      terminalTabsByServer,
      removeTerminalTab,
      maybeDisconnectIfIdle,
    ],
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
    updateServerStatus(server.id, "connecting");
    try {
      await ipcInvoke("ipc_connect_server", { serverId: server.id });
      updateServerStatus(
        server.id,
        "connected",
        server.last_known_ip || undefined,
      );
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
          new CustomEvent("edit-server", { detail: { serverId: server.id } }),
        );
      }
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
        if (tt.sessionId)
          ipcInvoke("ipc_terminal_close", { session_id: tt.sessionId }).catch(
            () => {},
          );
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
      if (tab.sessionId)
        ipcInvoke("ipc_terminal_close", { session_id: tab.sessionId }).catch(
          () => {},
        );
    }
    removeTerminalTab(serverId, tabId);
    maybeDisconnectIfIdle(serverId, currentTabs.length - 1);
  };

  // Close all disconnected tabs
  const closeDisconnectedTabs = () => {
    const serverId = selectedId || "";
    const currentTabs = terminalTabsByServer[serverId] || [];
    for (const tt of currentTabs) {
      if (tt.disconnected) {
        if (tt.sessionId)
          ipcInvoke("ipc_terminal_close", { session_id: tt.sessionId }).catch(
            () => {},
          );
      }
    }
    const remaining = currentTabs.filter((tt) => !tt.disconnected);
    setTerminalTabsForServer(serverId, remaining);
    maybeDisconnectIfIdle(serverId, remaining.length);
  };

  // Close all tabs except the given one
  const closeOtherTabs = (keepTabId: string) => {
    const serverId = selectedId || "";
    const currentTabs = terminalTabsByServer[serverId] || [];
    for (const tt of currentTabs) {
      if (tt.id !== keepTabId) {
        if (tt.sessionId)
          ipcInvoke("ipc_terminal_close", { session_id: tt.sessionId }).catch(
            () => {},
          );
      }
    }
    const remaining = currentTabs.filter((tt) => tt.id === keepTabId);
    setTerminalTabsForServer(serverId, remaining);
    if (activeTerminalTabByServer[serverId] !== keepTabId) {
      setActiveTerminalTab(serverId, "overview");
    }
    // remaining.length is 1 (the kept tab), so no disconnect needed
  };

  // Close all terminal tabs
  const closeAllTabs = () => {
    const serverId = selectedId || "";
    const currentTabs = terminalTabsByServer[serverId] || [];
    for (const tt of currentTabs) {
      if (tt.sessionId)
        ipcInvoke("ipc_terminal_close", { session_id: tt.sessionId }).catch(
          () => {},
        );
    }
    setTerminalTabsForServer(serverId, []);
    setActiveTerminalTab(serverId, "overview");
    maybeDisconnectIfIdle(serverId, 0);
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
    const proxyPort =
      server.proxy.mixed_port > 0
        ? server.proxy.mixed_port
        : server.proxy.socks5_port;
    const items: ContextMenuEntry[] = [
      ...(isConnected
        ? [
            {
              label: t("tab.disconnect"),
              onClick: () => handleDisconnect(),
              danger: true,
            } as ContextMenuEntry,
          ]
        : [
            {
              label: t("tab.connect"),
              onClick: () => handleConnect(),
            } as ContextMenuEntry,
          ]),
      { label: t("tab.login_server"), onClick: () => openTerminalFromMenu() },
      { separator: true },
      {
        label: t("tab.close_disconnected_terminals"),
        onClick: () => closeDisconnectedTabs(),
        disabled: !hasDisconnected,
      },
      {
        label: t("tab.close_all_terminals"),
        onClick: () => closeAllTabs(),
        disabled: currentTabs.length === 0,
      },
      { separator: true },
      ...(server.proxy_running
        ? [
            {
              label: t("tab.stop_proxy", { port: proxyPort }),
              onClick: () => handleToggleProxy(),
            } as ContextMenuEntry,
            ...(systemProxyEnabled
              ? [
                  {
                    label: t("tab.unset_system_proxy"),
                    onClick: () => handleClearSystemProxy(),
                  } as ContextMenuEntry,
                ]
              : [
                  {
                    label: t("tab.set_system_proxy"),
                    onClick: () => handleSetSystemProxy(),
                  } as ContextMenuEntry,
                ]),
          ]
        : [
            {
              label: t("tab.start_proxy", { port: proxyPort }),
              onClick: () => handleToggleProxy(),
            } as ContextMenuEntry,
          ]),
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
      {
        label: t("tab.rename"),
        onClick: () => handleRenameTab(tabId, tab.label),
      },
      {
        label: t("tab.restore_default_name"),
        onClick: () => restoreDefaultName(tabId),
        disabled: tab.label === tab.defaultLabel,
      },
      {
        label: t("tab.reconnect"),
        onClick: () => handleConnect(),
        disabled: isConnected,
      },
      {
        label: t("tab.disconnect"),
        onClick: () => handleDisconnect(),
        disabled: !isConnected,
        danger: true,
      },
      { separator: true },
      { label: t("tab.close_session"), onClick: () => closeTab(tabId) },
      {
        label: t("tab.close_disconnected_sessions"),
        onClick: () => closeDisconnectedTabs(),
        disabled: !hasDisconnected,
      },
      {
        label: t("tab.close_other_sessions"),
        onClick: () => closeOtherTabs(tabId),
        disabled: currentTabs.length <= 1,
      },
      {
        label: t("tab.close_all_sessions"),
        onClick: () => closeAllTabs(),
        disabled: currentTabs.length === 0,
      },
      { separator: true },
      {
        label: t("tab.new_clone_session"),
        onClick: () => openTerminalFromMenu(),
      },
    ];
    showContextMenu(e, items);
  };

  const handleToggleProxy = async () => {
    if (!server.id) return;
    const newEnabled = !server.proxy_running;

    // If starting proxy and not connected, auto-connect first
    if (newEnabled && !isConnected) {
      updateServerStatus(server.id, "connecting");
      try {
        await ipcInvoke("ipc_connect_server", { serverId: server.id });
        updateServerStatus(
          server.id,
          "connected",
          server.last_known_ip || undefined,
        );
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
            new CustomEvent("edit-server", { detail: { serverId: server.id } }),
          );
        }
        return;
      }
    }

    try {
      await ipcInvoke("ipc_toggle_proxy", {
        serverId: server.id,
        enabled: newEnabled,
      });
      setProxyStatus(server.id, newEnabled);

      // When stopping proxy and no terminal tabs are open, also disconnect
      // the SSH connection to avoid leaving an idle connection to the server.
      if (!newEnabled && termTabs.length === 0 && isConnected) {
        try {
          await ipcInvoke("ipc_disconnect_server", { serverId: server.id });
          updateServerStatus(server.id, "disconnected");
        } catch (e) {
          // Disconnect failure is non-fatal — proxy was already stopped
          console.error("disconnect after proxy stop failed:", e);
        }
      }
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

  const handleUpdateProxy = async (patch: {
    socks5_port?: number;
    http_port?: number;
    mixed_port?: number;
  }) => {
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
            : srv,
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
      useConfigStore
        .getState()
        .updateGeneral({ system_proxy_server_id: server.id });
      toast.success(t("server.set_system_proxy"));
    } catch (e) {
      const msg = formatIpcError(e);
      if (e instanceof IpcErrorImpl && e.code === "NeedsPrivilege") {
        toast.error(t("server.set_system_proxy_failed"), {
          description: msg,
          duration: 20000,
        });
      } else {
        toast.error(t("server.set_system_proxy_failed"), { description: msg });
      }
    }
  };

  const handleClearSystemProxy = async () => {
    try {
      await ipcInvoke("ipc_clear_system_proxy", {});
      useConfigStore.getState().updateGeneral({ system_proxy_server_id: null });
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
            reject(new Error(t("server.test_proxy_cancelled"))),
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
    ...termTabs.map((tt) => ({
      key: tt.id as Tab,
      label: tt.label,
      disconnected: tt.disconnected,
    })),
  ];

  const statusColor = isConnected
    ? "text-[#34C759]"
    : server.current_status === "auth_failed" ||
        server.current_status === "offline"
      ? "text-[#FF3B30]"
      : "text-gray-400";

  // When a terminal tab is active, remove all padding so the terminal fills
  // the panel edge-to-edge. When overview is active, keep the padded layout.
  const isTerminalActive = activeTab !== "overview";

  return (
    <div className="flex-1 flex flex-col h-full overflow-hidden bg-white dark:bg-[#1E1E1E]">
      {/* Tab bar — overview + terminal tabs (inverted top tabs) */}
      <div
        className="flex items-end gap-0 px-3 bg-white dark:bg-[#1E1E1E] border-b border-gray-200/80 dark:border-white/[0.06] flex-shrink-0 overflow-x-auto overflow-y-hidden scrollbar-hide"
        onWheel={(e) => {
          e.currentTarget.scrollLeft += e.deltaY;
          e.preventDefault();
        }}
      >
        {tabs.map((tab) => {
          const isOverview = tab.key === "overview";
          const isDraggable = !isOverview;
          const isActive = activeTab === tab.key;
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
                if (!isDraggable || !draggedTabId || draggedTabId === tab.key)
                  return;
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
              className={`flex items-center gap-1.5 px-4 py-2 text-sm font-medium transition-colors cursor-pointer rounded-b-lg flex-shrink-0 bg-white dark:bg-[#1E1E1E] border border-gray-200/80 dark:border-white/[0.06] border-t-0 ${
                isActive
                  ? "text-[#007AFF] dark:text-[#0A84FF] shadow-[0_3px_12px_rgba(0,0,0,0.1)] dark:shadow-[0_3px_12px_rgba(0,0,0,0.5)] z-10"
                  : "text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200"
              } ${tab.disconnected && !isOverview ? "opacity-50 italic" : ""} ${
                isDraggable ? "select-none" : ""
              } ${dragOverTabId === tab.key && draggedTabId && draggedTabId !== tab.key ? "ring-1 ring-[#007AFF]" : ""} ${
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
              title={
                tab.key !== "overview"
                  ? t("server.double_click_to_rename")
                  : undefined
              }
            >
              {renamingTabId === tab.key ? (
                <input
                  className="text-sm bg-transparent border-b border-[#007AFF] outline-none text-[#007AFF] dark:text-[#0A84FF] min-w-0 w-24"
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

      {/* Content area below the tab bar */}
      <div className="flex-1 flex flex-col overflow-hidden relative">
        {/* Disconnect confirmation — shown when disconnecting with active terminals */}
        {showDisconnectConfirm && (
          <ConfirmDialog
            level="low"
            title={t("server.disconnect")}
            message={t("server.disconnect_with_terminals_confirm", {
              count: termTabs.length,
            })}
            confirmLabel={t("server.disconnect")}
            onConfirm={() => {
              setShowDisconnectConfirm(false);
              doDisconnect();
            }}
            onCancel={() => setShowDisconnectConfirm(false)}
          />
        )}

        {activeTab === "overview" && (
          <div className="flex-1 overflow-y-auto p-6">
            <div className="space-y-6 max-w-6xl min-h-full pb-6">
              {/* Primary action cards */}
              <div className="grid grid-cols-1 md:grid-cols-2 gap-5">
                {/* Connection card — macOS Settings style grouped list */}
                <div className="bg-[#FBFBFB] dark:bg-[#1E1E1E] rounded-[16px] overflow-hidden border border-gray-200/80 dark:border-white/[0.06]">
                  {/* Header row with name + primary action */}
                  <div className="flex items-center justify-between px-4 py-4 border-b border-gray-100 dark:border-white/[0.06]">
                    <div className="min-w-0 flex items-center gap-3">
                      <div className="w-11 h-11 rounded-[13px] bg-gradient-to-br from-[#007AFF]/15 to-[#007AFF]/5 flex items-center justify-center text-[#007AFF] font-semibold text-lg shadow-sm">
                        {server.name.charAt(0).toUpperCase()}
                      </div>
                      <div>
                        <div className="text-base font-semibold text-gray-900 dark:text-gray-100 truncate">
                          {server.name}
                        </div>
                        <div className={`text-xs ${statusColor} font-medium`}>
                          {t(`server.status.${server.current_status}`)}
                        </div>
                      </div>
                    </div>
                    <div className="flex items-center gap-2 flex-shrink-0">
                      {isConnected && (
                        <button
                          className="px-3.5 py-1.5 text-sm rounded-lg bg-gray-100 dark:bg-[#2C2C2E] text-gray-700 dark:text-gray-200 hover:bg-gray-200 dark:hover:bg-[#3A3A3C] font-medium transition-colors"
                          onClick={handleDisconnect}
                        >
                          {t("server.disconnect")}
                        </button>
                      )}
                      <button
                        className="px-4 py-1.5 text-sm rounded-lg bg-[#34C759] text-white hover:bg-[#2EB34F] disabled:opacity-50 font-medium transition-colors "
                        onClick={handleOpenTerminal}
                        disabled={connecting}
                      >
                        {connecting
                          ? t("server.status.connecting")
                          : termTabs.length === 0
                            ? t("server.connect_terminal")
                            : t("server.login_server")}
                      </button>
                    </div>
                  </div>
                  <div className="divide-y divide-gray-100 dark:divide-white/[0.06]">
                    <div className="flex items-center justify-between px-4 py-3">
                      <span className="text-sm text-gray-500">
                        {t("server.host")}
                      </span>
                      <span className="font-mono text-sm text-[#1D1D1F] dark:text-gray-100 truncate pl-4">
                        {server.ssh?.host || "?"}:{server.ssh?.port || "?"}
                      </span>
                    </div>
                    <div className="flex items-center justify-between px-4 py-3">
                      <span className="text-sm text-gray-500">
                        {t("server.ip_label")}
                      </span>
                      <span className="font-mono text-sm text-[#1D1D1F] dark:text-gray-100 truncate pl-4">
                        {server.client_ip || "—"}
                      </span>
                    </div>
                    <div className="flex items-center justify-between px-4 py-3">
                      <span className="text-sm text-gray-500">
                        {t("server.auth_method")}
                      </span>
                      <span className="text-sm text-[#1D1D1F] dark:text-gray-100">
                        {server.ssh?.auth_method === "key"
                          ? t("server.ssh_key")
                          : t("server.password")}
                      </span>
                    </div>
                    <div className="flex items-center justify-between px-4 py-3">
                      <span className="text-sm text-gray-500">
                        {t("server.auto_reconnect")}
                      </span>
                      <div className="flex items-center gap-3">
                        <Toggle
                          checked={server.reconnect?.auto_reconnect ?? true}
                          onChange={(v) => {
                            ipcInvoke("ipc_update_server", {
                              server_id: server.id,
                              auto_reconnect: v,
                            }).catch(() => {});
                            useServerStore.setState((s) => ({
                              servers: s.servers.map((srv) =>
                                srv.id === server.id
                                  ? {
                                      ...srv,
                                      reconnect: {
                                        ...srv.reconnect,
                                        auto_reconnect: v,
                                      },
                                    }
                                  : srv,
                              ),
                            }));
                          }}
                        />
                        {(server.reconnect?.auto_reconnect ?? true) && (
                          <select
                            className="text-xs bg-[#F2F2F7]/80 dark:bg-[#2C2C2E]/80 border border-gray-200/80 dark:border-white/[0.08] rounded-lg px-2 py-1 text-[#1D1D1F] dark:text-gray-100 focus:outline-none focus:ring-1 focus:ring-[#007AFF]"
                            value={(() => {
                              const secs =
                                server.reconnect?.reconnect_timeout_secs ??
                                86400;
                              if (secs === 0) return "0";
                              if (secs < 60) return `${secs}s`;
                              if (secs < 3600)
                                return `${Math.floor(secs / 60)}m`;
                              if (secs < 86400)
                                return `${Math.floor(secs / 3600)}h`;
                              return `${Math.floor(secs / 86400)}d`;
                            })()}
                            onChange={(e) => {
                              const val = e.target.value;
                              let secs = 0;
                              if (val !== "0") {
                                const num = parseInt(val);
                                const unit = val.slice(-1);
                                secs =
                                  unit === "s"
                                    ? num
                                    : unit === "m"
                                      ? num * 60
                                      : unit === "h"
                                        ? num * 3600
                                        : num * 86400;
                                secs = Math.max(3, Math.min(259200, secs));
                              }
                              ipcInvoke("ipc_update_server", {
                                server_id: server.id,
                                reconnect_timeout_secs: secs,
                              }).catch(() => {});
                              useServerStore.setState((s) => ({
                                servers: s.servers.map((srv) =>
                                  srv.id === server.id
                                    ? {
                                        ...srv,
                                        reconnect: {
                                          ...srv.reconnect,
                                          reconnect_timeout_secs: secs,
                                        },
                                      }
                                    : srv,
                                ),
                              }));
                            }}
                          >
                            <option value="3s">3s</option>
                            <option value="10s">10s</option>
                            <option value="30s">30s</option>
                            <option value="1m">1m</option>
                            <option value="5m">5m</option>
                            <option value="15m">15m</option>
                            <option value="30m">30m</option>
                            <option value="1h">1h</option>
                            <option value="6h">6h</option>
                            <option value="12h">12h</option>
                            <option value="1d">1d</option>
                            <option value="2d">2d</option>
                            <option value="3d">3d</option>
                          </select>
                        )}
                      </div>
                    </div>
                  </div>

                  {server.auth_banner && (
                    <div className="border-t border-gray-100 dark:border-white/[0.06] px-4 py-3">
                      <div className="text-xs text-gray-500 mb-1.5">
                        {t("server.welcome_message")}
                      </div>
                      <pre className="font-mono text-xs text-gray-700 dark:text-gray-300 bg-gray-50/80 dark:bg-black/20 rounded-lg p-3 overflow-x-auto whitespace-pre-wrap">
                        {server.auth_banner}
                      </pre>
                    </div>
                  )}
                </div>

                {/* Proxy card — macOS Settings style grouped list */}
                <div className="bg-[#FBFBFB] dark:bg-[#1E1E1E] rounded-[16px] overflow-hidden border border-gray-200/80 dark:border-white/[0.06] flex flex-col">
                  {/* Header */}
                  <div className="flex items-center justify-between px-4 py-4 border-b border-gray-100 dark:border-white/[0.06]">
                    <div>
                      <div className="text-xs text-gray-500">
                        {t("server.proxy")}
                      </div>
                      <div
                        className={`text-base font-semibold mt-0.5 ${server.proxy_running ? "text-[#34C759]" : "text-gray-400"}`}
                      >
                        {!isConnected
                          ? t("proxy.not_connected")
                          : server.proxy_running
                            ? t("proxy.started")
                            : t("proxy.off")}
                      </div>
                    </div>
                    <button
                      className={`px-4 py-1.5 text-sm rounded-lg font-medium transition-colors ${
                        server.proxy_running
                          ? "bg-[#34C759] text-white hover:bg-[#2EB34F] "
                          : "bg-gray-100 dark:bg-[#2C2C2E] text-gray-700 dark:text-gray-200 hover:bg-gray-200 dark:hover:bg-[#3A3A3C]"
                      }`}
                      onClick={handleToggleProxy}
                      disabled={connecting && !server.proxy_running}
                    >
                      {server.proxy_running
                        ? t("server.stop_proxy")
                        : t("server.start_proxy")}
                    </button>
                  </div>

                  {/* Port configuration rows */}
                  <div className="divide-y divide-gray-100 dark:divide-white/[0.06]">
                    {server.proxy.mixed_port > 0 ? (
                      <div className="flex items-center justify-between px-4 py-3">
                        <span className="text-sm text-gray-500">Mixed</span>
                        {server.proxy_running ? (
                          <span className="text-sm font-mono text-[#1D1D1F] dark:text-gray-100">
                            {server.proxy.mixed_port}
                          </span>
                        ) : (
                          <input
                            type="number"
                            className="w-20 px-2 py-1 text-sm font-mono border border-gray-200/80 dark:border-white/[0.08] rounded-lg bg-[#FBFBFB] dark:bg-[#2C2C2E] text-[#1D1D1F] dark:text-gray-100 focus:outline-none focus:border-[#007AFF]"
                            value={server.proxy.mixed_port}
                            onChange={(e) =>
                              handleUpdateProxy({
                                mixed_port: parseInt(e.target.value) || 0,
                              })
                            }
                            disabled={server.proxy_running}
                          />
                        )}
                      </div>
                    ) : (
                      <>
                        <div className="grid grid-cols-2 divide-x divide-gray-100 dark:divide-white/[0.06]">
                          <div className="flex items-center justify-between px-4 py-3">
                            <span className="text-sm text-gray-500">
                              SOCKS5
                            </span>
                            {server.proxy_running ? (
                              <span className="text-sm font-mono text-[#1D1D1F] dark:text-gray-100">
                                {server.proxy.socks5_port}
                              </span>
                            ) : (
                              <input
                                type="number"
                                className="w-20 px-2 py-1 text-sm font-mono border border-gray-200/80 dark:border-white/[0.08] rounded-lg bg-[#FBFBFB] dark:bg-[#2C2C2E] text-[#1D1D1F] dark:text-gray-100 focus:outline-none focus:border-[#007AFF]"
                                value={server.proxy.socks5_port}
                                onChange={(e) =>
                                  handleUpdateProxy({
                                    socks5_port:
                                      parseInt(e.target.value) || 1080,
                                  })
                                }
                                disabled={server.proxy_running}
                              />
                            )}
                          </div>
                          <div className="flex items-center justify-between px-4 py-3">
                            <span className="text-sm text-gray-500">HTTP</span>
                            {server.proxy_running ? (
                              <span className="text-sm font-mono text-[#1D1D1F] dark:text-gray-100">
                                {server.proxy.http_port}
                              </span>
                            ) : (
                              <input
                                type="number"
                                className="w-20 px-2 py-1 text-sm font-mono border border-gray-200/80 dark:border-white/[0.08] rounded-lg bg-[#FBFBFB] dark:bg-[#2C2C2E] text-[#1D1D1F] dark:text-gray-100 focus:outline-none focus:border-[#007AFF]"
                                value={server.proxy.http_port}
                                onChange={(e) =>
                                  handleUpdateProxy({
                                    http_port: parseInt(e.target.value) || 8080,
                                  })
                                }
                                disabled={server.proxy_running}
                              />
                            )}
                          </div>
                        </div>
                      </>
                    )}
                    <div
                      className={`grid ${server.proxy_running ? "grid-cols-1" : "grid-cols-2"} divide-x divide-gray-100 dark:divide-white/[0.06]`}
                    >
                      {!server.proxy_running && (
                        <div className="flex items-center justify-between px-4 py-3">
                          <span className="text-sm text-gray-500">
                            {t("server.mixed_port")}
                          </span>
                          <Toggle
                            checked={server.proxy.mixed_port > 0}
                            onChange={(v) =>
                              handleUpdateProxy({
                                mixed_port: v
                                  ? server.proxy.socks5_port || 1080
                                  : 0,
                              })
                            }
                          />
                        </div>
                      )}
                      <div
                        className={`flex items-center justify-between px-4 py-3 ${!server.proxy_running ? "opacity-50 pointer-events-none" : ""}`}
                      >
                        <span className="text-sm text-gray-500">
                          {t("server.system_proxy")}
                        </span>
                        <Toggle
                          checked={systemProxyEnabled}
                          onChange={(v) =>
                            v
                              ? handleSetSystemProxy()
                              : handleClearSystemProxy()
                          }
                        />
                      </div>
                    </div>
                  </div>

                  {/* Active clients indicator */}
                  {server.proxy_running && server.active_channels > 0 && (
                    <div className="text-xs text-[#34C759] font-medium px-4 py-2 border-t border-gray-100 dark:border-white/[0.06]">
                      {server.active_channels} {t("server.active_clients")}
                    </div>
                  )}

                  {/* Test proxy section */}
                  <div className="border-t border-gray-100 dark:border-white/[0.06] px-4 py-3 mt-auto">
                    <div className="flex items-center gap-2">
                      <input
                        type="text"
                        className="flex-1 px-3 py-2 text-sm border border-gray-200/80 dark:border-white/[0.08] rounded-lg bg-[#FBFBFB] dark:bg-[#2C2C2E] focus:outline-none focus:border-[#007AFF]"
                        placeholder={t("server.test_proxy_url_placeholder")}
                        value={testProxyUrl}
                        onChange={(e) => setTestProxyUrl(e.target.value)}
                        disabled={!server.proxy_running}
                      />
                      <button
                        className="px-4 py-2 text-sm rounded-lg bg-[#007AFF] text-white hover:bg-[#0063D1] disabled:opacity-50 transition-colors"
                        onClick={handleTestProxy}
                        disabled={!server.proxy_running || testingProxy}
                      >
                        {testingProxy
                          ? t("common.testing")
                          : t("server.test_proxy_btn")}
                      </button>
                      {testingProxy && (
                        <button
                          className="px-4 py-2 text-sm rounded-lg bg-gray-100 dark:bg-[#2C2C2E] hover:bg-gray-200 dark:hover:bg-[#3A3A3C] transition-colors"
                          onClick={handleCancelTestProxy}
                        >
                          {t("common.cancel")}
                        </button>
                      )}
                    </div>
                    {testProxyResult && (
                      <div
                        className={`mt-3 p-3 rounded-lg text-sm ${
                          testProxyResult.success
                            ? "bg-[#34C759]/10 text-[#2EB34F] dark:text-[#5FE07A]"
                            : "bg-red-50 dark:bg-red-900/20 text-red-600 dark:text-red-400"
                        }`}
                      >
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
                            {testProxyResult.error
                              ? `: ${testProxyResult.error}`
                              : ""}
                          </span>
                        )}
                      </div>
                    )}
                  </div>
                </div>
              </div>

              {/* Triggers panel — full width */}
              <div className="bg-[#FBFBFB] dark:bg-[#1E1E1E] rounded-[16px] border border-gray-200/80 dark:border-white/[0.06] overflow-hidden">
                <div className="px-4 py-3 border-b border-gray-100 dark:border-white/[0.06] flex items-center justify-between">
                  <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100">
                    {t("trigger.title")}
                  </h3>
                </div>
                <div className="p-4">
                  <TriggerList serverId={server.id} />
                </div>
              </div>
            </div>
          </div>
        )}

        {/* Terminal tab content — all tabs kept mounted; hidden tabs use absolute
          positioning so xterm.js containers still have proper dimensions for fit() */}
        {termTabs.map((tt) => (
          <div
            key={tt.id}
            className={
              isTerminalActive
                ? "flex-1 min-h-0 h-full"
                : "h-[calc(100vh-200px)] min-h-[400px]"
            }
            style={
              activeTab === tt.id
                ? { position: "relative", visibility: "visible" }
                : {
                    position: "absolute",
                    left: "-9999px",
                    top: 0,
                    width: "100%",
                    height: "100%",
                    visibility: "hidden",
                  }
            }
          >
            <TerminalView
              sessionId={tt.sessionId}
              serverId={server.id}
              active={activeTab === tt.id}
              initialOutput={tt.initialOutput}
            />
            {tt.disconnected && (
              <div className="absolute top-0 left-0 right-0 flex items-center justify-between bg-black/70 px-4 py-2 z-10 pointer-events-auto">
                <p className="text-gray-400 text-sm">
                  {t("server.terminal_disconnected")}
                </p>
                <button
                  className="px-5 py-2.5 text-sm rounded-lg bg-green-500 text-white hover:bg-green-600 font-medium shadow-sm transition-colors"
                  onClick={async () => {
                    if (!server.id) return;
                    const serverId = server.id;
                    // Ensure SSH is connected first
                    const currentServer = useServerStore
                      .getState()
                      .servers.find((s) => s.id === serverId);
                    if (
                      !currentServer ||
                      currentServer.current_status !== "connected"
                    ) {
                      updateServerStatus(serverId, "connecting");
                      try {
                        await ipcInvoke("ipc_connect_server", { serverId });
                        updateServerStatus(
                          serverId,
                          "connected",
                          currentServer?.last_known_ip || undefined,
                        );
                      } catch (e) {
                        const errMsg = formatIpcError(e);
                        updateServerStatus(serverId, "offline");
                        toast.error(t("server.connect_failed"), {
                          description: errMsg,
                        });
                        return;
                      }
                    }
                    // Open a new terminal session to replace the disconnected one
                    try {
                      const result = await ipcInvoke<{
                        session_id: string;
                        initial_output: string;
                      }>("ipc_terminal_open", {
                        server_id: serverId,
                        cols: 80,
                        rows: 24,
                      });
                      const newSessionId = result.session_id;
                      const newInitialOutput = result.initial_output || "";
                      const newTabId: Tab = `term:${newSessionId}`;
                      const currentTabs =
                        useServerStore.getState().terminal_tabs_by_server[
                          serverId
                        ] || [];
                      const defaultLabel =
                        tt.defaultLabel ||
                        `${t("server.terminal")} ${currentTabs.length + 1}`;
                      // Replace the disconnected tab with the new one
                      removeTerminalTab(serverId, tt.id);
                      addTerminalTab(serverId, {
                        id: newTabId,
                        sessionId: newSessionId,
                        label: defaultLabel,
                        defaultLabel,
                        initialOutput: newInitialOutput,
                        disconnected: false,
                      });
                      setActiveTerminalTab(serverId, newTabId);
                    } catch (e) {
                      const msg = formatIpcError(e);
                      toast.error(t("server.terminal_open_failed"), {
                        description: msg,
                      });
                    }
                  }}
                >
                  {t("server.reconnect_terminal")}
                </button>
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

function Toggle({
  checked,
  onChange,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors duration-200 ${
        checked ? "bg-blue-500" : "bg-gray-200 dark:bg-gray-600"
      }`}
    >
      <span
        className="inline-block h-5 w-5 rounded-full bg-white shadow-sm transition-transform duration-200"
        style={{ transform: checked ? "translateX(22px)" : "translateX(2px)" }}
      />
    </button>
  );
}
