import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Smartphone, Plus, LogOut, X } from "lucide-react";
import { ipcInvoke } from "@/hooks/useIpc";
import { toast } from "@/components/ui/toast";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";

// === SECTION 1 END ===

/**
 * Pairing card — macOS Settings style grouped card for device pairing.
 * Shown on the "My Computer" overview page, right side of the connection card.
 *
 * Features:
 * - Login/register with email+password (backend account)
 * - Initiate pairing (generate QR code for mobile to scan)
 * - List paired devices with revoke (with confirmation dialog)
 * - Logout
 */
export function PairingCard() {
  const { t } = useTranslation();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [token, setToken] = useState<string | null>(null);
  const [pairingId, setPairingId] = useState<string | null>(null);
  const [pairingKey, setPairingKey] = useState<string | null>(null);
  const [polling, setPolling] = useState(false);
  const [devices, setDevices] = useState<any[]>([]);
  const [revokeTarget, setRevokeTarget] = useState<string | null>(null);

  // Restore token from localStorage on mount
  useEffect(() => {
    const saved = localStorage.getItem("pairing_token");
    if (saved) {
      setToken(saved);
      ipcInvoke<any>("ipc_pairing_list_devices", { token: saved })
        .then((r) => setDevices(r.devices || []))
        .catch(() => {});
    }
  }, []);

  const handleRegister = async () => {
    try {
      await ipcInvoke("ipc_pairing_register", { email, password });
      toast.success(t("pairing.register_success"));
    } catch (e: any) {
      toast.error(t("pairing.register_failed"), { description: String(e) });
    }
  };

  const handleLogin = async () => {
    try {
      const result = await ipcInvoke<any>("ipc_pairing_login", { email, password });
      const tok = result.access_token;
      setToken(tok);
      localStorage.setItem("pairing_token", tok);
      toast.success(t("pairing.login_success"));
      const devResult = await ipcInvoke<any>("ipc_pairing_list_devices", { token: tok });
      setDevices(devResult.devices || []);
    } catch (e: any) {
      toast.error(t("pairing.login_failed"), { description: String(e) });
    }
  };

  const handleInitiatePairing = async () => {
    if (!token) return;
    try {
      const result = await ipcInvoke<any>("ipc_pairing_initiate", {
        token,
        desktop_device_id: "desktop-" + Date.now(),
      });
      const pairingKey = await ipcInvoke<string>("ipc_generate_pairing_key");
      setPairingId(result.pairing_id);
      setPairingKey(pairingKey);
      setPolling(true);
    } catch (e: any) {
      toast.error(t("pairing.initiate_failed"), { description: String(e) });
    }
  };

  // Poll for pairing completion
  useEffect(() => {
    if (!polling || !pairingId || !token || !pairingKey) return;
    let stopped = false;
    let attempts = 0;
    const maxAttempts = 150;
    const poll = async () => {
      if (stopped || attempts >= maxAttempts) {
        setPolling(false);
        if (attempts >= maxAttempts) toast.error(t("pairing.timeout"));
        return;
      }
      attempts++;
      try {
        const result = await ipcInvoke<any>("ipc_pairing_status", { token, pairing_id: pairingId });
        if (result.status === "completed") {
          setPolling(false);
          toast.success(t("pairing.completed"));
          if (pairingKey) {
            try {
              await ipcInvoke("ipc_tunnel_start", {
                pairing_id: pairingId,
                pairing_key_hex: pairingKey,
                relay_url: "ws://sh.zimufan.com:39527/tunnel",
                jwt: token,
              });
            } catch (e: any) {
              toast.error("隧道启动失败", { description: String(e) });
            }
          }
          const devResult = await ipcInvoke<any>("ipc_pairing_list_devices", { token });
          setDevices(devResult.devices || []);
          setPairingId(null);
          setPairingKey(null);
          return;
        }
      } catch { /* ignore */ }
      setTimeout(poll, 2000);
    };
    poll();
    return () => { stopped = true; };
  }, [polling, pairingId, pairingKey, token, t]);

  const handleRevoke = async (pid: string) => {
    if (!token) return;
    try {
      await ipcInvoke("ipc_pairing_revoke", { token, pairing_id: pid });
      setDevices(devices.filter((d) => d.pairing_id !== pid));
      toast.success(t("pairing.revoked"));
    } catch (e: any) {
      toast.error(t("pairing.revoke_failed"), { description: String(e) });
    }
  };

  const handleLogout = () => {
    setToken(null);
    setDevices([]);
    setPairingId(null);
    setPairingKey(null);
    localStorage.removeItem("pairing_token");
  };

  const handleCancelPairing = () => {
    setPolling(false);
    setPairingId(null);
    setPairingKey(null);
  };

  const qrContent = pairingId && pairingKey
    ? JSON.stringify({
        pairing_id: pairingId,
        backend_url: "http://sh.zimufan.com:39527",
        pairing_key: pairingKey,
        relay_url: "ws://sh.zimufan.com:39527/tunnel",
      })
    : "";

  return (
    <div className="bg-[#FBFBFB] dark:bg-[#1E1E1E] rounded-[16px] overflow-hidden border border-gray-200/80 dark:border-white/[0.06] flex flex-col">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-4 border-b border-gray-100 dark:border-white/[0.06]">
        <div className="min-w-0 flex items-center gap-3">
          <div className="w-11 h-11 rounded-[13px] bg-gradient-to-br from-[#5856D6]/15 to-[#5856D6]/5 flex items-center justify-center text-[#5856D6] shadow-sm">
            <Smartphone className="w-6 h-6" />
          </div>
          <div>
            <div className="text-base font-semibold text-gray-900 dark:text-gray-100">
              {t("pairing.title")}
            </div>
            <div className="text-xs text-gray-500">
              {t("pairing.description", "配对手机端，实现远程终端访问")}
            </div>
          </div>
        </div>
        {token && !polling && !pairingId && (
          <button
            className="px-3.5 py-1.5 text-sm rounded-lg bg-[#007AFF] text-white hover:bg-[#0066DB] font-medium transition-colors flex items-center gap-1.5"
            onClick={handleInitiatePairing}
          >
            <Plus className="w-4 h-4" />
            {t("pairing.pair_new_device")}
          </button>
        )}
      </div>

      {/* Body */}
      <div className="divide-y divide-gray-100 dark:divide-white/[0.06]">
        {!token ? (
          /* Login / Register form */
          <div className="px-4 py-4 space-y-3">
            <input
              type="email"
              placeholder={t("pairing.email")}
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              className="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-transparent text-sm text-gray-900 dark:text-gray-100 focus:outline-none focus:ring-1 focus:ring-[#007AFF]"
            />
            <input
              type="password"
              placeholder={t("pairing.password")}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              className="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-transparent text-sm text-gray-900 dark:text-gray-100 focus:outline-none focus:ring-1 focus:ring-[#007AFF]"
            />
            <p className="text-xs text-gray-400 -mt-1">{t("pairing.password_hint", "密码至少8位")}</p>
            <div className="flex gap-2">
              <button
                onClick={handleRegister}
                disabled={email.length < 3 || password.length < 8}
                className="flex-1 px-4 py-2 rounded-lg bg-gray-100 dark:bg-[#2C2C2E] text-gray-700 dark:text-gray-200 hover:bg-gray-200 dark:hover:bg-[#3A3A3C] text-sm disabled:opacity-50 transition-colors"
              >
                {t("pairing.register")}
              </button>
              <button
                onClick={handleLogin}
                disabled={email.length < 3 || password.length < 8}
                className="flex-1 px-4 py-2 rounded-lg bg-[#007AFF] text-white hover:bg-[#0066DB] text-sm disabled:opacity-50 font-medium transition-colors"
              >
                {t("pairing.login")}
              </button>
            </div>
          </div>
        ) : (
          <>
            {/* Pairing QR code (polling state) */}
            {polling && pairingId && (
              <div className="px-4 py-4">
                <div className="flex items-start justify-between mb-3">
                  <p className="text-sm text-gray-500">{t("pairing.waiting_scan")}</p>
                  <button
                    onClick={handleCancelPairing}
                    className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-200"
                  >
                    <X className="w-4 h-4" />
                  </button>
                </div>
                <div className="bg-white p-4 rounded-lg flex items-center justify-center">
                  <QRCodeDisplay content={qrContent} />
                </div>
                <div className="mt-2 text-xs text-gray-400 text-center font-mono break-all">{pairingId}</div>
              </div>
            )}

            {/* Paired devices list */}
            {devices.length > 0 && (
              <div className="px-4 py-3">
                <h3 className="text-xs font-medium text-gray-500 mb-2">{t("pairing.paired_devices")}</h3>
                <div className="space-y-1">
                  {devices.map((d) => (
                    <div key={d.pairing_id} className="flex items-center justify-between py-2 px-3 rounded-lg bg-gray-50 dark:bg-[#2C2C2E]">
                      <div className="min-w-0">
                        <div className="text-sm text-gray-900 dark:text-gray-100 truncate">
                          {d.mobile_device_id || d.pairing_id}
                        </div>
                        <div className="text-xs text-gray-500">{d.status}</div>
                      </div>
                      <button
                        onClick={() => setRevokeTarget(d.pairing_id)}
                        className="text-xs text-red-500 hover:text-red-600 font-medium ml-2 flex-shrink-0"
                      >
                        {t("pairing.revoke")}
                      </button>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {/* Logout */}
            <div className="px-4 py-3">
              <button
                onClick={handleLogout}
                className="text-xs text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 flex items-center gap-1"
              >
                <LogOut className="w-3.5 h-3.5" />
                {t("pairing.logout", "退出登录")}
              </button>
            </div>
          </>
        )}
      </div>

      {/* Revoke confirmation dialog */}
      {revokeTarget && (
        <ConfirmDialog
          level="high"
          title={t("pairing.revoke")}
          message={t("pairing.revoke_confirm", "撤销配对后，该设备将无法再远程访问您的终端。确定要撤销吗？")}
          confirmLabel={t("pairing.revoke")}
          onConfirm={() => {
            const target = revokeTarget;
            setRevokeTarget(null);
            handleRevoke(target);
          }}
          onCancel={() => setRevokeTarget(null)}
        />
      )}
    </div>
  );
}

// === SECTION 1 END ===

function QRCodeDisplay({ content }: { content: string }) {
  const [svg, setSvg] = useState<string>("");
  useEffect(() => {
    import("qrcode")
      .then((QRCode) => {
        QRCode.toString(content, { type: "svg", margin: 1, width: 200 }, (err: any, s: string) => {
          if (!err) setSvg(s);
        });
      })
      .catch(() => {
        setSvg(`<text x="10" y="100" font-size="10">${content}</text>`);
      });
  }, [content]);
  return <div dangerouslySetInnerHTML={{ __html: svg || "<div>Generating...</div>" }} />;
}
