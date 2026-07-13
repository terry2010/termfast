// TemplateLibrary — template management UI (U20 / §9.4 / FP-8.7)

import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useTriggerStore } from "@/stores/triggerStore";
import { ipcInvoke } from "@/hooks/useIpc";
import type { TriggerTemplate } from "@/types";
import { Modal } from "@/components/ui/Modal";

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
      maxWidth="max-w-2xl"
      zIndex="z-40"
    >
      <div className="flex gap-2 mb-4">
        <button className="text-sm px-3 py-1.5 rounded-lg bg-blue-500 text-white hover:bg-blue-600 transition-colors" onClick={() => setCreating(true)}>
          {t("common.add")}
        </button>
        <button className="text-sm px-3 py-1.5 rounded-lg text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors" onClick={handleImport}>
          {t("common.import")}
        </button>
        <button className="text-sm px-3 py-1.5 rounded-lg text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors" onClick={handleExport}>
          {t("logs.export")}
        </button>
      </div>
      <div className="space-y-4">
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
    </Modal>
      {(editing || creating) && (
        <TemplateEditor
          template={editing}
          onClose={() => { setEditing(null); setCreating(false); }}
          onSaved={() => { setEditing(null); setCreating(false); reload(); }}
        />
      )}
    </>
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
                <span className="text-xs text-gray-500">{t(`trigger.event_types.${tpl.type}`)}</span>
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
    <Modal
      title={template ? t("common.edit") : t("common.add")}
      onClose={onClose}
      maxWidth="max-w-lg"
      footer={
        <>
          <button className="px-4 py-2 text-sm rounded-lg text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors" onClick={onClose}>{t("common.cancel")}</button>
          <button className="px-4 py-2 text-sm rounded-lg bg-blue-500 text-white hover:bg-blue-600 transition-colors font-medium" onClick={handleSave}>{t("common.save")}</button>
        </>
      }
    >
      <div className="space-y-3">
        <label className="block">
          <span className="text-sm text-gray-500">{t("template.name")}</span>
          <input className="input mt-1" value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <label className="block">
          <span className="text-sm text-gray-500">{t("template.type")}</span>
          <select className="input mt-1" value={type} onChange={(e) => setType(e.target.value)}>
            <option value="OnConnect">{t("trigger.event_types.OnConnect")}</option>
            <option value="OnReconnect">{t("trigger.event_types.OnReconnect")}</option>
            <option value="OnIpChange">{t("trigger.event_types.OnIpChange")}</option>
            <option value="OnProcessDead">{t("trigger.event_types.OnProcessDead")}</option>
            <option value="OnPortClosed">{t("trigger.event_types.OnPortClosed")}</option>
            <option value="ManualFire">{t("trigger.event_types.ManualFire")}</option>
          </select>
        </label>
        <label className="block">
          <span className="text-sm text-gray-500">{t("template.description")}</span>
          <input className="input mt-1" value={description} onChange={(e) => setDescription(e.target.value)} />
        </label>
        <label className="block">
          <span className="text-sm text-gray-500">{t("template.commands")}</span>
          <textarea className="input mt-1 font-mono h-32" value={commands} onChange={(e) => setCommands(e.target.value)} />
        </label>
      </div>
    </Modal>
  );
}
