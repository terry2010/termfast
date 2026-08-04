import { useState, useEffect, useCallback } from "react";
import { ipcInvoke } from "@/hooks/useIpc";
import { toast } from "@/components/ui/toast";
import { useTranslation } from "react-i18next";

interface PairingPageProps {
  onClose: () => void;
}

export function PairingPage({ onClose }: PairingPageProps) {
  const { t } = useTranslation();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [token, setToken] = useState<string | null>(null);
  const [pairingId, setPairingId] = useState<string | null>(null);
  const [polling, setPolling] = useState(false);
  const [devices, setDevices] = useState<any[]>([]);

  // Load saved token from localStorage
  useEffect(() => {
    const saved = localStorage.getItem("pairing_token");
    if (saved) setToken(saved);
  }, []);

  const handleRegister = useCallback(async () => {
    try {
      await ipcInvoke("ipc_pairing_register", { email, password });
      toast.success(t("pairing.register_success"));
    } catch (e: any) {
      toast.error(t("pairing.register_failed"), { description: String(e) });
    }
  }, [email, password, toast, t]);

  const handleLogin = useCallback(async () => {
    try {
      const result = await ipcInvoke<any>("ipc_pairing_login", { email, password });
      const tok = result.access_token;
      setToken(tok);
      localStorage.setItem("pairing_token", tok);
      toast.success(t("pairing.login_success"));
      // Load devices
      const devResult = await ipcInvoke<any>("ipc_pairing_list_devices", { token: tok });
      setDevices(devResult.devices || []);
    } catch (e: any) {
      toast.error(t("pairing.login_failed"), { description: String(e) });
    }
  }, [email, password, toast, t]);

  const handleInitiatePairing = useCallback(async () => {
    if (!token) return;
    try {
      const result = await ipcInvoke<any>("ipc_pairing_initiate", {
        token,
        desktop_device_id: "desktop-" + Date.now(),
      });
      setPairingId(result.pairing_id);
      setPolling(true);
    } catch (e: any) {
      toast.error(t("pairing.initiate_failed"), { description: String(e) });
    }
  }, [token, toast, t]);

  // Poll pairing status
  useEffect(() => {
    if (!polling || !pairingId || !token) return;
    let stopped = false;
    let attempts = 0;
    const maxAttempts = 150; // 5 minutes at 2s interval

    const poll = async () => {
      if (stopped || attempts >= maxAttempts) {
        setPolling(false);
        if (attempts >= maxAttempts) {
          toast.error(t("pairing.timeout"));
        }
        return;
      }
      attempts++;
      try {
        const result = await ipcInvoke<any>("ipc_pairing_status", {
          token,
          pairing_id: pairingId,
        });
        if (result.status === "completed") {
          setPolling(false);
          toast.success(t("pairing.completed"));
          // Refresh devices
          const devResult = await ipcInvoke<any>("ipc_pairing_list_devices", { token });
          setDevices(devResult.devices || []);
          return;
        }
      } catch {
        // ignore polling errors
      }
      setTimeout(poll, 2000);
    };
    poll();
    return () => { stopped = true; };
  }, [polling, pairingId, token, toast, t]);

  const handleRevoke = useCallback(async (pid: string) => {
    if (!token) return;
    try {
      await ipcInvoke("ipc_pairing_revoke", { token, pairing_id: pid });
      setDevices(devices.filter(d => d.pairing_id !== pid));
      toast.success(t("pairing.revoked"));
    } catch (e: any) {
      toast.error(t("pairing.revoke_failed"), { description: String(e) });
    }
  }, [token, devices, toast, t]);

  // Generate QR code content
  const qrContent = pairingId
    ? JSON.stringify({
        pairing_id: pairingId,
        relay_url: "https://termfast.xisj.com",
      })
    : "";

  if (!token) {
    return (
      <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
        <div className="bg-white dark:bg-[#2C2C2E] rounded-xl p-6 w-96 max-w-[90vw]">
          <h2 className="text-lg font-semibold mb-4">{t("pairing.login_title")}</h2>
          <input
            type="email"
            placeholder={t("pairing.email")}
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            className="w-full mb-3 px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-transparent"
          />
          <input
            type="password"
            placeholder={t("pairing.password")}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            className="w-full mb-4 px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-transparent"
          />
          <div className="flex gap-2">
            <button
              onClick={handleRegister}
              className="flex-1 px-4 py-2 rounded-lg bg-gray-200 dark:bg-gray-700"
            >
              {t("pairing.register")}
            </button>
            <button
              onClick={handleLogin}
              className="flex-1 px-4 py-2 rounded-lg bg-blue-500 text-white"
            >
              {t("pairing.login")}
            </button>
          </div>
          <button onClick={onClose} className="mt-3 text-sm text-gray-500 w-full">
            {t("common.cancel")}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-white dark:bg-[#2C2C2E] rounded-xl p-6 w-[500px] max-w-[90vw] max-h-[80vh] overflow-auto">
        <h2 className="text-lg font-semibold mb-4">{t("pairing.title")}</h2>

        {!pairingId && !polling && (
          <button
            onClick={handleInitiatePairing}
            className="w-full px-4 py-3 rounded-lg bg-blue-500 text-white mb-4"
          >
            {t("pairing.pair_new_device")}
          </button>
        )}

        {polling && pairingId && (
          <div className="text-center py-4">
            <div className="inline-block w-48 h-48 border-2 border-gray-300 rounded-lg flex items-center justify-center mb-4">
              <div className="text-xs text-gray-500 p-4 text-center break-all">
                {qrContent}
              </div>
            </div>
            <p className="text-sm text-gray-500">{t("pairing.waiting_scan")}</p>
            <div className="mt-2 text-xs text-gray-400">{pairingId}</div>
          </div>
        )}

        {devices.length > 0 && (
          <div className="mt-4">
            <h3 className="text-sm font-medium mb-2">{t("pairing.paired_devices")}</h3>
            {devices.map((d) => (
              <div key={d.pairing_id} className="flex items-center justify-between py-2 border-b border-gray-200 dark:border-gray-700">
                <div>
                  <div className="text-sm">{d.mobile_device_id || d.pairing_id}</div>
                  <div className="text-xs text-gray-500">{d.status}</div>
                </div>
                <button
                  onClick={() => handleRevoke(d.pairing_id)}
                  className="text-xs text-red-500"
                >
                  {t("pairing.revoke")}
                </button>
              </div>
            ))}
          </div>
        )}

        <button onClick={onClose} className="mt-4 text-sm text-gray-500 w-full">
          {t("common.close")}
        </button>
      </div>
    </div>
  );
}
