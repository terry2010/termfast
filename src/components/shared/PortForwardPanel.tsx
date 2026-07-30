// PortForwardPanel — port forwarding rules list + add/edit/start/stop (PF-6)
// Shows rules with status, allows add/edit/delete/start/stop

import { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { ipcInvoke, formatIpcError } from "@/hooks/useIpc";
import { Modal } from "@/components/ui/Modal";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { toast } from "@/components/ui/toast";
import type {
  PortForwardRule,
  PortForwardRuleWithStatus,
  PortForwardType,
} from "@/types";

interface PortForwardPanelProps {
  serverId: string;
}

const EMPTY_RULES: PortForwardRuleWithStatus[] = [];

// Quick templates for common services
const QUICK_TEMPLATES = [
  { name: "MySQL", local_port: 13306, remote_port: 3306 },
  { name: "Redis", local_port: 16379, remote_port: 6379 },
  { name: "PostgreSQL", local_port: 15432, remote_port: 5432 },
  { name: "Web", local_port: 18080, remote_port: 8080 },
];

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}K`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)}M`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)}G`;
}

// === SECTION 1 END ===

export function PortForwardPanel({ serverId }: PortForwardPanelProps) {
  const { t } = useTranslation();
  const [rules, setRules] = useState<PortForwardRuleWithStatus[]>(EMPTY_RULES);
  const [loading, setLoading] = useState(true);
  const [editingRule, setEditingRule] = useState<PortForwardRule | null | undefined>(
    undefined,
  );
  const [deletingRule, setDeletingRule] = useState<PortForwardRuleWithStatus | null>(
    null,
  );
  const [togglingId, setTogglingId] = useState<string | null>(null);

  const loadRules = useCallback(async () => {
    try {
      const data = await ipcInvoke<{ rules: PortForwardRuleWithStatus[] }>(
        "ipc_list_port_forwards",
        { server_id: serverId },
      );
      setRules(data.rules || []);
    } catch (e) {
      console.error("[PortForwardPanel] load failed:", e);
    } finally {
      setLoading(false);
    }
  }, [serverId]);

  useEffect(() => {
    setLoading(true);
    loadRules();
  }, [loadRules]);

  const handleStart = async (ruleId: string) => {
    setTogglingId(ruleId);
    try {
      await ipcInvoke("ipc_start_port_forward", {
        server_id: serverId,
        rule_id: ruleId,
      });
      toast.success(t("port_forward.started"));
      await loadRules();
    } catch (e) {
      toast.error(formatIpcError(e));
    } finally {
      setTogglingId(null);
    }
  };

  const handleStop = async (ruleId: string) => {
    setTogglingId(ruleId);
    try {
      await ipcInvoke("ipc_stop_port_forward", {
        server_id: serverId,
        rule_id: ruleId,
      });
      toast.success(t("port_forward.stopped"));
      await loadRules();
    } catch (e) {
      toast.error(formatIpcError(e));
    } finally {
      setTogglingId(null);
    }
  };

  const handleDelete = async () => {
    if (!deletingRule) return;
    try {
      await ipcInvoke("ipc_delete_port_forward", {
        server_id: serverId,
        rule_id: deletingRule.id,
      });
      toast.success(t("port_forward.deleted"));
      setDeletingRule(null);
      await loadRules();
    } catch (e) {
      toast.error(formatIpcError(e));
    }
  };

  const handleStartAll = async () => {
    for (const rule of rules) {
      if (!rule.running && rule.enabled) {
        await handleStart(rule.id);
      }
    }
  };

  const handleStopAll = async () => {
    for (const rule of rules) {
      if (rule.running) {
        await handleStop(rule.id);
      }
    }
  };

  const handleSaved = async () => {
    setEditingRule(undefined);
    await loadRules();
  };

  const handleTemplateAdd = (tmpl: (typeof QUICK_TEMPLATES)[0]) => {
    setEditingRule({
      id: "",
      name: tmpl.name,
      type: "local",
      local_host: "127.0.0.1",
      local_port: tmpl.local_port,
      remote_host: "127.0.0.1",
      remote_port: tmpl.remote_port,
      enabled: true,
      auto_start: false,
    });
  };

  // === SECTION 2 END ===

  if (loading) {
    return (
      <div className="text-sm text-gray-400 py-4 text-center">
        {t("common.loading")}
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {/* Header with actions */}
      <div className="flex items-center justify-between gap-2 flex-wrap">
        <div className="flex items-center gap-2 flex-wrap">
          {QUICK_TEMPLATES.map((tmpl) => (
            <button
              key={tmpl.name}
              onClick={() => handleTemplateAdd(tmpl)}
              className="px-2.5 py-1 text-xs rounded-md bg-gray-100 dark:bg-[#2C2C2E] text-gray-600 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-[#3A3A3C] transition-colors"
            >
              + {tmpl.name}
            </button>
          ))}
        </div>
        <div className="flex items-center gap-2">
          {rules.some((r) => r.running) && (
            <button
              onClick={handleStopAll}
              className="px-3 py-1.5 text-xs rounded-lg bg-gray-100 dark:bg-[#2C2C2E] text-gray-700 dark:text-gray-200 hover:bg-gray-200 dark:hover:bg-[#3A3A3C] font-medium transition-colors"
            >
              {t("port_forward.stop_all")}
            </button>
          )}
          {rules.some((r) => !r.running && r.enabled) && (
            <button
              onClick={handleStartAll}
              className="px-3 py-1.5 text-xs rounded-lg bg-gray-100 dark:bg-[#2C2C2E] text-gray-700 dark:text-gray-200 hover:bg-gray-200 dark:hover:bg-[#3A3A3C] font-medium transition-colors"
            >
              {t("port_forward.start_all")}
            </button>
          )}
          <button
            onClick={() => setEditingRule(null)}
            className="px-3 py-1.5 text-xs rounded-lg bg-[#007AFF] text-white hover:bg-[#0066D6] font-medium transition-colors"
          >
            + {t("port_forward.add_rule")}
          </button>
        </div>
      </div>

      {/* Rules list */}
      {rules.length === 0 ? (
        <div className="text-sm text-gray-400 py-8 text-center">
          {t("port_forward.empty")}
        </div>
      ) : (
        <div className="space-y-2">
          {rules.map((rule) => (
            <div
              key={rule.id}
              className="flex items-center justify-between px-3 py-2.5 rounded-lg bg-gray-50 dark:bg-[#2C2C2E]/60 border border-gray-100 dark:border-white/[0.04]"
            >
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span
                    className={`inline-block w-2 h-2 rounded-full ${
                      rule.error
                        ? "bg-red-500"
                        : rule.running
                          ? "bg-green-500"
                          : rule.enabled
                            ? "bg-gray-300 dark:bg-gray-600"
                            : "bg-gray-200 dark:bg-gray-700"
                    }`}
                  />
                  <span className="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">
                    {rule.name}
                  </span>
                  <span className="text-[10px] px-1.5 py-0.5 rounded font-mono uppercase tracking-wide bg-gray-100 dark:bg-[#3A3A3C] text-gray-500 dark:text-gray-400">
                    {rule.type === "local" ? "-L" : "-R"}
                  </span>
                </div>
                <div className="text-xs text-gray-500 dark:text-gray-400 font-mono mt-0.5 truncate">
                  {rule.local_host}:{rule.local_port} → {rule.remote_host}:{rule.remote_port}
                  {rule.active_connections > 0 && (
                    <span className="ml-2 text-green-600 dark:text-green-400">
                      · {rule.active_connections} {t("port_forward.connections")}
                    </span>
                  )}
                  {(rule.bytes_in > 0 || rule.bytes_out > 0) && (
                    <span className="ml-2 text-gray-400 dark:text-gray-500">
                      · ↓{formatBytes(rule.bytes_in)} ↑{formatBytes(rule.bytes_out)}
                    </span>
                  )}
                  {rule.error && (
                    <span className="ml-2 text-red-500 dark:text-red-400" title={rule.error}>
                      · {t("port_forward.error")}
                    </span>
                  )}
                </div>
              </div>
              <div className="flex items-center gap-1.5 flex-shrink-0">
                {rule.running ? (
                  <button
                    onClick={() => handleStop(rule.id)}
                    disabled={togglingId === rule.id}
                    className="px-2.5 py-1 text-xs rounded-md bg-gray-100 dark:bg-[#3A3A3C] text-gray-700 dark:text-gray-200 hover:bg-gray-200 dark:hover:bg-[#48484A] disabled:opacity-50 transition-colors"
                  >
                    {togglingId === rule.id ? "..." : t("common.stop")}
                  </button>
                ) : (
                  <button
                    onClick={() => handleStart(rule.id)}
                    disabled={togglingId === rule.id || !rule.enabled}
                    className="px-2.5 py-1 text-xs rounded-md bg-[#34C759] text-white hover:bg-[#2EB34F] disabled:opacity-50 transition-colors"
                  >
                    {togglingId === rule.id ? "..." : t("common.start")}
                  </button>
                )}
                <button
                  onClick={() => setEditingRule(rule)}
                  className="px-2.5 py-1 text-xs rounded-md text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 transition-colors"
                >
                  {t("common.edit")}
                </button>
                <button
                  onClick={() => setDeletingRule(rule)}
                  className="px-2.5 py-1 text-xs rounded-md text-red-500 hover:text-red-600 transition-colors"
                >
                  {t("common.delete")}
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Editor modal */}
      {editingRule !== undefined && (
        <PortForwardEditor
          serverId={serverId}
          rule={editingRule}
          onClose={() => setEditingRule(undefined)}
          onSaved={handleSaved}
        />
      )}

      {/* Delete confirmation */}
      {deletingRule && (
        <ConfirmDialog
          level="low"
          title={t("port_forward.delete_title")}
          message={
            deletingRule.running
              ? t("port_forward.delete_confirm_running", { name: deletingRule.name })
              : t("port_forward.delete_confirm", { name: deletingRule.name })
          }
          confirmLabel={t("common.delete")}
          onConfirm={handleDelete}
          onCancel={() => setDeletingRule(null)}
        />
      )}
    </div>
  );
}

// === SECTION 3 END ===

// === PortForwardEditor — add/edit modal ===

interface PortForwardEditorProps {
  serverId: string;
  rule: PortForwardRule | null; // null = new
  onClose: () => void;
  onSaved: () => void;
}

function PortForwardEditor({
  serverId,
  rule,
  onClose,
  onSaved,
}: PortForwardEditorProps) {
  const { t } = useTranslation();
  const [name, setName] = useState(rule?.name || "");
  const [type, setType] = useState<PortForwardType>(rule?.type || "local");
  const [localHost, setLocalHost] = useState(rule?.local_host || "127.0.0.1");
  const [localPort, setLocalPort] = useState(rule?.local_port || 1080);
  const [remoteHost, setRemoteHost] = useState(rule?.remote_host || "127.0.0.1");
  const [remotePort, setRemotePort] = useState(rule?.remote_port || 80);
  const [enabled, setEnabled] = useState(rule?.enabled ?? true);
  const [autoStart, setAutoStart] = useState(rule?.auto_start ?? false);
  const [saving, setSaving] = useState(false);

  const handleSave = async () => {
    if (!name.trim()) {
      toast.error(t("port_forward.name_required"));
      return;
    }
    if (localPort < 1 || localPort > 65535 || remotePort < 1 || remotePort > 65535) {
      toast.error(t("port_forward.invalid_port"));
      return;
    }

    setSaving(true);
    const ruleData = {
      name: name.trim(),
      type,
      local_host: localHost,
      local_port: localPort,
      remote_host: remoteHost,
      remote_port: remotePort,
      enabled,
      auto_start: autoStart,
    };

    try {
      if (rule && rule.id) {
        const result = await ipcInvoke<{ ok: boolean; was_running: boolean }>(
          "ipc_update_port_forward",
          {
            server_id: serverId,
            rule_id: rule.id,
            rule: ruleData,
          },
        );
        if (result?.was_running) {
          toast.info(t("port_forward.updated_need_restart"));
        } else {
          toast.success(t("port_forward.updated"));
        }
      } else {
        await ipcInvoke("ipc_add_port_forward", {
          server_id: serverId,
          rule: ruleData,
        });
        toast.success(t("port_forward.added"));
      }
      onSaved();
    } catch (e) {
      toast.error(formatIpcError(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal
      title={rule && rule.id ? t("port_forward.edit_rule") : t("port_forward.add_rule")}
      onClose={onClose}
      maxWidth="max-w-lg"
      footer={
        <div className="flex justify-end gap-2">
          <button
            onClick={onClose}
            className="px-4 py-2 text-sm rounded-lg bg-gray-100 dark:bg-[#2C2C2E] text-gray-700 dark:text-gray-200 hover:bg-gray-200 dark:hover:bg-[#3A3A3C] font-medium transition-colors"
          >
            {t("common.cancel")}
          </button>
          <button
            onClick={handleSave}
            disabled={saving}
            className="px-4 py-2 text-sm rounded-lg bg-[#007AFF] text-white hover:bg-[#0066D6] disabled:opacity-50 font-medium transition-colors"
          >
            {saving ? t("common.saving") : t("common.save")}
          </button>
        </div>
      }
    >
      <div className="space-y-4 py-2">
        {/* Name */}
        <div>
          <label className="block text-xs font-medium text-gray-500 mb-1">
            {t("port_forward.name")}
          </label>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder={t("port_forward.name_placeholder")}
            className="w-full px-3 py-2 text-sm rounded-lg border border-gray-200 dark:border-white/[0.08] bg-white dark:bg-[#1C1C1E] text-gray-900 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-[#007AFF]/40"
          />
        </div>

        {/* Type */}
        <div>
          <label className="block text-xs font-medium text-gray-500 mb-1">
            {t("port_forward.type")}
          </label>
          <div className="flex gap-2">
            <button
              onClick={() => setType("local")}
              className={`flex-1 px-3 py-2 text-sm rounded-lg border transition-colors ${
                type === "local"
                  ? "border-[#007AFF] bg-[#007AFF]/10 text-[#007AFF]"
                  : "border-gray-200 dark:border-white/[0.08] text-gray-600 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-[#2C2C2E]"
              }`}
            >
              <div className="font-medium">-L {t("port_forward.local")}</div>
              <div className="text-xs opacity-70 mt-0.5">
                {t("port_forward.local_desc")}
              </div>
            </button>
            <button
              onClick={() => setType("remote")}
              className={`flex-1 px-3 py-2 text-sm rounded-lg border transition-colors ${
                type === "remote"
                  ? "border-[#007AFF] bg-[#007AFF]/10 text-[#007AFF]"
                  : "border-gray-200 dark:border-white/[0.08] text-gray-600 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-[#2C2C2E]"
              }`}
            >
              <div className="font-medium">-R {t("port_forward.remote")}</div>
              <div className="text-xs opacity-70 mt-0.5">
                {t("port_forward.remote_desc")}
              </div>
            </button>
          </div>
        </div>

        {/* Hosts and ports */}
        <div className="grid grid-cols-2 gap-3">
          <div>
            <label className="block text-xs font-medium text-gray-500 mb-1">
              {type === "local"
                ? t("port_forward.local_host")
                : t("port_forward.remote_bind_host")}
            </label>
            <input
              type="text"
              value={localHost}
              onChange={(e) => setLocalHost(e.target.value)}
              className="w-full px-3 py-2 text-sm rounded-lg border border-gray-200 dark:border-white/[0.08] bg-white dark:bg-[#1C1C1E] text-gray-900 dark:text-gray-100 font-mono focus:outline-none focus:ring-2 focus:ring-[#007AFF]/40"
            />
          </div>
          <div>
            <label className="block text-xs font-medium text-gray-500 mb-1">
              {type === "local"
                ? t("port_forward.local_port")
                : t("port_forward.remote_bind_port")}
            </label>
            <input
              type="number"
              min={1}
              max={65535}
              value={localPort}
              onChange={(e) => setLocalPort(Number(e.target.value))}
              className="w-full px-3 py-2 text-sm rounded-lg border border-gray-200 dark:border-white/[0.08] bg-white dark:bg-[#1C1C1E] text-gray-900 dark:text-gray-100 font-mono focus:outline-none focus:ring-2 focus:ring-[#007AFF]/40"
            />
          </div>
          <div>
            <label className="block text-xs font-medium text-gray-500 mb-1">
              {type === "local"
                ? t("port_forward.remote_host")
                : t("port_forward.local_target_host")}
            </label>
            <input
              type="text"
              value={remoteHost}
              onChange={(e) => setRemoteHost(e.target.value)}
              className="w-full px-3 py-2 text-sm rounded-lg border border-gray-200 dark:border-white/[0.08] bg-white dark:bg-[#1C1C1E] text-gray-900 dark:text-gray-100 font-mono focus:outline-none focus:ring-2 focus:ring-[#007AFF]/40"
            />
          </div>
          <div>
            <label className="block text-xs font-medium text-gray-500 mb-1">
              {type === "local"
                ? t("port_forward.remote_port")
                : t("port_forward.local_target_port")}
            </label>
            <input
              type="number"
              min={1}
              max={65535}
              value={remotePort}
              onChange={(e) => setRemotePort(Number(e.target.value))}
              className="w-full px-3 py-2 text-sm rounded-lg border border-gray-200 dark:border-white/[0.08] bg-white dark:bg-[#1C1C1E] text-gray-900 dark:text-gray-100 font-mono focus:outline-none focus:ring-2 focus:ring-[#007AFF]/40"
            />
          </div>
        </div>

        {/* Toggles */}
        <div className="space-y-3">
          <div className="flex items-center justify-between">
            <span className="text-sm text-gray-700 dark:text-gray-300">
              {t("port_forward.enabled")}
            </span>
            <button
              type="button"
              role="switch"
              aria-checked={enabled}
              onClick={() => setEnabled(!enabled)}
              className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors ${
                enabled ? "bg-[#007AFF]" : "bg-gray-300 dark:bg-gray-600"
              }`}
            >
              <span
                className={`inline-block h-5 w-5 transform rounded-full bg-white transition-transform ${
                  enabled ? "translate-x-5" : "translate-x-0.5"
                }`}
              />
            </button>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-sm text-gray-700 dark:text-gray-300">
              {t("port_forward.auto_start")}
            </span>
            <button
              type="button"
              role="switch"
              aria-checked={autoStart}
              onClick={() => setAutoStart(!autoStart)}
              className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors ${
                autoStart ? "bg-[#007AFF]" : "bg-gray-300 dark:bg-gray-600"
              }`}
            >
              <span
                className={`inline-block h-5 w-5 transform rounded-full bg-white transition-transform ${
                  autoStart ? "translate-x-5" : "translate-x-0.5"
                }`}
              />
            </button>
          </div>
        </div>
      </div>
    </Modal>
  );
}

// === SECTION 4 END ===
