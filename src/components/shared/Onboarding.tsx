// Onboarding — first-run wizard (§18 / FP-8.1)
// Quick mode (3 steps) + Advanced mode (7 steps)
// Includes firewall detection, key generation, template selection

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ipcInvoke } from "@/hooks/useIpc";

type Mode = "quick" | "advanced";
type AuthMethod = "password" | "key" | "agent";

export function Onboarding({ onComplete }: { onComplete: () => void }) {
  const { t } = useTranslation();
  const [mode, setMode] = useState<Mode | null>(null);
  const [step, setStep] = useState(0);
  const [vpsHost, setVpsHost] = useState("");
  const [vpsPort, setVpsPort] = useState("22");
  const [vpsUser, setVpsUser] = useState("root");
  const [vpsPass, setVpsPass] = useState("");
  const [serverName, setServerName] = useState("");
  const [connecting, setConnecting] = useState(false);
  const [connectResult, setConnectResult] = useState<"none" | "success" | "error">("none");
  const [authMethod, setAuthMethod] = useState<AuthMethod>("password");
  const [keyPath, setKeyPath] = useState("");
  const [generatingKey, setGeneratingKey] = useState(false);
  const [firewallChecking, setFirewallChecking] = useState(false);
  const [firewallResult, setFirewallResult] = useState<"none" | "ok" | "blocked" | "warning">("none");
  const [protectedPort, setProtectedPort] = useState("");
  const [firewallWhitelist, setFirewallWhitelist] = useState(false);
  const [selectedTemplates, setSelectedTemplates] = useState<string[]>([]);
  const [proxySocksPort, setProxySocksPort] = useState("1080");
  const [proxyHttpPort, setProxyHttpPort] = useState("8080");
  const [createdServerId, setCreatedServerId] = useState<string | null>(null);

  if (!mode) {
    return <ModeSelection t={t} onSelect={setMode} onSkip={onComplete} />;
  }

  const totalSteps = mode === "quick" ? 3 : 7;
  const titles = mode === "quick"
    ? [t("onboarding.vps_info"), t("onboarding.test_connection"), t("onboarding.firewall_whitelist")]
    : [
        t("onboarding.welcome"),
        t("onboarding.vps_info"),
        t("onboarding.auth_method"),
        t("onboarding.test_connection"),
        t("onboarding.proxy_config"),
        t("onboarding.select_templates"),
        t("onboarding.complete"),
      ];

  const handleTestConnection = async () => {
    setConnecting(true);
    setConnectResult("none");
    try {
      // First add the server
      const serverId = await ipcInvoke<string>("ipc_add_server", {
        config: {
          id: `srv_${Date.now()}`,
          name: serverName || vpsHost,
          ssh: {
            host: vpsHost,
            port: parseInt(vpsPort) || 22,
            user: vpsUser,
            auth_method: authMethod,
            key_path: keyPath,
            key_auto_generated: false,
            connection_mode: "single",
            skip_hostkey_verify: true,
          },
          proxy: { enabled: true, socks5_port: parseInt(proxySocksPort) || 1080, http_port: parseInt(proxyHttpPort) || 8080, max_channels: 100, channel_idle_timeout: 300 },
          reconnect: { heartbeat_interval: 30, max_attempts: 5, initial_backoff_secs: 1, max_backoff_secs: 30 },
          ip_check: { enabled: true, interval_secs: 300 },
          last_known_ip: null,
          triggers: [],
          suppress_firewall_badge: false,
        },
      });
      setCreatedServerId(serverId);

      // Save credential (password only for now)
      if (authMethod === "password") {
        await ipcInvoke("ipc_save_credential", {
          serverId,
          credentialType: "password",
          value: vpsPass,
        });
      }

      // Try to connect
      await ipcInvoke("ipc_connect_server", { serverId });
      setConnectResult("success");
    } catch (e) {
      console.error("connection test failed:", e);
      setConnectResult("error");
    } finally {
      setConnecting(false);
    }
  };

  const handleGenerateKey = async () => {
    setGeneratingKey(true);
    try {
      // Generate SSH key pair via daemon
      const result = await ipcInvoke<{ key_path: string }>("ipc_generate_ssh_key", {
        keyType: "ed25519",
        comment: `${vpsUser}@${vpsHost}`,
      });
      setKeyPath(result.key_path);
      setAuthMethod("key");
    } catch (e) {
      console.error("key generation failed:", e);
    } finally {
      setGeneratingKey(false);
    }
  };

  const handleFirewallCheck = async () => {
    setFirewallChecking(true);
    setFirewallResult("none");
    try {
      if (!vpsHost || !vpsPort) {
        setFirewallResult("warning");
        return;
      }
      // First check if the SSH port is reachable
      const result = await ipcInvoke<{ reachable: boolean; latency_ms?: number }>(
        "ipc_check_port_reachable",
        { host: vpsHost, port: parseInt(vpsPort) || 22 }
      );
      if (!result.reachable) {
        setFirewallResult("blocked");
        return;
      }
      // If we have a connected server, detect firewall via SSH exec (FP-8.1)
      if (createdServerId) {
        try {
          const fwResult = await ipcInvoke<{
            firewall_type: string;
            listening_ports: number[];
            firewalld_open_ports: string[];
          }>("ipc_detect_firewall", { serverId: createdServerId });
          // Auto-fill protected port from listening ports if available
          if (fwResult.listening_ports.length > 0 && !protectedPort) {
            // Pick the highest non-standard port as the protected port
            const candidate = fwResult.listening_ports
              .filter((p) => p > 1024 && p !== 22)
              .pop();
            if (candidate) {
              setProtectedPort(String(candidate));
            }
          }
          // Set firewall whitelist checkbox based on detected firewall
          if (fwResult.firewall_type !== "none") {
            setFirewallWhitelist(true);
          }
        } catch (e) {
          // SSH exec failed — fall back to port reachable check only
          console.warn("firewall detection via SSH failed:", e);
        }
      }
      setFirewallResult("ok");
    } catch (e) {
      console.error("firewall check failed:", e);
      setFirewallResult("warning");
    } finally {
      setFirewallChecking(false);
    }
  };

  const handleComplete = async () => {
    // Apply selected templates if any
    if (createdServerId && selectedTemplates.length > 0) {
      try {
        for (const templateId of selectedTemplates) {
          await ipcInvoke("ipc_add_trigger_from_template", {
            server_id: createdServerId,
            template_id: templateId,
          });
        }
      } catch (e) {
        console.error("failed to apply templates:", e);
      }
    }
    onComplete();
  };

  const renderStepContent = () => {
    if (mode === "quick") {
      if (step === 0) {
        return <VpsInfoStep
          host={vpsHost} setHost={setVpsHost}
          port={vpsPort} setPort={setVpsPort}
          user={vpsUser} setUser={setVpsUser}
          pass={vpsPass} setPass={setVpsPass}
          name={serverName} setName={setServerName}
        />;
      }
      if (step === 1) {
        return <TestConnectionStep
          connecting={connecting}
          result={connectResult}
          onTest={handleTestConnection}
        />;
      }
      if (step === 2) {
        return <FirewallCheckStep
          checking={firewallChecking}
          result={firewallResult}
          onCheck={handleFirewallCheck}
          host={vpsHost}
          port={vpsPort}
        />;
      }
    } else {
      // Advanced mode
      if (step === 0) return <WelcomeStep />;
      if (step === 1) return <VpsInfoStep
        host={vpsHost} setHost={setVpsHost}
        port={vpsPort} setPort={setVpsPort}
        user={vpsUser} setUser={setVpsUser}
        pass={vpsPass} setPass={setVpsPass}
        name={serverName} setName={setServerName}
      />;
      if (step === 2) return <AuthMethodStep
        authMethod={authMethod} setAuthMethod={setAuthMethod}
        keyPath={keyPath} setKeyPath={setKeyPath}
        generatingKey={generatingKey}
        onGenerateKey={handleGenerateKey}
        pass={vpsPass} setPass={setVpsPass}
      />;
      if (step === 3) return <TestConnectionStep
        connecting={connecting}
        result={connectResult}
        onTest={handleTestConnection}
      />;
      if (step === 4) return <ProxyConfigStep
        socksPort={proxySocksPort} setSocksPort={setProxySocksPort}
        httpPort={proxyHttpPort} setHttpPort={setProxyHttpPort}
      />;
      if (step === 5) return <TemplateSelectionStep
        selected={selectedTemplates}
        setSelected={setSelectedTemplates}
      />;
      if (step === 6) return <CompleteStep onConfirm={handleComplete} />;
    }
    return null;
  };

  return (
    <div className="fixed inset-0 bg-white dark:bg-gray-900 flex flex-col items-center justify-center p-8">
      <div className="w-full max-w-2xl">
        <h1 className="text-2xl font-bold mb-2">{t("onboarding.welcome")}</h1>
        <p className="text-sm text-gray-500 mb-6">
          {t("onboarding.step", { current: step + 1, total: totalSteps })}
        </p>
        <h2 className="text-lg font-medium mb-4">{titles[step]}</h2>
        <div className="min-h-[200px]">{renderStepContent()}</div>
        <div className="flex justify-between mt-8">
          <button
            className="px-4 py-2 text-sm rounded hover:bg-gray-100 dark:hover:bg-gray-800"
            onClick={() => (step > 0 ? setStep(step - 1) : setMode(null))}
          >
            {t("common.cancel")}
          </button>
          <button
            className="px-4 py-2 text-sm rounded bg-blue-500 text-white hover:bg-blue-600"
            onClick={() => (step < totalSteps - 1 ? setStep(step + 1) : handleComplete())}
          >
            {step < totalSteps - 1 ? t("common.ok") : t("onboarding.complete")}
          </button>
        </div>
      </div>
    </div>
  );
}

// === SECTION 1 END ===

function VpsInfoStep({
  host, setHost, port, setPort, user, setUser, pass, setPass, name, setName,
}: {
  host: string; setHost: (v: string) => void;
  port: string; setPort: (v: string) => void;
  user: string; setUser: (v: string) => void;
  pass: string; setPass: (v: string) => void;
  name: string; setName: (v: string) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-3">
      <div>
        <label className="block text-sm text-gray-500 mb-1">{t("server.name")}</label>
        <input
          type="text"
          className="w-full px-3 py-2 rounded border border-gray-300 dark:border-gray-600 bg-transparent text-sm"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder={t("onboarding.name_placeholder")}
        />
      </div>
      <div>
        <label className="block text-sm text-gray-500 mb-1">{t("server.host")}</label>
        <input
          type="text"
          className="w-full px-3 py-2 rounded border border-gray-300 dark:border-gray-600 bg-transparent text-sm"
          value={host}
          onChange={(e) => setHost(e.target.value)}
          placeholder={t("onboarding.host_placeholder")}
        />
      </div>
      <div className="flex gap-3">
        <div className="flex-1">
          <label className="block text-sm text-gray-500 mb-1">{t("onboarding.port")}</label>
          <input
            type="text"
            className="w-full px-3 py-2 rounded border border-gray-300 dark:border-gray-600 bg-transparent text-sm"
            value={port}
            onChange={(e) => setPort(e.target.value)}
          />
        </div>
        <div className="flex-1">
          <label className="block text-sm text-gray-500 mb-1">{t("server.user")}</label>
          <input
            type="text"
            className="w-full px-3 py-2 rounded border border-gray-300 dark:border-gray-600 bg-transparent text-sm"
            value={user}
            onChange={(e) => setUser(e.target.value)}
          />
        </div>
      </div>
      <div>
        <label className="block text-sm text-gray-500 mb-1">{t("onboarding.password")}</label>
        <input
          type="password"
          className="w-full px-3 py-2 rounded border border-gray-300 dark:border-gray-600 bg-transparent text-sm"
          value={pass}
          onChange={(e) => setPass(e.target.value)}
        />
      </div>
    </div>
  );
}

function TestConnectionStep({
  connecting, result, onTest,
}: {
  connecting: boolean;
  result: "none" | "success" | "error";
  onTest: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-4">
      <p className="text-sm text-gray-600">{t("onboarding.test_connection")}</p>
      <button
        className="px-4 py-2 text-sm rounded bg-blue-500 text-white hover:bg-blue-600 disabled:opacity-50"
        onClick={onTest}
        disabled={connecting}
      >
        {connecting ? t("onboarding.testing") : t("onboarding.test_connection")}
      </button>
      {result === "success" && (
        <div className="text-sm text-green-600">{t("onboarding.conn_success")}</div>
      )}
      {result === "error" && (
        <div className="text-sm text-red-600">{t("onboarding.conn_failed")}</div>
      )}
    </div>
  );
}

function ModeSelection({ t, onSelect, onSkip }: { t: (k: string) => string; onSelect: (m: Mode) => void; onSkip: () => void }) {
  return (
    <div className="fixed inset-0 bg-white dark:bg-gray-900 flex flex-col items-center justify-center p-8">
      <div className="w-full max-w-md space-y-4">
        <h1 className="text-2xl font-bold text-center">{t("onboarding.welcome")}</h1>
        <button
          className="w-full px-6 py-4 rounded-lg bg-blue-500 text-white hover:bg-blue-600"
          onClick={() => onSelect("quick")}
        >
          <div className="text-lg font-medium">{t("onboarding.quick_mode")}</div>
          <div className="text-sm opacity-80">{t("onboarding.quick_steps")}</div>
        </button>
        <button
          className="w-full px-6 py-4 rounded-lg border border-gray-300 dark:border-gray-700 hover:bg-gray-100 dark:hover:bg-gray-800"
          onClick={() => onSelect("advanced")}
        >
          <div className="text-lg font-medium">{t("onboarding.advanced_mode")}</div>
          <div className="text-sm text-gray-500">{t("onboarding.advanced_steps")}</div>
        </button>
        <button
          className="w-full py-3 text-sm text-blue-600 dark:text-blue-400 hover:bg-blue-50 dark:hover:bg-blue-900/30 rounded-lg border border-blue-200 dark:border-blue-800"
          onClick={onSkip}
        >
          {t("onboarding.skip")}
        </button>
      </div>
    </div>
  );
}

// === SECTION 2 END ===

function FirewallCheckStep({
  checking, result, onCheck, host, port,
}: {
  checking: boolean;
  result: "none" | "ok" | "blocked" | "warning";
  onCheck: () => void;
  host: string;
  port: string;
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-4">
      <p className="text-sm text-gray-600">
        {t("onboarding.firewall_whitelist")}
      </p>
      <p className="text-xs text-gray-400">
        {t("onboarding.host_label")}: {host || "—"}, {t("onboarding.port")}: {port || "—"}
      </p>
      <button
        className="px-4 py-2 text-sm rounded bg-blue-500 text-white hover:bg-blue-600 disabled:opacity-50"
        onClick={onCheck}
        disabled={checking || !host}
      >
        {checking ? t("onboarding.checking") : t("onboarding.check_firewall")}
      </button>
      {result === "ok" && (
        <div className="text-sm text-green-600">
          {t("onboarding.fw_reachable")}
        </div>
      )}
      {result === "blocked" && (
        <div className="text-sm text-red-600">
          {t("onboarding.fw_blocked")}
        </div>
      )}
      {result === "warning" && (
        <div className="text-sm text-yellow-600">
          {t("onboarding.fw_unknown")}
        </div>
      )}
    </div>
  );
}

function AuthMethodStep({
  authMethod, setAuthMethod,
  keyPath, setKeyPath,
  generatingKey, onGenerateKey,
  pass, setPass,
}: {
  authMethod: AuthMethod;
  setAuthMethod: (m: AuthMethod) => void;
  keyPath: string;
  setKeyPath: (s: string) => void;
  generatingKey: boolean;
  onGenerateKey: () => void;
  pass: string;
  setPass: (s: string) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-4">
      <div className="space-y-2">
        <label className="flex items-center gap-2 text-sm">
          <input
            type="radio"
            checked={authMethod === "password"}
            onChange={() => setAuthMethod("password")}
          />
          Password
        </label>
        <label className="flex items-center gap-2 text-sm">
          <input
            type="radio"
            checked={authMethod === "key"}
            onChange={() => setAuthMethod("key")}
          />
          SSH Key
        </label>
        <label className="flex items-center gap-2 text-sm">
          <input
            type="radio"
            checked={authMethod === "agent"}
            onChange={() => setAuthMethod("agent")}
          />
          SSH Agent
        </label>
      </div>

      {authMethod === "password" && (
        <div>
          <label className="block text-sm text-gray-500 mb-1">{t("onboarding.password")}</label>
          <input
            type="password"
            className="w-full px-3 py-2 rounded border border-gray-300 dark:border-gray-600 bg-transparent text-sm"
            value={pass}
            onChange={(e) => setPass(e.target.value)}
          />
        </div>
      )}

      {authMethod === "key" && (
        <div className="space-y-2">
          <div>
            <label className="block text-sm text-gray-500 mb-1">{t("server.key_path")}</label>
            <input
              type="text"
              className="w-full px-3 py-2 rounded border border-gray-300 dark:border-gray-600 bg-transparent text-sm"
              value={keyPath}
              onChange={(e) => setKeyPath(e.target.value)}
              placeholder={t("onboarding.key_path_placeholder")}
            />
          </div>
          <button
            className="px-3 py-1.5 text-xs rounded bg-green-500 text-white hover:bg-green-600 disabled:opacity-50"
            onClick={onGenerateKey}
            disabled={generatingKey}
          >
            {generatingKey ? t("onboarding.generating") : t("onboarding.generate_key")}
          </button>
        </div>
      )}

      {authMethod === "agent" && (
        <p className="text-sm text-gray-500">
          {t("onboarding.ssh_agent_desc")}
        </p>
      )}
    </div>
  );
}

function ProxyConfigStep({
  socksPort, setSocksPort,
  httpPort, setHttpPort,
}: {
  socksPort: string;
  setSocksPort: (s: string) => void;
  httpPort: string;
  setHttpPort: (s: string) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-3">
      <p className="text-sm text-gray-600">{t("onboarding.proxy_config")}</p>
      <div className="grid grid-cols-2 gap-4">
        <div>
          <label className="block text-sm text-gray-500 mb-1">{t("onboarding.socks5_port")}</label>
          <input
            type="text"
            className="w-full px-3 py-2 rounded border border-gray-300 dark:border-gray-600 bg-transparent text-sm"
            value={socksPort}
            onChange={(e) => setSocksPort(e.target.value)}
          />
        </div>
        <div>
          <label className="block text-sm text-gray-500 mb-1">{t("onboarding.http_port")}</label>
          <input
            type="text"
            className="w-full px-3 py-2 rounded border border-gray-300 dark:border-gray-600 bg-transparent text-sm"
            value={httpPort}
            onChange={(e) => setHttpPort(e.target.value)}
          />
        </div>
      </div>
    </div>
  );
}

function TemplateSelectionStep({
  selected, setSelected,
}: {
  selected: string[];
  setSelected: (s: string[]) => void;
}) {
  const { t } = useTranslation();
  const templates = [
    { id: "tmpl_ip_notify", name: t("template.ip_notify_name"), desc: t("template.ip_notify_desc") },
    { id: "tmpl_health_check", name: t("template.health_check_name"), desc: t("template.health_check_desc") },
    { id: "tmpl_auto_reconnect", name: t("template.auto_reconnect_name"), desc: t("template.auto_reconnect_desc") },
    { id: "tmpl_cleanup", name: t("template.cleanup_name"), desc: t("template.cleanup_desc") },
  ];

  const toggle = (id: string) => {
    if (selected.includes(id)) {
      setSelected(selected.filter((s) => s !== id));
    } else {
      setSelected([...selected, id]);
    }
  };

  return (
    <div className="space-y-3">
      <p className="text-sm text-gray-600">{t("onboarding.select_templates")}</p>
      {templates.map((tmpl) => (
        <label
          key={tmpl.id}
          className="flex items-start gap-3 p-3 rounded border border-gray-200 dark:border-gray-700 cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-800"
        >
          <input
            type="checkbox"
            checked={selected.includes(tmpl.id)}
            onChange={() => toggle(tmpl.id)}
            className="mt-1"
          />
          <div>
            <div className="text-sm font-medium">{tmpl.name}</div>
            <div className="text-xs text-gray-500">{tmpl.desc}</div>
          </div>
        </label>
      ))}
    </div>
  );
}

function WelcomeStep() {
  const { t } = useTranslation();
  return (
    <div className="space-y-3">
      <p className="text-sm text-gray-600">
        {t("onboarding.welcome")}
      </p>
      <p className="text-sm text-gray-500">
        This wizard will guide you through setting up your first VPS connection.
        You can choose Quick mode (3 steps) or Advanced mode (7 steps) for more control.
      </p>
    </div>
  );
}

function CompleteStep({ onConfirm }: { onConfirm: () => void }) {
  const { t } = useTranslation();
  return (
    <div className="space-y-3">
      <p className="text-sm text-gray-600">
        {t("onboarding.complete")}
      </p>
      <p className="text-sm text-green-600">
        {t("onboarding.ready")}
      </p>
    </div>
  );
}

// === SECTION 3 END ===
