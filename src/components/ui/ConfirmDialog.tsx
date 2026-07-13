// ConfirmDialog — three-tier danger level confirmation (§9.11 / FP-8.11)
// Low: confirm dialog + "don't ask again" checkbox
// Medium: undo toast (Sonner, 5-10s undo)
// High: red icon + type name to confirm

import { useState } from "react";
import { useTranslation } from "react-i18next";

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

  const canConfirm = level !== "high" || (confirmName && typedName === confirmName);

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50" onClick={(e) => { if (e.target === e.currentTarget) onCancel(); }}>
      <div className="bg-white dark:bg-gray-800 rounded-lg p-6 max-w-md w-full mx-4">
        <div className="flex items-start gap-3 mb-4">
          {level === "high" && (
            <div className="text-red-500 text-2xl" aria-hidden>⚠</div>
          )}
          <div className="flex-1">
            <h2 className={`text-lg font-medium ${level === "high" ? "text-red-600 dark:text-red-400" : ""}`}>
              {title}
            </h2>
            <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">{message}</p>
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
            <p className="text-sm mb-2">
              {t("common.delete")} — {t("common.yes")} "{confirmName}"
            </p>
            <input
              type="text"
              value={typedName}
              onChange={(e) => setTypedName(e.target.value)}
              className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded bg-transparent"
              autoFocus
            />
          </div>
        )}

        {level === "low" && (
          <label className="flex items-center gap-2 mb-4 text-sm">
            <input
              type="checkbox"
              checked={dontAskAgain}
              onChange={(e) => setDontAskAgain(e.target.checked)}
            />
            {t("common.yes")}
          </label>
        )}

        <div className="flex justify-end gap-2">
          <button
            className="px-4 py-2 text-sm rounded hover:bg-gray-100 dark:hover:bg-gray-700"
            aria-label={t("common.close")}
            onClick={onCancel}
          >
            {t("common.cancel")}
          </button>
          <button
            className={`px-4 py-2 text-sm rounded text-white ${
              level === "high"
                ? "bg-red-500 hover:bg-red-600"
                : "bg-blue-500 hover:bg-blue-600"
            } ${!canConfirm ? "opacity-50 cursor-not-allowed" : ""}`}
            disabled={!canConfirm}
            onClick={onConfirm}
          >
            {confirmLabel || (level === "high" ? t("common.delete") : t("common.ok"))}
          </button>
        </div>
      </div>
    </div>
  );
}
