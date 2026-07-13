// LogViewer — full-featured log viewer (§9.4 / FP-6.7)
// Features: regex search, execution_id grouping, auto-scroll, unread badge

import { useState, useEffect, useRef, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { useLogStore, type LogLevel } from "@/stores/logStore";
import { ipcInvoke } from "@/hooks/useIpc";

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
        setSearchError(String(e));
        return entries;
      }
    }
    setSearchError(null);
    return entries.filter((e) =>
      (e.message || "").toLowerCase().includes(searchPattern.toLowerCase())
    );
  })();

  // Group by execution_id
  const grouped = (() => {
    if (!groupByExec) return null;
    const groups: Record<string, typeof entries> = {};
    for (const entry of filtered) {
      const key = (entry as { execution_id?: string }).execution_id || "ungrouped";
      if (!groups[key]) groups[key] = [];
      groups[key].push(entry);
    }
    return groups;
  })();

  const levelColor = (level: string) => {
    switch (level) {
      case "error": return "text-red-600";
      case "warn": return "text-yellow-600";
      case "info": return "text-blue-600";
      default: return "text-gray-600";
    }
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50" onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className="w-full max-w-4xl h-[80vh] bg-white dark:bg-gray-900 rounded-lg flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between p-4 border-b border-gray-200 dark:border-gray-700">
          <h2 className="text-lg font-bold flex items-center gap-2">
            {t("log.title")}
            {unreadCount > 0 && (
              <span className="px-2 py-0.5 text-xs bg-red-500 text-white rounded-full">
                {unreadCount}
              </span>
            )}
          </h2>
          <button onClick={onClose} className="text-gray-500 hover:text-gray-700 text-xl">×</button>
        </div>

        {/* Search bar */}
        <div className="flex items-center gap-2 p-3 border-b border-gray-200 dark:border-gray-700">
          <input
            type="text"
            className="flex-1 px-3 py-1.5 rounded border border-gray-300 dark:border-gray-600 bg-transparent text-sm"
            placeholder={useRegex ? t("logs.regex_placeholder") : t("logs.search_logs")}
            value={searchPattern}
            onChange={(e) => setSearchPattern(e.target.value)}
          />
          <label className="flex items-center gap-1 text-xs text-gray-500">
            <input type="checkbox" checked={useRegex} onChange={(e) => setUseRegex(e.target.checked)} />
            {t("logs.regex")}
          </label>
          <label className="flex items-center gap-1 text-xs text-gray-500">
            <input type="checkbox" checked={groupByExec} onChange={(e) => setGroupByExec(e.target.checked)} />
            {t("logs.group")}
          </label>
          <label className="flex items-center gap-1 text-xs text-gray-500">
            <input type="checkbox" checked={autoScroll} onChange={(e) => setAutoScroll(e.target.checked)} />
            {t("logs.auto")}
          </label>
        </div>
        {searchError && (
          <div className="px-3 py-1 text-xs text-red-600 bg-red-50 dark:bg-red-900/20">{searchError}</div>
        )}

        {/* Log entries */}
        <div ref={containerRef} onScroll={handleScroll} className="flex-1 overflow-y-auto p-3 font-mono text-xs space-y-0.5">
          {grouped ? (
            Object.entries(grouped).map(([key, items]) => (
              <div key={key} className="mb-3">
                <div className="text-xs font-bold text-gray-400 mb-1 border-b border-gray-200 dark:border-gray-700 pb-0.5">
                  exec: {key === "ungrouped" ? "(none)" : key} ({items.length})
                </div>
                {items.map((entry, i) => (
                  <LogLine key={i} entry={entry} levelColor={levelColor} />
                ))}
              </div>
            ))
          ) : (
            filtered.map((entry, i) => (
              <LogLine key={i} entry={entry} levelColor={levelColor} />
            ))
          )}
          {filtered.length === 0 && (
            <div className="text-center text-gray-400 py-8">{t("logs.no_logs")}</div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between p-2 border-t border-gray-200 dark:border-gray-700 text-xs text-gray-500">
          <span>{filtered.length} {t("logs.entries")}</span>
          <button
            className="px-2 py-1 text-xs rounded hover:bg-gray-100 dark:hover:bg-gray-800"
            onClick={() => { if (containerRef.current) { containerRef.current.scrollTop = containerRef.current.scrollHeight; setUnreadCount(0); } }}
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
  entry: { timestamp?: string; level?: string; message?: string; server_id?: string | null; kind?: string };
  levelColor: (level: string) => string;
}) {
  const ts = entry.timestamp ? new Date(entry.timestamp).toLocaleTimeString() : "";
  return (
    <div className="flex gap-2 hover:bg-gray-50 dark:hover:bg-gray-800 px-1 py-0.5 rounded">
      <span className="text-gray-400 shrink-0">{ts}</span>
      <span className={`shrink-0 font-bold ${levelColor(entry.level || "info")}`}>
        {(entry.level || "info").toUpperCase().padEnd(5)}
      </span>
      {entry.server_id && (
        <span className="text-gray-400 shrink-0">[{entry.server_id.slice(0, 8)}]</span>
      )}
      <span className="break-all">{entry.message}</span>
    </div>
  );
}

// === SECTION 2 END ===
