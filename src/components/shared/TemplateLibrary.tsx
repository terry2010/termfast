// TemplateLibrary — template management UI (U20 / §9.4 / FP-8.7)

import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useTriggerStore } from "@/stores/triggerStore";
import { useConfigStore } from "@/stores/configStore";
import { ipcInvoke } from "@/hooks/useIpc";
import type { TriggerTemplate, CustomVariable } from "@/types";
import { Modal } from "@/components/ui/Modal";

// System-defined variables (cannot be edited or deleted by users)
const SYSTEM_VARIABLES = [
  { name: "NewIP", desc: "本次连接的客户端 IP（自动注入）" },
  { name: "OldIP", desc: "上次连接的客户端 IP（首次为空，自动注入）" },
  { name: "IPFamily", desc: "IP 协议族 ipv4/ipv6（根据 NewIP 自动判断）" },
  { name: "ServerName", desc: "服务器名称（自动注入）" },
];

export function TemplateLibrary({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const templates = useTriggerStore((s) => s.templates);
  const setTemplates = useTriggerStore((s) => s.setTemplates);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [editing, setEditing] = useState<TriggerTemplate | null>(null);
  const [creating, setCreating] = useState(false);
  const [showVariables, setShowVariables] = useState(false);

  const reload = () => {
    ipcInvoke<{ templates: TriggerTemplate[] }>("ipc_list_templates")
      .then((data) => {
        if (data?.templates) setTemplates(data.templates);
      })
      .catch((e) => console.error("load templates failed:", e));
  };

  useEffect(() => { reload(); }, []);

  // ESC to close
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  const builtIn = templates.filter((t) => t.built_in);
  const user = templates.filter((t) => !t.built_in);

  const handleExport = () => {
    ipcInvoke("ipc_export_templates").catch((e) => console.error("export templates failed:", e));
  };

  const handleImport = () => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".json";
    input.onchange = (e) => {
      const file = (e.target as HTMLInputElement).files?.[0];
      if (!file) return;
      const reader = new FileReader();
      reader.onload = () => {
        try {
          const data = JSON.parse(reader.result as string);
          ipcInvoke("ipc_import_templates", { templates: data }).then(() => reload()).catch((e) => console.error("import failed:", e));
        } catch { /* ignore parse error */ }
      };
      reader.readAsText(file);
    };
    input.click();
  };

  return (
    <>
    <Modal
      title={t("menu.templates")}
      onClose={onClose}
      maxWidth="max-w-3xl"
      zIndex="z-40"
      footer={
        <div className="flex gap-2">
          <button
            className="px-4 py-2 text-sm rounded-lg bg-blue-500 text-white hover:bg-blue-600 transition-colors font-medium"
            onClick={() => setCreating(true)}
          >
            {t("common.add")}
          </button>
          <button
            className="px-4 py-2 text-sm rounded-lg text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
            onClick={() => setShowVariables(true)}
          >
            {t("template.variables")}
          </button>
          <button
            className="px-4 py-2 text-sm rounded-lg text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
            onClick={handleImport}
          >
            {t("common.import")}
          </button>
          <button
            className="px-4 py-2 text-sm rounded-lg text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
            onClick={handleExport}
          >
            {t("logs.export")}
          </button>
        </div>
      }
    >
      <div className="space-y-5">
        <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-200/80 dark:border-gray-700/80 overflow-hidden">
          <div className="px-4 py-3 border-b border-gray-100 dark:border-gray-700/80 bg-gray-50/50 dark:bg-gray-800/50">
            <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100">
              {t("template.built_in")}
            </h3>
          </div>
          <div className="p-2">
            <TemplateGroup
              templates={builtIn}
              expandedId={expandedId}
              onToggle={setExpandedId}
              onEdit={(tpl) => setEditing(tpl)}
            />
          </div>
        </div>

        <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-200/80 dark:border-gray-700/80 overflow-hidden">
          <div className="px-4 py-3 border-b border-gray-100 dark:border-gray-700/80 bg-gray-50/50 dark:bg-gray-800/50">
            <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100">
              {t("template.user")}
            </h3>
          </div>
          <div className="p-2">
            <TemplateGroup
              templates={user}
              expandedId={expandedId}
              onToggle={setExpandedId}
              onEdit={(tpl) => setEditing(tpl)}
              onDelete={reload}
            />
          </div>
        </div>
      </div>
    </Modal>
      {(editing || creating) && (
        <TemplateEditor
          template={editing}
          onClose={() => { setEditing(null); setCreating(false); }}
          onSaved={() => { setEditing(null); setCreating(false); reload(); }}
        />
      )}
      {showVariables && (
        <VariablesModal onClose={() => setShowVariables(false)} />
      )}
    </>
  );
}

function TemplateGroup({
  templates,
  expandedId,
  onToggle,
  onEdit,
  onDelete,
}: {
  templates: TriggerTemplate[];
  expandedId: string | null;
  onToggle: (id: string | null) => void;
  onEdit: (tpl: TriggerTemplate) => void;
  onDelete?: () => void;
}) {
  const { t } = useTranslation();
  if (templates.length === 0) {
    return (
      <div className="text-center py-6 text-sm text-gray-400">
        {t("trigger.no_templates")}
      </div>
    );
  }
  return (
    <div className="space-y-1">
      {templates.map((tpl) => (
        <div
          key={tpl.id}
          className="rounded-lg border border-gray-100 dark:border-gray-700/60 overflow-hidden transition-colors"
        >
          <div className="flex items-center justify-between px-3 py-2.5 hover:bg-gray-50/50 dark:hover:bg-gray-700/20">
            <button
              className="flex-1 flex items-center justify-between text-left"
              onClick={() => onToggle(expandedId === tpl.id ? null : tpl.id)}
            >
              <div className="flex items-center gap-2 min-w-0">
                <span className="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">{tpl.name}</span>
                <span className="text-[10px] px-1.5 py-0.5 rounded-full bg-gray-100 dark:bg-gray-700 text-gray-500 dark:text-gray-400 flex-shrink-0">
                  {t(`trigger.event_types.${tpl.type}`)}
                </span>
              </div>
              <svg
                className={`w-4 h-4 text-gray-400 transition-transform flex-shrink-0 ml-2 ${expandedId === tpl.id ? "rotate-180" : ""}`}
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                viewBox="0 0 24 24"
              >
                <path strokeLinecap="round" strokeLinejoin="round" d="M19 9l-7 7-7-7" />
              </svg>
            </button>
            <div className="flex items-center gap-1 flex-shrink-0 ml-2">
              <button
                className="text-xs px-2 py-1 rounded-md text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
                onClick={() => onEdit(tpl)}
              >
                {t("common.edit")}
              </button>
              {onDelete && !tpl.built_in && (
                <button
                  className="text-xs px-2 py-1 rounded-md text-red-500 hover:bg-red-50 dark:hover:bg-red-900/20 transition-colors"
                  onClick={() => {
                    if (confirm(t("template.confirm_delete"))) {
                      ipcInvoke("ipc_delete_template", { templateId: tpl.id })
                        .then(() => onDelete())
                        .catch((e) => console.error("delete template failed:", e));
                    }
                  }}
                >
                  {t("common.delete")}
                </button>
              )}
            </div>
          </div>
          {expandedId === tpl.id && (
            <div className="px-3 py-3 border-t border-gray-100 dark:border-gray-700/60 bg-gray-50/30 dark:bg-gray-800/30">
              <p className="text-xs text-gray-600 dark:text-gray-400 mb-2 leading-relaxed">{tpl.description}</p>
              <pre className="text-xs font-mono bg-gray-100 dark:bg-gray-900 p-2 rounded-lg overflow-x-auto">
                {tpl.commands.join("\n")}
              </pre>
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

function TemplateEditor({ template, onClose, onSaved }: { template: TriggerTemplate | null; onClose: () => void; onSaved: () => void }) {
  const { t } = useTranslation();
  const [name, setName] = useState(template?.name || "");
  const [type, setType] = useState<string>(template?.type || "OnConnect");
  const [description, setDescription] = useState(template?.description || "");
  const [commands, setCommands] = useState(template?.commands.join("\n") || "");

  const handleSave = () => {
    const cmdList = commands.split("\n").filter((c) => c.trim());
    const tpl = { name, type, description, commands: cmdList, built_in: false };
    if (template) {
      ipcInvoke("ipc_update_template", { templateId: template.id, template: tpl })
        .then(() => onSaved())
        .catch((e) => console.error("update template failed:", e));
    } else {
      ipcInvoke("ipc_create_template", tpl)
        .then(() => onSaved())
        .catch((e) => console.error("create template failed:", e));
    }
  };

  const eventTypes = [
    "OnConnect",
    "OnReconnect",
    "OnIpChange",
    "OnProcessDead",
    "OnPortClosed",
    "ManualFire",
  ];

  return (
    <Modal
      title={template ? t("common.edit") : t("common.add")}
      onClose={onClose}
      maxWidth="max-w-xl"
      footer={
        <>
          <button className="px-4 py-2 text-sm rounded-lg text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors" onClick={onClose}>{t("common.cancel")}</button>
          <button className="px-4 py-2 text-sm rounded-lg bg-blue-500 text-white hover:bg-blue-600 transition-colors font-medium" onClick={handleSave}>{t("common.save")}</button>
        </>
      }
    >
      <div className="space-y-5">
        <SettingGroup title={t("template.basic_info")}>
          <SettingRow label={t("template.name")}>
            <input className="input w-full" value={name} onChange={(e) => setName(e.target.value)} />
          </SettingRow>
          <SettingRow label={t("template.type")}>
            <select className="input w-full" value={type} onChange={(e) => setType(e.target.value)}>
              {eventTypes.map((et) => (
                <option key={et} value={et}>
                  {t(`trigger.event_types.${et}`)}
                </option>
              ))}
            </select>
          </SettingRow>
          <SettingRow label={t("template.description")}>
            <input className="input w-full" value={description} onChange={(e) => setDescription(e.target.value)} />
          </SettingRow>
        </SettingGroup>

        <SettingGroup title={t("template.commands")}>
          <div className="p-4">
            <textarea className="input font-mono h-40 w-full" value={commands} onChange={(e) => setCommands(e.target.value)} />
          </div>
        </SettingGroup>
      </div>
    </Modal>
  );
}

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

// Horizontal label + control row
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

// Variables modal — shows all available variables and lets users manage custom ones
function VariablesModal({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const config = useConfigStore((s) => s.config);
  const [customVars, setCustomVars] = useState<CustomVariable[]>(config?.general?.custom_variables || []);
  const [newName, setNewName] = useState("");
  const [newDesc, setNewDesc] = useState("");
  const [editingIdx, setEditingIdx] = useState<number | null>(null);
  const [editValue, setEditValue] = useState("");

  const systemNames = new Set(SYSTEM_VARIABLES.map((v) => v.name));

  const saveToBackend = (vars: CustomVariable[]) => {
    setCustomVars(vars);
    ipcInvoke("ipc_update_general_config", { custom_variables: vars }).catch((e) =>
      console.error("save custom variables failed:", e)
    );
  };

  const handleAdd = () => {
    const name = newName.trim();
    if (!name) return;
    // Don't allow names starting with lowercase system var names or duplicates
    if (systemNames.has(name)) return;
    if (customVars.some((v) => v.name === name)) return;
    saveToBackend([...customVars, { name, value: newDesc.trim() }]);
    setNewName("");
    setNewDesc("");
  };

  const handleDelete = (idx: number) => {
    saveToBackend(customVars.filter((_, i) => i !== idx));
  };

  const handleStartEdit = (idx: number) => {
    setEditingIdx(idx);
    setEditValue(customVars[idx].value);
  };

  const handleSaveEdit = () => {
    if (editingIdx === null) return;
    const updated = [...customVars];
    updated[editingIdx] = { ...updated[editingIdx], value: editValue.trim() };
    saveToBackend(updated);
    setEditingIdx(null);
    setEditValue("");
  };

  return (
    <Modal
      title={t("template.variables")}
      onClose={onClose}
      maxWidth="max-w-2xl"
      zIndex="z-50"
      footer={
        <button className="px-4 py-2 text-sm rounded-lg text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors" onClick={onClose}>
          {t("common.close")}
        </button>
      }
    >
      <div className="space-y-5">
        {/* Add new custom variable */}
        <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-200/80 dark:border-gray-700/80 p-4">
          <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100 mb-3">{t("template.add_custom_variable")}</h3>
          <div className="flex gap-2">
            <input
              className="input flex-1"
              placeholder={t("template.variable_name_placeholder")}
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter") handleAdd(); }}
            />
            <input
              className="input flex-1"
              placeholder={t("template.variable_value_placeholder")}
              value={newDesc}
              onChange={(e) => setNewDesc(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter") handleAdd(); }}
            />
            <button
              className="px-4 py-2 text-sm rounded-lg bg-blue-500 text-white hover:bg-blue-600 transition-colors font-medium flex-shrink-0"
              onClick={handleAdd}
              disabled={!newName.trim()}
            >
              {t("common.add")}
            </button>
          </div>
        </div>

        {/* System variables */}
        <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-200/80 dark:border-gray-700/80 overflow-hidden">
          <div className="px-4 py-3 border-b border-gray-100 dark:border-gray-700/80 bg-gray-50/50 dark:bg-gray-800/50">
            <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100">{t("template.system_variables")}</h3>
          </div>
          <div className="divide-y divide-gray-100 dark:divide-gray-700/60">
            {SYSTEM_VARIABLES.map((v) => (
              <div key={v.name} className="flex items-center justify-between px-4 py-3">
                <div className="min-w-0">
                  <code className="text-sm font-mono text-blue-600 dark:text-blue-400">{`{{.${v.name}}}`}</code>
                  <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">{v.desc}</p>
                </div>
                <span className="text-[10px] px-2 py-0.5 rounded-full bg-gray-100 dark:bg-gray-700 text-gray-500 dark:text-gray-400 flex-shrink-0">
                  {t("template.system")}
                </span>
              </div>
            ))}
          </div>
        </div>

        {/* Custom variables */}
        <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-200/80 dark:border-gray-700/80 overflow-hidden">
          <div className="px-4 py-3 border-b border-gray-100 dark:border-gray-700/80 bg-gray-50/50 dark:bg-gray-800/50">
            <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100">{t("template.custom_variables")}</h3>
          </div>
          {customVars.length === 0 ? (
            <div className="text-center py-6 text-sm text-gray-400">
              {t("template.no_custom_variables")}
            </div>
          ) : (
            <div className="divide-y divide-gray-100 dark:divide-gray-700/60">
              {customVars.map((v, idx) => (
                <div key={v.name} className="flex items-center justify-between px-4 py-3">
                  <div className="min-w-0 flex-1">
                    <code className="text-sm font-mono text-green-600 dark:text-green-400">{`{{.${v.name}}}`}</code>
                    {editingIdx === idx ? (
                      <div className="flex gap-2 mt-1">
                        <input
                          className="input flex-1 text-sm"
                          value={editValue}
                          onChange={(e) => setEditValue(e.target.value)}
                          autoFocus
                          onKeyDown={(e) => { if (e.key === "Enter") handleSaveEdit(); if (e.key === "Escape") setEditingIdx(null); }}
                        />
                        <button
                          className="text-xs px-2 py-1 rounded-md bg-blue-500 text-white hover:bg-blue-600 transition-colors"
                          onClick={handleSaveEdit}
                        >
                          {t("common.save")}
                        </button>
                        <button
                          className="text-xs px-2 py-1 rounded-md text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
                          onClick={() => setEditingIdx(null)}
                        >
                          {t("common.cancel")}
                        </button>
                      </div>
                    ) : (
                      <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5 break-all">{v.value || <span className="italic">（空）</span>}</p>
                    )}
                  </div>
                  {editingIdx !== idx && (
                    <div className="flex items-center gap-1 flex-shrink-0 ml-2">
                      <button
                        className="text-xs px-2 py-1 rounded-md text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
                        onClick={() => handleStartEdit(idx)}
                      >
                        {t("common.edit")}
                      </button>
                      <button
                        className="text-xs px-2 py-1 rounded-md text-red-500 hover:bg-red-50 dark:hover:bg-red-900/20 transition-colors"
                        onClick={() => handleDelete(idx)}
                      >
                        {t("common.delete")}
                      </button>
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </Modal>
  );
}
