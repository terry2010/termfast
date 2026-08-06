import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Smartphone, Plus, LogOut, ChevronRight, X } from "lucide-react";
import { ipcInvoke } from "@/hooks/useIpc";
import { toast } from "@/components/ui/toast";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { Modal } from "@/components/ui/Modal";
import { getDesktopDeviceId, fetchPairedDevices } from "@/lib/pairing";

// === SECTION 1 END ===

/**
 * Pairing card — macOS Settings style grouped card for device pairing.
 * Shown on the "My Computer" overview page, right side of the connection card.
 *
 * Card body is compact: shows paired device count.
 * "配对新设备" opens a modal with QR code.
 * Clicking the device count row opens a modal with the full device list + revoke.
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
  const [showQrModal, setShowQrModal] = useState(false);
  const [showDevicesModal, setShowDevicesModal] = useState(false);
  const [desktopName, setDesktopName] = useState<string>("");
  const [desktopDeviceId, setDesktopDeviceId] = useState<string>("");
  const [showAllDevices, setShowAllDevices] = useState(false);

  // On mount: get desktop_device_id + restore token, then fetch devices
  useEffect(() => {
    const saved = localStorage.getItem("pairing_token");
    getDesktopDeviceId()
      .then((deviceId) => {
        setDesktopDeviceId(deviceId);
        if (saved) {
          setToken(saved);
          return fetchPairedDevices(saved);
        }
        return null;
      })
      .then((devs) => {
        if (devs) setDevices(devs);
      })
      .catch(() => {});
  }, []);

  // Reload devices when showAllDevices toggles or modal opens.
  // Wait for desktopDeviceId to be set before fetching (unless showAllDevices
  // is true, in which case we intentionally pass empty to get all pairings).
  useEffect(() => {
    if (!token || !showDevicesModal) return;
    if (!showAllDevices && !desktopDeviceId) return; // not ready yet
    ipcInvoke<any>("ipc_pairing_list_devices", {
      token,
      desktop_device_id: showAllDevices ? "" : desktopDeviceId,
    })
      .then((r) => setDevices(r.devices || []))
      .catch(() => {});
  }, [showAllDevices, showDevicesModal, token, desktopDeviceId]);

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
      // Fetch devices filtered by this desktop's device_id
      const deviceId = await getDesktopDeviceId();
      setDesktopDeviceId(deviceId);
      const devs = await fetchPairedDevices(tok);
      setDevices(devs);
      // Restore tunnels for previously-persisted pairings (survives logout/login)
      ipcInvoke<any>("ipc_restore_tunnels", { jwt: tok }).catch((e) =>
        console.warn("[PairingCard] restore tunnels failed:", e),
      );
    } catch (e: any) {
      toast.error(t("pairing.login_failed"), { description: String(e) });
    }
  };

  const handleInitiatePairing = async () => {
    if (!token) return;
    try {
      const info = await ipcInvoke<any>("ipc_get_local_info");
      const hostname = info?.hostname || "unknown";
      const username = info?.username || "unknown";
      const desktopDeviceId = `${hostname}-${username}`;
      const dName = hostname;
      const result = await ipcInvoke<any>("ipc_pairing_initiate", {
        token,
        desktop_device_id: desktopDeviceId,
        desktop_name: dName,
      });
      const pairingKey = await ipcInvoke<string>("ipc_generate_pairing_key");
      setPairingId(result.pairing_id);
      setPairingKey(pairingKey);
      setDesktopName(dName);
      setPolling(true);
      setShowQrModal(true);
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
          const devs = await fetchPairedDevices(token!);
          setDevices(devs);
          setPairingId(null);
          setPairingKey(null);
          setShowQrModal(false);
          return;
        }
      } catch { /* ignore */ }
      setTimeout(poll, 2000);
    };
    poll();
    return () => { stopped = true; };
  }, [polling, pairingId, pairingKey, token, desktopDeviceId, t]);

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
    // Stop all tunnels without removing persisted pairing records,
    // so they can be restored when user logs back in.
    ipcInvoke("ipc_tunnel_stop_all", {}).catch(() => {});
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
    setDesktopName("");
    setShowQrModal(false);
  };

  const qrContent = pairingId && pairingKey
    ? JSON.stringify({
        pairing_id: pairingId,
        backend_url: "http://sh.zimufan.com:39527",
        pairing_key: pairingKey,
        relay_url: "ws://sh.zimufan.com:39527/tunnel",
        desktop_name: desktopName,
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
        {token && !polling && (
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
            {/* Paired devices count row — click to open device list modal */}
            <button
              className="flex items-center justify-between w-full px-4 py-3 text-left hover:bg-gray-50 dark:hover:bg-[#2C2C2E] transition-colors"
              onClick={() => setShowDevicesModal(true)}
            >
              <span className="text-sm text-gray-500">
                {t("pairing.paired_devices")}
              </span>
              <div className="flex items-center gap-1.5">
                <span className="text-sm font-medium text-gray-900 dark:text-gray-100">
                  {devices.length}
                </span>
                <ChevronRight className="w-4 h-4 text-gray-400" />
              </div>
            </button>

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

      {/* QR code modal — shown when pairing is initiated */}
      {showQrModal && polling && pairingId && (
        <Modal
          title={t("pairing.pair_new_device")}
          onClose={handleCancelPairing}
          maxWidth="max-w-sm"
        >
          <div className="flex flex-col items-center">
            <p className="text-sm text-gray-500 text-center mb-4">
              {t("pairing.waiting_scan")}
            </p>
            <div className="bg-white p-4 rounded-lg flex items-center justify-center">
              <QRCodeDisplay content={qrContent} />
            </div>
            <div className="mt-3 text-xs text-gray-400 text-center font-mono break-all">
              {pairingId}
            </div>
            <button
              onClick={handleCancelPairing}
              className="mt-4 px-4 py-2 rounded-lg bg-gray-100 dark:bg-[#2C2C2E] text-gray-700 dark:text-gray-200 hover:bg-gray-200 dark:hover:bg-[#3A3A3C] text-sm transition-colors flex items-center gap-1.5"
            >
              <X className="w-4 h-4" />
              {t("common.cancel")}
            </button>
          </div>
        </Modal>
      )}

      {/* Device list modal — shown when clicking paired devices count */}
      {showDevicesModal && (
        <Modal
          title={t("pairing.paired_devices")}
          onClose={() => { setShowDevicesModal(false); setShowAllDevices(false); }}
          maxWidth="max-w-md"
        >
          {/* Toggle: show all pairings in account (for emergency revoke) */}
          <div className="flex items-center justify-between mb-3 pb-3 border-b border-gray-200 dark:border-gray-700">
            <label className="text-xs text-gray-600 dark:text-gray-400 flex items-center gap-2 cursor-pointer">
              <input
                type="checkbox"
                checked={showAllDevices}
                onChange={(e) => setShowAllDevices(e.target.checked)}
                className="w-4 h-4 rounded"
              />
              {t("pairing.show_all_devices", "显示全部配对（紧急撤销）")}
            </label>
          </div>
          {devices.length === 0 ? (
            <p className="text-sm text-gray-500 text-center py-8">
              {t("pairing.no_devices", "暂无已配对设备")}
            </p>
          ) : (
            <div className="space-y-2">
              {devices.map((d) => (
                <div
                  key={d.pairing_id}
                  className="flex items-center justify-between py-2.5 px-3 rounded-lg bg-gray-50 dark:bg-[#2C2C2E]"
                >
                  <div className="min-w-0">
                    <div className="text-sm text-gray-900 dark:text-gray-100 truncate">
                      {showAllDevices ? (
                        <span>
                          <span className="font-medium">{d.desktop_name || d.desktop_device_id || "Unknown"}</span>
                          <span className="text-gray-400 mx-1">↔</span>
                          <span className="font-medium">{d.mobile_name || d.mobile_device_id || "Unknown"}</span>
                        </span>
                      ) : (
                        d.mobile_name || d.mobile_device_id || d.pairing_id
                      )}
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
          )}
        </Modal>
      )}

      {/* Revoke confirmation dialog */}
      {revokeTarget && (
        <ConfirmDialog
          level="medium"
          danger
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
