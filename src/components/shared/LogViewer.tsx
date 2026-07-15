// LogViewer — full-featured log viewer (§9.4 / FP-6.7)
// Features: regex search, execution_id grouping, auto-scroll, unread badge

import { useState, useEffect, useRef, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { useLogStore, type LogLevel } from "@/stores/logStore";
import { ipcInvoke, formatIpcError } from "@/hooks/useIpc";

export function LogViewer({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const entries = useLogStore((s) => s.entries);
  const setEntries = useLogStore((s) => s.setEntries);
  const [searchPattern, setSearchPattern] = useState("");
  const [useRegex, setUseRegex] = useState(false);
  const [autoScroll, setAutoScroll] = useState(true);
  const [groupByExec, setGroupByExec] = useState(false);
  const [unreadCount, setUnreadCount] = useState(0);
  const [searchError, setSearchError] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const lastSeenCount = useRef(0);

  // ESC to close
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  // Load logs on mount
  useEffect(() => {
    ipcInvoke<{ logs: typeof entries }>("ipc_get_logs", { limit: 5000 })
      .then((data) => {
        if (data.logs) setEntries(data.logs);
      })
      .catch(() => {});
  }, [setEntries]);

  // Track unread count
  useEffect(() => {
    if (entries.length > lastSeenCount.current) {
      setUnreadCount((c) => c + (entries.length - lastSeenCount.current));
    }
    lastSeenCount.current = entries.length;
  }, [entries.length]);

  // Auto-scroll to bottom
  useEffect(() => {
    if (autoScroll && containerRef.current) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight;
    }
  }, [entries, autoScroll]);

  // Reset unread on scroll to bottom
  const handleScroll = useCallback(() => {
    if (containerRef.current) {
      const el = containerRef.current;
      if (el.scrollHeight - el.scrollTop - el.clientHeight < 50) {
        setUnreadCount(0);
      }
    }
  }, []);

  // Filter entries by search
  const filtered = (() => {
    if (!searchPattern) return entries;
    if (useRegex) {
      try {
        const regex = new RegExp(searchPattern, "i");
        setSearchError(null);
        return entries.filter((e) => regex.test(e.message || ""));
      } catch (e) {
        setSearchError(formatIpcError(e));
        return entries;
      }
    }
    setSearchError(null);
    return entries.filter((e) =>
      (e.message || "").toLowerCase().includes(searchPattern.toLowerCase()),
    );
  })();

  // Group by execution_id
  const grouped = (() => {
    if (!groupByExec) return null;
    const groups: Record<string, typeof entries> = {};
    for (const entry of filtered) {
      const key =
        (entry as { execution_id?: string }).execution_id || "ungrouped";
      if (!groups[key]) groups[key] = [];
      groups[key].push(entry);
    }
    return groups;
  })();

  const levelColor = (level: string) => {
    switch (level) {
      case "error":
        return "text-red-600";
      case "warn":
        return "text-yellow-600";
      case "info":
        return "text-blue-600";
      default:
        return "text-gray-600";
    }
  };

  return (
    <div
      className="fixed inset-0 bg-black/40 flex items-center justify-center z-50 animate-[fadeIn_0.15s_ease-out]"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="w-full max-w-4xl h-[80vh] bg-white dark:bg-[#1E1E1E] rounded-xl shadow-2xl flex flex-col animate-[scaleIn_0.15s_ease-out] border border-gray-200/80/50 dark:border-white/[0.06]/50 overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between px-5 py-4 border-b border-gray-100 dark:border-white/[0.06]">
          <h2 className="text-base font-semibold flex items-center gap-2 text-gray-900 dark:text-gray-100">
            {t("log.title")}
            {unreadCount > 0 && (
              <span className="px-2 py-0.5 text-xs bg-red-500 text-white rounded-full">
                {unreadCount}
              </span>
            )}
          </h2>
          <button
            onClick={onClose}
            className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 transition-colors w-7 h-7 flex items-center justify-center rounded-md hover:bg-gray-100 dark:hover:bg-[#2C2C2E]"
          >
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
              <path
                d="M1 1L13 13M13 1L1 13"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeLinecap="round"
              />
            </svg>
          </button>
        </div>

        {/* Search bar */}
        <div className="flex items-center gap-2 px-4 py-2.5 border-b border-gray-100 dark:border-white/[0.06]">
          <input
            type="text"
            className="flex-1 input"
            placeholder={
              useRegex ? t("logs.regex_placeholder") : t("logs.search_logs")
            }
            value={searchPattern}
            onChange={(e) => setSearchPattern(e.target.value)}
          />
          <label className="flex items-center gap-1 text-xs text-gray-500">
            <input
              type="checkbox"
              checked={useRegex}
              onChange={(e) => setUseRegex(e.target.checked)}
              className="rounded"
            />
            {t("logs.regex")}
          </label>
          <label className="flex items-center gap-1 text-xs text-gray-500">
            <input
              type="checkbox"
              checked={groupByExec}
              onChange={(e) => setGroupByExec(e.target.checked)}
              className="rounded"
            />
            {t("logs.group")}
          </label>
          <label className="flex items-center gap-1 text-xs text-gray-500">
            <input
              type="checkbox"
              checked={autoScroll}
              onChange={(e) => setAutoScroll(e.target.checked)}
              className="rounded"
            />
            {t("logs.auto")}
          </label>
        </div>
        {searchError && (
          <div className="px-4 py-2 text-xs text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20">
            {searchError}
          </div>
        )}

        {/* Log entries */}
        <div
          ref={containerRef}
          onScroll={handleScroll}
          className="flex-1 overflow-y-auto p-3 font-mono text-xs space-y-0.5"
        >
          {grouped
            ? Object.entries(grouped).map(([key, items]) => (
                <div key={key} className="mb-3">
                  <div className="text-xs font-bold text-gray-400 mb-1 border-b border-gray-200/80 dark:border-white/[0.06] pb-0.5">
                    exec: {key === "ungrouped" ? "(none)" : key} ({items.length}
                    )
                  </div>
                  {items.map((entry, i) => (
                    <LogLine key={i} entry={entry} levelColor={levelColor} />
                  ))}
                </div>
              ))
            : filtered.map((entry, i) => (
                <LogLine key={i} entry={entry} levelColor={levelColor} />
              ))}
          {filtered.length === 0 && (
            <div className="text-center text-gray-400 py-8">
              {t("logs.no_logs")}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-4 py-2.5 border-t border-gray-100 dark:border-white/[0.06] text-xs text-gray-500 bg-[#FBFBFB] dark:bg-[#1E1E1E]/50">
          <span>
            {filtered.length} {t("logs.entries")}
          </span>
          <button
            className="px-2 py-1 text-xs rounded-md hover:bg-gray-100 dark:hover:bg-[#2C2C2E] transition-colors"
            onClick={() => {
              if (containerRef.current) {
                containerRef.current.scrollTop =
                  containerRef.current.scrollHeight;
                setUnreadCount(0);
              }
            }}
          >
            {t("logs.jump_latest")}
          </button>
        </div>
      </div>
    </div>
  );
}

// === SECTION 1 END ===

function LogLine({
  entry,
  levelColor,
}: {
  entry: {
    timestamp?: string;
    level?: string;
    message?: string;
    server_id?: string | null;
    kind?: string;
  };
  levelColor: (level: string) => string;
}) {
  const ts = entry.timestamp
    ? new Date(entry.timestamp).toLocaleTimeString()
    : "";
  return (
    <div className="flex gap-2 hover:bg-[#FBFBFB] dark:hover:bg-[#1E1E1E] px-1 py-0.5 rounded">
      <span className="text-gray-400 shrink-0">{ts}</span>
      <span
        className={`shrink-0 font-bold ${levelColor(entry.level || "info")}`}
      >
        {(entry.level || "info").toUpperCase().padEnd(5)}
      </span>
      {entry.server_id && (
        <span className="text-gray-400 shrink-0">
          [{entry.server_id.slice(0, 8)}]
        </span>
      )}
      <span className="break-all">{entry.message}</span>
    </div>
  );
}

// === SECTION 2 END ===
