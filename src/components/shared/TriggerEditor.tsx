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

export function TriggerEditor({ serverId, trigger, onClose, onSaved }: TriggerEditorProps) {
  const { t } = useTranslation();
  const editorRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const [name, setName] = useState(trigger?.name || "");
  const [eventType, setEventType] = useState<TriggerType>(
    (trigger as any)?.trigger_type || "ManualFire"
  );
  const [timeoutSecs, setTimeoutSecs] = useState(trigger?.timeout_secs || 30);
  const [cooldownSecs, setCooldownSecs] = useState(trigger?.cooldown_secs || 60);
  const [continueOnError, setContinueOnError] = useState(trigger?.continue_on_error || false);
  const [notifyOnSuccess, setNotifyOnSuccess] = useState(trigger?.notify_on_success || false);
  const [notifyOnFailure, setNotifyOnFailure] = useState<boolean>(trigger?.notify_on_failure ?? true);
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

    try {
      if (isEditing && trigger) {
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

  const eventTypes: TriggerType[] = [
    "OnConnect",
    "OnReconnect",
    "OnIpChange",
    "OnProcessDead",
    "OnPortClosed",
    "ManualFire",
  ];

  return (
    <>
    <Modal
      title={isEditing ? t("trigger.edit") : t("trigger.add")}
      onClose={onClose}
      maxWidth="max-w-3xl"
      footer={
        <>
          <button
            className="px-4 py-2 text-sm rounded-lg text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
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
          <div className="px-4 py-3 border-b border-gray-100 dark:border-gray-700/60 last:border-0">
            <button
              type="button"
              className="w-full px-3 py-2 text-sm rounded-lg bg-gray-100 dark:bg-gray-700 text-gray-700 dark:text-gray-200 hover:bg-gray-200 dark:hover:bg-gray-600 transition-colors font-medium"
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
              className="border border-gray-200 dark:border-gray-600 rounded-lg overflow-hidden"
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
              onChange={(e) => setCooldownSecs(parseInt(e.target.value) || 60)}
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
            className="px-4 py-2 text-sm rounded-lg text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
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
            <div className="text-center text-gray-400 py-8">{t("trigger.no_templates")}</div>
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
        <div className="w-64 flex-shrink-0 bg-gray-50 dark:bg-gray-900 rounded-xl border border-gray-200 dark:border-gray-700 p-4 overflow-y-auto max-h-[50vh]">
          {preview ? (
            <div className="space-y-3">
              <div>
                <div className="text-sm font-semibold text-gray-900 dark:text-gray-100">{preview.name}</div>
                <div className="text-xs text-gray-500 mt-0.5">{t(`trigger.event_types.${preview.type}`)}</div>
              </div>
              {preview.description && (
                <p className="text-xs text-gray-600 dark:text-gray-400 leading-relaxed">{preview.description}</p>
              )}
              <div>
                <div className="text-xs text-gray-500 mb-1">{t("trigger.commands")}</div>
                <pre className="text-xs font-mono bg-gray-100 dark:bg-gray-800 p-2 rounded-lg overflow-x-auto">
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
      <h4 className="text-xs font-semibold text-gray-400 uppercase tracking-wider mb-2">{title}</h4>
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
                : "text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-800 border border-transparent"
            }`}
          >
            <div className="flex items-center justify-between gap-2">
              <span className="font-medium truncate">{tpl.name}</span>
              <span className="text-[10px] text-gray-400 flex-shrink-0">{t(`trigger.event_types.${tpl.type}`)}</span>
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
      <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100 mb-1.5 px-1">{title}</h3>
      <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-200/80 dark:border-gray-700/80 overflow-hidden">
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
    <div className="flex items-center justify-between gap-4 px-4 py-3 border-b border-gray-100 dark:border-gray-700/60 last:border-0">
      <span className="text-sm font-medium text-gray-700 dark:text-gray-300 flex-shrink-0">{label}</span>
      <div className="flex-1 max-w-xs flex justify-end">{children}</div>
    </div>
  );
}

// macOS-style toggle switch
function Toggle({ checked, onChange }: { checked: boolean; onChange: (v: boolean) => void }) {
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
