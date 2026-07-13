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
import { ipcInvoke } from "@/hooks/useIpc";
import type { TriggerInstance, TriggerType } from "@/types";

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

  const isEditing = !!trigger;
  const commandsText = trigger?.commands.join("\n") || "";

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
      setError(String(e));
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
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50" onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className="bg-white dark:bg-gray-800 rounded-lg w-full max-w-3xl mx-4 max-h-[85vh] overflow-y-auto">
        {/* Header */}
        <div className="flex items-center justify-between p-4 border-b border-gray-200 dark:border-gray-700">
          <h2 className="text-lg font-medium">
            {isEditing ? t("trigger.edit") : t("trigger.add")}
          </h2>
          <button
            className="text-gray-500 hover:text-gray-700"
            onClick={onClose}
          >
            {t("common.close")}
          </button>
        </div>

        {/* Body */}
        <div className="p-4 space-y-4">
          {/* Name */}
          <div>
            <label className="block text-sm text-gray-500 mb-1">{t("trigger.name")}</label>
            <input
              type="text"
              className="w-full px-3 py-2 rounded border border-gray-300 dark:border-gray-600 bg-transparent text-sm"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t("trigger.name_placeholder")}
            />
          </div>

          {/* Event type */}
          <div>
            <label className="block text-sm text-gray-500 mb-1">{t("trigger.event_type")}</label>
            <select
              className="w-full px-3 py-2 rounded border border-gray-300 dark:border-gray-600 bg-transparent text-sm"
              value={eventType}
              onChange={(e) => setEventType(e.target.value as TriggerType)}
            >
              {eventTypes.map((et) => (
                <option key={et} value={et}>
                  {et.replace(/([A-Z])/g, " $1").trim()}
                </option>
              ))}
            </select>
          </div>

          {/* CodeMirror editor */}
          <div>
            <label className="block text-sm text-gray-500 mb-1">
              {t("trigger.commands")}
            </label>
            <div
              ref={editorRef}
              className="border border-gray-300 dark:border-gray-600 rounded overflow-hidden"
              style={{ minHeight: "200px" }}
            />
          </div>

          {/* Settings */}
          <div className="grid grid-cols-2 gap-4">
            <div>
              <label className="block text-sm text-gray-500 mb-1">
                {t("trigger.timeout")} (s)
              </label>
              <input
                type="number"
                className="w-full px-3 py-2 rounded border border-gray-300 dark:border-gray-600 bg-transparent text-sm"
                value={timeoutSecs}
                onChange={(e) => setTimeoutSecs(parseInt(e.target.value) || 30)}
                min={1}
                max={600}
              />
            </div>
            <div>
              <label className="block text-sm text-gray-500 mb-1">
                {t("trigger.cooldown")} (s)
              </label>
              <input
                type="number"
                className="w-full px-3 py-2 rounded border border-gray-300 dark:border-gray-600 bg-transparent text-sm"
                value={cooldownSecs}
                onChange={(e) => setCooldownSecs(parseInt(e.target.value) || 60)}
                min={0}
                max={3600}
              />
            </div>
          </div>

          {/* Checkboxes */}
          <div className="space-y-2">
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={continueOnError}
                onChange={(e) => setContinueOnError(e.target.checked)}
              />
              {t("trigger.continue_on_error")}
            </label>
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={notifyOnSuccess}
                onChange={(e) => setNotifyOnSuccess(e.target.checked)}
              />
              {t("trigger.notify_on_success")}
            </label>
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={notifyOnFailure}
                onChange={(e) => setNotifyOnFailure(e.target.checked)}
              />
              {t("trigger.notify_on_failure")}
            </label>
          </div>

          {/* Error */}
          {error && (
            <div className="text-sm text-red-500 bg-red-50 dark:bg-red-900/20 p-2 rounded">
              {error}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex justify-end gap-2 p-4 border-t border-gray-200 dark:border-gray-700">
          <button
            className="px-4 py-2 text-sm rounded hover:bg-gray-100 dark:hover:bg-gray-700"
            onClick={onClose}
          >
            {t("common.cancel")}
          </button>
          <button
            className="px-4 py-2 text-sm rounded bg-blue-500 text-white hover:bg-blue-600 disabled:opacity-50"
            onClick={handleSave}
            disabled={saving}
          >
            {saving ? t("common.saving") : t("common.save")}
          </button>
        </div>
      </div>
    </div>
  );
}

// === SECTION 2 END ===
