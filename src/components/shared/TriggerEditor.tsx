// TriggerEditor — trigger edit dialog with CodeMirror 6 (§6.5 / FP-8.5)
// Shell script editor with syntax highlighting, timeout/cooldown settings

import { useState, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap, lineNumbers } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { oneDark } from "@codemirror/theme-one-dark";
import { StreamLanguage } from "@codemirror/language";
import { shell } from "@codemirror/legacy-modes/mode/shell";
import { ipcInvoke, formatIpcError } from "@/hooks/useIpc";
import { Modal } from "@/components/ui/Modal";
import { useTriggerStore } from "@/stores/triggerStore";
import type { TriggerInstance, TriggerType, TriggerTemplate } from "@/types";

interface TriggerEditorProps {
  serverId: string;
  trigger: TriggerInstance | null; // null = creating new
  onClose: () => void;
  onSaved?: () => void;
}

// === SECTION 1 END ===

export function TriggerEditor({
  serverId,
  trigger,
  onClose,
  onSaved,
}: TriggerEditorProps) {
  const { t } = useTranslation();
  const editorRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const [name, setName] = useState(trigger?.name || "");
  const [eventType, setEventType] = useState<TriggerType>(
    (trigger as any)?.trigger_type || "ManualFire",
  );
  const [timeoutSecs, setTimeoutSecs] = useState(trigger?.timeout_secs || 1);
  const [cooldownSecs, setCooldownSecs] = useState(
    trigger?.cooldown_secs || 1,
  );
  const [continueOnError, setContinueOnError] = useState(
    trigger?.continue_on_error || false,
  );
  const [notifyOnSuccess, setNotifyOnSuccess] = useState(
    trigger?.notify_on_success || false,
  );
  const [notifyOnFailure, setNotifyOnFailure] = useState<boolean>(
    trigger?.notify_on_failure ?? true,
  );
  const [execInTerminal, setExecInTerminal] = useState<boolean>(
    trigger?.exec_in_terminal ?? false,
  );
  // Interval: stored as seconds in config, displayed as days+hours+minutes+seconds
  const rawInterval = trigger?.interval_secs || 300;
  const [intervalDays, setIntervalDays] = useState<number>(() => Math.floor(rawInterval / 86400));
  const [intervalHours, setIntervalHours] = useState<number>(() => Math.floor((rawInterval % 86400) / 3600));
  const [intervalMinutes, setIntervalMinutes] = useState<number>(() => Math.floor((rawInterval % 3600) / 60));
  const [intervalSeconds, setIntervalSeconds] = useState<number>(() => rawInterval % 60);
  const intervalSecs =
    intervalDays * 86400 +
    intervalHours * 3600 +
    intervalMinutes * 60 +
    intervalSeconds;

  // OnSchedule state
  const [scheduleMode, setScheduleMode] = useState<string>(
    trigger?.schedule_mode || "cron",
  );
  // Cron builder state: frequency + time + day-of-week + day-of-month
  const [cronFreq, setCronFreq] = useState<string>(() => {
    const expr = trigger?.cron_expr || "";
    if (!expr) return "daily";
    // Try to parse common patterns
    if (/^0 \d+ \* \* \*$/.test(expr)) return "daily";
    if (/^0 \d+ \* \* \d+$/.test(expr)) return "weekly";
    if (/^0 \d+ \d+ \* \*$/.test(expr)) return "monthly";
    return "custom";
  });
  const [cronHour, setCronHour] = useState<number>(() => {
    const expr = trigger?.cron_expr || "";
    const m = expr.match(/^\d+ (\d+) /);
    return m ? parseInt(m[1]) : 3;
  });
  const [cronMinute, setCronMinute] = useState<number>(() => {
    const expr = trigger?.cron_expr || "";
    const m = expr.match(/^(\d+) /);
    return m ? parseInt(m[1]) : 0;
  });
  const [cronDow, setCronDow] = useState<number>(() => {
    const expr = trigger?.cron_expr || "";
    const m = expr.match(/^0 \d+ \* \* (\d+)$/);
    return m ? parseInt(m[1]) : 1;
  });
  const [cronDom, setCronDom] = useState<number>(() => {
    const expr = trigger?.cron_expr || "";
    const m = expr.match(/^0 \d+ (\d+) \* \*$/);
    return m ? parseInt(m[1]) : 1;
  });
  const [cronCustom, setCronCustom] = useState<string>(() => {
    const expr = trigger?.cron_expr || "";
    if (/^0 \d+ \* \* \*$/.test(expr)) return "";
    if (/^0 \d+ \* \* \d+$/.test(expr)) return "";
    if (/^0 \d+ \d+ \* \*$/.test(expr)) return "";
    return expr;
  });
  // Build cron expression from builder fields
  const cronExpr = (() => {
    if (cronFreq === "custom") return cronCustom;
    if (cronFreq === "daily") return `${cronMinute} ${cronHour} * * *`;
    if (cronFreq === "weekly") return `${cronMinute} ${cronHour} * * ${cronDow}`;
    if (cronFreq === "monthly") return `${cronMinute} ${cronHour} ${cronDom} * *`;
    return cronCustom;
  })();
  // One-time task
  const [scheduledAt, setScheduledAt] = useState<string>(
    trigger?.scheduled_at || "",
  );

  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showTemplateSelector, setShowTemplateSelector] = useState(false);

  const templates = useTriggerStore((s) => s.templates);
  const isEditing = !!trigger;
  const commandsText = trigger?.commands.join("\n") || "";

  // ESC to close
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  // Initialize CodeMirror editor
  useEffect(() => {
    if (!editorRef.current) return;

    const isDark = document.documentElement.classList.contains("dark");
    const extensions = [
      history(),
      keymap.of([...defaultKeymap, ...historyKeymap]),
      lineNumbers(),
      StreamLanguage.define(shell),
      EditorView.lineWrapping,
      EditorState.tabSize.of(2),
      ...(isDark ? [oneDark] : []),
    ];

    const state = EditorState.create({
      doc: commandsText,
      extensions,
    });

    const view = new EditorView({
      state,
      parent: editorRef.current,
    });
    viewRef.current = view;

    return () => {
      view.destroy();
      viewRef.current = null;
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Apply a template's values to the editor form.
  const applyTemplate = (tpl: TriggerTemplate) => {
    setName(tpl.name);
    setEventType(tpl.type as TriggerType);
    setTimeoutSecs(tpl.timeout_secs ?? 30);
    const newCommands = tpl.commands.join("\n");
    if (viewRef.current) {
      const doc = viewRef.current.state.doc;
      viewRef.current.dispatch({
        changes: { from: 0, to: doc.length, insert: newCommands },
      });
    }
    setShowTemplateSelector(false);
    setError(null);
  };

  const handleSave = async () => {
    if (!name.trim()) {
      setError(t("trigger.name_required"));
      return;
    }

    const commands = (viewRef.current?.state.doc.toString() || "")
      .split("\n")
      .map((c) => c.trim())
      .filter((c) => c.length > 0);

    if (commands.length === 0) {
      setError(t("trigger.commands_required"));
      return;
    }

    setSaving(true);
    setError(null);

    const isLocal = serverId === "__local__";
    try {
      if (isLocal) {
        // 本地触发器：使用 ipc_save_local_trigger（add + update 共用）
        await ipcInvoke("ipc_save_local_trigger", {
          trigger: {
            id: isEditing && trigger ? trigger.id : `trig_${Date.now()}`,
            template_id: "",
            name,
            trigger_type: eventType,
            enabled: isEditing && trigger ? trigger.enabled : true,
            continue_on_error: continueOnError,
            commands,
            parameters: {},
            timeout_secs: timeoutSecs,
            cooldown_secs: cooldownSecs,
            notify_on_success: notifyOnSuccess,
            notify_on_failure: notifyOnFailure,
            last_fired_at: trigger?.last_fired_at ?? null,
            template_hash_at_addition: "",
            exec_in_terminal: execInTerminal,
            interval_secs: intervalSecs,
            schedule_mode: scheduleMode,
            cron_expr: cronExpr,
            scheduled_at: scheduledAt,
          },
        });
      } else if (isEditing && trigger) {
        // Update existing trigger
        await ipcInvoke("ipc_update_trigger", {
          params: {
            server_id: serverId,
            trigger_id: trigger.id,
            name,
            trigger_type: eventType,
            enabled: trigger.enabled,
            timeout_secs: timeoutSecs,
            cooldown_secs: cooldownSecs,
            continue_on_error: continueOnError,
            notify_on_success: notifyOnSuccess,
            notify_on_failure: notifyOnFailure,
            commands,
            exec_in_terminal: execInTerminal,
            interval_secs: intervalSecs,
            schedule_mode: scheduleMode,
            cron_expr: cronExpr,
            scheduled_at: scheduledAt,
          },
        });
      } else {
        // Create new trigger
        await ipcInvoke("ipc_add_trigger", {
          server_id: serverId,
          trigger: {
            id: `trig_${Date.now()}`,
            template_id: "",
            name,
            trigger_type: eventType,
            enabled: true,
            continue_on_error: continueOnError,
            commands,
            parameters: {},
            timeout_secs: timeoutSecs,
            cooldown_secs: cooldownSecs,
            notify_on_success: notifyOnSuccess,
            notify_on_failure: notifyOnFailure,
            last_fired_at: null,
            template_hash_at_addition: "",
            exec_in_terminal: execInTerminal,
            interval_secs: intervalSecs,
            schedule_mode: scheduleMode,
            cron_expr: cronExpr,
            scheduled_at: scheduledAt,
          },
        });
      }
      onSaved?.();
      onClose();
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setSaving(false);
    }
  };

  // 本地触发器可选的事件类型（与 SSH 服务器触发器不同）
  const localEventTypes: TriggerType[] = [
    "OnTerminalOpen",
    "BeforeTerminalClose",
    "OnNetworkDisconnect",
    "OnNetworkConnect",
    "OnLanIpChange",
    "OnPublicIpChange",
    "OnInterval",
    "OnSchedule",
    "ManualFire",
  ];
  const sshEventTypes: TriggerType[] = [
    "OnConnect",
    "OnReconnect",
    "OnIpChange",
    "OnProcessDead",
    "OnPortClosed",
    "OnInterval",
    "OnSchedule",
    "ManualFire",
  ];
  const eventTypes: TriggerType[] =
    serverId === "__local__" ? localEventTypes : sshEventTypes;

  return (
    <>
      <Modal
        title={isEditing ? t("trigger.edit") : t("trigger.add")}
        onClose={onClose}
        maxWidth="max-w-3xl"
        footer={
          <>
            <button
              className="px-4 py-2 text-sm rounded-lg text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-[#2C2C2E] transition-colors"
              onClick={onClose}
            >
              {t("common.cancel")}
            </button>
            <button
              className="px-4 py-2 text-sm rounded-lg bg-blue-500 text-white hover:bg-blue-600 disabled:opacity-40 disabled:cursor-not-allowed transition-colors font-medium"
              onClick={handleSave}
              disabled={saving}
            >
              {saving ? t("common.saving") : t("common.save")}
            </button>
          </>
        }
      >
        <div className="space-y-5">
          {/* Basic info */}
          <SettingGroup title={t("trigger.basic_info")}>
            <SettingRow label={t("trigger.name")}>
              <input
                type="text"
                data-testid="trigger-name-input"
                className="input w-full"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={t("trigger.name_placeholder")}
              />
            </SettingRow>
            <SettingRow label={t("trigger.event_type")}>
              <select
                className="input w-full"
                value={eventType}
                onChange={(e) => setEventType(e.target.value as TriggerType)}
              >
                {eventTypes.map((et) => (
                  <option key={et} value={et}>
                    {t(`trigger.event_types.${et}`)}
                  </option>
                ))}
              </select>
            </SettingRow>
            <div className="flex items-center justify-between px-4 py-3">
              <div>
                <span className="text-sm text-gray-700 dark:text-gray-200">
                  {t("trigger.exec_in_terminal")}
                </span>
                <p className="text-xs text-gray-400 dark:text-gray-500 mt-0.5">
                  {t("trigger.exec_in_terminal_hint")}
                </p>
              </div>
              <Toggle
                checked={execInTerminal}
                onChange={setExecInTerminal}
              />
            </div>
            {eventType === "OnInterval" && (
              <div className="px-4 py-3 border-b border-gray-100 dark:border-white/[0.06]">
                <div className="flex items-center justify-between gap-4">
                  <span className="text-sm font-medium text-gray-700 dark:text-gray-300 flex-shrink-0">
                    {t("trigger.interval_label")}
                  </span>
                  <div className="flex items-center gap-1.5 whitespace-nowrap overflow-x-auto">
                  <span className="text-xs text-gray-400 flex-shrink-0">{t("common.every")}</span>
                  <input
                    type="number"
                    min={0}
                    value={intervalDays}
                    onChange={(e) => setIntervalDays(Math.max(0, Number(e.target.value)))}
                    className="input flex-shrink-0"
                    style={{ width: "4rem" }}
                  />
                  <span className="text-xs text-gray-400 flex-shrink-0">{t("common.days")}</span>
                  <input
                    type="number"
                    min={0}
                    max={23}
                    value={intervalHours}
                    onChange={(e) => setIntervalHours(Math.min(23, Math.max(0, Number(e.target.value))))}
                    className="input flex-shrink-0"
                    style={{ width: "3.5rem" }}
                  />
                  <span className="text-xs text-gray-400 flex-shrink-0">{t("common.hours")}</span>
                  <input
                    type="number"
                    min={0}
                    max={59}
                    value={intervalMinutes}
                    onChange={(e) => setIntervalMinutes(Math.min(59, Math.max(0, Number(e.target.value))))}
                    className="input flex-shrink-0"
                    style={{ width: "3.5rem" }}
                  />
                  <span className="text-xs text-gray-400 flex-shrink-0">{t("common.minutes")}</span>
                  <input
                    type="number"
                    min={0}
                    max={59}
                    value={intervalSeconds}
                    onChange={(e) => setIntervalSeconds(Math.min(59, Math.max(0, Number(e.target.value))))}
                    className="input flex-shrink-0"
                    style={{ width: "3.5rem" }}
                  />
                  <span className="text-xs text-gray-400 flex-shrink-0">{t("common.seconds")}</span>
                  </div>
                </div>
              </div>
            )}
            {eventType === "OnSchedule" && (
              <>
                <div className="px-4 py-3 border-b border-gray-100 dark:border-white/[0.06]">
                  <div className="flex items-center justify-between mb-2">
                    <span className="text-sm text-gray-700 dark:text-gray-200">
                      {t("trigger.schedule_mode")}
                    </span>
                    <div className="inline-flex rounded-lg border border-gray-200 dark:border-white/[0.12] overflow-hidden">
                      <button
                        type="button"
                        className={`px-3 py-1.5 text-xs font-medium transition-colors ${
                          scheduleMode === "cron"
                            ? "bg-blue-500 text-white"
                            : "bg-transparent text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-[#2C2C2E]"
                        }`}
                        onClick={() => setScheduleMode("cron")}
                      >
                        {t("trigger.schedule_mode_cron")}
                      </button>
                      <button
                        type="button"
                        className={`px-3 py-1.5 text-xs font-medium transition-colors ${
                          scheduleMode === "once"
                            ? "bg-blue-500 text-white"
                            : "bg-transparent text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-[#2C2C2E]"
                        }`}
                        onClick={() => setScheduleMode("once")}
                      >
                        {t("trigger.schedule_mode_once")}
                      </button>
                    </div>
                  </div>
                </div>
                {scheduleMode === "cron" && (
                  <>
                    <SettingRow label={t("trigger.cron_frequency")}>
                      <select
                        className="input w-40"
                        value={cronFreq}
                        onChange={(e) => setCronFreq(e.target.value)}
                      >
                        <option value="daily">{t("trigger.cron_freq_daily")}</option>
                        <option value="weekly">{t("trigger.cron_freq_weekly")}</option>
                        <option value="monthly">{t("trigger.cron_freq_monthly")}</option>
                        <option value="custom">{t("trigger.cron_freq_custom")}</option>
                      </select>
                    </SettingRow>
                    {cronFreq !== "custom" && (
                      <SettingRow label={t("trigger.cron_time")}>
                        <div className="flex items-center gap-1">
                          <input
                            type="number"
                            min={0}
                            max={23}
                            value={cronHour}
                            onChange={(e) => setCronHour(Math.min(23, Math.max(0, Number(e.target.value))))}
                            className="input w-16"
                          />
                          <span className="text-gray-400">:</span>
                          <input
                            type="number"
                            min={0}
                            max={59}
                            value={cronMinute}
                            onChange={(e) => setCronMinute(Math.min(59, Math.max(0, Number(e.target.value))))}
                            className="input w-16"
                          />
                        </div>
                      </SettingRow>
                    )}
                    {cronFreq === "weekly" && (
                      <SettingRow label={t("trigger.cron_day_of_week")}>
                        <select
                          className="input w-40"
                          value={cronDow}
                          onChange={(e) => setCronDow(Number(e.target.value))}
                        >
                          <option value={0}>{t("trigger.dow_sun")}</option>
                          <option value={1}>{t("trigger.dow_mon")}</option>
                          <option value={2}>{t("trigger.dow_tue")}</option>
                          <option value={3}>{t("trigger.dow_wed")}</option>
                          <option value={4}>{t("trigger.dow_thu")}</option>
                          <option value={5}>{t("trigger.dow_fri")}</option>
                          <option value={6}>{t("trigger.dow_sat")}</option>
                        </select>
                      </SettingRow>
                    )}
                    {cronFreq === "monthly" && (
                      <SettingRow label={t("trigger.cron_day_of_month")}>
                        <input
                          type="number"
                          min={1}
                          max={28}
                          value={cronDom}
                          onChange={(e) => setCronDom(Math.min(28, Math.max(1, Number(e.target.value))))}
                          className="input w-20"
                        />
                      </SettingRow>
                    )}
                    {cronFreq === "custom" && (
                      <SettingRow label={t("trigger.cron_expr")}>
                        <input
                          type="text"
                          value={cronCustom}
                          onChange={(e) => setCronCustom(e.target.value)}
                          placeholder="0 3 * * *"
                          className="input w-full font-mono text-sm"
                        />
                      </SettingRow>
                    )}
                    {cronFreq !== "custom" && (
                      <div className="px-4 py-2 text-xs text-gray-400 dark:text-gray-500">
                        {t("trigger.cron_preview")}: <code className="font-mono">{cronExpr}</code>
                      </div>
                    )}
                  </>
                )}
                {scheduleMode === "once" && (
                  <SettingRow label={t("trigger.scheduled_at")}>
                    <input
                      type="datetime-local"
                      value={scheduledAt ? scheduledAt.slice(0, 16) : ""}
                      onChange={(e) => {
                        const v = e.target.value;
                        if (v) {
                          // Convert to RFC3339 with timezone
                          setScheduledAt(new Date(v).toISOString());
                        } else {
                          setScheduledAt("");
                        }
                      }}
                      className="input w-48"
                    />
                  </SettingRow>
                )}
              </>
            )}
            {(eventType === "OnInterval" || eventType === "OnSchedule") && (
              <div className="px-4 py-3 bg-amber-50 dark:bg-amber-900/20 border-t border-amber-100 dark:border-amber-900/30">
                <p className="text-xs text-amber-700 dark:text-amber-400 leading-relaxed">
                  {t("trigger.schedule_limitations")}
                </p>
              </div>
            )}
            <div className="px-4 py-3 border-b border-gray-100 dark:border-white/[0.06] last:border-0">
              <button
                type="button"
                className="w-full px-3 py-2 text-sm rounded-lg bg-gray-100 dark:bg-[#2C2C2E] text-gray-700 dark:text-gray-200 hover:bg-gray-200 dark:hover:bg-[#3A3A3C] transition-colors font-medium"
                onClick={() => setShowTemplateSelector(true)}
              >
                {t("trigger.choose_template")}
              </button>
            </div>
          </SettingGroup>

          {/* Command editor */}
          <SettingGroup title={t("trigger.commands")}>
            <div className="p-4">
              <div
                ref={editorRef}
                className="border border-gray-200/80 dark:border-white/[0.12] rounded-lg overflow-hidden"
                style={{ minHeight: "240px" }}
              />
            </div>
          </SettingGroup>

          {/* Execution settings */}
          <SettingGroup title={t("trigger.execution_settings")}>
            <SettingRow label={t("trigger.timeout")}>
              <input
                type="number"
                className="input w-24"
                value={timeoutSecs}
                onChange={(e) => setTimeoutSecs(parseInt(e.target.value) || 30)}
                min={1}
                max={600}
              />
            </SettingRow>
            <SettingRow label={t("trigger.cooldown")}>
              <input
                type="number"
                className="input w-24"
                value={cooldownSecs}
                onChange={(e) =>
                  setCooldownSecs(parseInt(e.target.value) || 60)
                }
                min={0}
                max={3600}
              />
            </SettingRow>
          </SettingGroup>

          {/* Notification settings */}
          <SettingGroup title={t("trigger.notifications")}>
            <SettingRow label={t("trigger.continue_on_error")}>
              <Toggle checked={continueOnError} onChange={setContinueOnError} />
            </SettingRow>
            <SettingRow label={t("trigger.notify_on_success")}>
              <Toggle checked={notifyOnSuccess} onChange={setNotifyOnSuccess} />
            </SettingRow>
            <SettingRow label={t("trigger.notify_on_failure")}>
              <Toggle checked={notifyOnFailure} onChange={setNotifyOnFailure} />
            </SettingRow>
          </SettingGroup>

          {/* Error */}
          {error && (
            <div className="text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 p-3 rounded-lg border border-red-200 dark:border-red-800/50">
              {error}
            </div>
          )}
        </div>
      </Modal>

      {/* Template selector overlay */}
      {showTemplateSelector && (
        <TemplateSelector
          templates={templates}
          onSelect={applyTemplate}
          onClose={() => setShowTemplateSelector(false)}
        />
      )}
    </>
  );
}

// === SECTION 2 END ===

function TemplateSelector({
  templates,
  onSelect,
  onClose,
}: {
  templates: TriggerTemplate[];
  onSelect: (tpl: TriggerTemplate) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [hoveredId, setHoveredId] = useState<string | null>(null);

  const selected = templates.find((t) => t.id === selectedId);
  const previewId = hoveredId || selectedId;
  const preview = templates.find((t) => t.id === previewId);

  const builtIn = templates.filter((t) => t.built_in);
  const user = templates.filter((t) => !t.built_in);

  return (
    <Modal
      title={t("trigger.choose_template")}
      onClose={onClose}
      maxWidth="max-w-3xl"
      zIndex="z-50"
      footer={
        <>
          <button
            className="px-4 py-2 text-sm rounded-lg text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-[#2C2C2E] transition-colors"
            onClick={onClose}
          >
            {t("common.cancel")}
          </button>
          <button
            className="px-4 py-2 text-sm rounded-lg bg-blue-500 text-white hover:bg-blue-600 disabled:opacity-40 disabled:cursor-not-allowed transition-colors font-medium"
            onClick={() => selected && onSelect(selected)}
            disabled={!selected}
          >
            {t("common.apply")}
          </button>
        </>
      }
    >
      <div className="flex gap-4" style={{ minHeight: "320px" }}>
        {/* Template list */}
        <div className="flex-1 space-y-4 overflow-y-auto max-h-[50vh] pr-1">
          {templates.length === 0 && (
            <div className="text-center text-gray-400 py-8">
              {t("trigger.no_templates")}
            </div>
          )}
          {builtIn.length > 0 && (
            <TemplateSelectorGroup
              title={t("template.built_in")}
              templates={builtIn}
              selectedId={selectedId}
              onSelect={setSelectedId}
              onHover={setHoveredId}
            />
          )}
          {user.length > 0 && (
            <TemplateSelectorGroup
              title={t("template.user")}
              templates={user}
              selectedId={selectedId}
              onSelect={setSelectedId}
              onHover={setHoveredId}
            />
          )}
        </div>

        {/* Preview panel */}
        <div className="w-64 flex-shrink-0 bg-[#FBFBFB] dark:bg-[#1E1E1E] rounded-xl border border-gray-200/80 dark:border-white/[0.06] p-4 overflow-y-auto max-h-[50vh]">
          {preview ? (
            <div className="space-y-3">
              <div>
                <div className="text-sm font-semibold text-gray-900 dark:text-gray-100">
                  {preview.name}
                </div>
                <div className="text-xs text-gray-500 mt-0.5">
                  {t(`trigger.event_types.${preview.type}`)}
                </div>
              </div>
              {preview.description && (
                <p className="text-xs text-gray-600 dark:text-gray-400 leading-relaxed">
                  {preview.description}
                </p>
              )}
              <div>
                <div className="text-xs text-gray-500 mb-1">
                  {t("trigger.commands")}
                </div>
                <pre className="text-xs font-mono bg-gray-100 dark:bg-[#1E1E1E] p-2 rounded-lg overflow-x-auto">
                  {preview.commands.join("\n")}
                </pre>
              </div>
            </div>
          ) : (
            <div className="h-full flex items-center justify-center text-xs text-gray-400 text-center">
              {t("trigger.template_preview_hint")}
            </div>
          )}
        </div>
      </div>
    </Modal>
  );
}

function TemplateSelectorGroup({
  title,
  templates,
  selectedId,
  onSelect,
  onHover,
}: {
  title: string;
  templates: TriggerTemplate[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onHover: (id: string | null) => void;
}) {
  const { t } = useTranslation();
  return (
    <div>
      <h4 className="text-xs font-semibold text-gray-400 uppercase tracking-wider mb-2">
        {title}
      </h4>
      <div className="space-y-1.5">
        {templates.map((tpl) => (
          <button
            key={tpl.id}
            type="button"
            onClick={() => onSelect(tpl.id)}
            onMouseEnter={() => onHover(tpl.id)}
            onMouseLeave={() => onHover(null)}
            className={`w-full text-left px-3 py-2.5 rounded-lg text-sm transition-colors ${
              selectedId === tpl.id
                ? "bg-blue-50 dark:bg-blue-900/20 text-blue-700 dark:text-blue-300 border border-blue-200 dark:border-blue-800"
                : "text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-[#1E1E1E] border border-transparent"
            }`}
          >
            <div className="flex items-center justify-between gap-2">
              <span className="font-medium truncate">{tpl.name}</span>
              <span className="text-[10px] text-gray-400 flex-shrink-0">
                {t(`trigger.event_types.${tpl.type}`)}
              </span>
            </div>
          </button>
        ))}
      </div>
    </div>
  );
}

// === SECTION 2 END ===

// macOS System Settings-style group: title above white rounded container
function SettingGroup({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section>
      <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100 mb-1.5 px-1">
        {title}
      </h3>
      <div className="bg-white dark:bg-[#1E1E1E] rounded-xl border border-gray-200/80 dark:border-white/[0.06] overflow-hidden">
        {children}
      </div>
    </section>
  );
}

// Horizontal label + control row (like SettingsPage SettingItem)
function SettingRow({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-4 px-4 py-3 border-b border-gray-100 dark:border-white/[0.06] last:border-0">
      <span className="text-sm font-medium text-gray-700 dark:text-gray-300 flex-shrink-0">
        {label}
      </span>
      <div className="flex-1 max-w-xs flex justify-end">{children}</div>
    </div>
  );
}

// macOS-style toggle switch
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
