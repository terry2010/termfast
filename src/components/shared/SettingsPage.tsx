// SettingsPage — settings UI (§9.5 / FP-8.8)
// Sidebar nav + single scrollable page with scroll-spy

import { useState, useRef, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useConfigStore } from "@/stores/configStore";
import { ipcInvoke } from "@/hooks/useIpc";
import type { SupportedLanguage } from "@/i18n/config";
import i18n, { resolveLanguage } from "@/i18n/config";
import { Modal } from "@/components/ui/Modal";
import { toast } from "sonner";
import {
  checkForUpdate,
  installUpdate,
  type UpdateResult,
} from "@/hooks/useUpdater";

type TabId =
  "general" | "logs" | "proxy" | "trigger" | "notification" | "credentials" | "cloud_sync" | "data" | "about";

export function SettingsPage({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<TabId>("general");
  const sectionRefs = useRef<Record<TabId, HTMLDivElement | null>>(
    {} as Record<TabId, HTMLDivElement | null>,
  );
  const scrollContainerRef = useRef<HTMLDivElement | null>(null);
  const isScrollingRef = useRef(false);

  const tabs: { id: TabId; label: string }[] = [
    { id: "general", label: t("settings.general.title") },
    { id: "logs", label: t("settings.logs.title") },
    { id: "proxy", label: t("settings.proxy.title") },
    { id: "trigger", label: t("settings.trigger.title") },
    { id: "notification", label: t("settings.notification.title") },
    { id: "credentials", label: t("credentials.settings_section") },
    { id: "cloud_sync", label: t("settings.cloud_sync.title") },
    { id: "data", label: t("settings.data.title") },
    { id: "about", label: t("settings.about.title") },
  ];

  // Scroll-spy: detect which section is in view
  const handleScroll = useCallback(() => {
    if (isScrollingRef.current) return;
    const container = scrollContainerRef.current;
    if (!container) return;
    const containerTop = container.getBoundingClientRect().top;
    let current: TabId = "general";
    let minDistance = Infinity;
    for (const tab of tabs) {
      const el = sectionRefs.current[tab.id];
      if (!el) continue;
      const elTop = el.getBoundingClientRect().top - containerTop;
      if (elTop <= 24 && Math.abs(elTop - 24) < minDistance) {
        minDistance = Math.abs(elTop - 24);
        current = tab.id;
      }
    }
    setActiveTab(current);
  }, []);

  // Click tab → scroll to section
  const handleTabClick = (id: TabId) => {
    const container = scrollContainerRef.current;
    const el = sectionRefs.current[id];
    if (!container || !el) return;
    isScrollingRef.current = true;
    setActiveTab(id);
    const containerTop = container.getBoundingClientRect().top;
    const elTop = el.getBoundingClientRect().top - containerTop;
    container.scrollTo({
      top: container.scrollTop + elTop - 16,
      behavior: "smooth",
    });
    // Release scroll lock after animation
    setTimeout(() => {
      isScrollingRef.current = false;
    }, 500);
  };

  return (
    <Modal
      title={t("settings.title")}
      onClose={onClose}
      maxWidth="max-w-4xl"
      zIndex="z-40"
      bodyClassName="overflow-hidden p-0"
    >
      <div className="flex" style={{ height: "70vh" }}>
        {/* Sidebar */}
        <nav className="w-52 flex-shrink-0 space-y-0.5 p-4 overflow-y-auto">
          {tabs.map((t2) => (
            <button
              key={t2.id}
              onClick={() => handleTabClick(t2.id)}
              className={`w-full relative flex items-center px-3 py-2 rounded-lg text-sm transition-all text-left ${
                activeTab === t2.id
                  ? "bg-blue-50 dark:bg-blue-900/20 text-blue-600 dark:text-blue-400 font-semibold"
                  : "text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-[#1E1E1E]/50"
              }`}
            >
              {activeTab === t2.id && (
                <span className="absolute left-0 top-1.5 bottom-1.5 w-0.5 bg-blue-500 rounded-r-full" />
              )}
              {t2.label}
            </button>
          ))}
        </nav>

        {/* Content — single scrollable page, macOS System Settings style */}
        <div
          ref={scrollContainerRef}
          onScroll={handleScroll}
          className="flex-1 min-w-0 bg-gray-100 dark:bg-[#1E1E1E] overflow-y-auto border-l border-gray-200/80 dark:border-white/[0.06]"
        >
          <div className="p-6 space-y-8">
            <div
              ref={(el) => {
                sectionRefs.current.general = el;
              }}
            >
              <GeneralSection />
            </div>
            <div
              ref={(el) => {
                sectionRefs.current.logs = el;
              }}
            >
              <LogSection />
            </div>
            <div
              ref={(el) => {
                sectionRefs.current.proxy = el;
              }}
            >
              <ProxyDefaultsSection />
            </div>
            <div
              ref={(el) => {
                sectionRefs.current.trigger = el;
              }}
            >
              <TriggerDefaultsSection />
            </div>
            <div
              ref={(el) => {
                sectionRefs.current.notification = el;
              }}
            >
              <NotificationSection />
            </div>
            <div
              ref={(el) => {
                sectionRefs.current.credentials = el;
              }}
            >
              <CredentialSection />
            </div>
            <div
              ref={(el) => {
                sectionRefs.current.cloud_sync = el;
              }}
            >
              <CloudSyncSection />
            </div>
            <div
              ref={(el) => {
                sectionRefs.current.data = el;
              }}
            >
              <DataManagementSection />
            </div>
            <div
              ref={(el) => {
                sectionRefs.current.about = el;
              }}
            >
              <AboutSection />
            </div>
          </div>
        </div>
      </div>
    </Modal>
  );
}

// === SECTION 1 END ===

// macOS System Settings-style group: section title + white rounded group container
function SettingGroup({
  title,
  subtitle,
  children,
}: {
  title: string;
  subtitle?: string;
  children: React.ReactNode;
}) {
  return (
    <section>
      <h3 className="text-base font-semibold text-gray-900 dark:text-gray-100 mb-1.5 px-1">
        {title}
      </h3>
      {subtitle && (
        <p className="text-sm text-gray-500 dark:text-gray-400 mb-3 px-1">
          {subtitle}
        </p>
      )}
      <div className="bg-white dark:bg-[#1E1E1E] rounded-xl border border-gray-200/80 dark:border-white/[0.06] overflow-hidden">
        {children}
      </div>
    </section>
  );
}

// Single setting item inside a group
function SettingItem({
  label,
  hint,
  children,
}: {
  label: React.ReactNode;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-4 px-4 py-3.5 border-b border-gray-100 dark:border-white/[0.06] last:border-0">
      <div className="min-w-0">
        <div className="text-sm font-medium text-gray-800 dark:text-gray-200">
          {label}
        </div>
        {hint && (
          <div className="text-xs text-gray-500 dark:text-gray-400 mt-0.5 leading-relaxed">
            {hint}
          </div>
        )}
      </div>
      <div className="flex-shrink-0">{children}</div>
    </div>
  );
}

// macOS-style toggle switch
function Toggle({
  checked,
  onChange,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors duration-200 ${
        checked ? "bg-blue-500" : "bg-gray-200 dark:bg-gray-600"
      }`}
    >
      <span
        className="inline-block h-5 w-5 rounded-full bg-white shadow-sm transition-transform duration-200"
        style={{ transform: checked ? "translateX(22px)" : "translateX(2px)" }}
      />
    </button>
  );
}

function GeneralSection() {
  const { t } = useTranslation();
  const config = useConfigStore((s) => s.config);
  const updateGeneral = useConfigStore((s) => s.updateGeneral);

  if (!config) return null;

  const updateAndSave = (patch: Record<string, unknown>) => {
    updateGeneral(patch as any);
    ipcInvoke("ipc_update_general_config", patch).catch((e) =>
      console.error("save general config failed:", e),
    );
  };

  return (
    <SettingGroup title={t("settings.general.title")}>
      <SettingItem label={t("settings.general.language")}>
        <select
          value={config.general.language}
          onChange={(e) => {
            const lang = e.target.value as SupportedLanguage;
            updateAndSave({ language: lang });
            i18n.changeLanguage(resolveLanguage(lang));
          }}
          className="input w-36"
        >
          <option value="system">{t("settings.theme.system")}</option>
          <option value="zh-CN">简体中文</option>
          <option value="en">English</option>
        </select>
      </SettingItem>
      <SettingItem label={t("settings.general.theme")}>
        <select
          value={config.general.theme}
          onChange={(e) => updateAndSave({ theme: e.target.value })}
          className="input w-36"
        >
          <option value="system">{t("settings.theme.system")}</option>
          <option value="light">{t("settings.theme.light")}</option>
          <option value="dark">{t("settings.theme.dark")}</option>
        </select>
      </SettingItem>
      <SettingItem label={t("settings.general.auto_start")}>
        <Toggle
          checked={config.general.auto_start}
          onChange={(v) => {
            updateAndSave({ auto_start: v });
            ipcInvoke("ipc_set_autostart", { enabled: v }).catch((err) =>
              console.error("set autostart failed:", err),
            );
          }}
        />
      </SettingItem>
      <SettingItem label={t("settings.general.minimize_to_tray")}>
        <Toggle
          checked={config.general.minimize_to_tray}
          onChange={(v) => updateAndSave({ minimize_to_tray: v })}
        />
      </SettingItem>
    </SettingGroup>
  );
}

// === SECTION 2 END ===

function LogSection() {
  const { t } = useTranslation();
  const config = useConfigStore((s) => s.config);
  const updateGeneral = useConfigStore((s) => s.updateGeneral);

  if (!config) return null;

  const updateAndSave = (patch: Record<string, unknown>) => {
    updateGeneral(patch as any);
    ipcInvoke("ipc_update_general_config", patch).catch((e) =>
      console.error("save log config failed:", e),
    );
  };

  return (
    <SettingGroup title={t("settings.logs.title")}>
      <SettingItem label={t("settings.general.log_level")}>
        <select
          value={config.general.log_level}
          onChange={(e) => updateAndSave({ log_level: e.target.value })}
          className="input w-32"
        >
          <option value="debug">{t("logs.level_debug")}</option>
          <option value="info">{t("logs.level_info")}</option>
          <option value="warn">{t("logs.level_warn")}</option>
          <option value="error">{t("logs.level_error")}</option>
        </select>
      </SettingItem>
      <SettingItem label={t("settings.logs.to_file")}>
        <Toggle
          checked={config.general.log_to_file}
          onChange={(v) => updateAndSave({ log_to_file: v })}
        />
      </SettingItem>
      <SettingItem label={t("settings.logs.max_days")}>
        <input
          type="number"
          value={config.general.log_max_days}
          onChange={(e) =>
            updateAndSave({ log_max_days: parseInt(e.target.value) || 30 })
          }
          className="input w-24"
        />
      </SettingItem>
      <SettingItem label={t("settings.logs.max_size")}>
        <input
          type="number"
          value={config.general.log_max_size_mb}
          onChange={(e) =>
            updateAndSave({ log_max_size_mb: parseInt(e.target.value) || 10 })
          }
          className="input w-24"
        />
      </SettingItem>
    </SettingGroup>
  );
}

// === SECTION 3 END ===

function EmptySection({ desc }: { desc: string }) {
  return (
    <div className="px-4 py-5 text-sm text-gray-500 dark:text-gray-400 leading-relaxed">
      {desc}
    </div>
  );
}

function ProxyDefaultsSection() {
  const { t } = useTranslation();
  return (
    <SettingGroup title={t("settings.proxy.title")}>
      <EmptySection desc={t("settings.proxy.desc")} />
    </SettingGroup>
  );
}

function TriggerDefaultsSection() {
  const { t } = useTranslation();
  return (
    <SettingGroup title={t("settings.trigger.title")}>
      <EmptySection desc={t("settings.trigger.desc")} />
    </SettingGroup>
  );
}

// === SECTION 4 END ===

function DataManagementSection() {
  const { t } = useTranslation();
  const Chevron = (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className="text-gray-400"
    >
      <polyline points="9 18 15 12 9 6" />
    </svg>
  );
  return (
    <SettingGroup title={t("settings.data.title")}>
      <button
        className="w-full text-left"
        onClick={() => ipcInvoke("ipc_export_config").catch(console.error)}
      >
        <SettingItem label={t("settings.data.export")}>{Chevron}</SettingItem>
      </button>
      <button
        className="w-full text-left"
        onClick={() => ipcInvoke("ipc_import_config").catch(console.error)}
      >
        <SettingItem label={t("settings.data.import")}>{Chevron}</SettingItem>
      </button>
    </SettingGroup>
  );
}

// === SECTION 5 END ===

function NotificationSection() {
  const { t } = useTranslation();
  const config = useConfigStore((s) => s.config);
  const updateGeneral = useConfigStore((s) => s.updateGeneral);

  if (!config) return null;

  const updateAndSave = (patch: Record<string, unknown>) => {
    updateGeneral(patch as any);
    ipcInvoke("ipc_update_general_config", patch).catch((e) =>
      console.error("save notification config failed:", e),
    );
  };

  const items: { key: keyof typeof config.general; label: string }[] = [
    {
      key: "notify_connect_success",
      label: t("settings.notification.connect_success"),
    },
    { key: "notify_disconnect", label: t("settings.notification.disconnect") },
    {
      key: "notify_reconnect_success",
      label: t("settings.notification.reconnect_success"),
    },
    { key: "notify_auth_fail", label: t("settings.notification.auth_fail") },
    {
      key: "notify_proxy_toggle",
      label: t("settings.notification.proxy_toggle"),
    },
    {
      key: "notify_proxy_port_conflict",
      label: t("settings.notification.proxy_port_conflict"),
    },
    {
      key: "notify_trigger_fail",
      label: t("settings.notification.trigger_fail"),
    },
    {
      key: "notify_trigger_success",
      label: t("settings.notification.trigger_success"),
    },
    { key: "notify_ip_change", label: t("settings.notification.ip_change") },
  ];

  return (
    <SettingGroup title={t("settings.notification.title")}>
      {items.map((item) => (
        <SettingItem key={item.key} label={item.label}>
          <Toggle
            checked={(config.general[item.key] as boolean) || false}
            onChange={(v) => updateAndSave({ [item.key]: v })}
          />
        </SettingItem>
      ))}
    </SettingGroup>
  );
}

// === SECTION 6 END ===

function AboutSection() {
  const { t } = useTranslation();
  const [checking, setChecking] = useState(false);

  const handleCheck = async () => {
    if (checking) return;
    setChecking(true);
    const toastId = toast.loading(t("settings.about.checking"));
    try {
      const result = await checkForUpdate();
      toast.dismiss(toastId);
      if (!result) {
        toast.success(t("settings.about.latest"));
      } else {
        promptInstallUpdate(t, result);
      }
    } catch (e) {
      toast.dismiss(toastId);
      toast.error(t("settings.about.failed"));
      console.error("[AboutSection] update check failed:", e);
    } finally {
      setChecking(false);
    }
  };

  return (
    <SettingGroup title={t("settings.about.title")}>
      <SettingItem
        label={
          <div className="flex items-center gap-3">
            <div className="w-9 h-9 rounded-lg bg-blue-500 text-white flex items-center justify-center text-base font-bold">
              V
            </div>
            <div>
              <div className="text-sm font-medium text-gray-900 dark:text-gray-100">
                TermFast
              </div>
              <div className="text-xs text-gray-500 dark:text-gray-400">
                v{APP_VERSION}
              </div>
            </div>
          </div>
        }
      >
        <span className="text-xs text-gray-500 dark:text-gray-400">
          {t("common.built_in")}
        </span>
      </SettingItem>
      <button
        className={`w-full text-left ${checking ? "opacity-50 cursor-not-allowed" : ""}`}
        onClick={handleCheck}
        disabled={checking}
      >
        <SettingItem label={t("settings.about.check_update")}>
          {checking ? (
            <span className="text-xs text-gray-400">
              {t("settings.about.checking")}
            </span>
          ) : (
            <svg
              width="16"
              height="16"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
              className="text-gray-400"
            >
              <polyline points="9 18 15 12 9 6" />
            </svg>
          )}
        </SettingItem>
      </button>
    </SettingGroup>
  );
}

function promptInstallUpdate(
  t: (key: string, options?: Record<string, string>) => string,
  result: UpdateResult,
) {
  const version = result.info.version;
  let progressToastId: string | number | undefined;

  toast(
    <div className="flex flex-col gap-2">
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
          progressToastId = toast.loading(t("settings.about.installing"));
          try {
            await installUpdate(result.update, (percent) => {
              if (progressToastId) {
                toast.loading(`${t("settings.about.installing")} ${percent}%`, {
                  id: progressToastId,
                });
              }
            });
            toast.dismiss(progressToastId);
            toast.success(t("settings.about.installed"));
          } catch (e) {
            toast.dismiss(progressToastId);
            toast.error(t("settings.about.failed"));
            console.error("[AboutSection] install failed:", e);
          }
        },
      },
    },
  );
}

const APP_VERSION = "0.1.0";

// === CREDENTIAL SECTION ===

function CredentialSection() {
  const { t } = useTranslation();
  const [credStatus, setCredStatus] = useState<string>("pending");
  const [showSetup, setShowSetup] = useState(false);
  const [showUnlock, setShowUnlock] = useState(false);
  const [unlockPw, setUnlockPw] = useState("");
  const [showChangePassword, setShowChangePassword] = useState(false);
  const [showReset, setShowReset] = useState(false);
  const [setupPw, setSetupPw] = useState("");
  const [setupConfirmPw, setSetupConfirmPw] = useState("");
  const [oldPw, setOldPw] = useState("");
  const [newPw, setNewPw] = useState("");
  const [confirmPw, setConfirmPw] = useState("");
  const [busy, setBusy] = useState(false);

  // Fetch credential status on mount.
  const refreshStatus = useCallback(async () => {
    try {
      const status = await ipcInvoke<string>("ipc_credential_status");
      setCredStatus(typeof status === "string" ? status : "pending");
    } catch {
      setCredStatus("pending");
    }
  }, []);
  useEffect(() => { refreshStatus(); }, [refreshStatus]);

  const handleSetup = useCallback(async () => {
    if (setupPw !== setupConfirmPw || setupPw.length < 4) return;
    setBusy(true);
    try {
      await ipcInvoke("ipc_initialize_credentials", { masterPassword: setupPw });
      toast.success(t("credentials.setup_title"));
      setShowSetup(false);
      setSetupPw("");
      setSetupConfirmPw("");
      refreshStatus();
    } catch (e: any) {
      toast.error(e?.message || String(e));
    } finally {
      setBusy(false);
    }
  }, [setupPw, setupConfirmPw, t, refreshStatus]);

  const handleUnlock = useCallback(async () => {
    if (!unlockPw) return;
    setBusy(true);
    try {
      await ipcInvoke("ipc_unlock_credentials", { masterPassword: unlockPw });
      toast.success(t("credentials.unlock_button"));
      setShowUnlock(false);
      setUnlockPw("");
      refreshStatus();
    } catch (e: any) {
      toast.error(e?.message || String(e));
    } finally {
      setBusy(false);
    }
  }, [unlockPw, t, refreshStatus]);

  const handleChangePassword = useCallback(async () => {
    if (newPw !== confirmPw || newPw.length < 4) return;
    setBusy(true);
    try {
      await ipcInvoke("ipc_change_credential_password", {
        oldPassword: oldPw,
        newPassword: newPw,
      });
      toast.success(t("credentials.change_password_title"));
      setShowChangePassword(false);
      setOldPw("");
      setNewPw("");
      setConfirmPw("");
    } catch (e: any) {
      toast.error(e?.message || String(e));
    } finally {
      setBusy(false);
    }
  }, [oldPw, newPw, confirmPw, t]);

  const handleReset = useCallback(async () => {
    setBusy(true);
    try {
      await ipcInvoke("ipc_reset_credentials");
      toast.success(t("credentials.reset_title"));
      setShowReset(false);
      refreshStatus();
    } catch (e: any) {
      toast.error(e?.message || String(e));
    } finally {
      setBusy(false);
    }
  }, [t, refreshStatus]);

  const handleExport = useCallback(async () => {
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const filePath = await save({
        defaultPath: "termfast-credentials.enc",
        filters: [{ name: "Encrypted Backup", extensions: ["enc"] }],
      });
      if (!filePath) return;
      await ipcInvoke("ipc_export_credentials", { destPath: filePath });
      toast.success(t("credentials.export_button"));
    } catch (e: any) {
      toast.error(e?.message || String(e));
    }
  }, [t]);

  const [showImportPw, setShowImportPw] = useState(false);
  const [importPath, setImportPath] = useState("");
  const [importPw, setImportPw] = useState("");

  const handleImport = useCallback(async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const filePath = await open({
        filters: [{ name: "Encrypted Backup", extensions: ["enc"] }],
        multiple: false,
      });
      if (!filePath || typeof filePath !== "string") return;
      setImportPath(filePath);
      setImportPw("");
      setShowImportPw(true);
    } catch (e: any) {
      toast.error(e?.message || String(e));
    }
  }, []);

  const handleImportConfirm = useCallback(async () => {
    if (!importPw) return;
    setBusy(true);
    try {
      await ipcInvoke("ipc_import_credentials", {
        srcPath: importPath,
        masterPassword: importPw,
      });
      toast.success(t("credentials.import_button"));
      setShowImportPw(false);
      setImportPw("");
      setImportPath("");
      refreshStatus();
    } catch (e: any) {
      toast.error(e?.message || String(e));
    } finally {
      setBusy(false);
    }
  }, [importPath, importPw, t, refreshStatus]);

  const isPending = credStatus === "pending";
  const isLocked = credStatus === "locked";

  return (
    <section className="space-y-4 pt-4">
      <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
        {t("credentials.settings_section")}
      </h3>

      <SettingGroup title={t("credentials.settings_section")}>
        {isPending ? (
          <SettingItem
            label={t("credentials.setup_title")}
            hint={t("credentials.setup_description")}
          >
            <button
              onClick={() => setShowSetup(true)}
              className="px-4 py-2 rounded-lg bg-blue-600 text-white text-sm font-medium hover:bg-blue-700 transition-colors"
            >
              {t("credentials.setup_button")}
            </button>
          </SettingItem>
        ) : isLocked ? (
          <SettingItem
            label={t("credentials.unlock_button")}
            hint={t("credentials.unlock_description")}
          >
            <button
              onClick={() => setShowUnlock(true)}
              className="px-4 py-2 rounded-lg bg-blue-600 text-white text-sm font-medium hover:bg-blue-700 transition-colors"
            >
              {t("credentials.unlock_button")}
            </button>
          </SettingItem>
        ) : (
          <>
            <SettingItem
              label={t("credentials.change_password_title")}
              hint={t("credentials.setup_description")}
            >
              <button
                onClick={() => setShowChangePassword(true)}
                className="px-4 py-2 rounded-lg bg-gray-100 dark:bg-[#2A2A2A] text-gray-700 dark:text-gray-300 text-sm font-medium hover:bg-gray-200 dark:hover:bg-[#333] transition-colors"
              >
                {t("credentials.change_password_button")}
              </button>
            </SettingItem>
            <SettingItem
              label={t("credentials.export_button")}
            >
              <button
                onClick={handleExport}
                className="px-4 py-2 rounded-lg bg-gray-100 dark:bg-[#2A2A2A] text-gray-700 dark:text-gray-300 text-sm font-medium hover:bg-gray-200 dark:hover:bg-[#333] transition-colors"
              >
                {t("credentials.export_button")}
              </button>
            </SettingItem>
            <SettingItem
              label={t("credentials.import_button")}
            >
              <button
                onClick={handleImport}
                className="px-4 py-2 rounded-lg bg-gray-100 dark:bg-[#2A2A2A] text-gray-700 dark:text-gray-300 text-sm font-medium hover:bg-gray-200 dark:hover:bg-[#333] transition-colors"
              >
                {t("credentials.import_button")}
              </button>
            </SettingItem>
            <SettingItem
              label={t("credentials.reset_title")}
              hint={t("credentials.reset_description")}
            >
              <button
                onClick={() => setShowReset(true)}
                className="px-4 py-2 rounded-lg bg-red-50 dark:bg-red-900/20 text-red-600 dark:text-red-400 text-sm font-medium hover:bg-red-100 dark:hover:bg-red-900/30 transition-colors"
              >
                {t("credentials.reset_button")}
              </button>
            </SettingItem>
          </>
        )}
      </SettingGroup>

      {/* Setup password modal */}
      {showSetup && (
        <Modal
          title={t("credentials.setup_title")}
          onClose={() => setShowSetup(false)}
        >
          <div className="p-6 space-y-4">
            <h3 className="text-lg font-semibold">
              {t("credentials.setup_title")}
            </h3>
            <p className="text-sm text-gray-600 dark:text-gray-400">
              {t("credentials.setup_description")}
            </p>
            <input
              type="password"
              placeholder={t("credentials.master_password")}
              value={setupPw}
              onChange={(e) => setSetupPw(e.target.value)}
              className="w-full px-4 py-2.5 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-[#2A2A2A] text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-blue-500 outline-none"
            />
            <input
              type="password"
              placeholder={t("credentials.confirm_password")}
              value={setupConfirmPw}
              onChange={(e) => setSetupConfirmPw(e.target.value)}
              className="w-full px-4 py-2.5 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-[#2A2A2A] text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-blue-500 outline-none"
            />
            {setupPw && setupConfirmPw && setupPw !== setupConfirmPw && (
              <p className="text-xs text-red-500">
                {t("credentials.password_mismatch")}
              </p>
            )}
            <div className="flex gap-3 justify-end">
              <button
                onClick={() => setShowSetup(false)}
                className="px-4 py-2 rounded-lg text-gray-600 dark:text-gray-400 text-sm"
              >
                {t("common.cancel")}
              </button>
              <button
                onClick={handleSetup}
                disabled={busy || setupPw.length < 4 || setupPw !== setupConfirmPw}
                className="px-4 py-2 rounded-lg bg-blue-600 text-white text-sm font-medium hover:bg-blue-700 disabled:opacity-50"
              >
                {busy ? t("common.loading") : t("credentials.setup_button")}
              </button>
            </div>
          </div>
        </Modal>
      )}

      {/* Unlock password modal */}
      {showUnlock && (
        <Modal
          title={t("credentials.unlock_button")}
          onClose={() => setShowUnlock(false)}
        >
          <div className="p-6 space-y-4">
            <h3 className="text-lg font-semibold">
              {t("credentials.unlock_button")}
            </h3>
            <p className="text-sm text-gray-600 dark:text-gray-400">
              {t("credentials.unlock_description")}
            </p>
            <input
              type="password"
              placeholder={t("credentials.master_password")}
              value={unlockPw}
              onChange={(e) => setUnlockPw(e.target.value)}
              className="w-full px-4 py-2.5 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-[#2A2A2A] text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-blue-500 outline-none"
            />
            <div className="flex gap-3 justify-end">
              <button
                onClick={() => setShowUnlock(false)}
                className="px-4 py-2 rounded-lg text-gray-600 dark:text-gray-400 text-sm"
              >
                {t("common.cancel")}
              </button>
              <button
                onClick={handleUnlock}
                disabled={busy || !unlockPw}
                className="px-4 py-2 rounded-lg bg-blue-600 text-white text-sm font-medium hover:bg-blue-700 disabled:opacity-50"
              >
                {busy ? t("common.loading") : t("credentials.unlock_button")}
              </button>
            </div>
          </div>
        </Modal>
      )}

      {/* Change password modal */}
      {showChangePassword && (
        <Modal
          title={t("credentials.change_password_title")}
          onClose={() => setShowChangePassword(false)}
        >
          <div className="p-6 space-y-4">
            <h3 className="text-lg font-semibold">
              {t("credentials.change_password_title")}
            </h3>
            <input
              type="password"
              placeholder={t("credentials.change_password_old")}
              value={oldPw}
              onChange={(e) => setOldPw(e.target.value)}
              className="w-full px-4 py-2.5 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-[#2A2A2A] text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-blue-500 outline-none"
            />
            <input
              type="password"
              placeholder={t("credentials.change_password_new")}
              value={newPw}
              onChange={(e) => setNewPw(e.target.value)}
              className="w-full px-4 py-2.5 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-[#2A2A2A] text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-blue-500 outline-none"
            />
            <input
              type="password"
              placeholder={t("credentials.change_password_confirm")}
              value={confirmPw}
              onChange={(e) => setConfirmPw(e.target.value)}
              className="w-full px-4 py-2.5 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-[#2A2A2A] text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-blue-500 outline-none"
            />
            {newPw && confirmPw && newPw !== confirmPw && (
              <p className="text-xs text-red-500">
                {t("credentials.password_mismatch")}
              </p>
            )}
            <div className="flex gap-3 justify-end">
              <button
                onClick={() => setShowChangePassword(false)}
                className="px-4 py-2 rounded-lg text-gray-600 dark:text-gray-400 text-sm"
              >
                {t("common.cancel")}
              </button>
              <button
                onClick={handleChangePassword}
                disabled={busy || !oldPw || newPw.length < 4 || newPw !== confirmPw}
                className="px-4 py-2 rounded-lg bg-blue-600 text-white text-sm font-medium hover:bg-blue-700 disabled:opacity-50"
              >
                {busy ? t("common.loading") : t("credentials.change_password_button")}
              </button>
            </div>
          </div>
        </Modal>
      )}

      {/* Reset confirm modal */}
      {showReset && (
        <Modal
          title={t("credentials.reset_title")}
          onClose={() => setShowReset(false)}
        >
          <div className="p-6 space-y-4">
            <h3 className="text-lg font-semibold text-red-600">
              {t("credentials.reset_title")}
            </h3>
            <p className="text-sm text-gray-600 dark:text-gray-400">
              {t("credentials.reset_description")}
            </p>
            <div className="flex gap-3 justify-end">
              <button
                onClick={() => setShowReset(false)}
                className="px-4 py-2 rounded-lg text-gray-600 dark:text-gray-400 text-sm"
              >
                {t("common.cancel")}
              </button>
              <button
                onClick={handleReset}
                disabled={busy}
                className="px-4 py-2 rounded-lg bg-red-600 text-white text-sm font-medium hover:bg-red-700 disabled:opacity-50"
              >
                {busy ? t("common.loading") : t("credentials.reset_button")}
              </button>
            </div>
          </div>
        </Modal>
      )}

      {/* Import password modal */}
      {showImportPw && (
        <Modal
          title={t("credentials.import_button")}
          onClose={() => setShowImportPw(false)}
        >
          <div className="p-6 space-y-4">
            <h3 className="text-lg font-semibold">
              {t("credentials.import_button")}
            </h3>
            <p className="text-sm text-gray-600 dark:text-gray-400">
              {t("credentials.import_password_hint")}
            </p>
            <input
              type="password"
              placeholder={t("credentials.master_password")}
              value={importPw}
              onChange={(e) => setImportPw(e.target.value)}
              className="w-full px-4 py-2.5 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-[#2A2A2A] text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-blue-500 outline-none"
            />
            <div className="flex gap-3 justify-end">
              <button
                onClick={() => setShowImportPw(false)}
                className="px-4 py-2 rounded-lg text-gray-600 dark:text-gray-400 text-sm"
              >
                {t("common.cancel")}
              </button>
              <button
                onClick={handleImportConfirm}
                disabled={busy || !importPw}
                className="px-4 py-2 rounded-lg bg-blue-600 text-white text-sm font-medium hover:bg-blue-700 disabled:opacity-50"
              >
                {busy ? t("common.loading") : t("credentials.import_button")}
              </button>
            </div>
          </div>
        </Modal>
      )}
    </section>
  );
}

// === Cloud Sync Section ===

function CloudSyncSection() {
  const { t } = useTranslation();
  const [provider, setProvider] = useState<"dropbox" | "baidu">("dropbox");
  const [passphrase, setPassphrase] = useState("");
  const [masterPassword, setMasterPassword] = useState("");
  const [authUrl, setAuthUrl] = useState("");
  const [codeVerifier, setCodeVerifier] = useState("");
  const [authCode, setAuthCode] = useState("");
  const [accessToken, setAccessToken] = useState("");
  const [isAuthed, setIsAuthed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [step, setStep] = useState<"idle" | "auth" | "code" | "token">("idle");

  const checkAuth = async () => {
    if (!passphrase) return;
    try {
      const res = await ipcInvoke<{
        authenticated: boolean;
        access_token?: string;
      }>("ipc_cloud_sync_load_token", { provider, passphrase });
      setIsAuthed(res.authenticated);
      if (res.access_token) setAccessToken(res.access_token);
    } catch {
      setIsAuthed(false);
    }
  };

  const startAuth = async () => {
    setBusy(true);
    try {
      const res = await ipcInvoke<{
        auth_url: string;
        code_verifier: string | null;
      }>("ipc_cloud_sync_auth_url", { provider });
      setAuthUrl(res.auth_url);
      setCodeVerifier(res.code_verifier ?? "");
      setStep(res.code_verifier ? "code" : "token");
      // Open URL in browser
      window.open(res.auth_url, "_blank");
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  };

  const exchangeCode = async () => {
    setBusy(true);
    try {
      const res = await ipcInvoke<{
        access_token: string;
        refresh_token?: string;
        expires_at?: number;
      }>("ipc_cloud_sync_exchange_code", {
        provider,
        code: authCode,
        code_verifier: codeVerifier,
      });
      await saveToken(res.access_token, res.refresh_token, res.expires_at);
      setStep("idle");
      setAuthCode("");
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  };

  const saveToken = async (
    token: string,
    refreshToken?: string,
    expiresAt?: number,
  ) => {
    try {
      await ipcInvoke("ipc_cloud_sync_save_token", {
        provider,
        passphrase,
        access_token: token,
        refresh_token: refreshToken,
        expires_at: expiresAt,
      });
      setAccessToken(token);
      setIsAuthed(true);
      toast.success(t("settings.cloud_sync.connected"));
    } catch (e) {
      toast.error(String(e));
    }
  };

  const saveManualToken = async () => {
    if (!accessToken.trim()) {
      toast.error(t("settings.cloud_sync.enter_token"));
      return;
    }
    setBusy(true);
    try {
      await saveToken(accessToken.trim());
      setStep("idle");
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  };

  const upload = async () => {
    if (!masterPassword) {
      toast.error(t("settings.cloud_sync.enter_master_password"));
      return;
    }
    setBusy(true);
    try {
      await ipcInvoke("ipc_cloud_sync_upload", {
        provider,
        passphrase,
        master_password: masterPassword,
      });
      toast.success(t("settings.cloud_sync.upload_success"));
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  };

  const download = async () => {
    if (!masterPassword) {
      toast.error(t("settings.cloud_sync.enter_master_password"));
      return;
    }
    setBusy(true);
    try {
      const res = await ipcInvoke<{ blob: string; size: number }>(
        "ipc_cloud_sync_download",
        { provider, passphrase },
      );
      // Import the downloaded blob
      await ipcInvoke("ipc_import_full", {
        master_password: masterPassword,
        blob: res.blob,
      });
      toast.success(t("settings.cloud_sync.download_success"));
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  };

  const disconnect = async () => {
    setBusy(true);
    try {
      await ipcInvoke("ipc_cloud_sync_disconnect", { provider, passphrase });
      setIsAuthed(false);
      setAccessToken("");
      toast.success(t("settings.cloud_sync.disconnected"));
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="space-y-4">
      <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
        {t("settings.cloud_sync.title")}
      </h3>
      <p className="text-sm text-gray-500 dark:text-gray-400">
        {t("settings.cloud_sync.description")}
      </p>

      {/* Provider selection */}
      <div className="flex gap-2">
        {(["dropbox", "baidu"] as const).map((p) => (
          <button
            key={p}
            onClick={() => {
              setProvider(p);
              setIsAuthed(false);
              setStep("idle");
            }}
            className={`px-4 py-2 rounded-lg text-sm font-medium transition-colors ${
              provider === p
                ? "bg-blue-500 text-white"
                : "bg-gray-200 dark:bg-gray-700 text-gray-700 dark:text-gray-300 hover:bg-gray-300 dark:hover:bg-gray-600"
            }`}
          >
            {p === "dropbox" ? "Dropbox" : "百度网盘"}
          </button>
        ))}
      </div>

      {/* Passphrase for token encryption */}
      <div>
        <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
          {t("settings.cloud_sync.token_passphrase")}
        </label>
        <input
          type="password"
          value={passphrase}
          onChange={(e) => setPassphrase(e.target.value)}
          onBlur={checkAuth}
          placeholder={t("settings.cloud_sync.token_passphrase_placeholder")}
          className="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm text-gray-900 dark:text-gray-100"
        />
      </div>

      {/* Auth status + connect/disconnect */}
      {!isAuthed ? (
        <div className="space-y-3">
          <button
            onClick={startAuth}
            disabled={busy || !passphrase}
            className="px-4 py-2 bg-blue-500 text-white rounded-lg text-sm font-medium hover:bg-blue-600 disabled:opacity-50"
          >
            {busy ? t("common.loading") : t("settings.cloud_sync.connect")}
          </button>

          {step === "code" && (
            <div className="space-y-2 p-3 bg-blue-50 dark:bg-blue-900/20 rounded-lg">
              <p className="text-sm text-blue-700 dark:text-blue-300">
                {t("settings.cloud_sync.paste_code")}
              </p>
              <input
                type="text"
                value={authCode}
                onChange={(e) => setAuthCode(e.target.value)}
                placeholder="authorization code"
                className="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm"
              />
              <button
                onClick={exchangeCode}
                disabled={busy || !authCode}
                className="px-4 py-2 bg-green-500 text-white rounded-lg text-sm font-medium hover:bg-green-600 disabled:opacity-50"
              >
                {t("settings.cloud_sync.exchange")}
              </button>
            </div>
          )}

          {step === "token" && (
            <div className="space-y-2 p-3 bg-blue-50 dark:bg-blue-900/20 rounded-lg">
              <p className="text-sm text-blue-700 dark:text-blue-300">
                {t("settings.cloud_sync.paste_token")}
              </p>
              <textarea
                value={accessToken}
                onChange={(e) => setAccessToken(e.target.value)}
                placeholder="access_token"
                rows={3}
                className="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm font-mono"
              />
              <button
                onClick={saveManualToken}
                disabled={busy || !accessToken.trim()}
                className="px-4 py-2 bg-green-500 text-white rounded-lg text-sm font-medium hover:bg-green-600 disabled:opacity-50"
              >
                {t("settings.cloud_sync.save")}
              </button>
            </div>
          )}
        </div>
      ) : (
        <div className="space-y-3">
          <div className="flex items-center gap-2 text-sm text-green-600 dark:text-green-400">
            <span className="w-2 h-2 rounded-full bg-green-500" />
            {t("settings.cloud_sync.connected_status")}
          </div>

          {/* Master password for config encryption */}
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
              {t("settings.cloud_sync.master_password")}
            </label>
            <input
              type="password"
              value={masterPassword}
              onChange={(e) => setMasterPassword(e.target.value)}
              placeholder={t("settings.cloud_sync.master_password_placeholder")}
              className="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm"
            />
          </div>

          <div className="flex gap-2">
            <button
              onClick={upload}
              disabled={busy || !masterPassword}
              className="px-4 py-2 bg-blue-500 text-white rounded-lg text-sm font-medium hover:bg-blue-600 disabled:opacity-50"
            >
              {busy ? t("common.loading") : t("settings.cloud_sync.upload")}
            </button>
            <button
              onClick={download}
              disabled={busy || !masterPassword}
              className="px-4 py-2 bg-blue-500 text-white rounded-lg text-sm font-medium hover:bg-blue-600 disabled:opacity-50"
            >
              {busy ? t("common.loading") : t("settings.cloud_sync.download")}
            </button>
            <button
              onClick={disconnect}
              disabled={busy}
              className="px-4 py-2 bg-red-500 text-white rounded-lg text-sm font-medium hover:bg-red-600 disabled:opacity-50"
            >
              {t("settings.cloud_sync.disconnect")}
            </button>
          </div>
        </div>
      )}
    </section>
  );
}
