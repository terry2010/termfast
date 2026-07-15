// AddServerDialog — add server form (FP-8.9)
// Calls ipc_add_server IPC to actually add the server to daemon config.

import { useState, useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { ipcInvoke, formatIpcError, IpcErrorImpl } from "@/hooks/useIpc";
import { Modal } from "@/components/ui/Modal";
import { useServerStore } from "@/stores/serverStore";

interface AddServerDialogProps {
  onAdd: () => void;
  onCancel: () => void;
  editServer?: {
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
  } | null;
}

export function AddServerDialog({
  onAdd,
  onCancel,
  editServer,
}: AddServerDialogProps) {
  const { t } = useTranslation();
  const isEdit = !!editServer;
  const existingServers = useServerStore((s) => s.servers);

  // Compute next available ports based on existing servers
  const { nextSocks5, nextHttp } = useMemo(() => {
    const usedSocks5 = new Set<number>();
    const usedHttp = new Set<number>();
    for (const s of existingServers) {
      if (s.proxy?.socks5_port) usedSocks5.add(s.proxy.socks5_port);
      if (s.proxy?.http_port) usedHttp.add(s.proxy.http_port);
      if (s.proxy?.mixed_port && s.proxy.mixed_port > 0) {
        usedSocks5.add(s.proxy.mixed_port);
        usedHttp.add(s.proxy.mixed_port);
      }
    }
    let socks5 = 1080;
    while (usedSocks5.has(socks5)) socks5++;
    let http = 8080;
    while (usedHttp.has(http)) http++;
    return { nextSocks5: socks5, nextHttp: http };
  }, [existingServers]);

  const [name, setName] = useState(editServer?.name || "");
  const [host, setHost] = useState(editServer?.host || "");
  const [port, setPort] = useState(String(editServer?.port || 22));
  const [username, setUsername] = useState(editServer?.username || "root");
  const [authType, setAuthType] = useState<"password" | "key">(
    editServer?.authType || "password",
  );
  const [password, setPassword] = useState("");
  const [keyPath, setKeyPath] = useState(editServer?.keyPath || "");
  const [socks5Port, setSocks5Port] = useState(
    String(editServer?.socks5Port || nextSocks5),
  );
  const [httpPort, setHttpPort] = useState(
    String(editServer?.httpPort || nextHttp),
  );
  // Default to mixed port enabled for new servers (mixed port = socks5 port,
  // which also serves HTTP on the same port). For edit mode, preserve the
  // existing value (0 = disabled).
  const [mixedPort, setMixedPort] = useState(
    String(editServer ? (editServer.mixedPort ?? 0) : nextSocks5),
  );
  const mixedEnabled = parseInt(mixedPort) > 0;
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{
    success: boolean;
    message: string;
  } | null>(null);

  // ESC to close
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onCancel]);

  const handleSubmit = async () => {
    if (!name || !host || !username) return;
    setAdding(true);
    setError(null);
    try {
      if (isEdit && editServer) {
        // Update existing server
        await ipcInvoke("ipc_update_server", {
          server_id: editServer.id,
          name,
          socks5_port: parseInt(socks5Port) || 1080,
          http_port: parseInt(httpPort) || 8080,
          mixed_port: parseInt(mixedPort) || 0,
          ssh: {
            host,
            port: parseInt(port) || 22,
            user: username,
            auth_method: authType,
            key_path: keyPath || "",
          },
        });
        // Save new password if provided (don't fail the whole edit if credential save fails)
        if (authType === "password" && password) {
          try {
            await ipcInvoke("ipc_save_credential", {
              serverId: editServer.id,
              credentialType: "password",
              value: password,
            });
          } catch (credErr) {
            console.error("save credential failed:", credErr);
          }
        }
      } else {
        // Add new server
        const serverId = `srv_${Date.now()}`;
        const result = await ipcInvoke<{ server_id: string }>(
          "ipc_add_server",
          {
            config: {
              id: serverId,
              name,
              ssh: {
                host,
                port: parseInt(port) || 22,
                user: username,
                auth_method: authType,
                key_path: keyPath || "",
                key_auto_generated: false,
                connection_mode: "single",
                skip_hostkey_verify: false,
              },
              proxy: {
                enabled: false,
                socks5_port: parseInt(socks5Port) || 1080,
                http_port: parseInt(httpPort) || 8080,
                mixed_port: parseInt(mixedPort) || 0,
                max_channels: 64,
                channel_idle_timeout: 300,
              },
              reconnect: {
                auto_reconnect: true,
                heartbeat_interval: 10,
                max_attempts: 999,
                reconnect_timeout_secs: 86400,
                initial_backoff_secs: 1,
                max_backoff_secs: 60,
              },
              ip_check: {
                enabled: true,
                interval_secs: 300,
              },
              last_known_ip: null,
              triggers: [],
              suppress_firewall_badge: false,
            },
          },
        );
        // Save password to credential store if auth method is password
        const finalId = result?.server_id || serverId;
        if (authType === "password" && password) {
          await ipcInvoke("ipc_save_credential", {
            serverId: finalId,
            credentialType: "password",
            value: password,
          });
        }
      }
      onAdd();
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setAdding(false);
    }
  };

  // === SECTION 1 END ===

  const handleTestConnection = async () => {
    // Validate required fields before testing
    if (!host) {
      setError(t("server.host") + " " + t("common.required"));
      return;
    }
    if (!username) {
      setError(t("server.username") + " " + t("common.required"));
      return;
    }
    if (authType === "password" && !password && !isEdit) {
      setError(t("server.password") + " " + t("common.required"));
      return;
    }
    if (authType === "key" && !keyPath) {
      setError(t("server.key_path") + " " + t("common.required"));
      return;
    }
    // For edit mode with password auth, if no new password entered, we can't test
    // (the existing credential is stored encrypted and not accessible here)
    if (authType === "password" && isEdit && !password) {
      setError(t("server.password_required_for_test"));
      return;
    }

    setError(null);
    setTesting(true);
    setTestResult(null);
    try {
      const result = await ipcInvoke<{ success: boolean; message: string }>(
        "ipc_test_connection",
        {
          host,
          port: parseInt(port) || 22,
          username,
          auth_method: authType,
          password: authType === "password" ? password : null,
          key_path: authType === "key" ? keyPath : null,
        },
      );
      setTestResult(result);
    } catch (e) {
      const msg = formatIpcError(e);
      setTestResult({ success: false, message: msg });
    } finally {
      setTesting(false);
    }
  };

  return (
    <Modal
      title={isEdit ? t("server.edit") : t("server.add")}
      onClose={onCancel}
      maxWidth="max-w-xl"
      footer={
        <>
          <button
            className="px-4 py-2 text-sm rounded-lg text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-[#2C2C2E] transition-colors"
            onClick={onCancel}
          >
            {t("common.cancel")}
          </button>
          <button
            className="px-4 py-2 text-sm rounded-lg bg-gray-100 dark:bg-[#2C2C2E] text-gray-700 dark:text-gray-200 hover:bg-gray-200 dark:hover:bg-[#3A3A3C] disabled:opacity-40 disabled:cursor-not-allowed transition-colors font-medium"
            onClick={handleTestConnection}
            disabled={testing || adding}
          >
            {testing ? t("common.testing") : t("onboarding.test_connection")}
          </button>
          <button
            className="px-4 py-2 text-sm rounded-lg bg-blue-500 text-white hover:bg-blue-600 disabled:opacity-40 disabled:cursor-not-allowed transition-colors font-medium"
            onClick={handleSubmit}
            disabled={!name || !host || adding}
          >
            {adding
              ? t("common.saving")
              : isEdit
                ? t("common.save")
                : t("common.add")}
          </button>
        </>
      }
    >
      <div className="space-y-5">
        {/* Basic info */}
        <SettingGroup title={t("server.basic_info")}>
          <SettingRow label={t("server.name")}>
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              className="input w-full"
            />
          </SettingRow>
          <SettingRow label={t("server.host")}>
            <input
              value={host}
              onChange={(e) => setHost(e.target.value)}
              className="input w-full"
            />
          </SettingRow>
          <SettingRow label={t("server.port")}>
            <input
              value={port}
              onChange={(e) => setPort(e.target.value)}
              className="input w-24"
              type="number"
            />
          </SettingRow>
          <SettingRow label={t("server.username")}>
            <input
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              className="input w-full"
            />
          </SettingRow>
        </SettingGroup>

        {/* Authentication */}
        <SettingGroup title={t("server.authentication")}>
          <SettingRow label={t("onboarding.auth_method")}>
            <select
              value={authType}
              onChange={(e) =>
                setAuthType(e.target.value as "password" | "key")
              }
              className="input w-full"
            >
              <option value="password">{t("server.password")}</option>
              <option value="key">{t("server.ssh_key")}</option>
            </select>
          </SettingRow>
          {authType === "password" && (
            <SettingRow label={t("server.password")}>
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder={isEdit ? t("server.password_keep") : ""}
                className="input w-full"
              />
            </SettingRow>
          )}
          {authType === "key" && (
            <SettingRow label={t("server.key_path")}>
              <input
                value={keyPath}
                onChange={(e) => setKeyPath(e.target.value)}
                className="input w-full"
              />
            </SettingRow>
          )}
        </SettingGroup>

        {/* Proxy settings */}
        <SettingGroup title={t("server.proxy_settings")}>
          <SettingRow label={t("server.mixed_port")}>
            <Toggle
              checked={mixedEnabled}
              onChange={(v) => {
                if (v) {
                  const port = parseInt(socks5Port) || 1080;
                  setMixedPort(String(port));
                  setSocks5Port(String(port));
                  setHttpPort(String(port));
                } else {
                  setMixedPort("0");
                }
              }}
            />
          </SettingRow>
          {mixedEnabled && (
            <SettingRow label={t("server.mixed_port")}>
              <input
                value={mixedPort}
                onChange={(e) => {
                  const val = e.target.value;
                  setMixedPort(val);
                  const port = parseInt(val) || 0;
                  if (port > 0) {
                    setSocks5Port(String(port));
                    setHttpPort(String(port));
                  }
                }}
                className="input w-24"
                type="number"
              />
            </SettingRow>
          )}
          <SettingRow label={t("server.socks5_port")}>
            <input
              value={socks5Port}
              onChange={(e) => setSocks5Port(e.target.value)}
              className="input w-24"
              type="number"
              disabled={mixedEnabled}
            />
          </SettingRow>
          <SettingRow label={t("server.http_port")}>
            <input
              value={httpPort}
              onChange={(e) => setHttpPort(e.target.value)}
              className="input w-24"
              type="number"
              disabled={mixedEnabled}
            />
          </SettingRow>
        </SettingGroup>
      </div>
      {error && (
        <div className="mt-4 text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 p-3 rounded-lg border border-red-200 dark:border-red-800/50">
          {error}
        </div>
      )}
      {testResult && (
        <div
          className={`mt-4 text-sm p-3 rounded-lg border ${
            testResult.success
              ? "text-green-600 dark:text-green-400 bg-green-50 dark:bg-green-900/20 border-green-200 dark:border-green-800/50"
              : "text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 border-red-200 dark:border-red-800/50"
          }`}
        >
          {testResult.success ? "✓ " : "✕"}
          {testResult.message}
        </div>
      )}
    </Modal>
  );
}

// === SECTION 2 END ===

// macOS System Settings-style group: title above white rounded container
function SettingGroup({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section>
      <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100 mb-1.5 px-1">
        {title}
      </h3>
      <div className="bg-white dark:bg-[#1E1E1E] rounded-xl border border-gray-200/80 dark:border-white/[0.06] overflow-hidden">
        {children}
      </div>
    </section>
  );
}

// Horizontal label + control row (like SettingsPage SettingItem)
function SettingRow({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-4 px-4 py-3 border-b border-gray-100 dark:border-white/[0.06] last:border-0">
      <span className="text-sm font-medium text-gray-700 dark:text-gray-300 flex-shrink-0">
        {label}
      </span>
      <div className="flex-1 max-w-xs flex justify-end">{children}</div>
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
