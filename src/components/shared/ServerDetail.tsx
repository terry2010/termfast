// ServerDetail — right panel showing selected server details (§9.4)
// Shows connection controls, proxy toggle, IP, and trigger status
// Tab-based UI: Connection / Proxy / Triggers / Auth (FP-8.3)

import { useState, useRef, useCallback, useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Monitor } from "lucide-react";
import { useServerStore, type TerminalTab } from "@/stores/serverStore";
import { AgentStatusDot } from "@/components/shared/AgentStatusDot";
import type { AgentStatus } from "@/hooks/agentStateMachine";
import { useTriggerStore } from "@/stores/triggerStore";
import type { ServerState } from "@/stores/serverStore";
import { useLogStore } from "@/stores/logStore";
import { useConfigStore } from "@/stores/configStore";
import { ipcInvoke, formatIpcError, IpcErrorImpl } from "@/hooks/useIpc";
import { TriggerList } from "@/components/shared/TriggerList";
import { TabTriggerManager } from "@/components/shared/TabTriggerManager";
import { PortForwardPanel, PortForwardPanelHandle } from "@/components/shared/PortForwardPanel";
import { TerminalView } from "@/components/shared/TerminalView";
import { openTerminalWithChannel, attachTerminalChannel } from "@/lib/terminal";
import { Channel } from "@tauri-apps/api/core";
import { dispatchTerminalOutput } from "@/components/shared/TerminalView";
import { TmuxSessionPicker } from "@/components/shared/TmuxSessionPicker";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { PairingCard } from "@/components/shared/PairingCard";
import { useRemoteDesktopStore } from "@/stores/remoteDesktopStore";
import { RemoteTerminalView } from "@/components/remote-desktop/RemoteTerminalView";
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
  const isRemoteSelected = selectedId?.startsWith("remote:") ?? false;
  // Remote desktop terminal list (from LIST_RESPONSE frames)
  const [remoteTerminals, setRemoteTerminals] = useState<{ terminal_id: number; name: string }[]>([]);
  const [remoteActiveTerminal, setRemoteActiveTerminal] = useState<number | null>(null);
  // Grace period ref: after OK frame sets activeTerminal, don't let LIST_RESPONSE reset it
  // for 3 seconds (the terminal might not appear in the list immediately)
  const okGraceUntilRef = useRef<number>(0);
  const activeTab: Tab = isRemoteSelected
    ? (remoteActiveTerminal !== null ? `remote_term:${remoteActiveTerminal}` as Tab : "overview")
    : (activeTerminalTabByServer[selectedId || ""] as Tab) || "overview";
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
  const portForwardRef = useRef<PortForwardPanelHandle>(null);
  const [pfRules, setPfRules] = useState<{ running: boolean; enabled: boolean }[]>([]);
  // Tab rename state: which tab id is being renamed, and the current edit text
  const [renamingTabId, setRenamingTabId] = useState<string | null>(null);
  const [renameText, setRenameText] = useState("");
  // Per-tab trigger manager dialog
  const [triggerMgrTabId, setTriggerMgrTabId] = useState<string | null>(null);
  // Disconnect confirmation: shown when user clicks disconnect with active terminals
  const [showDisconnectConfirm, setShowDisconnectConfirm] = useState(false);
  const [pendingCloseTab, setPendingCloseTab] = useState<string | null>(null);
  // Remote desktop system info (from INFO_RESPONSE frame)
  const [remoteInfo, setRemoteInfo] = useState<{
    shell_name: string;
    os_name: string;
    os_version: string;
    os_arch: string;
    hostname: string;
    username: string;
    real_name: string;
    available_shells: string[];
  } | null>(null);
  const [selectedRemoteShell, setSelectedRemoteShell] = useState<string | null>(null);
  // Tmux session picker: shown when tmux_mode="ask" and sessions are available
  const [showTmuxPicker, setShowTmuxPicker] = useState(false);
  const [tmuxPickerCols, setTmuxPickerCols] = useState(80);
  const [tmuxPickerRows, setTmuxPickerRows] = useState(24);
  // Timestamp of last contextmenu event — macOS fires click BEFORE contextmenu
  // on right-click, so we can't use a simple boolean flag. Instead we record
  // when contextmenu fires, and in onClick we check if a contextmenu happened
  // very recently (within 500ms). We also set a flag on mousedown for button=2.
  const rightClickTimeRef = useRef(0);
  const rightClickButtonRef = useRef(false);
  // Ref to track remoteActiveTerminal without re-registering event listener
  const remoteActiveTerminalRef = useRef<number | null>(null);
  // Drag-to-reorder state for terminal tabs (overview tab is not draggable)
  const [draggedTabId, setDraggedTabId] = useState<string | null>(null);
  const [dragOverTabId, setDragOverTabId] = useState<string | null>(null);

  const isLocal = selectedId === "__local__";
  const isRemote = selectedId?.startsWith("remote:") ?? false;
  const remotePairingId = isRemote ? selectedId!.slice("remote:".length) : null;
  const remotePeers = useRemoteDesktopStore((s) => s.peers);
  const remotePeer = remotePairingId ? remotePeers.find((p) => p.pairingId === remotePairingId) : null;
  const remoteActiveConnection = useRemoteDesktopStore((s) => s.activeConnection);
  // For client role: connected if activeConnection matches and peer is online.
  // For server role: connected if peer is online (tunnel is active, no remote_client_state event).
  const remoteIsConnected = remotePairingId ? (!!remotePeer?.online) : false;
  const server = servers.find((s) => s.id === selectedId);

  // Reset transient UI state when switching to a different server
  // (the component is reused across servers, so local state persists otherwise)
  useEffect(() => {
    setTestProxyResult(null);
    setTestingProxy(false);
    setShowDisconnectConfirm(false);
    setRenamingTabId(null);
    setDraggedTabId(null);
    setDragOverTabId(null);
    setRemoteTerminals([]);
    setRemoteActiveTerminal(null);
    remoteActiveTerminalRef.current = null;
    setRemoteInfo(null);
    setSelectedRemoteShell(null);
  }, [selectedId]);

  // Remote desktop: load terminal list when a remote peer is selected
  useEffect(() => {
    if (!isRemote || !remotePairingId || !remotePeer) return;
    // Connect if not connected (only for client role — server role uses tunnel)
    // We try connect regardless; for server role, ipc_remote_client_connect
    // will be handled by the backend (or fail silently if tunnel is already active).
    if (!remoteIsConnected && remotePeer.pairingKeyHex) {
      ipcInvoke("ipc_remote_client_connect", {
        pairing_id: remotePairingId,
        pairing_key_hex: remotePeer.pairingKeyHex,
        pairing_jwt: remotePeer.jwt,
        relay_url: remotePeer.relayUrl,
      }).catch(() => {
        // Connection may fail for server role (no RemoteClientManager) — that's OK,
        // the tunnel is already active and frames can be sent via RemoteServer.
      });
    }
    // Request terminal list (works for both client and server roles)
    ipcInvoke("ipc_remote_client_list_terminals", {
      pairing_id: remotePairingId,
    }).catch((e: any) => {
      toast.error(`Failed to list terminals: ${e?.message || e}`);
    });
    // Request system info (works for both client and server roles)
    ipcInvoke("ipc_remote_client_get_info", {
      pairing_id: remotePairingId,
    }).catch(() => {});
  }, [isRemote, remotePairingId, remotePeer, remoteIsConnected]);

  // Remote desktop: listen for LIST_RESPONSE, INFO_RESPONSE, OK frames
  useEffect(() => {
    if (!isRemote || !remotePairingId) return;
    const unlisten = listen<{
      pairing_id: string;
      frame_type: number;
      terminal_id: number;
      data: string;
    }>("remote_client_frame", (event) => {
      const payload = event.payload;
      if (payload.pairing_id !== remotePairingId) return;
      if (payload.frame_type === 0x02) {
        // LIST_RESPONSE — payload is JSON array of terminals
        try {
          const parsed = JSON.parse(atob(payload.data));
          const terms = (Array.isArray(parsed) ? parsed : parsed.terminals || []).map((t: any) => ({
            terminal_id: t.id ?? t.terminal_id,
            name: t.name || `Terminal #${t.id ?? t.terminal_id}`,
          }));
          // Auto-select new terminals that weren't in the list before
          setRemoteTerminals((prev) => {
            const prevIds = new Set(prev.map((t) => t.terminal_id));
            const newTerms = terms.filter((t: any) => !prevIds.has(t.terminal_id));
            // Auto-switch to the first new terminal (like "My Computer" auto-selects new tabs)
            if (newTerms.length > 0 && remoteActiveTerminalRef.current === null) {
              const firstNew = newTerms[0];
              remoteActiveTerminalRef.current = firstNew.terminal_id;
              setRemoteActiveTerminal(firstNew.terminal_id);
              ipcInvoke("ipc_remote_client_subscribe", {
                pairing_id: remotePairingId!,
                terminal_id: firstNew.terminal_id,
              }).catch(() => {});
            }
            return terms;
          });
          // If the currently active terminal is no longer in the list, switch to overview.
          // Only do this if the list is non-empty AND we're not in the OK grace period
          // (the OK frame may have set activeTerminal before the terminal appears in the list).
          if (
            terms.length > 0 &&
            remoteActiveTerminalRef.current !== null &&
            !terms.some((t: any) => t.terminal_id === remoteActiveTerminalRef.current) &&
            Date.now() > okGraceUntilRef.current
          ) {
            remoteActiveTerminalRef.current = null;
            setRemoteActiveTerminal(null);
          }
        } catch {
          // ignore parse errors
        }
      } else if (payload.frame_type === 0x0C) {
        // NOTIFY — could be LIST_CHANGED or other notification
        try {
          const info = JSON.parse(atob(payload.data));
          if (info.type === "list_changed") {
            // Terminal list changed on the remote desktop — re-request list
            ipcInvoke("ipc_remote_client_list_terminals", {
              pairing_id: remotePairingId,
            }).catch(() => {});
          }
        } catch {
          // ignore parse errors
        }
      } else if (payload.frame_type === 0x13) {
        // INFO_RESPONSE — payload is JSON with system info
        try {
          const info = JSON.parse(atob(payload.data));
          setRemoteInfo(info);
        } catch {
          // ignore parse errors
        }
      } else if (payload.frame_type === 0x0D) {
        // OK frame — could be response to NEW_TERMINAL (with payload) or CLOSE_TERMINAL
        if (payload.data) {
          try {
            const parsed = JSON.parse(atob(payload.data));
            if (parsed.terminal_id !== undefined) {
              // NEW_TERMINAL response
              const newTermId = parsed.terminal_id;
              // Add to remoteTerminals immediately so the tab appears
              setRemoteTerminals((prev) => {
                if (prev.some((t) => t.terminal_id === newTermId)) return prev;
                return [...prev, { terminal_id: newTermId, name: `Terminal ${prev.length + 1}` }];
              });
              // Auto-subscribe and switch to the new terminal
              remoteActiveTerminalRef.current = newTermId;
              setRemoteActiveTerminal(newTermId);
              // Set grace period: don't let LIST_RESPONSE reset activeTerminal for 3s
              okGraceUntilRef.current = Date.now() + 3000;
              ipcInvoke("ipc_remote_client_subscribe", {
                pairing_id: remotePairingId,
                terminal_id: newTermId,
              }).catch((e: any) => {
                toast.error(`Subscribe failed: ${e?.message || e}`);
              });
              // Refresh terminal list after a short delay to get accurate names.
              // The delay ensures the new terminal has registered on the remote side
              // before we request the list, avoiding a stale list that doesn't include
              // the new terminal (which would trigger an unwanted reset of activeTerminal).
              setTimeout(() => {
                ipcInvoke("ipc_remote_client_list_terminals", {
                  pairing_id: remotePairingId,
                }).catch(() => {});
              }, 500);
            }
          } catch {
            // ignore
          }
        } else {
          // OK frame without payload — could be SUBSCRIBE, UNSUBSCRIBE, or CLOSE_TERMINAL response.
          // Do NOT reset activeTerminal here: SUBSCRIBE's OK response also has no payload,
          // and resetting it would cause a mount/unmount loop (subscribe → OK → reset → unsubscribe → ...).
          // CLOSE_TERMINAL is handled via NOTIFY(list_changed) which refreshes the list,
          // and the "terminal not in list" check will clean up activeTerminal naturally.
        }
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [isRemote, remotePairingId]);
  // Virtual server for local terminal (no SSH config, reuses overview UI)
  const displayServer: ServerState = isLocal
    ? ({
        id: "__local__",
        name: t("server.my_computer"),
        ssh: null as any,
        proxy: { enabled: false, socks5_port: 0, http_port: 0, mixed_port: 0, max_channels: 0, channel_idle_timeout: 0 },
        reconnect: { auto_reconnect: false, heartbeat_interval: 0, max_attempts: 0, reconnect_timeout_secs: 0, initial_backoff_secs: 0, max_backoff_secs: 0 },
        ip_check: { enabled: false, interval_secs: 0 },
        last_known_ip: null,
        triggers: [],
        suppress_firewall_badge: false,
        current_status: "connected",
        current_ip: null,
        client_ip: null,
        connected_since: null,
        reconnect_count: 0,
        max_attempts: 0,
        proxy_running: false,
        active_channels: 0,
        bytes_in: 0,
        bytes_out: 0,
        auth_banner: null,
        rz_available: false,
      } as ServerState)
    : isRemote && remotePeer
    ? ({
        id: selectedId!,
        name: remotePeer.peerName || remotePeer.pairingId,
        ssh: null as any,
        proxy: { enabled: false, socks5_port: 0, http_port: 0, mixed_port: 0, max_channels: 0, channel_idle_timeout: 0 },
        reconnect: { auto_reconnect: false, heartbeat_interval: 0, max_attempts: 0, reconnect_timeout_secs: 0, initial_backoff_secs: 0, max_backoff_secs: 0 },
        ip_check: { enabled: false, interval_secs: 0 },
        last_known_ip: null,
        triggers: [],
        suppress_firewall_badge: false,
        current_status: remoteIsConnected ? "connected" : "disconnected",
        current_ip: null,
        client_ip: null,
        connected_since: null,
        reconnect_count: 0,
        max_attempts: 0,
        proxy_running: false,
        active_channels: 0,
        bytes_in: 0,
        bytes_out: 0,
        auth_banner: null,
        rz_available: false,
      } as ServerState)
    : server!;
  const isConnected = isLocal || (isRemote ? remoteIsConnected : server?.current_status === "connected");
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

  // Listen for terminal:opened events — auto-create a tab for "My Computer"
  // when a terminal is opened by a remote desktop (via RemoteServer).
  // Skip if the tab already exists (opened by handleOpenLocalTerminal).
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    listen<{ sessionId: string }>("terminal:opened", (event) => {
      const sid = event.payload.sessionId;
      const serverId = "__local__";
      const store = useServerStore.getState();
      const tabs = store.terminal_tabs_by_server[serverId] || [];
      const tabId: Tab = `term:${sid}`;
      // Skip if tab already exists (opened by handleOpenLocalTerminal)
      if (tabs.some((t) => t.id === tabId)) return;
      // Auto-create tab for this terminal
      const defaultLabel = `${t("server.terminal")} ${tabs.length + 1}`;
      addTerminalTab(serverId, {
        id: tabId,
        sessionId: sid,
        label: defaultLabel,
        defaultLabel,
        initialOutput: "",
        disconnected: false,
        agentStatus: null,
      });
      // Attach a binary output Channel so terminal output is received
      attachTerminalChannel(sid).catch((e) => {
        console.error("Failed to attach terminal channel:", e);
      });
      // Only auto-select if "My Computer" is currently active
      if (selectedId === "local") {
        setActiveTerminalTab(serverId, tabId);
      }
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      if (unlisten) unlisten();
    };
  }, [selectedId, t, addTerminalTab, setActiveTerminalTab]);

  // Open a new terminal session and add a tab for it.
  // Flow: click → connecting → SSH connect + terminal open → tab created → connected
  // Requests are queued and processed serially to avoid SSH channel conflicts.
  const openQueueRef = useRef<Promise<void>>(Promise.resolve());

  // Helper: create a Channel for binary terminal output and call ipc_terminal_open.
  // The Channel receives raw ArrayBuffer from the Rust backend; we convert to Uint8Array
  // and dispatch to the registered TerminalView callback.
  // (Uses the shared openTerminalWithChannel from @/lib/terminal)

  // Helper: open a normal (non-tmux) terminal and add a tab
  const openNormalTerminal = useCallback(
    async (serverId: string, triggerOverrides?: Record<string, boolean>) => {
      const currentTabs =
        useServerStore.getState().terminal_tabs_by_server[serverId] || [];
      const defaultLabel = `${t("server.terminal")} ${currentTabs.length + 1}`;
      const result = await openTerminalWithChannel(serverId, 80, 24, {
        name: defaultLabel,
        triggerOverrides:
          triggerOverrides && Object.keys(triggerOverrides).length > 0
            ? triggerOverrides
            : undefined,
      });
      const sessionId = result.session_id;
      const initialOutput = result.initial_output || "";
      const tabId: Tab = `term:${sessionId}`;
      addTerminalTab(serverId, {
        id: tabId,
        sessionId,
        label: defaultLabel,
        defaultLabel,
        initialOutput,
        disconnected: false,
        agentStatus: null,
      });
      setActiveTerminalTab(serverId, tabId);
    },
    [t],
  );

  // Helper: create a new tmux session and add a tab
  const openTmuxNewSession = useCallback(
    async (serverId: string, description: string, cols: number, rows: number) => {
      let sessionId = "";
      const onOutput = new Channel<ArrayBuffer>();
      onOutput.onmessage = (data: ArrayBuffer) => {
        if (sessionId) {
          dispatchTerminalOutput(sessionId, new Uint8Array(data), false);
        }
      };
      const result = await ipcInvoke<{ session_id: string; initial_output: string; tmux_session_name?: string }>(
        "ipc_tmux_new_session",
        { server_id: serverId, description, cols, rows, on_output: onOutput },
      );
      sessionId = result.session_id;
      const initialOutput = result.initial_output || "";
      const tmuxName = result.tmux_session_name || null;
      const tabId: Tab = `term:${sessionId}`;
      const currentTabs =
        useServerStore.getState().terminal_tabs_by_server[serverId] || [];
      const defaultLabel = `${t("server.terminal")} ${currentTabs.length + 1}`;
      addTerminalTab(serverId, {
        id: tabId,
        sessionId,
        label: defaultLabel,
        defaultLabel,
        initialOutput,
        disconnected: false,
        agentStatus: null,
        tmuxSessionName: tmuxName,
      });
      setActiveTerminalTab(serverId, tabId);
    },
    [t],
  );

  // Helper: attach to an existing tmux session and add a tab
  const openTmuxAttachSession = useCallback(
    async (serverId: string, tmuxName: string, cols: number, rows: number) => {
      let sessionId = "";
      const onOutput = new Channel<ArrayBuffer>();
      onOutput.onmessage = (data: ArrayBuffer) => {
        if (sessionId) {
          dispatchTerminalOutput(sessionId, new Uint8Array(data), false);
        }
      };
      const result = await ipcInvoke<{ session_id: string; initial_output: string }>(
        "ipc_tmux_attach_session",
        { server_id: serverId, tmux_session_name: tmuxName, cols, rows, on_output: onOutput },
      );
      sessionId = result.session_id;
      const initialOutput = result.initial_output || "";
      const tabId: Tab = `term:${sessionId}`;
      const currentTabs =
        useServerStore.getState().terminal_tabs_by_server[serverId] || [];
      const defaultLabel = `${t("server.terminal")} ${currentTabs.length + 1}`;
      addTerminalTab(serverId, {
        id: tabId,
        sessionId,
        label: defaultLabel,
        defaultLabel,
        initialOutput,
        disconnected: false,
        agentStatus: null,
        tmuxSessionName: tmuxName,
      });
      setActiveTerminalTab(serverId, tabId);
    },
    [t],
  );

  const handleOpenTerminal = useCallback(async () => {
    if (!server?.id) return;
    const serverId = server.id;

    // Chain onto the queue — each open waits for the previous to finish
    openQueueRef.current = openQueueRef.current.then(async () => {
      // Read live connection state from store (may have changed since queueing)
      const liveServer = useServerStore.getState().servers.find(
        (s) => s.id === serverId,
      );
      const liveConnected = liveServer?.current_status === "connected";

      // If not connected, connect first
      if (!liveConnected) {
        updateServerStatus(serverId, "connecting");
        try {
          const result = await ipcInvoke<{ rz_available?: boolean }>("ipc_connect_server", { serverId });
          if (result && typeof result.rz_available === "boolean") {
            useServerStore.getState().setRzAvailable(serverId, result.rz_available);
          }
          updateServerStatus(
            serverId,
            "connected",
            liveServer?.last_known_ip || undefined,
          );
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
        // Read trigger overrides before opening terminal (passed to daemon
        // in the same call to avoid race condition with terminal event consumer).
        const triggerOverrides = useTriggerStore.getState().serverExecInTerminalOverrides[serverId];

        // Check tmux_mode for this server
        const liveServer2 = useServerStore.getState().servers.find(
          (s) => s.id === serverId,
        );
        const tmuxMode = liveServer2?.tmux_mode || "ask";

        if (tmuxMode === "disabled") {
          // Normal terminal — no tmux
          await openNormalTerminal(serverId, triggerOverrides);
        } else if (tmuxMode === "always_new") {
          // Create new tmux session directly
          await openTmuxNewSession(serverId, "", 80, 24);
        } else if (tmuxMode === "auto") {
          // Auto-restore: list sessions, attach to most recent or create new
          try {
            const tmuxResult = await ipcInvoke<{
              sessions: Array<{ name: string; last_activity: number }>;
              tmux_installed: boolean;
            }>("ipc_tmux_list_sessions", { server_id: serverId });
            if (!tmuxResult.tmux_installed || tmuxResult.sessions.length === 0) {
              // No tmux or no sessions — create new
              await openTmuxNewSession(serverId, "", 80, 24);
            } else {
              // Sort by last_activity descending, attach to most recent
              const sorted = [...tmuxResult.sessions].sort(
                (a, b) => b.last_activity - a.last_activity,
              );
              await openTmuxAttachSession(serverId, sorted[0].name, 80, 24);
            }
          } catch {
            // tmux check failed — fallback to normal terminal
            await openNormalTerminal(serverId, triggerOverrides);
          }
        } else {
          // "ask" mode — show picker
          setTmuxPickerCols(80);
          setTmuxPickerRows(24);
          setShowTmuxPicker(true);
        }
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
    }).catch(() => {
      // Swallow rejections so the chain keeps going for subsequent clicks
    });

    // Await so the caller (button) sees completion
    await openQueueRef.current;
  }, [
    server?.id,
    server?.last_known_ip,
    isConnected,
    t,
    addTerminalTab,
    setActiveTerminalTab,
    updateServerStatus,
  ]);

  // Fetch local terminal info (default shell, OS details, hostname, username) from backend
  const [localInfo, setLocalInfo] = useState<{
    default_shell: string;
    shell_name: string;
    os_name: string;
    os_version: string;
    os_arch: string;
    hostname: string;
    username: string;
    real_name: string;
    available_shells: string[];
  } | null>(null);

  useEffect(() => {
    if (!isLocal) return;
    ipcInvoke<{
      default_shell: string;
      shell_name: string;
      os_name: string;
      os_version: string;
      os_arch: string;
      hostname: string;
      username: string;
      real_name: string;
      available_shells: string[];
    }>("ipc_get_local_info")
      .then((info) => setLocalInfo(info))
      .catch(() => {});
  }, [isLocal]);

  // Selected shell for local terminal (null = use default)
  const [selectedShell, setSelectedShell] = useState<string | null>(null);

  // Open a local terminal (no SSH connection needed).
  // Bypasses handleOpenTerminal entirely — calls openTerminalWithChannel
  // with backend: "local" and manages the tab directly.
  const handleOpenLocalTerminal = useCallback(async (shell?: string) => {
    if (!isLocal) return;
    const serverId = "__local__";
    // Use explicitly passed shell, or the selected shell from toggle, or null (default)
    const effectiveShell = shell ?? selectedShell ?? undefined;
    // Read trigger overrides before opening terminal (passed to daemon
    // in the same call to avoid race condition with terminal event consumer).
    const triggerOverrides = useTriggerStore.getState().serverExecInTerminalOverrides[serverId];
    try {
      const currentTabs =
        useServerStore.getState().terminal_tabs_by_server[serverId] || [];
      const defaultLabel = `${t("server.terminal")} ${currentTabs.length + 1}`;
      const result = await openTerminalWithChannel(serverId, 80, 24, {
        backend: "local",
        shell: effectiveShell,
        name: defaultLabel,
        triggerOverrides: triggerOverrides && Object.keys(triggerOverrides).length > 0 ? triggerOverrides : undefined,
      });
      const sessionId = result.session_id;
      const tabId: Tab = `term:${sessionId}`;
      addTerminalTab(serverId, {
        id: tabId,
        sessionId,
        label: defaultLabel,
        defaultLabel,
        initialOutput: result.initial_output || "",
        disconnected: false,
        agentStatus: null,
      });
      setActiveTerminalTab(serverId, tabId);
    } catch (e) {
      const msg = formatIpcError(e);
      toast.error(t("server.terminal_open_failed"), { description: msg });
    }
  }, [isLocal, t, addTerminalTab, setActiveTerminalTab, selectedShell]);

  // Open a terminal from the context menu. Uses the same logic as the login button.
  // Shares the same serial queue as handleOpenTerminal to avoid SSH channel conflicts.
  const openTerminalFromMenu = useCallback(async () => {
    openQueueRef.current = openQueueRef.current.then(async () => {
      const store = useServerStore.getState();
      const serverId = store.selected_server_id;
      if (!serverId) return;
      const currentServer = store.servers.find((s) => s.id === serverId);
      if (!currentServer) return;
      const alreadyConnected = currentServer.current_status === "connected";

      if (!alreadyConnected) {
        store.updateServerStatus(serverId, "connecting");
        try {
          const result = await ipcInvoke<{ rz_available?: boolean }>("ipc_connect_server", { serverId });
          if (result && typeof result.rz_available === "boolean") {
            useServerStore.getState().setRzAvailable(serverId, result.rz_available);
          }
          store.updateServerStatus(
            serverId,
            "connected",
            currentServer.last_known_ip || undefined,
          );
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

      try {
        const result = await openTerminalWithChannel(serverId);
        const sessionId = result.session_id;
        const initialOutput = result.initial_output || "";
        const tabId: Tab = `term:${sessionId}`;
        const currentTabs = store.terminal_tabs_by_server[serverId] || [];
        const defaultLabel = `${t("server.terminal")} ${currentTabs.length + 1}`;
        store.addTerminalTab(serverId, {
          id: tabId,
          sessionId,
          label: defaultLabel,
          defaultLabel,
          initialOutput,
          disconnected: false,
          agentStatus: null,
        });
        store.setActiveTerminalTab(serverId, tabId);
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
    }).catch(() => {
      // Swallow rejections so the chain keeps going for subsequent calls
    });
    await openQueueRef.current;
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

  if (!server && !isLocal && !(isRemote && remotePeer)) {
    return (
      <div className="flex-1 flex items-center justify-center text-gray-500">
        {t("server.add")}
      </div>
    );
  }

  const handleConnect = async () => {
    if (!server?.id) return;
    updateServerStatus(server.id, "connecting");
    try {
      const result = await ipcInvoke<{ rz_available?: boolean }>("ipc_connect_server", { serverId: server.id });
      updateServerStatus(
        server.id,
        "connected",
        server.last_known_ip || undefined,
      );
      if (result && typeof result.rz_available === "boolean") {
        useServerStore.getState().setRzAvailable(server.id, result.rz_available);
      }
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
    if (!server?.id) return;
    // If there are active terminal sessions, confirm before disconnecting
    // (disconnecting will close all terminals)
    if (termTabs.length > 0) {
      setShowDisconnectConfirm(true);
      return;
    }
    doDisconnect();
  };

  // Disconnect a single terminal tab's session (closes PTY, keeps SSH alive)
  const handleDisconnectTerminal = async (tabId: string) => {
    const serverId = displayServer.id;
    const currentTabs = useServerStore.getState().terminal_tabs_by_server[serverId] || [];
    const tab = currentTabs.find((tt) => tt.id === tabId);
    if (!tab || !tab.sessionId) return;
    if (tab.disconnected) return; // already disconnected
    // Close the PTY session — mark tab as disconnected (SSH stays alive)
    ipcInvoke("ipc_terminal_close", { session_id: tab.sessionId }).catch(
      () => {},
    );
    setTerminalTabDisconnected(serverId, tab.sessionId);
  };

  const doDisconnect = async () => {
    if (!server?.id) return;
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
    if (!renamingTabId || !server?.id) {
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

  // Kill the tmux session on the server, then close the tab
  const killTmuxSession = (tabId: string) => {
    const serverId = selectedId || "";
    const currentTabs = terminalTabsByServer[serverId] || [];
    const tab = currentTabs.find((tt) => tt.id === tabId);
    if (!tab || !tab.tmuxSessionName) return;
    const tmuxName = tab.tmuxSessionName;
    ipcInvoke("ipc_tmux_kill_session", {
      server_id: serverId,
      tmux_session_name: tmuxName,
    })
      .then(() => {
        // Close the tab after killing the session
        if (tab.sessionId)
          ipcInvoke("ipc_terminal_close", { session_id: tab.sessionId }).catch(
            () => {},
          );
        removeTerminalTab(serverId, tabId);
        maybeDisconnectIfIdle(serverId, currentTabs.length - 1);
      })
      .catch((e) => {
        toast.error(t("server.tmux_kill_failed"), {
          description: formatIpcError(e),
        });
      });
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
      displayServer.proxy.mixed_port > 0
        ? displayServer.proxy.mixed_port
        : displayServer.proxy.socks5_port;
    const items: ContextMenuEntry[] = [
      ...(isConnected && !isLocal
        ? [
            {
              label: t("server.disconnect"),
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
      { label: isLocal ? t("server.open_local_terminal") : t("tab.login_server"), onClick: () => isLocal ? handleOpenLocalTerminal() : openTerminalFromMenu() },
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
      ...(!isLocal && displayServer.proxy_running
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
        label: t("tab.manage_triggers"),
        onClick: () => setTriggerMgrTabId(tabId),
      },
      {
        label: t("tab.reconnect"),
        onClick: () => handleConnect(),
        disabled: isConnected,
      },
      {
        label: t("tab.disconnect"),
        onClick: () => handleDisconnectTerminal(tabId),
        disabled: !isConnected || tab.disconnected,
        danger: true,
      },
      { separator: true },
      ...(tab.tmuxSessionName ? [{
        label: t("tab.kill_tmux_session"),
        onClick: () => killTmuxSession(tabId),
        danger: true,
      }] : []),
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
        onClick: () => isLocal ? handleOpenLocalTerminal() : openTerminalFromMenu(),
      },
    ];
    showContextMenu(e, items);
  };

  const handleToggleProxy = async () => {
    if (!server?.id) return;
    const newEnabled = !server.proxy_running;

    // If starting proxy and not connected, auto-connect first
    if (newEnabled && !isConnected) {
      updateServerStatus(server.id, "connecting");
      try {
        const result = await ipcInvoke<{ rz_available?: boolean }>("ipc_connect_server", { serverId: server.id });
        if (result && typeof result.rz_available === "boolean") {
          useServerStore.getState().setRzAvailable(server.id, result.rz_available);
        }
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
    if (!server?.id) return;
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
    if (!server?.id) return;
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
    if (!server?.id) return;
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

  const tabs: { key: Tab; label: string; disconnected: boolean; agentStatus: AgentStatus | null }[] = isRemote
    ? [
        { key: "overview", label: t("server.overview"), disconnected: false, agentStatus: null },
        ...remoteTerminals.map((rt) => ({
          key: `remote_term:${rt.terminal_id}` as Tab,
          label: rt.name,
          disconnected: !remoteIsConnected,
          agentStatus: null,
        })),
      ]
    : [
        { key: "overview", label: t("server.overview"), disconnected: false, agentStatus: null },
        ...termTabs.map((tt) => ({
          key: tt.id as Tab,
          label: tt.label,
          disconnected: tt.disconnected,
          agentStatus: tt.agentStatus ?? null,
        })),
      ];

  const statusColor = isConnected
    ? "text-[#34C759]"
    : displayServer.current_status === "auth_failed" ||
        displayServer.current_status === "offline"
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
              className={`flex items-center gap-1 pl-4 ${isOverview ? "pr-4" : "pr-0"} py-2 text-sm font-medium transition-colors cursor-pointer rounded-b-lg flex-shrink-0 bg-white dark:bg-[#1E1E1E] border border-gray-200/80 dark:border-white/[0.06] border-t-0 ${
                isActive
                  ? "text-[#007AFF] dark:text-[#0A84FF] shadow-[0_3px_12px_rgba(0,0,0,0.1)] dark:shadow-[0_3px_12px_rgba(0,0,0,0.5)] z-10"
                  : "text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200"
              } ${tab.disconnected && !isOverview ? "opacity-50 italic" : ""} ${
                isDraggable ? "select-none" : ""
              } ${dragOverTabId === tab.key && draggedTabId && draggedTabId !== tab.key ? "ring-1 ring-[#007AFF]" : ""} ${
                draggedTabId === tab.key ? "opacity-40" : ""
              }`}
              onClick={(e) => {
                // Skip if this click was triggered by a right-click sequence
                if (rightClickButtonRef.current) {
                  rightClickButtonRef.current = false;
                  return;
                }
                if (isRemote && tab.key === "overview") {
                  remoteActiveTerminalRef.current = null;
                  setRemoteActiveTerminal(null);
                } else if (isRemote && tab.key.startsWith("remote_term:")) {
                  const termId = parseInt(tab.key.slice("remote_term:".length), 10);
                  remoteActiveTerminalRef.current = termId;
                  setRemoteActiveTerminal(termId);
                  if (remotePairingId) {
                    ipcInvoke("ipc_remote_client_subscribe", {
                      pairing_id: remotePairingId,
                      terminal_id: termId,
                    }).catch((e: any) => {
                      toast.error(`Subscribe failed: ${e?.message || e}`);
                    });
                  }
                } else {
                  setActiveTerminalTab(displayServer.id, tab.key);
                }
              }}
              onMouseDown={(e) => {
                // Detect right-click on mousedown (before click fires).
                // macOS: real right-click = button=2, Ctrl+click = button=0+ctrlKey
                if (e.button === 2 || e.ctrlKey) {
                  rightClickButtonRef.current = true;
                }
              }}
              onDoubleClick={(e) => {
                if (tab.key !== "overview") {
                  e.stopPropagation();
                  handleRenameTab(tab.key, tab.label);
                }
              }}
              onContextMenu={(e) => {
                rightClickButtonRef.current = true;
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
                <span className="flex items-center gap-1.5">
                  {tab.agentStatus && tab.agentStatus !== "unknown" && (
                    <AgentStatusDot status={tab.agentStatus} />
                  )}
                  {tab.label}
                </span>
              )}
              {tab.key !== "overview" && renamingTabId !== tab.key && (
                <span
                  className="flex items-center justify-center pl-1 pr-2.5 h-full text-gray-400 hover:text-red-500 transition-colors text-xs leading-none cursor-pointer"
                  onMouseDown={(e) => {
                    if (e.button === 2 || e.ctrlKey) {
                      rightClickButtonRef.current = true;
                    }
                  }}
                  onClick={(e) => {
                    e.stopPropagation();
                    if (rightClickButtonRef.current) {
                      rightClickButtonRef.current = false;
                      return;
                    }
                    if (isRemote && tab.key.startsWith("remote_term:")) {
                      // Remote terminal close — send CLOSE_TERMINAL frame
                      const termId = parseInt(tab.key.slice("remote_term:".length), 10);
                      if (remotePairingId) {
                        ipcInvoke("ipc_remote_client_close_terminal", {
                          pairing_id: remotePairingId,
                          terminal_id: termId,
                        }).catch((err: any) => {
                          toast.error(`Close failed: ${err?.message || err}`);
                        });
                      }
                    } else {
                      setPendingCloseTab(tab.key);
                    }
                  }}
                  onContextMenu={(e) => {
                    e.stopPropagation();
                    rightClickButtonRef.current = true;
                  }}
                  title={t("common.close")}
                >
                  ✕
                </span>
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

        {pendingCloseTab && (
          <ConfirmDialog
            level="low"
            title={t("tab.close_session")}
            message={t("tab.close_confirm")}
            confirmLabel={t("common.close")}
            onConfirm={() => {
              closeTab(pendingCloseTab);
              setPendingCloseTab(null);
            }}
            onCancel={() => setPendingCloseTab(null)}
          />
        )}

        {/* Tmux session picker — shown when tmux_mode="ask" */}
        {server?.id && (
          <TmuxSessionPicker
            visible={showTmuxPicker}
            serverId={server.id}
            cols={tmuxPickerCols}
            rows={tmuxPickerRows}
            openTmuxTabs={(() => {
              const map: Record<string, string> = {};
              for (const tab of termTabs) {
                if (tab.tmuxSessionName) {
                  map[tab.tmuxSessionName] = tab.id;
                }
              }
              return map;
            })()}
            onSessionCreated={(sessionId, initialOutput, tmuxSessionName) => {
              setShowTmuxPicker(false);
              const tabId: Tab = `term:${sessionId}`;
              const currentTabs =
                useServerStore.getState().terminal_tabs_by_server[server.id] || [];
              const defaultLabel = `${t("server.terminal")} ${currentTabs.length + 1}`;
              addTerminalTab(server.id, {
                id: tabId,
                sessionId,
                label: defaultLabel,
                defaultLabel,
                initialOutput,
                disconnected: false,
                agentStatus: null,
                tmuxSessionName: tmuxSessionName || null,
              });
              setActiveTerminalTab(server.id, tabId);
            }}
            onSkipTmux={async () => {
              setShowTmuxPicker(false);
              // Open normal terminal without tmux
              try {
                const triggerOverrides = useTriggerStore.getState().serverExecInTerminalOverrides[server.id];
                await openNormalTerminal(server.id, triggerOverrides);
              } catch (e: any) {
                const msg = formatIpcError(e);
                toast.error(t("server.terminal_open_failed"), { description: msg });
              }
            }}
            onSwitchToTab={(tabId) => {
              setShowTmuxPicker(false);
              setActiveTerminalTab(server.id, tabId);
            }}
            onCancel={() => setShowTmuxPicker(false)}
          />
        )}

        {triggerMgrTabId && (() => {
          const tab = termTabs.find((tt) => tt.id === triggerMgrTabId);
          if (!tab) return null;
          return (
            <TabTriggerManager
              serverId={displayServer.id}
              sessionId={tab.sessionId}
              onClose={() => setTriggerMgrTabId(null)}
            />
          );
        })()}

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
                        {isLocal ? (
                          <Monitor className="w-6 h-6" />
                        ) : (
                          displayServer.name.charAt(0).toUpperCase()
                        )}
                      </div>
                      <div>
                        <div className="text-base font-semibold text-gray-900 dark:text-gray-100 truncate">
                          {displayServer.name}
                        </div>
                        <div className={`text-xs ${statusColor} font-medium`}>
                          {isLocal
                            ? t("server.local_ready")
                            : t(`server.status.${displayServer.current_status}`)}
                        </div>
                      </div>
                    </div>
                    <div className="flex items-center gap-2 flex-shrink-0">
                      {!isLocal && !isRemote && isConnected && (
                        <button
                          className="px-3.5 py-1.5 text-sm rounded-lg bg-gray-100 dark:bg-[#2C2C2E] text-gray-700 dark:text-gray-200 hover:bg-gray-200 dark:hover:bg-[#3A3A3C] font-medium transition-colors"
                          onClick={handleDisconnect}
                        >
                          {t("server.disconnect")}
                        </button>
                      )}
                      <button
                        className="px-4 py-1.5 text-sm rounded-lg bg-[#34C759] text-white hover:bg-[#2EB34F] disabled:opacity-50 font-medium transition-colors "
                        onClick={isLocal ? () => handleOpenLocalTerminal() : isRemote ? () => {
                          // For remote, create a new terminal on the remote desktop
                          if (remotePairingId) {
                            const defaultLabel = `${t("server.terminal")} ${remoteTerminals.length + 1}`;
                            ipcInvoke("ipc_remote_client_new_terminal", {
                              pairing_id: remotePairingId,
                              shell: selectedRemoteShell ?? undefined,
                              name: defaultLabel,
                            }).catch((e: any) => {
                              toast.error(`Failed to create terminal: ${e?.message || e}`);
                            });
                          }
                        } : handleOpenTerminal}
                        disabled={isLocal ? false : isRemote ? !remoteIsConnected : connecting}
                      >
                        {isLocal
                          ? t("server.open_local_terminal")
                          : isRemote
                            ? t("server.open_local_terminal")
                            : connecting
                              ? t("server.status.connecting")
                              : termTabs.length === 0
                                ? t("server.connect_terminal")
                                : t("server.login_server")}
                      </button>
                    </div>
                  </div>
                  <div className="divide-y divide-gray-100 dark:divide-white/[0.06]">
                    {isLocal ? (
                      <>
                        <div className="flex items-center justify-between px-4 py-3">
                          <span className="text-sm text-gray-500">
                            {t("server.local_shell")}
                          </span>
                          <div className="flex items-center gap-1.5">
                            {/* Default shell button — shows shell name + (默认) */}
                            <button
                              onClick={() => setSelectedShell(null)}
                              className={`px-2.5 py-1 text-xs rounded-md font-medium transition-colors ${
                                selectedShell === null
                                  ? "bg-[#007AFF] text-white"
                                  : "bg-gray-100 dark:bg-[#2C2C2E] text-gray-600 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-[#3A3A3C]"
                              }`}
                            >
                              {localInfo?.shell_name || t("server.shell.default")}
                              <span className="ml-1 opacity-60">
                                ({t("server.shell.default_label")})
                              </span>
                            </button>
                            {/* Other available shells */}
                            {localInfo?.available_shells
                              ?.filter((s) => s !== localInfo?.shell_name)
                              .map((shell) => (
                                <button
                                  key={shell}
                                  onClick={() => setSelectedShell(shell)}
                                  className={`px-2.5 py-1 text-xs rounded-md font-medium transition-colors ${
                                    selectedShell === shell
                                      ? "bg-[#007AFF] text-white"
                                      : "bg-gray-100 dark:bg-[#2C2C2E] text-gray-600 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-[#3A3A3C]"
                                  }`}
                                >
                                  {shell}
                                </button>
                              ))}
                          </div>
                        </div>
                        <div className="flex items-center justify-between px-4 py-3">
                          <span className="text-sm text-gray-500">
                            {t("server.local_os")}
                          </span>
                          <span className="font-mono text-sm text-[#1D1D1F] dark:text-gray-100 text-right">
                            {localInfo
                              ? `${localInfo.os_name} ${localInfo.os_version} (${localInfo.os_arch})`
                              : "—"}
                          </span>
                        </div>
                        <div className="flex items-center justify-between px-4 py-3">
                          <span className="text-sm text-gray-500">
                            {t("server.local_hostname")}
                          </span>
                          <span className="font-mono text-sm text-[#1D1D1F] dark:text-gray-100">
                            {localInfo?.hostname || "—"}
                          </span>
                        </div>
                        <div className="flex items-center justify-between px-4 py-3">
                          <span className="text-sm text-gray-500">
                            {t("server.local_user")}
                          </span>
                          <span className="font-mono text-sm text-[#1D1D1F] dark:text-gray-100">
                            {localInfo
                              ? `${localInfo.real_name} (${localInfo.username})`
                              : "—"}
                          </span>
                        </div>
                      </>
                    ) : isRemote ? (
                      <>
                        <div className="flex items-center justify-between px-4 py-3">
                          <span className="text-sm text-gray-500">
                            {t("server.local_shell")}
                          </span>
                          <div className="flex items-center gap-1.5">
                            {remoteInfo?.available_shells ? (
                              <>
                                <button
                                  onClick={() => setSelectedRemoteShell(null)}
                                  className={`px-2.5 py-1 text-xs rounded-md font-medium transition-colors ${
                                    selectedRemoteShell === null
                                      ? "bg-[#007AFF] text-white"
                                      : "bg-gray-100 dark:bg-[#2C2C2E] text-gray-600 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-[#3A3A3C]"
                                  }`}
                                >
                                  {remoteInfo.shell_name}
                                  <span className="ml-1 opacity-60">
                                    ({t("server.shell.default_label")})
                                  </span>
                                </button>
                                {remoteInfo.available_shells
                                  .filter((s) => s !== remoteInfo.shell_name)
                                  .map((shell) => (
                                    <button
                                      key={shell}
                                      onClick={() => setSelectedRemoteShell(shell)}
                                      className={`px-2.5 py-1 text-xs rounded-md font-medium transition-colors ${
                                        selectedRemoteShell === shell
                                          ? "bg-[#007AFF] text-white"
                                          : "bg-gray-100 dark:bg-[#2C2C2E] text-gray-600 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-[#3A3A3C]"
                                      }`}
                                    >
                                      {shell}
                                    </button>
                                  ))}
                              </>
                            ) : (
                              <span className="text-sm text-gray-400">—</span>
                            )}
                          </div>
                        </div>
                        <div className="flex items-center justify-between px-4 py-3">
                          <span className="text-sm text-gray-500">
                            {t("server.local_os")}
                          </span>
                          <span className="font-mono text-sm text-[#1D1D1F] dark:text-gray-100 text-right">
                            {remoteInfo
                              ? `${remoteInfo.os_name} ${remoteInfo.os_version} (${remoteInfo.os_arch})`
                              : "—"}
                          </span>
                        </div>
                        <div className="flex items-center justify-between px-4 py-3">
                          <span className="text-sm text-gray-500">
                            {t("server.local_hostname")}
                          </span>
                          <span className="font-mono text-sm text-[#1D1D1F] dark:text-gray-100">
                            {remoteInfo?.hostname || "—"}
                          </span>
                        </div>
                        <div className="flex items-center justify-between px-4 py-3">
                          <span className="text-sm text-gray-500">
                            {t("server.local_user")}
                          </span>
                          <span className="font-mono text-sm text-[#1D1D1F] dark:text-gray-100">
                            {remoteInfo
                              ? `${remoteInfo.real_name} (${remoteInfo.username})`
                              : "—"}
                          </span>
                        </div>
                      </>
                    ) : (
                      <>
                    <div className="flex items-center justify-between px-4 py-3">
                      <span className="text-sm text-gray-500">
                        {t("server.host")}
                      </span>
                      <span className="font-mono text-sm text-[#1D1D1F] dark:text-gray-100 truncate pl-4">
                        {displayServer.ssh?.host || "?"}:{displayServer.ssh?.port || "?"}
                      </span>
                    </div>
                    <div className="flex items-center justify-between px-4 py-3">
                      <span className="text-sm text-gray-500">
                        {t("server.ip_label")}
                      </span>
                      <span className="font-mono text-sm text-[#1D1D1F] dark:text-gray-100 truncate pl-4">
                        {displayServer.client_ip || "—"}
                      </span>
                    </div>
                    <div className="flex items-center justify-between px-4 py-3">
                      <span className="text-sm text-gray-500">
                        {t("server.auth_method")}
                      </span>
                      <span className="text-sm text-[#1D1D1F] dark:text-gray-100">
                        {displayServer.ssh?.auth_method === "key"
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
                          checked={displayServer.reconnect?.auto_reconnect ?? true}
                          onChange={(v) => {
                            ipcInvoke("ipc_update_server", {
                              server_id: displayServer.id,
                              auto_reconnect: v,
                            }).catch(() => {});
                            useServerStore.setState((s) => ({
                              servers: s.servers.map((srv) =>
                                srv.id === displayServer.id
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
                        {(displayServer.reconnect?.auto_reconnect ?? true) && (
                          <select
                            className="text-xs bg-[#F2F2F7]/80 dark:bg-[#2C2C2E]/80 border border-gray-200/80 dark:border-white/[0.08] rounded-lg px-2 py-1 text-[#1D1D1F] dark:text-gray-100 focus:outline-none focus:ring-1 focus:ring-[#007AFF]"
                            value={(() => {
                              const secs =
                                displayServer.reconnect?.reconnect_timeout_secs ??
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
                                server_id: displayServer.id,
                                reconnect_timeout_secs: secs,
                              }).catch(() => {});
                              useServerStore.setState((s) => ({
                                servers: s.servers.map((srv) =>
                                  srv.id === displayServer.id
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
                    {/* tmux behavior — 3-option segmented control */}
                    <div className="flex items-center justify-between px-4 py-3">
                      <span className="text-sm text-gray-500">
                        {t("server.tmux_mode_label")}
                      </span>
                      <div className="flex items-center gap-1 bg-[#F2F2F7]/80 dark:bg-[#2C2C2E]/80 rounded-lg p-0.5">
                        {(["auto", "ask", "always_new", "disabled"] as const).map((mode) => (
                          <button
                            key={mode}
                            onClick={() => {
                              ipcInvoke("ipc_update_config", {
                                path: `servers[${displayServer.id}].tmux_mode`,
                                value: mode,
                              }).catch(() => {});
                              useServerStore.setState((s) => ({
                                servers: s.servers.map((srv) =>
                                  srv.id === displayServer.id
                                    ? { ...srv, tmux_mode: mode }
                                    : srv,
                                ),
                              }));
                            }}
                            className={`px-2.5 py-1 text-xs rounded-md font-medium transition-colors ${
                              (displayServer.tmux_mode || "ask") === mode
                                ? "bg-white dark:bg-[#48484A] text-[#1D1D1F] dark:text-gray-100 shadow-sm"
                                : "text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200"
                            }`}
                          >
                            {t(`server.tmux_mode_${mode}`)}
                          </button>
                        ))}
                      </div>
                    </div>
                      </>
                    )}
                  </div>

                  {!isLocal && server?.auth_banner && (
                    <div className="border-t border-gray-100 dark:border-white/[0.06] px-4 py-3">
                      <div className="text-xs text-gray-500 mb-1.5">
                        {t("server.welcome_message")}
                      </div>
                      <pre className="font-mono text-xs text-gray-700 dark:text-gray-300 bg-gray-50/80 dark:bg-black/20 rounded-lg p-3 overflow-x-auto whitespace-pre-wrap">
                        {displayServer.auth_banner}
                      </pre>
                    </div>
                  )}
                </div>

                {/* Device pairing card — shown for local terminal (right side of grid) */}
                {isLocal && <PairingCard />}

                {/* Proxy card — macOS Settings style grouped list (hidden for local terminal and remote) */}
                {!isLocal && !isRemote && (
                <div className="bg-[#FBFBFB] dark:bg-[#1E1E1E] rounded-[16px] overflow-hidden border border-gray-200/80 dark:border-white/[0.06] flex flex-col">
                  {/* Header */}
                  <div className="flex items-center justify-between px-4 py-4 border-b border-gray-100 dark:border-white/[0.06]">
                    <div>
                      <div className="text-xs text-gray-500">
                        {t("server.proxy")}
                      </div>
                      <div
                        className={`text-base font-semibold mt-0.5 ${displayServer.proxy_running ? "text-[#34C759]" : "text-gray-400"}`}
                      >
                        {!isConnected
                          ? t("proxy.not_connected")
                          : displayServer.proxy_running
                            ? t("proxy.started")
                            : t("proxy.off")}
                      </div>
                    </div>
                    <button
                      className={`px-4 py-1.5 text-sm rounded-lg font-medium transition-colors ${
                        displayServer.proxy_running
                          ? "bg-[#34C759] text-white hover:bg-[#2EB34F] "
                          : "bg-gray-100 dark:bg-[#2C2C2E] text-gray-700 dark:text-gray-200 hover:bg-gray-200 dark:hover:bg-[#3A3A3C]"
                      }`}
                      onClick={handleToggleProxy}
                      disabled={connecting && !displayServer.proxy_running}
                    >
                      {displayServer.proxy_running
                        ? t("server.stop_proxy")
                        : t("server.start_proxy")}
                    </button>
                  </div>

                  {/* Port configuration rows */}
                  <div className="divide-y divide-gray-100 dark:divide-white/[0.06]">
                    {displayServer.proxy.mixed_port > 0 ? (
                      <div className="flex items-center justify-between px-4 py-3">
                        <span className="text-sm text-gray-500">Mixed</span>
                        {displayServer.proxy_running ? (
                          <span className="text-sm font-mono text-[#1D1D1F] dark:text-gray-100">
                            {displayServer.proxy.mixed_port}
                          </span>
                        ) : (
                          <input
                            type="number"
                            className="w-20 px-2 py-1 text-sm font-mono border border-gray-200/80 dark:border-white/[0.08] rounded-lg bg-[#FBFBFB] dark:bg-[#2C2C2E] text-[#1D1D1F] dark:text-gray-100 focus:outline-none focus:border-[#007AFF]"
                            value={displayServer.proxy.mixed_port}
                            onChange={(e) =>
                              handleUpdateProxy({
                                mixed_port: parseInt(e.target.value) || 0,
                              })
                            }
                            disabled={displayServer.proxy_running}
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
                            {displayServer.proxy_running ? (
                              <span className="text-sm font-mono text-[#1D1D1F] dark:text-gray-100">
                                {displayServer.proxy.socks5_port}
                              </span>
                            ) : (
                              <input
                                type="number"
                                className="w-20 px-2 py-1 text-sm font-mono border border-gray-200/80 dark:border-white/[0.08] rounded-lg bg-[#FBFBFB] dark:bg-[#2C2C2E] text-[#1D1D1F] dark:text-gray-100 focus:outline-none focus:border-[#007AFF]"
                                value={displayServer.proxy.socks5_port}
                                onChange={(e) =>
                                  handleUpdateProxy({
                                    socks5_port:
                                      parseInt(e.target.value) || 1080,
                                  })
                                }
                                disabled={displayServer.proxy_running}
                              />
                            )}
                          </div>
                          <div className="flex items-center justify-between px-4 py-3">
                            <span className="text-sm text-gray-500">HTTP</span>
                            {displayServer.proxy_running ? (
                              <span className="text-sm font-mono text-[#1D1D1F] dark:text-gray-100">
                                {displayServer.proxy.http_port}
                              </span>
                            ) : (
                              <input
                                type="number"
                                className="w-20 px-2 py-1 text-sm font-mono border border-gray-200/80 dark:border-white/[0.08] rounded-lg bg-[#FBFBFB] dark:bg-[#2C2C2E] text-[#1D1D1F] dark:text-gray-100 focus:outline-none focus:border-[#007AFF]"
                                value={displayServer.proxy.http_port}
                                onChange={(e) =>
                                  handleUpdateProxy({
                                    http_port: parseInt(e.target.value) || 8080,
                                  })
                                }
                                disabled={displayServer.proxy_running}
                              />
                            )}
                          </div>
                        </div>
                      </>
                    )}
                    <div
                      className={`grid ${displayServer.proxy_running ? "grid-cols-1" : "grid-cols-2"} divide-x divide-gray-100 dark:divide-white/[0.06]`}
                    >
                      {!displayServer.proxy_running && (
                        <div className="flex items-center justify-between px-4 py-3">
                          <span className="text-sm text-gray-500">
                            {t("server.mixed_port")}
                          </span>
                          <Toggle
                            checked={displayServer.proxy.mixed_port > 0}
                            onChange={(v) =>
                              handleUpdateProxy({
                                mixed_port: v
                                  ? displayServer.proxy.socks5_port || 1080
                                  : 0,
                              })
                            }
                          />
                        </div>
                      )}
                      <div
                        className={`flex items-center justify-between px-4 py-3 ${!displayServer.proxy_running ? "opacity-50 pointer-events-none" : ""}`}
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
                  {displayServer.proxy_running && displayServer.active_channels > 0 && (
                    <div className="text-xs text-[#34C759] font-medium px-4 py-2 border-t border-gray-100 dark:border-white/[0.06]">
                      {displayServer.active_channels} {t("server.active_clients")}
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
                        disabled={!displayServer.proxy_running}
                      />
                      <button
                        className="px-4 py-2 text-sm rounded-lg bg-[#007AFF] text-white hover:bg-[#0063D1] disabled:opacity-50 transition-colors"
                        onClick={handleTestProxy}
                        disabled={!displayServer.proxy_running || testingProxy}
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
                )}
              </div>

              {/* Triggers panel — full width (hidden for remote) */}
              {!isRemote && (
              <div className="bg-[#FBFBFB] dark:bg-[#1E1E1E] rounded-[16px] border border-gray-200/80 dark:border-white/[0.06] overflow-hidden">
                <div className="px-4 py-3 border-b border-gray-100 dark:border-white/[0.06] flex items-center justify-between">
                  <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100">
                    {t("trigger.title")}
                  </h3>
                  <button
                    className="text-xs px-3 py-1.5 rounded-lg bg-blue-500 text-white hover:bg-blue-600 font-medium transition-colors shadow-sm"
                    onClick={() =>
                      window.dispatchEvent(
                        new CustomEvent("trigger-add", {
                          detail: { serverId: displayServer.id },
                        }),
                      )
                    }
                  >
                    + {t("trigger.add")}
                  </button>
                </div>
                <div className="p-4">
                  <TriggerList serverId={displayServer.id} />
                </div>
              </div>
              )}

              {/* Port forwarding panel — full width (PF-6) */}
              {!isLocal && !isRemote && (
              <div className="bg-[#FBFBFB] dark:bg-[#1E1E1E] rounded-[16px] border border-gray-200/80 dark:border-white/[0.06] overflow-hidden">
                <div className="px-4 py-3 border-b border-gray-100 dark:border-white/[0.06] flex items-center justify-between">
                  <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100">
                    {t("port_forward.title")}
                  </h3>
                  <div className="flex items-center gap-2">
                    {pfRules.some((r) => r.running) && (
                      <button
                        onClick={() => portForwardRef.current?.stopAll()}
                        className="px-3 py-1.5 text-xs rounded-lg bg-gray-100 dark:bg-[#2C2C2E] text-gray-700 dark:text-gray-200 hover:bg-gray-200 dark:hover:bg-[#3A3A3C] font-medium transition-colors"
                      >
                        {t("port_forward.stop_all")}
                      </button>
                    )}
                    {pfRules.some((r) => !r.running && r.enabled) && (
                      <button
                        onClick={() => portForwardRef.current?.startAll()}
                        className="px-3 py-1.5 text-xs rounded-lg bg-gray-100 dark:bg-[#2C2C2E] text-gray-700 dark:text-gray-200 hover:bg-gray-200 dark:hover:bg-[#3A3A3C] font-medium transition-colors"
                      >
                        {t("port_forward.start_all")}
                      </button>
                    )}
                    <button
                      onClick={() =>
                        window.dispatchEvent(
                          new CustomEvent("port-forward-add", {
                            detail: { serverId: displayServer.id },
                          }),
                        )
                      }
                      className="px-3 py-1.5 text-xs rounded-lg bg-[#007AFF] text-white hover:bg-[#0066D6] font-medium transition-colors"
                    >
                      + {t("port_forward.add_rule")}
                    </button>
                  </div>
                </div>
                <div className="p-4">
                  <PortForwardPanel
                    ref={portForwardRef}
                    serverId={displayServer.id}
                    onRulesChange={setPfRules}
                  />
                </div>
              </div>
              )}

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
              serverId={displayServer.id}
              active={activeTab === tt.id}
              initialOutput={tt.initialOutput}
              rzAvailable={displayServer.rz_available}
              tabId={tt.id}
              tmuxSessionName={tt.tmuxSessionName ?? undefined}
            />
            {tt.disconnected && (
              <div className="absolute top-0 left-0 right-0 flex items-center justify-between bg-black/70 px-4 py-2 z-10 pointer-events-auto">
                <p className="text-gray-400 text-sm">
                  {t("server.terminal_disconnected")}
                </p>
                <button
                  className="px-5 py-2.5 text-sm rounded-lg bg-green-500 text-white hover:bg-green-600 font-medium shadow-sm transition-colors"
                  onClick={async () => {
                    if (!server?.id) return;
                    const serverId = displayServer.id;
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
                        const result = await ipcInvoke<{ rz_available?: boolean }>("ipc_connect_server", { serverId });
                        if (result && typeof result.rz_available === "boolean") {
                          useServerStore.getState().setRzAvailable(serverId, result.rz_available);
                        }
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
                      const result = await openTerminalWithChannel(serverId);
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
                        agentStatus: null,
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

        {/* Remote desktop terminal view — only when a remote terminal tab is active */}
        {isRemote && remotePairingId && remoteActiveTerminal !== null && isTerminalActive && (
          <div className="flex-1 min-h-0 h-full">
            <RemoteTerminalView
              key={`${remotePairingId}:${remoteActiveTerminal}`}
              pairingId={remotePairingId}
              terminalId={remoteActiveTerminal}
              terminalName={remoteTerminals.find((t) => t.terminal_id === remoteActiveTerminal)?.name || `Terminal #${remoteActiveTerminal}`}
            />
          </div>
        )}
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
