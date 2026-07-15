// SettingsPage — settings UI (§9.5 / FP-8.8)
// Sidebar nav + single scrollable page with scroll-spy

import { useState, useRef, useCallback } from "react";
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
  "general" | "logs" | "proxy" | "trigger" | "notification" | "data" | "about";

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
