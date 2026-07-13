// UndoToast — toast notification with undo button (§9.5 / FP-8.6)
// Shows after destructive actions (delete server/trigger/template)

import { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";

interface UndoToastState {
  message: string;
  onUndo?: () => void;
  duration: number;
}

let toastState: UndoToastState | null = null;
let listeners: ((toast: UndoToastState | null) => void)[] = [];

export function showUndoToast(message: string, onUndo?: () => void, duration = 5000) {
  toastState = { message, onUndo, duration };
  listeners.forEach((l) => l(toastState));
}

export function dismissUndoToast() {
  toastState = null;
  listeners.forEach((l) => l(null));
}

export function UndoToast() {
  const { t } = useTranslation();
  const [toast, setToast] = useState<UndoToastState | null>(null);

  useEffect(() => {
    const listener = (newToast: UndoToastState | null) => setToast(newToast);
    listeners.push(listener);
    return () => {
      listeners = listeners.filter((l) => l !== listener);
    };
  }, []);

  const handleUndo = useCallback(() => {
    if (toast?.onUndo) {
      toast.onUndo();
    }
    dismissUndoToast();
  }, [toast]);

  const handleDismiss = useCallback(() => {
    dismissUndoToast();
  }, []);

  useEffect(() => {
    if (toast) {
      const timer = setTimeout(() => dismissUndoToast(), toast.duration);
      return () => clearTimeout(timer);
    }
  }, [toast]);

  if (!toast) return null;

  return (
    <div className="fixed bottom-4 left-1/2 -translate-x-1/2 z-50 animate-fade-in">
      <div className="flex items-center gap-3 px-4 py-3 bg-gray-900 dark:bg-gray-700 text-white rounded-lg shadow-lg">
        <span className="text-sm">{toast.message}</span>
        {toast.onUndo && (
          <button
            className="text-sm font-medium text-blue-400 hover:text-blue-300"
            onClick={handleUndo}
          >
            {t("common.undo")}
          </button>
        )}
        <button
          className="text-gray-400 hover:text-white text-lg leading-none"
          onClick={handleDismiss}
        >
          ×
        </button>
      </div>
    </div>
  );
}

// === SECTION 1 END ===