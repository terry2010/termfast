// TriggerList — trigger cards (§6.5 / FP-8.4)
// Type tag + exec_in_terminal toggle + run/edit buttons

import { useState, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useTriggerStore } from "@/stores/triggerStore";
import { useServerStore } from "@/stores/serverStore";
import type { TriggerExecution, CommandResult } from "@/stores/triggerStore";
import { ipcInvoke, formatIpcError } from "@/hooks/useIpc";
import { toast } from "@/components/ui/toast";
import { TriggerEditor } from "@/components/shared/TriggerEditor";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import type { TriggerInstance, TriggerType } from "@/types";

const EVENT_TYPE_COLORS: Record<TriggerType, string> = {
  OnConnect:
    "bg-green-100 text-green-700 dark:bg-green-900 dark:text-green-300",
  OnReconnect: "bg-blue-100 text-blue-700 dark:bg-blue-900 dark:text-blue-300",
  OnIpChange:
    "bg-purple-100 text-purple-700 dark:bg-purple-900 dark:text-purple-300",
  OnProcessDead:
    "bg-orange-100 text-orange-700 dark:bg-orange-900 dark:text-orange-300",
  OnPortClosed:
    "bg-yellow-100 text-yellow-700 dark:bg-yellow-900 dark:text-yellow-300",
  OnTerminalOpen:
    "bg-green-100 text-green-700 dark:bg-green-900 dark:text-green-300",
  BeforeTerminalClose:
    "bg-orange-100 text-orange-700 dark:bg-orange-900 dark:text-orange-300",
  OnNetworkDisconnect:
    "bg-red-100 text-red-700 dark:bg-red-900 dark:text-red-300",
  OnNetworkConnect:
    "bg-green-100 text-green-700 dark:bg-green-900 dark:text-green-300",
  OnLanIpChange:
    "bg-purple-100 text-purple-700 dark:bg-purple-900 dark:text-purple-300",
  OnPublicIpChange:
    "bg-teal-100 text-teal-700 dark:bg-teal-900 dark:text-teal-300",
  OnInterval:
    "bg-indigo-100 text-indigo-700 dark:bg-indigo-900 dark:text-indigo-300",
  OnSchedule:
    "bg-violet-100 text-violet-700 dark:bg-violet-900 dark:text-violet-300",
  ManualFire: "bg-gray-100 text-gray-700 dark:bg-[#2C2C2E] dark:text-gray-300",
};

const EMPTY_TRIGGERS: TriggerInstance[] = [];

export function TriggerList({ serverId }: { serverId: string }) {
  const { t } = useTranslation();
  const triggers = useTriggerStore(
    (s) => s.serverTriggers[serverId] || EMPTY_TRIGGERS,
  );
  const setServerTriggers = useTriggerStore((s) => s.setServerTriggers);
  const executing = useTriggerStore((s) => s.executing);
  const templates = useTriggerStore((s) => s.templates);
  const servers = useServerStore((s) => s.servers);
  const [editingTrigger, setEditingTrigger] = useState<
    TriggerInstance | null | undefined
  >(undefined);
  const serverStatus = servers.find((s) => s.id === serverId)?.current_status;

  // Load triggers from server config — only update if server has triggers data
  useEffect(() => {
    const server = servers.find((s) => s.id === serverId);
    if (server?.triggers && server.triggers.length > 0) {
      setServerTriggers(serverId, server.triggers);
    }
  }, [serverId, servers, setServerTriggers]);

  // Listen for "trigger-add" event from the panel header button
  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.serverId === serverId) {
        setEditingTrigger(null);
      }
    };
    window.addEventListener("trigger-add", handler);
    return () => window.removeEventListener("trigger-add", handler);
  }, [serverId]);

  // Fetch triggers directly via IPC on mount and when serverId changes
  useEffect(() => {
    const isLocal = serverId === "__local__";
    const ipcName = isLocal ? "ipc_list_local_triggers" : "ipc_list_triggers";
    const ipcParams = isLocal ? {} : { server_id: serverId };
    ipcInvoke<TriggerInstance[]>(ipcName, ipcParams)
      .then((triggers) => {
        if (Array.isArray(triggers)) {
          setServerTriggers(serverId, triggers);
        }
      })
      .catch((e) =>
        console.warn(`[TriggerList] ${ipcName} failed:`, String(e)),
      );
  }, [serverId, setServerTriggers]);

  const handleSaved = () => {
    const isLocal = serverId === "__local__";
    const ipcName = isLocal ? "ipc_list_local_triggers" : "ipc_list_triggers";
    const ipcParams = isLocal ? {} : { server_id: serverId };
    ipcInvoke<TriggerInstance[]>(ipcName, ipcParams)
      .then((triggers) => {
        if (Array.isArray(triggers)) {
          setServerTriggers(serverId, triggers);
        }
        if (!isLocal) {
          ipcInvoke<{ servers: any[] }>("ipc_list_servers")
            .then((data) => {
              if (data?.servers) {
                useServerStore.setState({ servers: data.servers });
              }
            })
            .catch(() => {});
        }
      })
      .catch(() => {});
  };

  return (
    <div>
      {editingTrigger !== undefined && (
        <TriggerEditor
          serverId={serverId}
          trigger={editingTrigger}
          onClose={() => setEditingTrigger(undefined)}
          onSaved={handleSaved}
        />
      )}
      {triggers.length === 0 ? (
        <div className="py-8 text-center">
          <div className="w-10 h-10 rounded-full bg-gray-100 dark:bg-[#2C2C2E]/50 flex items-center justify-center text-gray-400 mx-auto mb-3">
            <svg
              width="18"
              height="18"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <polyline points="13 17 18 12 13 7" />
              <polyline points="6 17 11 12 6 7" />
            </svg>
          </div>
          <div className="text-sm text-gray-500 dark:text-gray-400 mb-2">
            {t("trigger.empty_title")}
          </div>
          <div className="text-xs text-gray-400 dark:text-gray-500 max-w-md mx-auto leading-relaxed">
            {t("trigger.empty_description")}
          </div>
        </div>
      ) : (
        <div className="bg-white dark:bg-[#1E1E1E] rounded-xl border border-gray-200/80 dark:border-white/[0.06] overflow-hidden">
          {triggers.map((trigger) => (
            <TriggerCard
              key={trigger.id}
              trigger={trigger}
              executing={Object.values(executing).find(
                (e) => e.trigger_id === trigger.id,
              )}
              serverId={serverId}
              triggerType={
                trigger.trigger_type ||
                templates.find((tpl) => tpl.id === trigger.template_id)?.type ||
                "ManualFire"
              }
              builtIn={
                templates.find((tpl) => tpl.id === trigger.template_id)
                  ?.built_in || false
              }
              onEdit={() => setEditingTrigger(trigger)}
              connected={serverStatus === "connected"}
            />
          ))}
        </div>
      )}
    </div>
  );
}

// === SECTION 1 END ===

function TriggerCard({
  trigger,
  executing,
  serverId,
  triggerType,
  builtIn,
  onEdit,
  connected,
}: {
  trigger: TriggerInstance;
  executing?: TriggerExecution;
  serverId: string;
  triggerType: TriggerType;
  builtIn: boolean;
  onEdit: () => void;
  connected: boolean;
}) {
  const { t } = useTranslation();
  const startExecution = useTriggerStore((s) => s.startExecution);
  const finishExecution = useTriggerStore((s) => s.finishExecution);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [showTooltip, setShowTooltip] = useState(false);
  const infoRef = useRef<HTMLDivElement>(null);
  // The list toggle reflects bind_new_terminals (persisted config: whether
  // this trigger auto-injects into new terminals).
  const effectiveBindNewTerminals = trigger.bind_new_terminals;

  const handleToggleBindNewTerminals = async (v: boolean) => {
    try {
      const isLocal = serverId === "__local__";
      if (isLocal) {
        const updated = { ...trigger, bind_new_terminals: v };
        await ipcInvoke("ipc_save_local_trigger", { trigger: updated });
      } else {
        await ipcInvoke("ipc_update_trigger", {
          params: {
            server_id: serverId,
            trigger_id: trigger.id,
            name: trigger.name,
            trigger_type: trigger.trigger_type,
            enabled: trigger.enabled,
            timeout_secs: trigger.timeout_secs,
            cooldown_secs: trigger.cooldown_secs,
            continue_on_error: trigger.continue_on_error,
            notify_on_success: trigger.notify_on_success,
            notify_on_failure: trigger.notify_on_failure,
            commands: trigger.commands,
            exec_in_terminal: trigger.exec_in_terminal,
            bind_new_terminals: v,
            interval_secs: trigger.interval_secs,
            schedule_mode: trigger.schedule_mode,
            cron_expr: trigger.cron_expr,
            scheduled_at: trigger.scheduled_at,
          },
        });
      }
      // Refresh trigger list
      const ipcName = isLocal ? "ipc_list_local_triggers" : "ipc_list_triggers";
      const ipcParams = isLocal ? {} : { server_id: serverId };
      const triggers = await ipcInvoke<TriggerInstance[]>(ipcName, ipcParams);
      if (Array.isArray(triggers)) {
        useTriggerStore.getState().setServerTriggers(serverId, triggers);
      }
    } catch (e) {
      console.error("toggle bind_new_terminals failed:", e);
      toast.error(formatIpcError(e));
    }
  };

  const handleDelete = async () => {
    try {
      if (serverId === "__local__") {
        await ipcInvoke("ipc_remove_local_trigger", {
          trigger_id: trigger.id,
        });
      } else {
        await ipcInvoke("ipc_remove_trigger", {
          params: {
            server_id: serverId,
            trigger_id: trigger.id,
          },
        });
      }
      // Refresh trigger list after successful deletion
      const isLocal = serverId === "__local__";
      const ipcName = isLocal ? "ipc_list_local_triggers" : "ipc_list_triggers";
      const ipcParams = isLocal ? {} : { server_id: serverId };
      const triggers = await ipcInvoke<TriggerInstance[]>(ipcName, ipcParams);
      if (Array.isArray(triggers)) {
        useTriggerStore.getState().setServerTriggers(serverId, triggers);
      }
    } catch (e) {
      console.error("delete trigger failed:", e);
      toast.error(formatIpcError(e));
    }
    setConfirmDelete(false);
  };

  const handleFire = async () => {
    const execId = `${trigger.id}-${Date.now()}`;
    startExecution({
      execution_id: execId,
      server_id: serverId,
      trigger_id: trigger.id,
      trigger_name: trigger.name,
      total_commands: trigger.commands.length,
      executed_commands: 0,
      current_command: null,
      success: null,
    });
    try {
      const isLocal = serverId === "__local__";
      const result = await ipcInvoke<{
        success: boolean;
        executed_commands: number;
        total_commands: number;
        results?: CommandResult[];
      }>(isLocal ? "ipc_manual_fire_local_trigger" : "ipc_manual_fire_trigger",
        isLocal
          ? { trigger_id: trigger.id }
          : { serverId, triggerId: trigger.id },
      );
      useTriggerStore.getState().updateExecution(execId, {
        success: result.success,
        executed_commands: result.executed_commands,
        results: result.results,
      });
    } catch (e) {
      console.error("fire trigger failed:", e);
      useTriggerStore.getState().updateExecution(execId, {
        success: false,
        results: [
          {
            command: "error",
            exit_code: -1,
            stdout: "",
            stderr: formatIpcError(e),
            success: false,
          },
        ],
      });
    }
  };

  return (
    <div className="border-b border-gray-100 dark:border-white/[0.06] last:border-0 hover:bg-[#FBFBFB] dark:hover:bg-[#2C2C2E]/20 transition-colors">
      <div className="flex items-center gap-3 px-4 py-3.5">
        {/* Left: bind_new_terminals toggle (persisted config) */}
        <div
          className="flex-shrink-0"
          title={t("trigger.bind_new_terminals_tooltip")}
        >
          <Toggle
            checked={effectiveBindNewTerminals}
            onChange={handleToggleBindNewTerminals}
          />
        </div>

        {/* Center: trigger info (no command display — hover tooltip instead) */}
        <div
          ref={infoRef}
          className="min-w-0 flex-1"
          onMouseEnter={() => setShowTooltip(true)}
          onMouseLeave={() => setShowTooltip(false)}
        >
          <div className="flex items-center gap-2 flex-wrap">
            <span
              className={`text-[10px] px-1.5 py-0.5 rounded-full font-medium ${EVENT_TYPE_COLORS[triggerType]}`}
            >
              {t(`trigger.event_types.${triggerType}`)}
            </span>
            <span className="text-sm font-semibold text-gray-900 dark:text-gray-100">
              {trigger.name}
            </span>
            {builtIn && (
              <span className="text-[10px] text-gray-400">
                {t("trigger.built_in")}
              </span>
            )}
          </div>
        </div>
        {/* Hover tooltip — rendered as fixed to escape overflow-hidden containers */}
        {showTooltip && trigger.commands.length > 0 && infoRef.current && (() => {
          const rect = infoRef.current.getBoundingClientRect();
          return (
            <div
              className="fixed z-[200] max-w-md p-2 rounded-lg bg-gray-900 dark:bg-[#2C2C2E] text-xs text-gray-100 dark:text-gray-200 shadow-lg border border-gray-700 dark:border-white/[0.1] font-mono whitespace-pre-wrap break-all"
              style={{ top: rect.bottom + 4, left: rect.left }}
            >
              {trigger.commands.map((cmd, i) => (
                <div key={i} className={i > 0 ? "mt-1" : ""}>
                  <span className="text-green-400">$</span> {cmd}
                </div>
              ))}
            </div>
          );
        })()}

        {/* Right: action buttons */}
        <div className="flex items-center gap-1 flex-shrink-0">
          <button
            className="text-xs px-2.5 py-1.5 rounded-lg bg-blue-500 text-white hover:bg-blue-600 disabled:opacity-40 disabled:cursor-not-allowed font-medium transition-colors"
            onClick={handleFire}
            disabled={!connected && serverId !== "__local__"}
            title={!connected && serverId !== "__local__" ? t("server.connect_first") : undefined}
          >
            ▶ {t("trigger.fire")}
          </button>
          <button
            className="text-xs px-2.5 py-1.5 rounded-lg hover:bg-gray-100 dark:hover:bg-[#2C2C2E] text-gray-600 dark:text-gray-300 transition-colors"
            onClick={onEdit}
          >
            {t("common.edit")}
          </button>
          <button
            className="text-xs px-2.5 py-1.5 rounded-lg text-red-500 hover:bg-red-50 dark:hover:bg-red-900/30 transition-colors"
            onClick={() => setConfirmDelete(true)}
          >
            {t("common.delete")}
          </button>
        </div>
      </div>
      {executing && (
        <div className="px-4 pb-3.5">
          <div className="h-1 bg-gray-200 dark:bg-[#2C2C2E] rounded-full overflow-hidden mb-2">
            <div
              className={`h-full transition-all ${executing.success === false ? "bg-red-500" : "bg-blue-500"}`}
              style={{
                width: `${(executing.executed_commands / executing.total_commands) * 100}%`,
              }}
            />
          </div>
          {/* Execution results — close button has large clickable area (full right side) */}
          <div className="text-xs text-gray-500 flex items-center justify-between mb-2">
            <span>
              {executing.executed_commands}/{executing.total_commands}
            </span>
            <div className="flex items-center gap-2">
              {executing.success === true && (
                <span className="text-green-500">✓ {t("common.success")}</span>
              )}
              {executing.success === false && (
                <span className="text-red-500">✗ {t("common.failed")}</span>
              )}
              <button
                className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 text-xs px-3 py-1 -mr-3 rounded-lg hover:bg-gray-100 dark:hover:bg-[#2C2C2E] transition-colors"
                onClick={() => finishExecution(executing.execution_id)}
                title={t("common.close")}
              >
                ✕
              </button>
            </div>
          </div>
          {executing.results && executing.results.length > 0 && (
            <div className="mt-2 space-y-1 max-h-40 overflow-y-auto bg-[#FBFBFB] dark:bg-[#1E1E1E] rounded-lg p-2 font-mono text-xs">
              {executing.results.map((r, i) => (
                <div key={i}>
                  <div className="text-gray-600 dark:text-gray-400">
                    <span
                      className={r.success ? "text-green-500" : "text-red-500"}
                    >
                      $
                    </span>{" "}
                    {r.command}
                  </div>
                  {r.stdout && (
                    <pre className="whitespace-pre-wrap text-gray-700 dark:text-gray-300 mt-0.5">
                      {r.stdout}
                    </pre>
                  )}
                  {r.stderr && (
                    <pre className="whitespace-pre-wrap text-red-500 mt-0.5">
                      {r.stderr}
                    </pre>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      )}
      {confirmDelete && (
        <ConfirmDialog
          level="medium"
          title={t("trigger.delete_title")}
          message={t("trigger.delete_message", { name: trigger.name })}
          onConfirm={handleDelete}
          onCancel={() => setConfirmDelete(false)}
        />
      )}
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
      className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors duration-200 ${
        checked ? "bg-blue-500" : "bg-gray-200 dark:bg-gray-600"
      }`}
    >
      <span
        className="inline-block h-4 w-4 rounded-full bg-white shadow-sm transition-transform duration-200"
        style={{ transform: checked ? "translateX(18px)" : "translateX(2px)" }}
      />
    </button>
  );
}

