// LogPanel — bottom collapsible log panel (§9.4)
// Shows log entries with level/category filters and search

import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import {
  useLogStore,
  type LogLevel,
  type LogCategory,
} from "@/stores/logStore";
import { ipcInvoke } from "@/hooks/useIpc";

export function LogPanel({ onExpand }: { onExpand?: () => void }) {
  const { t } = useTranslation();
  const expanded = useLogStore((s) => s.expanded);
  const setExpanded = useLogStore((s) => s.setExpanded);
  const entries = useLogStore((s) => s.entries);
  const clear = useLogStore((s) => s.clear);
  const setEntries = useLogStore((s) => s.setEntries);
  const filterLevel = useLogStore((s) => s.filter_level);
  const setFilterLevel = useLogStore((s) => s.setFilterLevel);
  const filterCategory = useLogStore((s) => s.filter_category);
  const setFilterCategory = useLogStore((s) => s.setFilterCategory);
  const searchQuery = useLogStore((s) => s.search_query);
  const setSearchQuery = useLogStore((s) => s.setSearchQuery);
  const filterServerId = useLogStore((s) => s.filter_server_id);

  // Compute filtered entries from state
  const filteredEntries = entries.filter((entry) => {
    if (filterLevel !== "all" && entry.level !== filterLevel) return false;
    if (filterCategory !== "all" && entry.category !== filterCategory)
      return false;
    if (filterServerId && entry.server_id !== filterServerId) return false;
    if (searchQuery) {
      const q = searchQuery.toLowerCase();
      if (
        !entry.message.toLowerCase().includes(q) &&
        !(entry.command?.toLowerCase().includes(q) ?? false)
      ) {
        return false;
      }
    }
    return true;
  });

  // Load logs when expanded
  useEffect(() => {
    if (expanded) {
      ipcInvoke<{ logs: never[] }>("ipc_get_logs", { limit: 1000 })
        .then((data) => {
          if (data.logs && data.logs.length > 0) {
            // Backend uses field name "kind", frontend uses "category" — map it
            const mapped = (data.logs as Record<string, unknown>[]).map(
              (log) => ({
                id: `${log.timestamp}-${Math.random().toString(36).slice(2)}`,
                timestamp: log.timestamp as string,
                server_id: (log.server_id as string) ?? null,
                level: log.level as never,
                category: ((log.category as string) ??
                  (log.kind as string) ??
                  "System") as never,
                message: log.message as string,
                execution_id: (log.execution_id as string) ?? null,
                command: null,
                exit_code: null,
                stdout: null,
                stderr: null,
              }),
            );
            setEntries(mapped);
          }
        })
        .catch(() => {});
    }
  }, [expanded, setEntries]);

  if (!expanded) {
    const recentErrors = filteredEntries
      .filter((e) => e.level === "error")
      .slice(-3);
    return (
      <div
        className="border-t border-gray-200 dark:border-white/[0.06] px-4 py-1 flex items-center justify-between gap-4 cursor-pointer"
        onClick={() => setExpanded(true)}
      >
        <button
          className="text-sm text-gray-500 hover:text-gray-700 shrink-0"
          onClick={(e) => {
            e.stopPropagation();
            setExpanded(true);
          }}
        >
          {t("logs.title")} ({filteredEntries.length})
        </button>
        {recentErrors.length > 0 && (
          <div className="flex-1 flex items-center gap-2 overflow-hidden">
            {recentErrors.map((e) => (
              <span
                key={e.id}
                className="text-xs text-red-500 truncate"
                title={e.message}
              >
                ⚠ {e.message}
              </span>
            ))}
          </div>
        )}
        <button
          className="text-xs text-gray-400 hover:text-gray-600 shrink-0"
          onClick={(e) => {
            e.stopPropagation();
            setExpanded(true);
          }}
        >
          ▲
        </button>
      </div>
    );
  }

  const displayEntries = filteredEntries.slice(-200);
  const levels: LogLevel[] = ["all", "info", "warn", "error"];
  const categories: LogCategory[] = [
    "all",
    "Connection",
    "Trigger",
    "Proxy",
    "Config",
    "Error",
    "System",
  ];

  return (
    <div className="border-t border-gray-200 dark:border-white/[0.06] h-48 flex flex-col">
      <div
        className="flex items-center justify-between px-4 py-1 border-b border-gray-200 dark:border-white/[0.06] cursor-pointer"
        onClick={() => setExpanded(false)}
      >
        <span className="text-sm font-medium">{t("logs.title")}</span>
        <div
          className="flex items-center gap-2"
          onClick={(e) => e.stopPropagation()}
        >
          <select
            className="text-xs px-1 py-0.5 rounded border border-gray-300 dark:border-white/[0.12] bg-transparent"
            value={filterLevel}
            onChange={(e) => setFilterLevel(e.target.value as LogLevel)}
          >
            {levels.map((l) => (
              <option key={l} value={l}>
                {l}
              </option>
            ))}
          </select>
          <select
            className="text-xs px-1 py-0.5 rounded border border-gray-300 dark:border-white/[0.12] bg-transparent"
            value={filterCategory}
            onChange={(e) => setFilterCategory(e.target.value as LogCategory)}
          >
            {categories.map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
          <input
            type="text"
            placeholder={t("logs.search_placeholder")}
            className="text-xs px-2 py-0.5 rounded border border-gray-300 dark:border-white/[0.12] bg-transparent w-32"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
          />
          <button
            className="text-xs px-2 py-0.5 rounded hover:bg-gray-100 dark:hover:bg-[#1E1E1E]"
            onClick={clear}
          >
            {t("logs.clear")}
          </button>
          {onExpand && (
            <button
              className="text-xs px-2 py-0.5 rounded hover:bg-gray-100 dark:hover:bg-[#1E1E1E]"
              onClick={onExpand}
            >
              {t("logs.expand")}
            </button>
          )}
          <button
            className="text-xs px-2 py-0.5 rounded hover:bg-gray-100 dark:hover:bg-[#1E1E1E]"
            onClick={() => setExpanded(false)}
          >
            {t("common.close")}
          </button>
        </div>
      </div>
      <div className="flex-1 overflow-y-auto px-4 py-1 font-mono text-xs">
        {displayEntries.length === 0 ? (
          <div className="text-gray-500">{t("logs.no_logs")}</div>
        ) : (
          displayEntries.map((entry) => (
            <div key={entry.id} className="py-0.5">
              <span className="text-gray-500">[{entry.timestamp}]</span>{" "}
              <span
                className={
                  entry.level === "error"
                    ? "text-red-500 font-bold"
                    : entry.level === "warn"
                      ? "text-yellow-500"
                      : "text-gray-700 dark:text-gray-300"
                }
              >
                [{entry.level.toUpperCase()}] [{entry.category}] {entry.message}
              </span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
// test 1783863616
// test 1783863640
// test 1783863652
// hmr test Sun Jul 12 22:03:05 CST 2026
