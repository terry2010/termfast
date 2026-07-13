// PendingEventsBanner — alert banner for events needing user action (FP-8.9)
// Shows: auth failed, hostkey mismatch, port conflict, key lost, migration failed

import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useTauriEvent } from "@/hooks/useIpc";

export interface PendingEvent {
  id: string;
  severity: "error" | "warning" | "info";
  message: string;
  server_id?: string;
}

// === SECTION 1 END ===

export function PendingEventsBanner() {
  const { t } = useTranslation();
  const [events, setEvents] = useState<PendingEvent[]>([]);
  const [dismissed, setDismissed] = useState<Set<string>>(new Set());

  // Listen for pending event broadcasts from daemon
  useTauriEvent<PendingEvent>("daemon:pending-event", (event) => {
    if (!dismissed.has(event.id)) {
      setEvents((prev) => {
        // Avoid duplicates
        if (prev.some((e) => e.id === event.id)) return prev;
        return [...prev, event];
      });
    }
  });

  // Auto-dismiss info events after 10s
  useEffect(() => {
    const timers = events
      .filter((e) => e.severity === "info")
      .map((e) =>
        setTimeout(() => dismiss(e.id), 10000)
      );
    return () => timers.forEach(clearTimeout);
  }, [events]);

  const dismiss = (id: string) => {
    setDismissed((prev) => new Set(prev).add(id));
    setEvents((prev) => prev.filter((e) => e.id !== id));
  };

  const visible = events.filter((e) => !dismissed.has(e.id));
  if (visible.length === 0) return null;

  const severityStyles = {
    error: "bg-red-50 dark:bg-red-900/30 text-red-700 dark:text-red-300 border-red-200 dark:border-red-800",
    warning: "bg-yellow-50 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-300 border-yellow-200 dark:border-yellow-800",
    info: "bg-blue-50 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300 border-blue-200 dark:border-blue-800",
  };

  return (
    <div className="border-b border-gray-200 dark:border-gray-700">
      {visible.map((event) => (
        <div
          key={event.id}
          className={`flex items-center justify-between px-4 py-2 text-sm border-b ${severityStyles[event.severity]}`}
        >
          <span>{event.message}</span>
          <button
            className="ml-2 text-xs opacity-60 hover:opacity-100"
            onClick={() => dismiss(event.id)}
          >
            ✕
          </button>
        </div>
      ))}
    </div>
  );
}

// === SECTION 2 END ===