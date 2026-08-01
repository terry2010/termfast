// TabTriggerManager — per-terminal trigger management dialog
// Shows all triggers for the server with per-session exec_in_terminal toggles.
// Changes are runtime-only (not persisted) and only affect this terminal.

import { useState, useEffect, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useTriggerStore } from "@/stores/triggerStore";
import { ipcInvoke } from "@/hooks/useIpc";
import type { TriggerInstance, TriggerType } from "@/types";

const EVENT_TYPE_COLORS: Record<TriggerType, string> = {
  OnConnect: "bg-green-100 text-green-700 dark:bg-green-900 dark:text-green-300",
  OnReconnect: "bg-blue-100 text-blue-700 dark:bg-blue-900 dark:text-blue-300",
  OnIpChange: "bg-purple-100 text-purple-700 dark:bg-purple-900 dark:text-purple-300",
  OnProcessDead: "bg-orange-100 text-orange-700 dark:bg-orange-900 dark:text-orange-300",
  OnPortClosed: "bg-yellow-100 text-yellow-700 dark:bg-yellow-900 dark:text-yellow-300",
  OnTerminalOpen: "bg-green-100 text-green-700 dark:bg-green-900 dark:text-green-300",
  BeforeTerminalClose: "bg-orange-100 text-orange-700 dark:bg-orange-900 dark:text-orange-300",
  OnNetworkDisconnect: "bg-red-100 text-red-700 dark:bg-red-900 dark:text-red-300",
  OnNetworkConnect: "bg-green-100 text-green-700 dark:bg-green-900 dark:text-green-300",
  OnLanIpChange: "bg-purple-100 text-purple-700 dark:bg-purple-900 dark:text-purple-300",
  OnPublicIpChange: "bg-teal-100 text-teal-700 dark:bg-teal-900 dark:text-teal-300",
  OnInterval: "bg-indigo-100 text-indigo-700 dark:bg-indigo-900 dark:text-indigo-300",
  OnSchedule: "bg-violet-100 text-violet-700 dark:bg-violet-900 dark:text-violet-300",
  ManualFire: "bg-gray-100 text-gray-700 dark:bg-[#2C2C2E] dark:text-gray-300",
};

export function TabTriggerManager({
  serverId,
  sessionId,
  onClose,
}: {
  serverId: string;
  sessionId: string;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const triggers = useTriggerStore(
    (s) => s.serverTriggers[serverId] || [],
  );
  const templates = useTriggerStore((s) => s.templates);
  const [overrides, setOverrides] = useState<Record<string, boolean>>({});
  const [showTooltipFor, setShowTooltipFor] = useState<string | null>(null);

  // Load current per-session overrides from daemon
  useEffect(() => {
    ipcInvoke<{ overrides: Record<string, boolean> }>(
      "ipc_get_trigger_overrides",
      { sessionId },
    )
      .then((data) => {
        if (data?.overrides) setOverrides(data.overrides);
      })
      .catch(() => {});
  }, [sessionId]);

  // ESC to close
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  const handleToggle = useCallback(
    (triggerId: string, configValue: boolean) => {
      const newValue = !(triggerId in overrides ? overrides[triggerId] : configValue);
      const next = { ...overrides, [triggerId]: newValue };
      setOverrides(next);
      // Persist to daemon (per-session, not persisted to config)
      ipcInvoke("ipc_set_trigger_overrides", {
        sessionId,
        overrides: next,
      }).catch(() => {});
    },
    [overrides, sessionId],
  );

  return (
    <div
      className="fixed inset-0 z-[200] flex items-center justify-center bg-black/30"
      onClick={(e) => {
        e.stopPropagation();
        onClose();
      }}
    >
      <div
        className="w-full max-w-2xl max-h-[80vh] bg-white dark:bg-[#1E1E1E] rounded-2xl shadow-2xl border border-gray-200/80 dark:border-white/[0.06] flex flex-col overflow-hidden"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="px-5 py-4 border-b border-gray-100 dark:border-white/[0.06] flex items-center justify-between">
          <h3 className="text-base font-semibold text-gray-900 dark:text-gray-100">
            {t("tab.manage_triggers")}
          </h3>
          <button
            className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 text-lg px-2"
            onClick={onClose}
          >
            ✕
          </button>
        </div>

        {/* Hint */}
        <div className="px-5 py-2 text-xs text-gray-400 dark:text-gray-500 border-b border-gray-100 dark:border-white/[0.06]">
          {t("trigger.per_terminal_hint")}
        </div>

        {/* Trigger list */}
        <div className="flex-1 overflow-y-auto">
          {triggers.length === 0 ? (
            <div className="py-12 text-center text-sm text-gray-400">
              {t("trigger.empty_title")}
            </div>
          ) : (
            triggers.map((trigger) => {
              const triggerType =
                trigger.trigger_type ||
                templates.find((tpl) => tpl.id === trigger.template_id)?.type ||
                "ManualFire";
              const configValue = trigger.exec_in_terminal;
              const currentValue =
                trigger.id in overrides ? overrides[trigger.id] : configValue;
              return (
                <TriggerRow
                  key={trigger.id}
                  trigger={trigger}
                  triggerType={triggerType}
                  currentValue={currentValue}
                  overridden={trigger.id in overrides}
                  showTooltip={showTooltipFor === trigger.id}
                  onHover={(hovering) =>
                    setShowTooltipFor(hovering ? trigger.id : null)
                  }
                  onToggle={() => handleToggle(trigger.id, configValue)}
                  t={t}
                />
              );
            })
          )}
        </div>

        {/* Footer */}
        <div className="px-5 py-3 border-t border-gray-100 dark:border-white/[0.06] flex justify-end">
          <button
            className="text-xs px-4 py-2 rounded-lg bg-gray-100 dark:bg-[#2C2C2E] text-gray-700 dark:text-gray-200 hover:bg-gray-200 dark:hover:bg-[#3A3A3C] transition-colors font-medium"
            onClick={onClose}
          >
            {t("common.done")}
          </button>
        </div>
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

function TriggerRow({
  trigger,
  triggerType,
  currentValue,
  overridden,
  showTooltip,
  onHover,
  onToggle,
  t,
}: {
  trigger: TriggerInstance;
  triggerType: TriggerType;
  currentValue: boolean;
  overridden: boolean;
  showTooltip: boolean;
  onHover: (hovering: boolean) => void;
  onToggle: () => void;
  t: (key: string) => string;
}) {
  const infoRef = useRef<HTMLDivElement>(null);
  return (
    <div
      className="flex items-center gap-3 px-5 py-3 border-b border-gray-100 dark:border-white/[0.06] last:border-0 hover:bg-[#FBFBFB] dark:hover:bg-[#2C2C2E]/20 transition-colors"
      onMouseEnter={() => onHover(true)}
      onMouseLeave={() => onHover(false)}
    >
      <Toggle checked={currentValue} onChange={onToggle} />
      <div ref={infoRef} className="min-w-0 flex-1">
        <div className="flex items-center gap-2 flex-wrap">
          <span
            className={`text-[10px] px-1.5 py-0.5 rounded-full font-medium ${EVENT_TYPE_COLORS[triggerType]}`}
          >
            {t(`trigger.event_types.${triggerType}`)}
          </span>
          <span className="text-sm font-semibold text-gray-900 dark:text-gray-100">
            {trigger.name}
          </span>
          {overridden && (
            <span className="text-[10px] text-blue-500">
              ({t("trigger.overridden")})
            </span>
          )}
        </div>
      </div>
      {showTooltip && trigger.commands.length > 0 && infoRef.current && (() => {
        const rect = infoRef.current.getBoundingClientRect();
        return (
          <div
            className="fixed z-[200] max-w-sm p-2 rounded-lg bg-gray-900 dark:bg-[#2C2C2E] text-xs text-gray-100 dark:text-gray-200 shadow-lg border border-gray-700 dark:border-white/[0.1] font-mono whitespace-pre-wrap break-all"
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
    </div>
  );
}
