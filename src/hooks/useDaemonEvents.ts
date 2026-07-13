// useDaemonEvents — unified daemon event listener (FP-8.17)
// Listens for all daemon broadcast events and updates stores.
// CLI operations produce the same events as GUI operations — no distinction needed.

import { useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { sendNotification } from "@tauri-apps/plugin-notification";
import { useServerStore } from "@/stores/serverStore";
import { useLogStore } from "@/stores/logStore";
import { useTriggerStore } from "@/stores/triggerStore";
import { useConfigStore } from "@/stores/configStore";
import { ipcInvoke } from "@/hooks/useIpc";

/** Send a system notification via Rust backend (best-effort) */
async function notify(title: string, body: string) {
  try {
    await ipcInvoke("ipc_send_notification", { title, body });
    console.log("[notify] sent:", title, body);
  } catch (e) {
    console.error("[notify] failed:", e);
    // Fallback: try frontend plugin
    try {
      const { sendNotification } = await import("@tauri-apps/plugin-notification");
      sendNotification({ title, body });
    } catch { /* ignore */ }
  }
}

export function useDaemonEvents() {
  const updateServerStatus = useServerStore((s) => s.updateServerStatus);
  const addEntry = useLogStore((s) => s.addEntry);
  const updateTriggerExecution = useTriggerStore((s) => s.updateExecution);
  const setProxyStatus = useServerStore((s) => s.setProxyStatus);

  // Reload server list from daemon
  const reloadServers = async () => {
    try {
      const data = await ipcInvoke<{ servers: any[] }>("ipc_list_servers");
      console.log("[reloadServers] got", data?.servers?.length, "servers");
      if (data?.servers) {
        for (const s of data.servers) {
          console.log(`[reloadServers] server ${s.name} has ${s.triggers?.length || 0} triggers`);
        }
        useServerStore.setState({ servers: data.servers });
      }
    } catch (e) {
      console.error("[reloadServers] failed:", e);
    }
  };

  useEffect(() => {
    const unlistenFns: UnlistenFn[] = [];
    let mounted = true;

    const addListener = async <T>(
      event: string,
      handler: (data: T) => void
    ) => {
      const unlisten = await listen<T>(event, (e) => {
        if (mounted) handler(e.payload);
      });
      unlistenFns.push(unlisten);
    };

    // server:status_changed
    addListener<{ server_id: string; status: string; ip?: string }>(
      "server:status_changed",
      (data) => {
        updateServerStatus(
          data.server_id,
          data.status as never,
          data.ip
        );
        // System notifications based on notification preferences
        const cfg = useConfigStore.getState().config;
        const serverName = useServerStore.getState().servers.find((s) => s.id === data.server_id)?.name || data.server_id;
        if (cfg) {
          if (data.status === "connected" && cfg.general.notify_connect_success) {
            notify("VPS Guard", `Server "${serverName}" connected successfully`);
          } else if (data.status === "disconnected" && cfg.general.notify_disconnect) {
            notify("VPS Guard", `Server "${serverName}" disconnected`);
          } else if (data.status === "connected" && cfg.general.notify_reconnect_success) {
            // Could distinguish reconnect from initial connect in future
          }
        }
      }
    );

    // proxy:status_changed
    addListener<{ server_id: string; proxy_enabled: boolean }>(
      "proxy:status_changed",
      (data) => {
        setProxyStatus(data.server_id, data.proxy_enabled);
        const cfg = useConfigStore.getState().config;
        if (cfg?.general.notify_proxy_toggle) {
          const serverName = useServerStore.getState().servers.find((s) => s.id === data.server_id)?.name || data.server_id;
          notify("VPS Guard", `Proxy ${data.proxy_enabled ? "enabled" : "disabled"} on "${serverName}"`);
        }
      }
    );

    // trigger:fired
    addListener<{
      server_id: string;
      trigger_id: string;
      trigger_name: string;
      total_commands: number;
    }>("trigger:fired", (_data) => {
      // Trigger execution started — progress will be updated by command_executed events
    });

    // trigger:command_executed
    addListener<{
      server_id: string;
      trigger_id: string;
      command_index: number;
      total_commands: number;
      command: string;
      output: string;
      success: boolean;
    }>("trigger:command_executed", (data) => {
      // Find execution by trigger_id (not execution_id)
      const state = useTriggerStore.getState();
      const exec = Object.values(state.executing).find((e) => e.trigger_id === data.trigger_id);
      if (exec) {
        updateTriggerExecution(exec.execution_id, {
          executed_commands: data.command_index + 1,
          total_commands: data.total_commands,
          current_command: data.command,
        });
      }
    });

    // trigger:completed
    addListener<{
      server_id: string;
      trigger_id: string;
      trigger_name?: string;
      success: boolean;
      executed_commands: number;
      total_commands: number;
      results?: Array<{ command: string; exit_code: number; stdout: string; stderr: string }>;
    }>("trigger:completed", (data) => {
      console.log("[useDaemonEvents] trigger:completed received", data);
      // System notification based on preferences
      const cfg = useConfigStore.getState().config;
      if (cfg) {
        if (!data.success && cfg.general.notify_trigger_fail) {
          notify("VPS Guard — Trigger Failed", `Trigger "${data.trigger_name || data.trigger_id}" failed (${data.executed_commands}/${data.total_commands})`);
        } else if (data.success && cfg.general.notify_trigger_success) {
          notify("VPS Guard — Trigger Succeeded", `Trigger "${data.trigger_name || data.trigger_id}" succeeded (${data.executed_commands}/${data.total_commands})`);
        }
      }
      const state = useTriggerStore.getState();
      const exec = Object.values(state.executing).find((e) => e.trigger_id === data.trigger_id);
      if (exec) {
        updateTriggerExecution(exec.execution_id, {
          success: data.success,
          executed_commands: data.executed_commands,
          results: data.results?.map((r) => ({
            command: r.command,
            exit_code: r.exit_code,
            stdout: r.stdout,
            stderr: r.stderr,
            success: r.exit_code === 0,
          })),
        });
        // Auto-remove after 1 hour
        setTimeout(() => {
          useTriggerStore.getState().finishExecution(exec.execution_id);
        }, 3600000);
      } else {
        // Auto-triggered (OnConnect/OnReconnect) — create execution entry
        const execId = `auto-${data.trigger_id}-${Date.now()}`;
        useTriggerStore.getState().startExecution({
          execution_id: execId,
          server_id: data.server_id,
          trigger_id: data.trigger_id,
          trigger_name: data.trigger_name || data.trigger_id,
          total_commands: data.total_commands,
          executed_commands: data.executed_commands,
          current_command: null,
          success: data.success,
          results: data.results?.map((r) => ({
            command: r.command,
            exit_code: r.exit_code,
            stdout: r.stdout,
            stderr: r.stderr,
            success: r.exit_code === 0,
          })),
        });
        setTimeout(() => {
          useTriggerStore.getState().finishExecution(execId);
        }, 3600000);
      }
    });

    // log:entry
    addListener<{
      server_id: string;
      level: string;
      kind: string;
      message: string;
      timestamp: string;
    }>("log:entry", (data) => {
      addEntry({
        id: `${Date.now()}-${Math.random().toString(36).slice(2)}`,
        server_id: data.server_id,
        level: data.level as never,
        category: data.kind as never,
        message: data.message,
        timestamp: data.timestamp,
        execution_id: null,
        command: null,
        exit_code: null,
        stdout: null,
        stderr: null,
      });
    });

    // daemon:shutdown
    addListener<unknown>("daemon:shutdown", () => {
      // Handle daemon shutdown — could show a notification
    });

    // server:added / server:removed / server:reordered — reload server list from daemon
    addListener<{ server_id: string }>("server:added", () => {
      reloadServers();
    });
    addListener<{ server_id: string }>("server:removed", () => {
      reloadServers();
    });
    addListener<{ server_ids: string[] }>("server:reordered", () => {
      reloadServers();
    });

    // cli:focus — CLI operations focus a server and/or tab in the GUI
    addListener<{ server_id: string; tab?: string }>("cli:focus", (data) => {
      useServerStore.getState().selectServer(data.server_id);
      if (data.tab) {
        useServerStore.getState().setActiveTab(data.tab);
      }
    });



    // trigger:added / trigger:removed — reload server list to sync triggers
    addListener<{ server_id: string }>("trigger:added", (data) => {
      console.log("[useDaemonEvents] trigger:added received", data);
      reloadServers();
    });
    addListener<{ server_id: string }>("trigger:removed", () => {
      reloadServers();
    });

    // ssh:hostkey_mismatch (§17.2: triple notification — system notification + tray red + log highlight)
    addListener<{
      server_id: string;
      expected: string;
      actual: string;
    }>("ssh:hostkey_mismatch", (data) => {
      // 1. Log highlight — add an error log entry
      addEntry({
        id: `hostkey-${Date.now()}-${Math.random().toString(36).slice(2)}`,
        server_id: data.server_id,
        level: "error" as never,
        category: "ssh" as never,
        message: `HostKey mismatch! Expected ${data.expected}, got ${data.actual}`,
        timestamp: new Date().toISOString(),
        execution_id: null,
        command: null,
        exit_code: null,
        stdout: null,
        stderr: null,
      });
      // 2. System notification (via Tauri notification plugin)
      // 3. Tray icon red — handled by GlobalIndicator listening to this event
      const cfg = useConfigStore.getState().config;
      if (cfg?.general.notify_auth_fail) {
        notify("VPS Guard — Security Alert", `HostKey mismatch on server ${data.server_id}. Possible MITM attack!`);
      }
    });

    // network:offline / network:online (FP-6.9)
    addListener<{ servers_to_reconnect?: string[] }>("network:offline", () => {
      addEntry({
        id: `net-offline-${Date.now()}`,
        server_id: "",
        level: "warn" as never,
        category: "network" as never,
        message: "Network went offline — reconnections paused",
        timestamp: new Date().toISOString(),
        execution_id: null,
        command: null,
        exit_code: null,
        stdout: null,
        stderr: null,
      });
    });

    addListener<{ reconnected_servers?: string[] }>("network:online", () => {
      addEntry({
        id: `net-online-${Date.now()}`,
        server_id: "",
        level: "info" as never,
        category: "network" as never,
        message: "Network back online — reconnecting servers",
        timestamp: new Date().toISOString(),
        execution_id: null,
        command: null,
        exit_code: null,
        stdout: null,
        stderr: null,
      });
    });

    // Poll active_channels every 2s when any proxy is running
    const pollInterval = setInterval(async () => {
      if (!mounted) return;
      const servers = useServerStore.getState().servers;
      const anyProxyRunning = servers.some((s) => s.proxy_running);
      if (anyProxyRunning) {
        try {
          const data = await ipcInvoke<{ servers: any[] }>("ipc_list_servers");
          if (data?.servers && mounted) {
            useServerStore.setState({ servers: data.servers });
          }
        } catch { /* ignore */ }
      }
    }, 2000);

    return () => {
      mounted = false;
      clearInterval(pollInterval);
      unlistenFns.forEach((fn) => fn());
    };
  }, [updateServerStatus, addEntry, updateTriggerExecution, setProxyStatus]);
}
