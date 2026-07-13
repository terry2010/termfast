// ServerDetail — right panel showing selected server details (§9.4)
// Shows connection controls, proxy toggle, IP, and trigger status
// Tab-based UI: Connection / Proxy / Triggers / Auth (FP-8.3)

import { useState, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useServerStore } from "@/stores/serverStore";
import { useLogStore } from "@/stores/logStore";
import { ipcInvoke, formatIpcError, IpcErrorImpl } from "@/hooks/useIpc";
import { TriggerList } from "@/components/shared/TriggerList";
import { toast } from "@/components/ui/toast";

type Tab = "overview" | "terminal";

const STATUS_COLORS: Record<string, string> = {
  connected: "bg-green-500",
  connecting: "bg-yellow-400",
  reconnecting: "bg-yellow-500",
  auth_failed: "bg-red-500",
  disconnected: "bg-gray-400",
  offline: "bg-gray-500",
};

export function ServerDetail() {
  const { t } = useTranslation();
  const selectedId = useServerStore((s) => s.selected_server_id);
  const servers = useServerStore((s) => s.servers);
  const updateServerStatus = useServerStore((s) => s.updateServerStatus);
  const setProxyStatus = useServerStore((s) => s.setProxyStatus);
  const [activeTab, setActiveTab] = useState<Tab>("overview");
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
  const [systemProxyEnabled, setSystemProxyEnabled] = useState(false);

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
      const errMsg = formatIpcError(e);
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
      // If credential is missing, open edit dialog so user can re-enter password
      if (e instanceof IpcErrorImpl && e.code === "CredentialNotFound") {
        window.dispatchEvent(
          new CustomEvent("edit-server", { detail: { serverId: server.id } })
        );
      }
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
      const msg = formatIpcError(e);
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
      const errMsg = formatIpcError(e);
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
      const msg = formatIpcError(e);
      toast.error(t("server.proxy_update_failed"), { description: msg });
    }
  };

  const handleSetSystemProxy = async () => {
    if (!server.id) return;
    try {
      await ipcInvoke("ipc_set_system_proxy", { serverId: server.id });
      setSystemProxyEnabled(true);
      toast.success(t("server.set_system_proxy"));
    } catch (e) {
      const msg = formatIpcError(e);
      toast.error(t("server.set_system_proxy_failed"), { description: msg });
    }
  };

  const handleClearSystemProxy = async () => {
    try {
      await ipcInvoke("ipc_clear_system_proxy", {});
      setSystemProxyEnabled(false);
      toast.success(t("server.clear_system_proxy"));
    } catch (e) {
      const msg = formatIpcError(e);
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
          error: formatIpcError(e),
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

  const isConnected = server.current_status === "connected";

  const tabs: { key: Tab; label: string }[] = [
    { key: "overview", label: t("server.overview") },
  ];

  const statusColor = isConnected ? "text-green-500" : server.current_status === "auth_failed" || server.current_status === "offline" ? "text-red-500" : "text-gray-400";

  return (
    <div className="flex-1 overflow-y-auto p-6 bg-gray-50/50 dark:bg-gray-900/50">
      {/* Top header: server identity */}
      <div className="mb-6">
        <div className="flex items-center gap-3 mb-1">
          <div className={`w-3 h-3 rounded-full ${STATUS_COLORS[server.current_status]}`} />
          <h1 className="text-2xl font-semibold text-gray-900 dark:text-gray-100">{server.name}</h1>
          <span className={`text-sm font-medium ${statusColor}`}>
            {t(`server.status.${server.current_status}`)}
          </span>
        </div>
        <div className="text-sm text-gray-500 font-mono ml-6">
          {server.ssh?.user || "root"}@{server.ssh?.host || "?"}:{server.ssh?.port || "?"}
        </div>
      </div>

      {/* Tab bar — overview + future terminal tabs */}
      <div className="flex gap-1 border-b border-gray-200 dark:border-gray-700 mb-6">
        {tabs.map((tab) => (
          <button
            key={tab.key}
            className={`px-4 py-2 text-sm font-medium rounded-t transition-colors ${
              activeTab === tab.key
                ? "bg-white dark:bg-gray-800 text-blue-600 dark:text-blue-400 border-b-2 border-blue-500"
                : "text-gray-500 hover:text-gray-700 dark:hover:text-gray-300"
            }`}
            onClick={() => setActiveTab(tab.key)}
          >
            {tab.label}
          </button>
        ))}
        {/* Terminal tabs will be appended here */}
      </div>

      {activeTab === "overview" && (
        <div className="space-y-5 max-w-5xl">
          {/* Primary action cards */}
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {/* Connection card */}
            <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4 shadow-sm">
              <div className="flex items-center justify-between mb-4">
                <div>
                  <div className="text-xs text-gray-500 uppercase tracking-wide">{t("server.connection")}</div>
                  <div className={`text-lg font-semibold mt-1 ${statusColor}`}>
                    {t(`server.status.${server.current_status}`)}
                  </div>
                </div>
                {!isConnected ? (
                  <button
                    className="px-4 py-2 text-sm rounded-md bg-blue-500 text-white hover:bg-blue-600 disabled:opacity-50 font-medium"
                    onClick={handleConnect}
                    disabled={connecting}
                  >
                    {connecting ? t("server.status.connecting") : t("server.connect")}
                  </button>
                ) : (
                  <button
                    className="px-4 py-2 text-sm rounded-md bg-red-500 text-white hover:bg-red-600 font-medium"
                    onClick={handleDisconnect}
                  >
                    {t("server.disconnect")}
                  </button>
                )}
              </div>
              <div className="grid grid-cols-3 gap-3 text-sm border-t border-gray-100 dark:border-gray-700 pt-4">
                <div>
                  <div className="text-xs text-gray-500 mb-1">{t("server.host")}</div>
                  <div className="font-mono text-gray-900 dark:text-gray-100 truncate">{server.ssh?.host || "?"}:{server.ssh?.port || "?"}</div>
                </div>
                <div>
                  <div className="text-xs text-gray-500 mb-1">{t("server.ip_label")}</div>
                  <div className="font-mono text-gray-900 dark:text-gray-100 truncate">{server.client_ip || "—"}</div>
                </div>
                <div>
                  <div className="text-xs text-gray-500 mb-1">{t("server.auth_method")}</div>
                  <div className="text-gray-900 dark:text-gray-100">{server.ssh?.auth_method === "key" ? t("server.ssh_key") : t("server.password")}</div>
                </div>
              </div>
            </div>

            {/* Proxy card */}
            <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4 shadow-sm flex flex-col">
              {/* Header: status + toggle */}
              <div className="flex items-center justify-between mb-3">
                <div>
                  <div className="text-xs text-gray-500 uppercase tracking-wide">{t("server.proxy")}</div>
                  <div className={`text-lg font-semibold mt-1 ${server.proxy_running ? "text-green-500" : "text-gray-400"}`}>
                    {server.proxy_running ? t("proxy.on") : t("proxy.off")}
                  </div>
                </div>
                <button
                  className={`px-4 py-2 text-sm rounded-md font-medium transition-colors ${
                    server.proxy_running
                      ? "bg-green-500 text-white hover:bg-green-600"
                      : "bg-gray-200 dark:bg-gray-700 hover:bg-gray-300 dark:hover:bg-gray-600"
                  }`}
                  onClick={handleToggleProxy}
                  disabled={!isConnected}
                >
                  {server.proxy_running ? t("server.stop_proxy") : t("server.start_proxy")}
                </button>
              </div>

              {/* Port configuration + system proxy row */}
              <div className="flex items-center gap-3 mb-3 flex-wrap">
                {server.proxy.mixed_port > 0 ? (
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-gray-500">Mixed</span>
                    <input
                      type="number"
                      className="w-20 px-2 py-1 text-sm font-mono border border-gray-200 dark:border-gray-600 rounded bg-transparent text-gray-900 dark:text-gray-100 focus:outline-none focus:border-blue-400"
                      value={server.proxy.mixed_port}
                      onChange={(e) => handleUpdateProxy({ mixed_port: parseInt(e.target.value) || 0 })}
                      disabled={server.proxy_running}
                    />
                  </div>
                ) : (
                  <>
                    <div className="flex items-center gap-2">
                      <span className="text-xs text-gray-500">SOCKS5</span>
                      <input
                        type="number"
                        className="w-20 px-2 py-1 text-sm font-mono border border-gray-200 dark:border-gray-600 rounded bg-transparent text-gray-900 dark:text-gray-100 focus:outline-none focus:border-blue-400"
                        value={server.proxy.socks5_port}
                        onChange={(e) => handleUpdateProxy({ socks5_port: parseInt(e.target.value) || 1080 })}
                        disabled={server.proxy_running}
                      />
                    </div>
                    <div className="flex items-center gap-2">
                      <span className="text-xs text-gray-500">HTTP</span>
                      <input
                        type="number"
                        className="w-20 px-2 py-1 text-sm font-mono border border-gray-200 dark:border-gray-600 rounded bg-transparent text-gray-900 dark:text-gray-100 focus:outline-none focus:border-blue-400"
                        value={server.proxy.http_port}
                        onChange={(e) => handleUpdateProxy({ http_port: parseInt(e.target.value) || 8080 })}
                        disabled={server.proxy_running}
                      />
                    </div>
                  </>
                )}
                {/* Mixed port checkbox */}
                <label className="flex items-center gap-1.5 text-xs text-gray-500 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={server.proxy.mixed_port > 0}
                    onChange={(e) => handleUpdateProxy({ mixed_port: e.target.checked ? (server.proxy.socks5_port || 1080) : 0 })}
                    disabled={server.proxy_running}
                    className="rounded"
                  />
                  {t("server.mixed_port")}
                </label>
                {/* System proxy checkbox — rightmost */}
                <label className={`flex items-center gap-1.5 text-xs text-gray-500 cursor-pointer ml-auto ${!server.proxy_running ? "opacity-50 pointer-events-none" : ""}`}>
                  <input
                    type="checkbox"
                    checked={systemProxyEnabled}
                    onChange={(e) => {
                      if (e.target.checked) handleSetSystemProxy();
                      else handleClearSystemProxy();
                    }}
                    disabled={!server.proxy_running}
                    className="rounded"
                  />
                  {t("server.set_system_proxy")}
                </label>
              </div>

              {/* Active clients indicator */}
              {server.proxy_running && server.active_channels > 0 && (
                <div className="text-xs text-green-500 font-medium mb-3">
                  {server.active_channels} {t("server.active_clients")}
                </div>
              )}

              {/* Test proxy section */}
              <div className="border-t border-gray-100 dark:border-gray-700 pt-3 mt-auto">
                <div className="flex items-center gap-2">
                  <input
                    type="text"
                    className="flex-1 px-2 py-1.5 text-sm border border-gray-300 dark:border-gray-600 rounded bg-transparent"
                    placeholder={t("server.test_proxy_url_placeholder")}
                    value={testProxyUrl}
                    onChange={(e) => setTestProxyUrl(e.target.value)}
                    disabled={!server.proxy_running}
                  />
                  <button
                    className="px-3 py-1.5 text-sm rounded-md bg-blue-500 text-white hover:bg-blue-600 disabled:opacity-50"
                    onClick={handleTestProxy}
                    disabled={!server.proxy_running || testingProxy}
                  >
                    {testingProxy ? t("common.testing") : t("server.test_proxy_btn")}
                  </button>
                  {testingProxy && (
                    <button
                      className="px-3 py-1.5 text-sm rounded-md bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600"
                      onClick={handleCancelTestProxy}
                    >
                      {t("common.cancel")}
                    </button>
                  )}
                </div>
                {testProxyResult && (
                  <div className={`mt-2 p-2.5 rounded-md text-sm ${
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
          </div>

          {/* Triggers panel — full width */}
          <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 shadow-sm overflow-hidden">
            <div className="px-4 py-3 border-b border-gray-100 dark:border-gray-700 bg-gray-50/50 dark:bg-gray-800/50">
              <h3 className="text-sm font-medium text-gray-900 dark:text-gray-100">{t("trigger.title")}</h3>
            </div>
            <div className="p-4">
              <TriggerList serverId={server.id} />
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
