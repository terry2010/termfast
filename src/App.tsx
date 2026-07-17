// Main App component — FP-7.3
// §9.4 GUI information architecture

import { useEffect, useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import {
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";
import { useServerStore } from "@/stores/serverStore";
import { useConfigStore } from "@/stores/configStore";
import { useTriggerStore } from "@/stores/triggerStore";
import i18n, { resolveLanguage } from "@/i18n/config";
import { useDaemonEvents } from "@/hooks/useDaemonEvents";
import { useKeyboardShortcuts } from "@/hooks/useKeyboardShortcuts";
import { ipcInvoke } from "@/hooks/useIpc";
import { scheduleAutoUpdateCheck } from "@/hooks/useUpdater";
import { ServerList } from "@/components/shared/ServerList";
import { LogPanel } from "@/components/shared/LogPanel";
import { PendingEventsBanner } from "@/components/shared/PendingEventsBanner";
import { ServerDetail } from "@/components/shared/ServerDetail";
import { TitleBar } from "@/components/desktop/TitleBar";
import { Onboarding } from "@/components/shared/Onboarding";
import { SettingsPage } from "@/components/shared/SettingsPage";
import { TemplateLibrary } from "@/components/shared/TemplateLibrary";
import { AddServerDialog } from "@/components/shared/AddServerDialog";
import { LogViewer } from "@/components/shared/LogViewer";
import { UndoToast } from "@/components/shared/UndoToast";
import { ConfirmDialog, type DangerLevel } from "@/components/ui/ConfirmDialog";
import { ContextMenuProvider } from "@/components/ui/ContextMenu";
import { Toaster, toast } from "sonner";
import { useTrayMenu } from "@/hooks/useTrayMenu";

export default function App() {
  // Listen for all daemon events (server status, proxy, triggers, logs)
  useDaemonEvents();
  // Dynamically build the system tray menu (i18n + server list)
  useTrayMenu();
  const { t } = useTranslation();

  const servers = useServerStore((s) => s.servers);
  const setServers = useServerStore((s) => s.setServers);
  const config = useConfigStore((s) => s.config);
  const setConfig = useConfigStore((s) => s.setConfig);
  const loadTemplates = useTriggerStore((s) => s.loadTemplates);

  // Load config and server list from daemon on mount
  useEffect(() => {
    // Request notification permission on macOS
    (async () => {
      try {
        let granted = await isPermissionGranted();
        console.log("[App] notification permission granted:", granted);
        if (!granted) {
          const perm = await requestPermission();
          console.log("[App] requestPermission result:", perm);
          granted = perm === "granted";
        }
        if (granted) {
          const { sendNotification } =
            await import("@tauri-apps/plugin-notification");
          sendNotification({
            title: "TermFast",
            body: "Notifications enabled",
          });
          console.log("[App] test notification sent");
        }
      } catch (e) {
        console.error("[App] notification init failed:", e);
      }
    })();

    ipcInvoke<any>("ipc_get_config")
      .then((data) => {
        if (data) {
          setConfig(data);
          // Apply saved language preference on startup
          const savedLang = data?.general?.language;
          if (savedLang) {
            i18n.changeLanguage(resolveLanguage(savedLang));
          }
        }
      })
      .catch((e) => console.error("load config failed:", e));

    ipcInvoke<any>("ipc_list_servers")
      .then((data) => {
        if (data?.servers) setServers(data.servers);
      })
      .catch((e) => console.error("load servers failed:", e));

    // Pre-load trigger templates so the selector in TriggerEditor has data
    // before the TemplateLibrary modal is ever opened.
    loadTemplates().catch((e) => console.error("load templates failed:", e));

    // Silent auto-check for updates 5s after startup (FP-10.2)
    const cancelAutoCheck = scheduleAutoUpdateCheck(5000, (result) => {
      const version = result.info.version;
      toast(
        <div className="flex flex-col gap-1">
          <div className="text-sm font-medium">
            {t("settings.about.available", { version })}
          </div>
          {result.info.body && (
            <div className="text-xs text-gray-500 dark:text-gray-400 max-h-24 overflow-y-auto whitespace-pre-line">
              {result.info.body}
            </div>
          )}
        </div>,
        {
          duration: 20000,
          action: {
            label: t("settings.about.install"),
            onClick: async () => {
              const progressId = toast.loading(t("settings.about.installing"));
              try {
                const { installUpdate } = await import("@/hooks/useUpdater");
                await installUpdate(result.update, (percent) => {
                  toast.loading(
                    `${t("settings.about.installing")} ${percent}%`,
                    { id: progressId },
                  );
                });
                toast.dismiss(progressId);
                toast.success(t("settings.about.installed"));
              } catch (e) {
                toast.dismiss(progressId);
                toast.error(t("settings.about.failed"));
                console.error("[App] auto update install failed:", e);
              }
            },
          },
        },
      );
    });

    return () => cancelAutoCheck();
  }, []);

  // UI state for modals
  const [showOnboarding, setShowOnboarding] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [showTemplates, setShowTemplates] = useState(false);
  const [showAddServer, setShowAddServer] = useState(false);
  const [editServer, setEditServer] = useState<{
    id: string;
    name: string;
    host: string;
    port: number;
    username: string;
    authType: "password" | "key";
    keyPath: string;
    socks5Port: number;
    httpPort: number;
    mixedPort: number;
  } | null>(null);
  const [showLogViewer, setShowLogViewer] = useState(false);
  const [logPanelExpanded, setLogPanelExpanded] = useState(false);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<{
    serverId: string;
    serverName: string;
  } | null>(null);

  const selectServer = useServerStore((s) => s.selectServer);

  // Listen for delete-server event from context menu
  useEffect(() => {
    const deleteHandler = (e: Event) => {
      const { serverId, serverName } = (e as CustomEvent).detail;
      setConfirmDelete({ serverId, serverName });
    };
    const editHandler = (e: Event) => {
      const serverId = (e as CustomEvent).detail.serverId;
      const server = servers.find((s) => s.id === serverId);
      if (server) {
        setEditServer({
          id: server.id,
          name: server.name,
          host: server.ssh?.host || "",
          port: server.ssh?.port || 22,
          username: server.ssh?.user || "root",
          authType:
            (server.ssh?.auth_method as "password" | "key") || "password",
          keyPath: server.ssh?.key_path || "",
          socks5Port: server.proxy?.socks5_port || 1080,
          httpPort: server.proxy?.http_port || 8080,
          mixedPort: server.proxy?.mixed_port || 0,
        });
      }
    };
    window.addEventListener("delete-server", deleteHandler);
    window.addEventListener("edit-server", editHandler);
    // Tray menu events
    const trayAddServerHandler = () => setShowAddServer(true);
    const trayOpenSettingsHandler = () => setShowSettings(true);
    window.addEventListener("tray-add-server", trayAddServerHandler);
    window.addEventListener("tray-open-settings", trayOpenSettingsHandler);
    return () => {
      window.removeEventListener("delete-server", deleteHandler);
      window.removeEventListener("edit-server", editHandler);
      window.removeEventListener("tray-add-server", trayAddServerHandler);
      window.removeEventListener("tray-open-settings", trayOpenSettingsHandler);
    };
  }, [servers]);

  // Show onboarding on first run (no servers and no config)
  useEffect(() => {
    if (servers.length === 0 && !config) {
      setShowOnboarding(true);
    } else if (servers.length > 0) {
      // Hide onboarding once servers are loaded
      setShowOnboarding(false);
    }
  }, [servers.length, config]);

  // Apply theme — reads config.general.theme ("system" | "light" | "dark")
  useEffect(() => {
    const applyTheme = () => {
      const theme = config?.general?.theme || "system";
      let isDark: boolean;
      if (theme === "dark") {
        isDark = true;
      } else if (theme === "light") {
        isDark = false;
      } else {
        // system
        isDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
      }
      if (isDark) {
        document.documentElement.classList.add("dark");
      } else {
        document.documentElement.classList.remove("dark");
      }
    };
    applyTheme();
    // Only listen to system changes when theme is "system"
    const theme = config?.general?.theme || "system";
    if (theme === "system") {
      const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
      const handler = () => applyTheme();
      mediaQuery.addEventListener("change", handler);
      return () => mediaQuery.removeEventListener("change", handler);
    }
  }, [config?.general?.theme]);

  // Keyboard shortcuts (§9.8 / FP-7.4)
  useKeyboardShortcuts({
    onSelectServer: (index) => {
      if (servers[index]) selectServer(servers[index].id);
    },
    onAddServer: () => setShowAddServer(true),
    onOpenSettings: () => setShowSettings(true),
    onFocusLogs: () => {
      const logPanel = document.querySelector("[data-log-panel]");
      logPanel?.scrollIntoView({ behavior: "smooth" });
    },
    onFocusLogSearch: () => {
      const searchInput = document.querySelector(
        "[data-log-search]",
      ) as HTMLInputElement | null;
      searchInput?.focus();
    },
    onToggleProxy: () => {
      const selected = servers.find(
        (s) => s.id === useServerStore.getState().selected_server_id,
      );
      if (selected) {
        ipcInvoke("ipc_toggle_proxy", {
          serverId: selected.id,
          enabled: !selected.proxy_running,
        }).catch(() => {});
      }
    },
    onToggleTriggers: () => {
      ipcInvoke("ipc_pause_all_triggers", {}).catch(() => {});
    },
    onToggleConnection: () => {
      const selected = servers.find(
        (s) => s.id === useServerStore.getState().selected_server_id,
      );
      if (!selected) return;
      if (selected.current_status === "connected") {
        ipcInvoke("ipc_disconnect_server", { serverId: selected.id }).catch(
          () => {},
        );
      } else {
        ipcInvoke("ipc_connect_server", { serverId: selected.id }).catch(
          () => {},
        );
      }
    },
    onToggleLogPanel: () => setLogPanelExpanded((v) => !v),
    onToggleSidebar: () => setSidebarCollapsed((v) => !v),
    onQuit: () => {
      ipcInvoke("ipc_quit_app", {}).catch(() => {});
    },
    onRefresh: () => {
      ipcInvoke("ipc_list_servers")
        .then((data: any) => {
          if (data?.servers) useServerStore.setState({ servers: data.servers });
        })
        .catch(() => {});
    },
    onEscape: () => {
      setShowSettings(false);
      setShowTemplates(false);
      setShowAddServer(false);
      setShowLogViewer(false);
      setShowOnboarding(false);
      setEditServer(null);
      setConfirmDelete(null);
    },
  });

  const handleConfirmDelete = useCallback(async () => {
    if (!confirmDelete) return;
    try {
      await ipcInvoke("ipc_remove_server", {
        serverId: confirmDelete.serverId,
      });
    } catch (e) {
      console.error("delete server failed:", e);
    }
    setConfirmDelete(null);
  }, [confirmDelete]);

  return (
    <ContextMenuProvider>
      <div className="flex flex-col h-screen bg-white dark:bg-[#121212] text-gray-900 dark:text-gray-100">
        <TitleBar />
        <PendingEventsBanner />
        <div className="flex flex-1 overflow-hidden">
          <ServerList
            onAddServer={() => setShowAddServer(true)}
            onOpenSettings={() => setShowSettings(true)}
            onOpenTemplates={() => setShowTemplates(true)}
            collapsed={sidebarCollapsed}
            onToggleCollapse={() => setSidebarCollapsed((v) => !v)}
          />
          <div className="flex-1 overflow-hidden bg-white dark:bg-[#1E1E1E]">
            <ServerDetail />
          </div>
        </div>
        <LogPanel onExpand={() => setShowLogViewer(true)} />

        {/* Modals */}
        {showOnboarding && (
          <Onboarding onComplete={() => setShowOnboarding(false)} />
        )}
        {showSettings && (
          <SettingsPage onClose={() => setShowSettings(false)} />
        )}
        {showTemplates && (
          <TemplateLibrary onClose={() => setShowTemplates(false)} />
        )}
        {showAddServer && (
          <AddServerDialog
            onAdd={() => setShowAddServer(false)}
            onCancel={() => setShowAddServer(false)}
          />
        )}
        {editServer && (
          <AddServerDialog
            editServer={editServer}
            onAdd={() => {
              setEditServer(null);
              // Reload server list
              console.log("[edit] onAdd callback, reloading servers");
              ipcInvoke<{ servers: any[] }>("ipc_list_servers")
                .then((data) => {
                  if (data?.servers)
                    useServerStore.setState({ servers: data.servers });
                })
                .catch(() => {});
            }}
            onCancel={() => setEditServer(null)}
          />
        )}
        {showLogViewer && <LogViewer onClose={() => setShowLogViewer(false)} />}
        <UndoToast />

        {/* Confirm dialog for server deletion (C3 fix) */}
        {confirmDelete && (
          <ConfirmDialog
            level="high"
            title={t("server.delete_title")}
            message={t("server.delete_message", {
              name: confirmDelete.serverName,
            })}
            confirmName={confirmDelete.serverName}
            actions={[
              t("server.delete_action_disconnect"),
              t("server.delete_action_triggers"),
              t("server.delete_action_config"),
            ]}
            onConfirm={handleConfirmDelete}
            onCancel={() => setConfirmDelete(null)}
          />
        )}
      </div>
      <Toaster position="top-right" richColors closeButton />
    </ContextMenuProvider>
  );
}
