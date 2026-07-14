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
