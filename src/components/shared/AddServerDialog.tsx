// AddServerDialog — add server form (FP-8.9)
// Calls ipc_add_server IPC to actually add the server to daemon config.

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ipcInvoke } from "@/hooks/useIpc";

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
  } | null;
}

export function AddServerDialog({ onAdd, onCancel, editServer }: AddServerDialogProps) {
  const { t } = useTranslation();
  const isEdit = !!editServer;
  const [name, setName] = useState(editServer?.name || "");
  const [host, setHost] = useState(editServer?.host || "");
  const [port, setPort] = useState(String(editServer?.port || 22));
  const [username, setUsername] = useState(editServer?.username || "root");
  const [authType, setAuthType] = useState<"password" | "key">(editServer?.authType || "password");
  const [password, setPassword] = useState("");
  const [keyPath, setKeyPath] = useState(editServer?.keyPath || "");
  const [socks5Port, setSocks5Port] = useState(String(editServer?.socks5Port || 1080));
  const [httpPort, setHttpPort] = useState(String(editServer?.httpPort || 8080));
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);

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
        const result = await ipcInvoke<{ server_id: string }>("ipc_add_server", {
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
              max_channels: 64,
              channel_idle_timeout: 300,
            },
            reconnect: {
              heartbeat_interval: 15,
              max_attempts: 10,
              initial_backoff_secs: 1,
              max_backoff_secs: 300,
            },
            ip_check: {
              enabled: true,
              interval_secs: 300,
            },
            last_known_ip: null,
            triggers: [],
            suppress_firewall_badge: false,
          },
        });
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
      setError(String(e));
    } finally {
      setAdding(false);
    }
  };

// === SECTION 1 END ===

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50" onClick={(e) => { if (e.target === e.currentTarget) onCancel(); }}>
      <div className="bg-white dark:bg-gray-800 rounded-lg p-6 max-w-md w-full mx-4 max-h-[80vh] overflow-y-auto">
        <h2 className="text-lg font-medium mb-4">{isEdit ? t("server.edit") : t("server.add")}</h2>
        <div className="space-y-3">
          <Field label={t("server.name")}>
            <input value={name} onChange={(e) => setName(e.target.value)} className="input" />
          </Field>
          <div className="flex gap-2">
            <Field label={t("server.host")} className="flex-1">
              <input value={host} onChange={(e) => setHost(e.target.value)} className="input" />
            </Field>
            <Field label={t("server.port")} className="w-20">
              <input value={port} onChange={(e) => setPort(e.target.value)} className="input" type="number" />
            </Field>
          </div>
          <Field label={t("server.username")}>
            <input value={username} onChange={(e) => setUsername(e.target.value)} className="input" />
          </Field>
          <Field label={t("onboarding.auth_method")}>
            <select
              value={authType}
              onChange={(e) => setAuthType(e.target.value as "password" | "key")}
              className="input"
            >
              <option value="password">{t("server.password")}</option>
              <option value="key">{t("server.ssh_key")}</option>
            </select>
          </Field>
          {authType === "password" && (
            <Field label={t("server.password")}>
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder={isEdit ? t("server.password_keep") : ""}
                className="input"
              />
            </Field>
          )}
          {authType === "key" && (
            <Field label={t("server.key_path")}>
              <input value={keyPath} onChange={(e) => setKeyPath(e.target.value)} className="input" />
            </Field>
          )}
          <div className="flex gap-2">
            <Field label={t("server.socks5_port")} className="flex-1">
              <input
                value={socks5Port}
                onChange={(e) => setSocks5Port(e.target.value)}
                className="input"
                type="number"
              />
            </Field>
            <Field label={t("server.http_port")} className="flex-1">
              <input
                value={httpPort}
                onChange={(e) => setHttpPort(e.target.value)}
                className="input"
                type="number"
              />
            </Field>
          </div>
        </div>
        {error && (
          <div className="mt-3 text-sm text-red-600 bg-red-50 dark:bg-red-900/20 p-2 rounded">
            {error}
          </div>
        )}
        <div className="flex justify-end gap-2 mt-6">
          <button className="px-4 py-2 text-sm rounded hover:bg-gray-100 dark:hover:bg-gray-700" onClick={onCancel}>
            {t("common.cancel")}
          </button>
          <button
            className="px-4 py-2 text-sm rounded bg-blue-500 text-white hover:bg-blue-600 disabled:opacity-50"
            onClick={handleSubmit}
            disabled={!name || !host || adding}
          >
            {adding ? t("common.saving") : isEdit ? t("common.save") : t("common.add")}
          </button>
        </div>
      </div>
    </div>
  );
}

function Field({ label, children, className = "" }: { label: string; children: React.ReactNode; className?: string }) {
  return (
    <label className={`block ${className}`}>
      <span className="text-xs text-gray-500 block mb-1">{label}</span>
      {children}
    </label>
  );
}

// === SECTION 2 END ===