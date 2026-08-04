// TmuxSessionPicker — modal popup showing existing TermFast tmux sessions
// for the user to restore, or create a new session with a description.
//
// Shown when:
//   1. User clicks "Open Terminal" on a server with tmux_mode = "ask"
//   2. Server has tmux installed
//   3. There are @termfast-tagged sessions available
//
// Actions:
//   - Click a session row → onAttach(sessionName)
//   - Click "Create New Session" → show description input → onCreate(description)
//   - Click "Cancel" → onCancel()

import { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { Modal } from "@/components/ui/Modal";
import { ipcInvoke } from "@/hooks/useIpc";
import { Channel } from "@tauri-apps/api/core";
import { dispatchTerminalOutput } from "@/components/shared/TerminalView";

export interface TmuxSessionInfo {
  name: string;
  description: string;
  created: number;
  server: string;
  size: [number, number];
  windows: number;
  attached_count: number;
  last_activity: number;
}

interface TmuxSessionPickerProps {
  visible: boolean;
  serverId: string;
  cols: number;
  rows: number;
  /** Map of tmux_session_name → tabId for already-open tabs */
  openTmuxTabs: Record<string, string>;
  onSessionCreated: (sessionId: string, initialOutput: string, tmuxSessionName?: string) => void;
  onSkipTmux: () => void;
  onSwitchToTab: (tabId: string) => void;
  onCancel: () => void;
}

export function TmuxSessionPicker({
  visible,
  serverId,
  cols,
  rows,
  openTmuxTabs,
  onSessionCreated,
  onSkipTmux,
  onSwitchToTab,
  onCancel,
}: TmuxSessionPickerProps) {
  const { t } = useTranslation();
  const [loading, setLoading] = useState(true);
  const [sessions, setSessions] = useState<TmuxSessionInfo[]>([]);
  const [tmuxInstalled, setTmuxInstalled] = useState(true);
  const [showCreateForm, setShowCreateForm] = useState(false);
  const [description, setDescription] = useState("");
  const [actionInProgress, setActionInProgress] = useState(false);

  // Load sessions when visible
  useEffect(() => {
    if (!visible || !serverId) return;
    setLoading(true);
    setSessions([]);
    setTmuxInstalled(true);
    setShowCreateForm(false);
    setDescription("");
    ipcInvoke<{ sessions: TmuxSessionInfo[]; tmux_installed: boolean }>(
      "ipc_tmux_list_sessions",
      { server_id: serverId },
    )
      .then((result) => {
        setSessions(result.sessions || []);
        setTmuxInstalled(result.tmux_installed);
        setLoading(false);
      })
      .catch(() => {
        setLoading(false);
        setTmuxInstalled(false);
      });
  }, [visible, serverId]);

  const handleAttach = useCallback(
    async (sessionName: string) => {
      if (actionInProgress) return;
      setActionInProgress(true);
      try {
        let sessionId = "";
        const onOutput = new Channel<ArrayBuffer>();
        onOutput.onmessage = (data: ArrayBuffer) => {
          if (sessionId) {
            dispatchTerminalOutput(sessionId, new Uint8Array(data), false);
          }
        };
        const result = await ipcInvoke<{ session_id: string; initial_output: string; tmux_session_name?: string }>(
          "ipc_tmux_attach_session",
          {
            server_id: serverId,
            tmux_session_name: sessionName,
            cols,
            rows,
            on_output: onOutput,
          },
        );
        sessionId = result.session_id;
        onSessionCreated(result.session_id, result.initial_output || "", sessionName);
      } catch (e: any) {
        console.error("tmux attach failed:", e);
      } finally {
        setActionInProgress(false);
      }
    },
    [actionInProgress, serverId, cols, rows, onSessionCreated],
  );

  const handleCreate = useCallback(async () => {
    if (actionInProgress) return;
    setActionInProgress(true);
    try {
      let sessionId = "";
      const onOutput = new Channel<ArrayBuffer>();
      onOutput.onmessage = (data: ArrayBuffer) => {
        if (sessionId) {
          dispatchTerminalOutput(sessionId, new Uint8Array(data), false);
        }
      };
      const result = await ipcInvoke<{ session_id: string; initial_output: string; tmux_session_name?: string }>(
        "ipc_tmux_new_session",
        {
          server_id: serverId,
          description,
          cols,
          rows,
          on_output: onOutput,
        },
      );
      sessionId = result.session_id;
      onSessionCreated(result.session_id, result.initial_output || "", result.tmux_session_name);
    } catch (e: any) {
      console.error("tmux create failed:", e);
    } finally {
      setActionInProgress(false);
    }
  }, [actionInProgress, serverId, description, cols, rows, onSessionCreated]);

  if (!visible) return null;

  // Format timestamp to readable date
  const formatDate = (ts: number) => {
    if (!ts) return "—";
    const d = new Date(ts * 1000);
    return d.toLocaleDateString() + " " + d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  };

  return (
    <Modal
      title={t("server.tmux_session_picker_title")}
      onClose={onCancel}
      maxWidth="max-w-2xl"
      zIndex="z-[60]"
    >
      <div className="space-y-3">
        {loading ? (
          <div className="py-8 text-center text-sm text-neutral-500 dark:text-neutral-400">
            {t("server.tmux_loading")}
          </div>
        ) : !tmuxInstalled ? (
          <div className="py-8 text-center text-sm text-neutral-500 dark:text-neutral-400">
            {t("server.tmux_tmux_not_installed")}
          </div>
        ) : showCreateForm ? (
          /* Create new session form */
          <div className="space-y-3">
            <p className="text-sm text-neutral-600 dark:text-neutral-300">
              {t("server.tmux_description_label")}
            </p>
            <input
              type="text"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder={t("server.tmux_description_placeholder")}
              className="w-full rounded-md border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-800 px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
              autoFocus
              onKeyDown={(e) => {
                if (e.key === "Enter" && !actionInProgress) handleCreate();
              }}
            />
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setShowCreateForm(false)}
                disabled={actionInProgress}
                className="px-3 py-1.5 text-sm rounded-md border border-neutral-300 dark:border-neutral-700 hover:bg-neutral-100 dark:hover:bg-neutral-800"
              >
                {t("server.tmux_cancel")}
              </button>
              <button
                onClick={handleCreate}
                disabled={actionInProgress}
                className="px-3 py-1.5 text-sm rounded-md bg-blue-600 text-white hover:bg-blue-700 disabled:opacity-50"
              >
                {t("server.tmux_create_new")}
              </button>
            </div>
          </div>
        ) : sessions.length === 0 ? (
          /* No sessions — show create option + skip tmux */
          <div className="space-y-3">
            <p className="py-4 text-center text-sm text-neutral-500 dark:text-neutral-400">
              {t("server.tmux_no_sessions")}
            </p>
            <button
              onClick={() => setShowCreateForm(true)}
              className="w-full px-3 py-2 text-sm rounded-md bg-blue-600 text-white hover:bg-blue-700"
            >
              {t("server.tmux_create_new")}
            </button>
            <button
              onClick={onSkipTmux}
              disabled={actionInProgress}
              className="w-full px-3 py-2 text-sm rounded-md border border-neutral-300 dark:border-neutral-700 hover:bg-neutral-100 dark:hover:bg-neutral-800"
            >
              {t("server.tmux_skip")}
            </button>
          </div>
        ) : (
          /* Session list */
          <div className="space-y-2">
            <p className="text-xs text-neutral-500 dark:text-neutral-400">
              {t("server.tmux_session_picker_subtitle")}
            </p>
            {sessions.map((s) => {
              const openTabId = openTmuxTabs[s.name];
              const isOpen = !!openTabId;
              return (
                <div
                  key={s.name}
                  className="flex items-center justify-between rounded-md border border-neutral-200 dark:border-neutral-700 p-3 hover:bg-neutral-50 dark:hover:bg-neutral-800/50"
                >
                  <div className="flex-1 min-w-0">
                    <div className="text-sm font-medium truncate">
                      <span className="font-mono">{s.name}</span>
                      {s.description && (
                        <span className="text-neutral-500 dark:text-neutral-400 ml-1">— {s.description}</span>
                      )}
                    </div>
                    <div className="mt-1 flex flex-wrap gap-x-3 gap-y-0.5 text-xs text-neutral-500 dark:text-neutral-400">
                      <span>{t("server.tmux_session_created")}: {formatDate(s.created)}</span>
                      <span>{t("server.tmux_session_windows")}: {s.windows}</span>
                      <span>{t("server.tmux_session_attached")}: {s.attached_count}</span>
                      <span>{t("server.tmux_session_activity")}: {formatDate(s.last_activity)}</span>
                    </div>
                  </div>
                  {isOpen ? (
                    <button
                      onClick={() => onSwitchToTab(openTabId)}
                      disabled={actionInProgress}
                      className="ml-3 px-3 py-1 text-xs rounded-md bg-gray-500 text-white hover:bg-gray-600 disabled:opacity-50 whitespace-nowrap"
                    >
                      {t("server.tmux_goto_tab")}
                    </button>
                  ) : (
                    <button
                      onClick={() => handleAttach(s.name)}
                      disabled={actionInProgress}
                      className="ml-3 px-3 py-1 text-xs rounded-md bg-green-600 text-white hover:bg-green-700 disabled:opacity-50 whitespace-nowrap"
                    >
                      {t("server.tmux_attach")}
                    </button>
                  )}
                </div>
              );
            })}
            {/* Create new session button */}
            <button
              onClick={() => setShowCreateForm(true)}
              disabled={actionInProgress}
              className="w-full px-3 py-2 text-sm rounded-md border border-dashed border-neutral-300 dark:border-neutral-700 hover:bg-neutral-50 dark:hover:bg-neutral-800/50"
            >
              + {t("server.tmux_create_new")}
            </button>
            {/* Skip tmux — direct shell connection */}
            <button
              onClick={onSkipTmux}
              disabled={actionInProgress}
              className="w-full px-3 py-2 text-sm rounded-md border border-neutral-300 dark:border-neutral-700 hover:bg-neutral-100 dark:hover:bg-neutral-800"
            >
              {t("server.tmux_skip")}
            </button>
          </div>
        )}
      </div>
    </Modal>
  );
}
