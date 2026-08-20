// src/components/shared/DesktopPairMode.tsx — Desktop interconnect pairing mode QR
//
// When the desktop enters "pairing mode", it displays a QR code containing:
//   { type: "desktop_pair", device_id, device_name, ecdh_public_key, user_id, backend_url }
//
// The mobile app scans this QR (along with another desktop's QR) to establish
// a desktop-to-desktop pairing via ECDH key agreement.

import { useState, useEffect, useCallback } from "react";
import { Link2, X, Loader2, RefreshCw } from "lucide-react";
import { ipcInvoke } from "@/hooks/useIpc";

interface DesktopPairModeProps {
  onClose: () => void;
}

export function DesktopPairMode({ onClose }: DesktopPairModeProps) {
  const [qrSvg, setQrSvg] = useState<string>("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string>("");
  const [deviceInfo, setDeviceInfo] = useState<{
    device_id: string;
    device_name: string;
    ecdh_public_key: string;
    user_id: number;
  } | null>(null);

  const generateQr = useCallback(async () => {
    setLoading(true);
    setError("");
    try {
      // 1. Get local device info (device_id, device_name, user_id)
      const info = await ipcInvoke<any>("ipc_get_local_info");
      // 2. Get ECDH public key
      const ecdhPubKey = await ipcInvoke<string>("ipc_get_ecdh_public_key");
      // 3. Get user_id from token (stored locally)
      const token = localStorage.getItem("pairing_token") || "";
      // user_id is in the JWT payload; decode it
      let userId = 0;
      try {
        const payload = JSON.parse(atob(token.split(".")[1]));
        userId = payload.user_id || 0;
      } catch {}

      const qrData = {
        type: "desktop_pair",
        device_id: info.device_id || info.hostname,
        device_name: info.device_name || info.hostname,
        ecdh_public_key: ecdhPubKey,
        user_id: userId,
        backend_url: "http://sh.zimufan.com:39527",
      };

      setDeviceInfo({
        device_id: qrData.device_id,
        device_name: qrData.device_name,
        ecdh_public_key: ecdhPubKey,
        user_id: userId,
      });

      // Generate QR code SVG
      const QRCode = await import("qrcode");
      QRCode.toString(JSON.stringify(qrData), {
        type: "svg",
        margin: 1,
        width: 240,
      }, (err: any, svg: string) => {
        if (err) {
          setError("QR generation failed: " + String(err));
        } else {
          // Security: sanitize SVG to strip any potential script tags
          // (defense-in-depth, qrcode lib shouldn't produce them)
          const sanitized = svg.replace(/<script[\s\S]*?<\/script>/gi, "");
          setQrSvg(sanitized);
        }
        setLoading(false);
      });
    } catch (e: any) {
      setError(String(e?.message || e));
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    generateQr();
  }, [generateQr]);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-sm" onClick={onClose}>
      <div
        className="bg-white dark:bg-[#1E1E1E] rounded-2xl shadow-2xl p-6 max-w-sm w-full mx-4 border border-gray-200 dark:border-white/10"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between mb-4">
          <div className="flex items-center gap-2">
            <Link2 className="w-5 h-5 text-[#5856D6]" />
            <h2 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
              桌面互联配对模式
            </h2>
          </div>
          <button
            onClick={onClose}
            className="p-1 rounded-lg hover:bg-gray-100 dark:hover:bg-white/10 text-gray-500"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        <p className="text-sm text-gray-500 dark:text-gray-400 mb-4">
          用手机端 TermFast 扫描此二维码，再扫描另一台桌面端的二维码，即可建立互联。
        </p>

        {/* QR Code */}
        <div className="flex flex-col items-center gap-3">
          {loading ? (
            <div className="w-[240px] h-[240px] flex items-center justify-center">
              <Loader2 className="w-8 h-8 animate-spin text-[#5856D6]" />
            </div>
          ) : error ? (
            <div className="w-[240px] h-[240px] flex items-center justify-center text-red-500 text-sm text-center px-4">
              {error}
            </div>
          ) : (
            <div
              className="bg-white p-3 rounded-xl border border-gray-200"
              dangerouslySetInnerHTML={{ __html: qrSvg }}
            />
          )}

          {/* Device info */}
          {deviceInfo && (
            <div className="w-full space-y-1 text-xs text-gray-500 dark:text-gray-400">
              <div className="flex justify-between">
                <span>设备名:</span>
                <span className="font-mono">{deviceInfo.device_name}</span>
              </div>
              <div className="flex justify-between">
                <span>设备 ID:</span>
                <span className="font-mono">{deviceInfo.device_id}</span>
              </div>
              <div className="flex justify-between">
                <span>ECDH 公钥:</span>
                <span className="font-mono">{deviceInfo.ecdh_public_key.slice(0, 16)}...</span>
              </div>
            </div>
          )}

          {/* Refresh button */}
          <button
            onClick={generateQr}
            className="flex items-center gap-1.5 text-xs text-[#5856D6] hover:underline mt-2"
          >
            <RefreshCw className="w-3.5 h-3.5" />
            重新生成
          </button>
        </div>

        {/* Security note */}
        <div className="mt-4 p-3 bg-[#5856D6]/5 rounded-lg text-xs text-gray-500 dark:text-gray-400">
          <p className="font-medium text-[#5856D6] mb-1">安全说明</p>
          <p>二维码只含 ECDH 公钥（非秘密）。私钥存储在本机安全存储中，不离开设备。</p>
        </div>
      </div>
    </div>
  );
}
