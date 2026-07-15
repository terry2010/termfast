// ConfirmDialog — three-tier danger level confirmation (§9.11 / FP-8.11)
// Low: confirm dialog + "don't ask again" checkbox
// Medium: undo toast (Sonner, 5-10s undo)
// High: red icon + type name to confirm

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Modal } from "@/components/ui/Modal";

export type DangerLevel = "low" | "medium" | "high";

interface ConfirmDialogProps {
  level: DangerLevel;
  title: string;
  message: string;
  /** For high danger: name that user must type to confirm */
  confirmName?: string;
  /** List of actions that will be performed (for high danger) */
  actions?: string[];
  confirmLabel?: string;
  onConfirm: () => void;
  onCancel: () => void;
}

export function ConfirmDialog({
  level,
  title,
  message,
  confirmName,
  actions,
  confirmLabel,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const { t } = useTranslation();
  const [dontAskAgain, setDontAskAgain] = useState(false);
  const [typedName, setTypedName] = useState("");

  const canConfirm =
    level !== "high" || (confirmName && typedName === confirmName);

  return (
    <Modal
      title={title}
      onClose={onCancel}
      maxWidth="max-w-md"
      footer={
        <>
          <button
            className="px-4 py-2 text-sm rounded-lg text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-[#2C2C2E] transition-colors"
            onClick={onCancel}
          >
            {t("common.cancel")}
          </button>
          <button
            className={`px-4 py-2 text-sm rounded-lg text-white transition-colors font-medium ${
              level === "high"
                ? "bg-red-500 hover:bg-red-600"
                : "bg-blue-500 hover:bg-blue-600"
            } ${!canConfirm ? "opacity-40 cursor-not-allowed" : ""}`}
            disabled={!canConfirm}
            onClick={onConfirm}
          >
            {confirmLabel ||
              (level === "high" ? t("common.delete") : t("common.ok"))}
          </button>
        </>
      }
    >
      <div className="flex items-start gap-3 mb-4">
        {level === "high" && (
          <div className="text-red-500 text-2xl flex-shrink-0" aria-hidden>
            ⚠
          </div>
        )}
        <div className="flex-1">
          <p
            className={`text-sm ${level === "high" ? "text-red-600 dark:text-red-400" : "text-gray-600 dark:text-gray-400"}`}
          >
            {message}
          </p>
        </div>
      </div>

      {actions && actions.length > 0 && (
        <ul className="text-sm text-gray-600 dark:text-gray-400 mb-4 list-disc list-inside">
          {actions.map((a, i) => (
            <li key={i}>{a}</li>
          ))}
        </ul>
      )}

      {level === "high" && confirmName && (
        <div className="mb-4">
          <p className="text-sm mb-2 text-gray-600 dark:text-gray-400">
            {t("common.type_to_confirm", { name: confirmName })}
          </p>
          <input
            type="text"
            value={typedName}
            onChange={(e) => setTypedName(e.target.value)}
            className="input"
            autoFocus
          />
        </div>
      )}

      {level === "low" && (
        <label className="flex items-center gap-2 text-sm text-gray-600 dark:text-gray-400">
          <input
            type="checkbox"
            checked={dontAskAgain}
            onChange={(e) => setDontAskAgain(e.target.checked)}
          />
          {t("common.yes")}
        </label>
      )}
    </Modal>
  );
}
