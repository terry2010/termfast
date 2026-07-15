// TriggerTemplatePicker — select a trigger template to add (§9.3 / FP-8.5)
// Shows available templates with descriptions and parameter inputs

import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { ipcInvoke, formatIpcError } from "@/hooks/useIpc";
import { Modal } from "@/components/ui/Modal";

interface Template {
  id: string;
  name: string;
  type: string;
  description: string;
  built_in: boolean;
  parameters_schema: {
    name: string;
    description: string;
    required: boolean;
    default?: string;
  }[];
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

  // ESC to close
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

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
      setError(formatIpcError(e));
    } finally {
      setAdding(false);
    }
  };

  return (
    <Modal
      title={t("trigger.add_from_template")}
      onClose={onClose}
      maxWidth="max-w-2xl"
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
            onClick={handleAdd}
            disabled={!selectedId || adding}
          >
            {adding ? t("onboarding.adding") : t("common.add")}
          </button>
        </>
      }
    >
      <div className="space-y-2">
        {templates.length === 0 && (
          <div className="text-center text-gray-400 py-8">
            {t("trigger.no_templates")}
          </div>
        )}
        {templates.map((tmpl) => (
          <label
            key={tmpl.id}
            className={`block p-3 rounded-lg border cursor-pointer transition ${
              selectedId === tmpl.id
                ? "border-blue-500 bg-blue-50 dark:bg-blue-900/20"
                : "border-gray-200/80 dark:border-white/[0.06] hover:bg-[#FBFBFB] dark:hover:bg-[#1E1E1E]"
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
                    <span className="px-1.5 py-0.5 text-xs bg-gray-200 dark:bg-[#2C2C2E] rounded">
                      {t("trigger.built_in")}
                    </span>
                  )}
                </div>
                <div className="text-xs text-gray-500 mt-0.5">
                  {tmpl.description}
                </div>
                <div className="text-xs text-gray-400 mt-1">
                  {t("trigger.commands_count", { count: tmpl.commands.length })}
                </div>
              </div>
            </div>
          </label>
        ))}
      </div>

      {selected && selected.parameters_schema.length > 0 && (
        <div className="mt-4 pt-4 border-t border-gray-200/80 dark:border-white/[0.06] space-y-2">
          <div className="text-sm font-medium text-gray-600 dark:text-gray-300">
            {t("trigger.parameters")}
          </div>
          {selected.parameters_schema.map((p) => (
            <div key={p.name}>
              <label className="block text-xs text-gray-500 mb-0.5">
                {p.name} {p.required && <span className="text-red-500">*</span>}
              </label>
              <input
                type="text"
                className="input"
                value={params[p.name] || p.default || ""}
                onChange={(e) =>
                  setParams({ ...params, [p.name]: e.target.value })
                }
                placeholder={p.description}
              />
            </div>
          ))}
        </div>
      )}

      {error && (
        <div className="mt-3 text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 p-3 rounded-lg border border-red-200 dark:border-red-800/50">
          {error}
        </div>
      )}
    </Modal>
  );
}

// === SECTION 1 END ===
