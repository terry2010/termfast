// ServerDetail — right panel showing selected server details (§9.4)
// Shows connection controls, proxy toggle, IP, and trigger status
// Tab-based UI: Connection / Proxy / Triggers / Auth (FP-8.3)

import { useState, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useServerStore } from "@/stores/serverStore";
import { useLogStore } from "@/stores/logStore";
import { ipcInvoke } from "@/hooks/useIpc";
import { TriggerList } from "@/components/shared/TriggerList";
import { toast } from "@/components/ui/toast";

type Tab = "connection" | "proxy" | "triggers" | "auth";

export function ServerDetail({ onDeleteServer }: { onDeleteServer?: (serverId: string, serverName: string) => void }) {
  const { t } = useTranslation();
  const selectedId = useServerStore((s) => s.selected_server_id);
  const servers = useServerStore((s) => s.servers);
  const updateServerStatus = useServerStore((s) => s.updateServerStatus);
  const setProxyStatus = useServerStore((s) => s.setProxyStatus);
  const activeTabs = useServerStore((s) => s.active_tabs);
  const setActiveTab = useServerStore((s) => s.setActiveTab);
  const activeTab = (selectedId ? activeTabs[selectedId] || "connection" : "connection") as Tab;
  const [connecting, setConnecting] = useState(false);
  const [testProxyUrl, setTestProxyUrl] = useState("");
  const [testProxyResult, setTestProxyResult] = useState<{
    success: boolean;
    exit_ip: string | null;
    latency_ms: number;
    error?: string;
  } | null>(null);
  const [testingProxy, setTestingProxy] = useState(false);
  const testProxyAbort = useRef<AbortController | null>(null);

  const server = servers.find((s) => s.id === selectedId);

  if (!server) {
    return (
      <div className="flex-1 flex items-center justify-center text-gray-500">
        {t("server.add")}
      </div>
    );
  }

  const handleConnect = async () => {
    if (!server.id) return;
    setConnecting(true);
    updateServerStatus(server.id, "connecting");
    try {
      await ipcInvoke("ipc_connect_server", { serverId: server.id });
      // Optimistic update — daemon event will confirm/refine this
      updateServerStatus(server.id, "connected", server.last_known_ip || undefined);
    } catch (e: any) {
      const errMsg = e?.detail || e?.message || String(e);
      updateServerStatus(server.id, "offline");
      // Write log entry directly (fallback if daemon event didn't arrive)
      useLogStore.getState().addEntry({
        id: `conn-err-${Date.now()}-${Math.random().toString(36).slice(2)}`,
        timestamp: new Date().toISOString(),
        server_id: server.id,
        level: "error",
        category: "Connection",
        message: `Connection failed: ${errMsg}`,
        execution_id: null,
        command: null,
        exit_code: null,
        stdout: null,
        stderr: null,
      });
      toast.error(t("server.connect_failed"), { description: errMsg });
    } finally {
      setConnecting(false);
    }
  };

  const handleDisconnect = async () => {
    if (!server.id) return;
    try {
      await ipcInvoke("ipc_disconnect_server", { serverId: server.id });
      // Optimistic update — daemon event will confirm/refine this
      updateServerStatus(server.id, "disconnected");
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error(t("server.disconnect_failed"), { description: msg });
    }
  };

  const handleToggleProxy = async () => {
    if (!server.id) return;
    const newEnabled = !server.proxy_running;
    try {
      await ipcInvoke("ipc_toggle_proxy", {
        serverId: server.id,
        enabled: newEnabled,
      });
      setProxyStatus(server.id, newEnabled);
    } catch (e) {
      const errMsg = e instanceof Error ? e.message : String(e);
      useLogStore.getState().addEntry({
        id: `proxy-toggle-${Date.now()}-${Math.random().toString(36).slice(2)}`,
        timestamp: new Date().toISOString(),
        server_id: server.id,
        level: "error",
        category: "Proxy",
        message: `Proxy toggle failed: ${errMsg}`,
        execution_id: null,
        command: null,
        exit_code: null,
        stdout: null,
        stderr: null,
      });
      toast.error(t("server.proxy_toggle_failed"), { description: errMsg });
    }
  };

  const handleUpdateProxy = async (patch: { socks5_port?: number; http_port?: number; mixed_port?: number }) => {
    if (!server.id) return;
    try {
      await ipcInvoke("ipc_update_server", {
        server_id: server.id,
        ...patch,
      });
      // Update local store
      useServerStore.setState((s) => ({
        servers: s.servers.map((srv) =>
          srv.id === server.id
            ? { ...srv, proxy: { ...srv.proxy, ...patch } }
            : srv
        ),
      }));
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error(t("server.proxy_update_failed"), { description: msg });
    }
  };

  const handleSetSystemProxy = async () => {
    if (!server.id) return;
    try {
      await ipcInvoke("ipc_set_system_proxy", { serverId: server.id });
      toast.success(t("server.set_system_proxy"));
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error(t("server.set_system_proxy_failed"), { description: msg });
    }
  };

  const handleClearSystemProxy = async () => {
    try {
      await ipcInvoke("ipc_clear_system_proxy", {});
      toast.success(t("server.clear_system_proxy"));
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error(t("server.clear_system_proxy_failed"), { description: msg });
    }
  };

  const handleTestProxy = async () => {
    if (!server.id) return;
    setTestingProxy(true);
    setTestProxyResult(null);
    const abort = new AbortController();
    testProxyAbort.current = abort;
    try {
      const result = await Promise.race([
        ipcInvoke<{
          success: boolean;
          exit_ip: string | null;
          latency_ms: number;
          error?: string;
        }>("ipc_test_proxy", {
          server_id: server.id,
          url: testProxyUrl || undefined,
        }),
        new Promise<never>((_, reject) => {
          abort.signal.addEventListener("abort", () =>
            reject(new Error(t("server.test_proxy_cancelled")))
          );
        }),
      ]);
      setTestProxyResult(result);
    } catch (e) {
      if (abort.signal.aborted) {
        // User cancelled — don't show error result
      } else {
        setTestProxyResult({
          success: false,
          exit_ip: null,
          latency_ms: 0,
          error: e instanceof Error ? e.message : String(e),
        });
      }
    } finally {
      setTestingProxy(false);
      testProxyAbort.current = null;
    }
  };

  const handleCancelTestProxy = () => {
    testProxyAbort.current?.abort();
  };

  const handleSwitchAuth = async (authMethod: string) => {
    if (!server.id) return;
    try {
      await ipcInvoke("ipc_switch_auth_method", {
        serverId: server.id,
        authMethod: authMethod,
      });
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error(t("server.switch_auth_failed"), { description: msg });
    }
  };

  const isConnected = server.current_status === "connected";

  const tabs: { key: Tab; label: string }[] = [
    { key: "connection", label: t("server.connect") },
    { key: "proxy", label: t("server.proxy") },
    { key: "triggers", label: t("trigger.title") },
    { key: "auth", label: "Auth" },
  ];

  return (
    <div className="flex-1 overflow-y-auto p-4">
      {/* Header with server name and connect/disconnect buttons */}
      <div className="flex items-center justify-between mb-4">
        <h2 className="text-lg font-medium">{server.name}</h2>
        <div className="flex gap-2">
          {!isConnected ? (
            <button
              className="px-3 py-1 text-sm rounded bg-blue-500 text-white hover:bg-blue-600 disabled:opacity-50"
              onClick={handleConnect}
              disabled={connecting}
            >
              {connecting ? t("server.status.connecting") : t("server.connect")}
            </button>
          ) : (
            <button
              className="px-3 py-1 text-sm rounded bg-red-500 text-white hover:bg-red-600"
              onClick={handleDisconnect}
            >
              {t("server.disconnect")}
            </button>
          )}
          {onDeleteServer && (
            <button
              className="px-3 py-1 text-sm rounded border border-red-300 text-red-600 hover:bg-red-50 dark:hover:bg-red-900/30"
              onClick={() => onDeleteServer(server.id, server.name)}
              title={t("server.delete_title")}
            >
              ✕
            </button>
          )}
        </div>
      </div>

      {/* Tab bar */}
      <div className="flex gap-1 border-b border-gray-200 dark:border-gray-700 mb-4">
        {tabs.map((tab) => (
          <button
            key={tab.key}
            className={`px-3 py-2 text-sm rounded-t ${
              activeTab === tab.key
                ? "bg-blue-50 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400 border-b-2 border-blue-500"
                : "text-gray-500 hover:text-gray-700 dark:hover:text-gray-300"
            }`}
            onClick={() => setActiveTab(tab.key)}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* Tab content */}
      {activeTab === "connection" && (
        <div className="space-y-4">
          <div>
            <label className="text-sm text-gray-500">{t("server.host")}</label>
            <div className="text-sm">{server.ssh?.host || "?"}:{server.ssh?.port || "?"}</div>
          </div>
          <div>
            <label className="text-sm text-gray-500">{t("server.status_label")}</label>
            <div className="text-sm">{t(`server.status.${server.current_status}`)}</div>
          </div>
          {server.current_ip && (
            <div>
              <label className="text-sm text-gray-500">{t("server.ip_label")}</label>
              <div className="text-sm font-mono">{server.current_ip}</div>
            </div>
          )}
        </div>
      )}

      {activeTab === "proxy" && (
        <div className="space-y-4">
          <div>
            <label className="text-sm text-gray-500">{t("server.proxy")}</label>
            <div className="flex items-center gap-2 mt-1">
              <button
                className={`px-4 py-1.5 text-sm rounded font-medium ${
                  server.proxy_running
                    ? "bg-green-500 text-white hover:bg-green-600"
                    : "bg-gray-200 dark:bg-gray-700 hover:bg-gray-300 dark:hover:bg-gray-600"
                }`}
                onClick={handleToggleProxy}
                disabled={!isConnected}
              >
                {server.proxy_running ? `■ ${t("server.stop_proxy")}` : `▶ ${t("server.start_proxy")}`}
              </button>
              <span className="text-xs text-gray-500">
                SOCKS5 :{server.proxy.socks5_port}  HTTP :{server.proxy.http_port}
                {server.proxy.mixed_port > 0 && `  Mixed :${server.proxy.mixed_port}`}
              </span>
              {server.proxy_running && server.active_channels > 0 && (
                <span className="text-xs text-green-500 ml-2">
                  {server.active_channels} {t("server.active_clients")}
                </span>
              )}
            </div>
          </div>
          <div>
            <label className="text-sm text-gray-500">{t("server.mixed_port")}</label>
            <div className="flex items-center gap-2 mt-1">
              <input
                type="number"
                className="w-24 px-2 py-1 text-sm border border-gray-300 dark:border-gray-600 rounded bg-transparent"
                value={server.proxy.mixed_port || 0}
                onChange={(e) => handleUpdateProxy({ mixed_port: parseInt(e.target.value) || 0 })}
                disabled={server.proxy_running}
              />
              <span className="text-xs text-gray-400">0 = {t("server.mixed_port_disabled")}</span>
            </div>
          </div>
          <div>
            <label className="text-sm text-gray-500">{t("server.socks5_port")}</label>
            <div className="flex items-center gap-2 mt-1">
              <input
                type="number"
                className="w-24 px-2 py-1 text-sm border border-gray-300 dark:border-gray-600 rounded bg-transparent"
                value={server.proxy.socks5_port}
                onChange={(e) => handleUpdateProxy({ socks5_port: parseInt(e.target.value) || 1080 })}
                disabled={server.proxy_running || (server.proxy.mixed_port > 0)}
              />
            </div>
          </div>
          <div>
            <label className="text-sm text-gray-500">{t("server.http_port")}</label>
            <div className="flex items-center gap-2 mt-1">
              <input
                type="number"
                className="w-24 px-2 py-1 text-sm border border-gray-300 dark:border-gray-600 rounded bg-transparent"
                value={server.proxy.http_port}
                onChange={(e) => handleUpdateProxy({ http_port: parseInt(e.target.value) || 8080 })}
                disabled={server.proxy_running || (server.proxy.mixed_port > 0)}
              />
            </div>
          </div>
          <div>
            <label className="text-sm text-gray-500">{t("server.system_proxy")}</label>
            <div className="flex items-center gap-2 mt-1">
              <button
                className="px-3 py-1 text-sm rounded bg-blue-500 text-white hover:bg-blue-600 disabled:opacity-50"
                onClick={handleSetSystemProxy}
                disabled={!server.proxy_running}
              >
                {t("server.set_system_proxy")}
              </button>
              <button
                className="px-3 py-1 text-sm rounded bg-gray-200 dark:bg-gray-700 hover:bg-gray-300"
                onClick={handleClearSystemProxy}
              >
                {t("server.clear_system_proxy")}
              </button>
            </div>
          </div>
          {/* Test proxy */}
          <div>
            <label className="text-sm text-gray-500">{t("server.test_proxy")}</label>
            <div className="flex items-center gap-2 mt-1">
              <input
                type="text"
                className="flex-1 px-2 py-1 text-sm border border-gray-300 dark:border-gray-600 rounded bg-transparent"
                placeholder={t("server.test_proxy_url_placeholder")}
                value={testProxyUrl}
                onChange={(e) => setTestProxyUrl(e.target.value)}
                disabled={!server.proxy_running}
              />
              <button
                className="px-3 py-1 text-sm rounded bg-blue-500 text-white hover:bg-blue-600 disabled:opacity-50"
                onClick={handleTestProxy}
                disabled={!server.proxy_running || testingProxy}
              >
                {testingProxy ? t("common.testing") : t("server.test_proxy_btn")}
              </button>
              {testingProxy && (
                <button
                  className="px-3 py-1 text-sm rounded bg-gray-200 dark:bg-gray-700 hover:bg-gray-300 dark:hover:bg-gray-600"
                  onClick={handleCancelTestProxy}
                >
                  {t("common.cancel")}
                </button>
              )}
              {testProxyResult && !testingProxy && (
                <button
                  className="px-3 py-1 text-sm rounded bg-gray-200 dark:bg-gray-700 hover:bg-gray-300 dark:hover:bg-gray-600"
                  onClick={() => setTestProxyResult(null)}
                >
                  {t("common.cancel")}
                </button>
              )}
            </div>
            {testProxyResult && (
              <div className={`mt-2 p-2 rounded text-sm ${
                testProxyResult.success
                  ? "bg-green-50 dark:bg-green-900/30 text-green-700 dark:text-green-400"
                  : "bg-red-50 dark:bg-red-900/30 text-red-700 dark:text-red-400"
              }`}>
                {testProxyResult.success ? (
                  <span>
                    {t("server.test_proxy_success", {
                      ip: testProxyResult.exit_ip,
                      latency: testProxyResult.latency_ms,
                    })}
                  </span>
                ) : (
                  <span>
                    {t("server.test_proxy_failed")}
                    {testProxyResult.error ? `: ${testProxyResult.error}` : ""}
                  </span>
                )}
              </div>
            )}
          </div>
        </div>
      )}

      {activeTab === "triggers" && (
        <TriggerList serverId={server.id} />
      )}

      {activeTab === "auth" && (
        <div className="space-y-4">
          <div>
            <label className="text-sm text-gray-500">{t("server.auth_method")}</label>
            <div className="flex gap-2 mt-1">
              <button
                className="px-3 py-1 text-sm rounded bg-blue-500 text-white"
                onClick={() => handleSwitchAuth("password")}
              >
                {t("server.password")}
              </button>
              <button
                className="px-3 py-1 text-sm rounded bg-gray-200 dark:bg-gray-700"
                onClick={() => handleSwitchAuth("key")}
              >
                {t("server.ssh_key")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
