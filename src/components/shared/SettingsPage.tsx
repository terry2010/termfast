// SettingsPage — settings UI (§9.5 / FP-8.8)
// Sections: General, Logs, Notifications, About

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useConfigStore } from "@/stores/configStore";
import { ipcInvoke } from "@/hooks/useIpc";
import type { SupportedLanguage } from "@/i18n/config";
import i18n, { resolveLanguage } from "@/i18n/config";

export function SettingsPage({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();

  return (
    <div
      className="fixed inset-0 bg-black/50 flex items-center justify-center z-40"
      onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div
        className="bg-white dark:bg-gray-800 rounded-lg w-full max-w-2xl mx-4 max-h-[80vh] overflow-y-auto"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between p-4 border-b border-gray-200 dark:border-gray-700">
          <h2 className="text-lg font-medium">{t("settings.title")}</h2>
          <button
            className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 text-xl leading-none"
            onClick={onClose}
            aria-label="Close"
          >
            ✕
          </button>
        </div>
        <div className="p-4 space-y-6">
          <GeneralSection />
          <LogSection />
          <ProxyDefaultsSection />
          <TriggerDefaultsSection />
          <NotificationSection />
          <DataManagementSection />
          <AboutSection />
        </div>
      </div>
    </div>
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
      console.error("save general config failed:", e)
    );
  };

  return (
    <section>
      <h3 className="text-sm font-medium mb-3">{t("settings.general.title")}</h3>
      <div className="space-y-3">
        <label className="flex items-center justify-between">
          <span className="text-sm">{t("settings.general.language")}</span>
          <select
            value={config.general.language}
            onChange={(e) => {
              const lang = e.target.value as SupportedLanguage;
              updateAndSave({ language: lang });
              i18n.changeLanguage(resolveLanguage(lang));
            }}
            className="px-2 py-1 text-sm border border-gray-300 dark:border-gray-600 rounded bg-transparent"
          >
            <option value="system">{t("settings.theme.system")}</option>
            <option value="zh-CN">简体中文</option>
            <option value="en">English</option>
          </select>
        </label>
        <label className="flex items-center justify-between">
          <span className="text-sm">{t("settings.general.theme")}</span>
          <select
            value={config.general.theme}
            onChange={(e) => updateAndSave({ theme: e.target.value })}
            className="px-2 py-1 text-sm border border-gray-300 dark:border-gray-600 rounded bg-transparent"
          >
            <option value="system">{t("settings.theme.system")}</option>
            <option value="light">{t("settings.theme.light")}</option>
            <option value="dark">{t("settings.theme.dark")}</option>
          </select>
        </label>
        <label className="flex items-center justify-between">
          <span className="text-sm">{t("settings.general.auto_start")}</span>
          <input
            type="checkbox"
            checked={config.general.auto_start}
            onChange={(e) => {
              updateAndSave({ auto_start: e.target.checked });
              ipcInvoke("ipc_set_autostart", { enabled: e.target.checked }).catch((err) =>
                console.error("set autostart failed:", err)
              );
            }}
          />
        </label>
        <label className="flex items-center justify-between">
          <span className="text-sm">{t("settings.general.minimize_to_tray")}</span>
          <input
            type="checkbox"
            checked={config.general.minimize_to_tray}
            onChange={(e) => updateAndSave({ minimize_to_tray: e.target.checked })}
          />
        </label>
      </div>
    </section>
  );
}

// === SECTION 1 END ===

function LogSection() {
  const { t } = useTranslation();
  const config = useConfigStore((s) => s.config);
  const updateGeneral = useConfigStore((s) => s.updateGeneral);

  if (!config) return null;

  const updateAndSave = (patch: Record<string, unknown>) => {
    updateGeneral(patch as any);
    ipcInvoke("ipc_update_general_config", patch).catch((e) =>
      console.error("save log config failed:", e)
    );
  };

  return (
    <section>
      <h3 className="text-sm font-medium mb-3">{t("settings.logs.title")}</h3>
      <div className="space-y-3">
        <label className="flex items-center justify-between">
          <span className="text-sm">{t("settings.general.log_level") }</span>
          <select
            value={config.general.log_level}
            onChange={(e) => updateAndSave({ log_level: e.target.value })}
            className="px-2 py-1 text-sm border border-gray-300 dark:border-gray-600 rounded bg-transparent"
          >
            <option value="debug">{t("logs.level_debug")}</option>
            <option value="info">{t("logs.level_info")}</option>
            <option value="warn">{t("logs.level_warn")}</option>
            <option value="error">{t("logs.level_error")}</option>
          </select>
        </label>
        <label className="flex items-center justify-between">
          <span className="text-sm">{t("settings.logs.to_file") }</span>
          <input
            type="checkbox"
            checked={config.general.log_to_file}
            onChange={(e) => updateAndSave({ log_to_file: e.target.checked })}
          />
        </label>
        <label className="flex items-center justify-between">
          <span className="text-sm">{t("settings.logs.max_days") }</span>
          <input
            type="number"
            value={config.general.log_max_days}
            onChange={(e) => updateAndSave({ log_max_days: parseInt(e.target.value) || 30 })}
            className="w-20 px-2 py-1 text-sm border border-gray-300 dark:border-gray-600 rounded bg-transparent"
          />
        </label>
        <label className="flex items-center justify-between">
          <span className="text-sm">{t("settings.logs.max_size") }</span>
          <input
            type="number"
            value={config.general.log_max_size_mb}
            onChange={(e) => updateAndSave({ log_max_size_mb: parseInt(e.target.value) || 10 })}
            className="w-20 px-2 py-1 text-sm border border-gray-300 dark:border-gray-600 rounded bg-transparent"
          />
        </label>
      </div>
    </section>
  );
}

function ProxyDefaultsSection() {
  const { t } = useTranslation();
  return (
    <section>
      <h3 className="text-sm font-medium mb-3">{t("settings.proxy.title") }</h3>
      <div className="space-y-3 text-sm text-gray-500">
        <p>{t("settings.proxy.desc") }</p>
      </div>
    </section>
  );
}

function TriggerDefaultsSection() {
  const { t } = useTranslation();
  return (
    <section>
      <h3 className="text-sm font-medium mb-3">{t("settings.trigger.title") }</h3>
      <div className="space-y-3 text-sm text-gray-500">
        <p>{t("settings.trigger.desc") }</p>
      </div>
    </section>
  );
}

function DataManagementSection() {
  const { t } = useTranslation();
  return (
    <section>
      <h3 className="text-sm font-medium mb-3">{t("settings.data.title") }</h3>
      <div className="space-y-3">
        <button
          className="px-3 py-1 text-sm rounded border border-gray-300 dark:border-gray-600 hover:bg-gray-100 dark:hover:bg-gray-700"
          onClick={() => ipcInvoke("ipc_export_config").catch(console.error)}
        >
          {t("settings.data.export") }
        </button>
        <button
          className="px-3 py-1 text-sm rounded border border-gray-300 dark:border-gray-600 hover:bg-gray-100 dark:hover:bg-gray-700"
          onClick={() => ipcInvoke("ipc_import_config").catch(console.error)}
        >
          {t("settings.data.import") }
        </button>
      </div>
    </section>
  );
}

function NotificationSection() {
  const { t } = useTranslation();
  const config = useConfigStore((s) => s.config);
  const updateGeneral = useConfigStore((s) => s.updateGeneral);

  if (!config) return null;

  const updateAndSave = (patch: Record<string, unknown>) => {
    updateGeneral(patch as any);
    ipcInvoke("ipc_update_general_config", patch).catch((e) =>
      console.error("save notification config failed:", e)
    );
  };

  const items: { key: keyof typeof config.general; label: string }[] = [
    { key: "notify_connect_success", label: t("settings.notification.connect_success") },
    { key: "notify_disconnect", label: t("settings.notification.disconnect") },
    { key: "notify_reconnect_success", label: t("settings.notification.reconnect_success") },
    { key: "notify_auth_fail", label: t("settings.notification.auth_fail") },
    { key: "notify_proxy_toggle", label: t("settings.notification.proxy_toggle") },
    { key: "notify_proxy_port_conflict", label: t("settings.notification.proxy_port_conflict") },
    { key: "notify_trigger_fail", label: t("settings.notification.trigger_fail") },
    { key: "notify_trigger_success", label: t("settings.notification.trigger_success") },
    { key: "notify_ip_change", label: t("settings.notification.ip_change") },
  ];

  return (
    <section>
      <h3 className="text-sm font-medium mb-3">{t("settings.notification.title")}</h3>
      <div className="space-y-3">
        {items.map((item) => (
          <label key={item.key} className="flex items-center justify-between">
            <span className="text-sm">{item.label}</span>
            <input
              type="checkbox"
              checked={(config.general[item.key] as boolean) || false}
              onChange={(e) => updateAndSave({ [item.key]: e.target.checked })}
            />
          </label>
        ))}
      </div>
    </section>
  );
}

function AboutSection() {
  const { t } = useTranslation();
  return (
    <section>
      <h3 className="text-sm font-medium mb-3">{t("settings.about.title")}</h3>
      <div className="space-y-2">
        <div className="text-sm">VPS Guard v{APP_VERSION}</div>
        <button
          className="px-3 py-1 text-sm rounded bg-blue-500 text-white hover:bg-blue-600"
          onClick={() => {/* TODO: check for updates */}}
        >
          {t("settings.about.check_update")}
        </button>
      </div>
    </section>
  );
}

const APP_VERSION = "0.1.0";