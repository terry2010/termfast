// TriggerTemplatePicker — select a trigger template to add (§9.3 / FP-8.5)
// Shows available templates with descriptions and parameter inputs

import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { ipcInvoke } from "@/hooks/useIpc";

interface Template {
  id: string;
  name: string;
  type: string;
  description: string;
  built_in: boolean;
  parameters_schema: { name: string; description: string; required: boolean; default?: string }[];
  commands: string[];
}

export function TriggerTemplatePicker({
  serverId,
  onAdded,
  onClose,
}: {
  serverId: string;
  onAdded: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [templates, setTemplates] = useState<Template[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [params, setParams] = useState<Record<string, string>>({});
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    ipcInvoke<{ templates: Template[] }>("ipc_list_templates")
      .then((data) => {
        setTemplates(data.templates || []);
      })
      .catch((e) => setError(String(e)));
  }, []);

  const selected = templates.find((t) => t.id === selectedId);

  const handleAdd = async () => {
    if (!selectedId) return;
    setAdding(true);
    setError(null);
    try {
      await ipcInvoke("ipc_add_trigger_from_template", {
        server_id: serverId,
        template_id: selectedId,
      });
      onAdded();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setAdding(false);
    }
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50" onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className="w-full max-w-2xl max-h-[80vh] bg-white dark:bg-gray-900 rounded-lg flex flex-col">
        <div className="flex items-center justify-between p-4 border-b border-gray-200 dark:border-gray-700">
          <h2 className="text-lg font-bold">{t("trigger.add_from_template")}</h2>
          <button onClick={onClose} className="text-gray-500 hover:text-gray-700 text-xl">×</button>
        </div>

        <div className="flex-1 overflow-y-auto p-4 space-y-2">
          {templates.length === 0 && (
            <div className="text-center text-gray-400 py-8">{t("trigger.no_templates")}</div>
          )}
          {templates.map((tmpl) => (
            <label
              key={tmpl.id}
              className={`block p-3 rounded border cursor-pointer transition ${
                selectedId === tmpl.id
                  ? "border-blue-500 bg-blue-50 dark:bg-blue-900/20"
                  : "border-gray-200 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-800"
              }`}
            >
              <div className="flex items-start gap-2">
                <input
                  type="radio"
                  checked={selectedId === tmpl.id}
                  onChange={() => {
                    setSelectedId(tmpl.id);
                    setParams({});
                  }}
                  className="mt-1"
                />
                <div className="flex-1">
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-medium">{tmpl.name}</span>
                    {tmpl.built_in && (
                      <span className="px-1.5 py-0.5 text-xs bg-gray-200 dark:bg-gray-700 rounded">{t("trigger.built_in")}</span>
                    )}
                  </div>
                  <div className="text-xs text-gray-500 mt-0.5">{tmpl.description}</div>
                  <div className="text-xs text-gray-400 mt-1">{t("trigger.commands_count", { count: tmpl.commands.length })}</div>
                </div>
              </div>
            </label>
          ))}
        </div>

        {selected && selected.parameters_schema.length > 0 && (
          <div className="p-4 border-t border-gray-200 dark:border-gray-700 space-y-2">
            <div className="text-sm font-medium">{t("trigger.parameters")}</div>
            {selected.parameters_schema.map((p) => (
              <div key={p.name}>
                <label className="block text-xs text-gray-500 mb-0.5">
                  {p.name} {p.required && <span className="text-red-500">*</span>}
                </label>
                <input
                  type="text"
                  className="w-full px-2 py-1 rounded border border-gray-300 dark:border-gray-600 bg-transparent text-sm"
                  value={params[p.name] || p.default || ""}
                  onChange={(e) => setParams({ ...params, [p.name]: e.target.value })}
                  placeholder={p.description}
                />
              </div>
            ))}
          </div>
        )}

        {error && (
          <div className="px-4 py-2 text-xs text-red-600 bg-red-50 dark:bg-red-900/20">{error}</div>
        )}

        <div className="flex justify-end gap-2 p-4 border-t border-gray-200 dark:border-gray-700">
          <button
            className="px-4 py-2 text-sm rounded hover:bg-gray-100 dark:hover:bg-gray-800"
            onClick={onClose}
          >
            {t("common.cancel")}
          </button>
          <button
            className="px-4 py-2 text-sm rounded bg-blue-500 text-white hover:bg-blue-600 disabled:opacity-50"
            onClick={handleAdd}
            disabled={!selectedId || adding}
          >
            {adding ? t("onboarding.adding") : t("common.add")}
          </button>
        </div>
      </div>
    </div>
  );
}

// === SECTION 1 END ===