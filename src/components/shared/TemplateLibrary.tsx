// TemplateLibrary — template management UI (U20 / §9.4 / FP-8.7)

import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useTriggerStore } from "@/stores/triggerStore";
import { ipcInvoke } from "@/hooks/useIpc";
import type { TriggerTemplate } from "@/types";

export function TemplateLibrary({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const templates = useTriggerStore((s) => s.templates);
  const setTemplates = useTriggerStore((s) => s.setTemplates);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [editing, setEditing] = useState<TriggerTemplate | null>(null);
  const [creating, setCreating] = useState(false);

  const reload = () => {
    ipcInvoke<{ templates: TriggerTemplate[] }>("ipc_list_templates")
      .then((data) => {
        if (data?.templates) setTemplates(data.templates);
      })
      .catch((e) => console.error("load templates failed:", e));
  };

  useEffect(() => { reload(); }, []);

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
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-40" onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className="bg-white dark:bg-gray-800 rounded-lg w-full max-w-2xl mx-4 max-h-[80vh] overflow-y-auto">
        <div className="flex items-center justify-between p-4 border-b border-gray-200 dark:border-gray-700">
          <h2 className="text-lg font-medium">{t("menu.templates")}</h2>
          <div className="flex gap-2">
            <button className="text-sm px-2 py-1 rounded hover:bg-gray-100 dark:hover:bg-gray-700" onClick={() => setCreating(true)}>
              {t("common.add")}
            </button>
            <button className="text-sm px-2 py-1 rounded hover:bg-gray-100 dark:hover:bg-gray-700" onClick={handleImport}>
              {t("common.import")}
            </button>
            <button className="text-sm px-2 py-1 rounded hover:bg-gray-100 dark:hover:bg-gray-700" onClick={handleExport}>
              {t("logs.export")}
            </button>
            <button className="text-gray-500 hover:text-gray-700" onClick={onClose}>
              {t("common.close")}
            </button>
          </div>
        </div>
        <div className="p-4 space-y-4">
          <TemplateGroup
            title={t("menu.templates") + " (" + t("template.built_in") + ")"}
            templates={builtIn}
            expandedId={expandedId}
            onToggle={setExpandedId}
            onEdit={(tpl) => setEditing(tpl)}
          />
          <TemplateGroup
            title={t("menu.templates") + " (" + t("template.user") + ")"}
            templates={user}
            expandedId={expandedId}
            onToggle={setExpandedId}
            onEdit={(tpl) => setEditing(tpl)}
            onDelete={reload}
          />
        </div>
      </div>
      {(editing || creating) && (
        <TemplateEditor
          template={editing}
          onClose={() => { setEditing(null); setCreating(false); }}
          onSaved={() => { setEditing(null); setCreating(false); reload(); }}
        />
      )}
    </div>
  );
}

function TemplateGroup({
  title,
  templates,
  expandedId,
  onToggle,
  onEdit,
  onDelete,
}: {
  title: string;
  templates: TriggerTemplate[];
  expandedId: string | null;
  onToggle: (id: string | null) => void;
  onEdit: (tpl: TriggerTemplate) => void;
  onDelete?: () => void;
}) {
  const { t } = useTranslation();
  if (templates.length === 0) return null;
  return (
    <div>
      <h3 className="text-sm font-medium mb-2">{title}</h3>
      <div className="space-y-1">
        {templates.map((tpl) => (
          <div key={tpl.id} className="border border-gray-200 dark:border-gray-700 rounded">
            <div className="flex items-center justify-between">
              <button
                className="flex-1 flex items-center justify-between px-3 py-2 text-left hover:bg-gray-50 dark:hover:bg-gray-700/50"
                onClick={() => onToggle(expandedId === tpl.id ? null : tpl.id)}
              >
                <span className="text-sm font-medium">{tpl.name}</span>
                <span className="text-xs text-gray-500">{tpl.type.replace(/([A-Z])/g, " $1").trim()}</span>
              </button>
              <div className="flex gap-1 px-2">
                <button
                  className="text-xs px-2 py-0.5 rounded hover:bg-gray-200 dark:hover:bg-gray-600"
                  onClick={() => onEdit(tpl)}
                >
                  {t("common.edit")}
                </button>
                {onDelete && !tpl.built_in && (
                  <button
                    className="text-xs px-2 py-0.5 rounded text-red-500 hover:bg-red-50 dark:hover:bg-red-900/20"
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
              <div className="px-3 py-2 border-t border-gray-200 dark:border-gray-700">
                <p className="text-xs text-gray-500 mb-2">{tpl.description}</p>
                <pre className="text-xs font-mono bg-gray-100 dark:bg-gray-900 p-2 rounded overflow-x-auto">
                  {tpl.commands.join("\n")}
                </pre>
              </div>
            )}
          </div>
        ))}
      </div>
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

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50" onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className="bg-white dark:bg-gray-800 rounded-lg w-full max-w-lg mx-4 max-h-[80vh] overflow-y-auto" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between p-4 border-b border-gray-200 dark:border-gray-700">
          <h2 className="text-lg font-medium">{template ? t("common.edit") : t("common.add")}</h2>
          <button className="text-gray-400 hover:text-gray-600 text-xl" onClick={onClose}>✕</button>
        </div>
        <div className="p-4 space-y-3">
          <label className="block">
            <span className="text-sm">{t("template.name")}</span>
            <input className="w-full mt-1 px-2 py-1 text-sm border border-gray-300 dark:border-gray-600 rounded bg-transparent" value={name} onChange={(e) => setName(e.target.value)} />
          </label>
          <label className="block">
            <span className="text-sm">{t("template.type")}</span>
            <select className="w-full mt-1 px-2 py-1 text-sm border border-gray-300 dark:border-gray-600 rounded bg-transparent" value={type} onChange={(e) => setType(e.target.value)}>
              <option value="OnConnect">OnConnect</option>
              <option value="OnReconnect">OnReconnect</option>
              <option value="OnIpChange">OnIpChange</option>
              <option value="OnProcessDead">OnProcessDead</option>
              <option value="OnPortClosed">OnPortClosed</option>
              <option value="ManualFire">ManualFire</option>
            </select>
          </label>
          <label className="block">
            <span className="text-sm">{t("template.description")}</span>
            <input className="w-full mt-1 px-2 py-1 text-sm border border-gray-300 dark:border-gray-600 rounded bg-transparent" value={description} onChange={(e) => setDescription(e.target.value)} />
          </label>
          <label className="block">
            <span className="text-sm">{t("template.commands")}</span>
            <textarea className="w-full mt-1 px-2 py-1 text-sm border border-gray-300 dark:border-gray-600 rounded bg-transparent font-mono h-32" value={commands} onChange={(e) => setCommands(e.target.value)} />
          </label>
          <div className="flex justify-end gap-2">
            <button className="px-3 py-1 text-sm rounded border border-gray-300 dark:border-gray-600" onClick={onClose}>{t("common.cancel")}</button>
            <button className="px-3 py-1 text-sm rounded bg-blue-500 text-white hover:bg-blue-600" onClick={handleSave}>{t("common.save")}</button>
          </div>
        </div>
      </div>
    </div>
  );
}
