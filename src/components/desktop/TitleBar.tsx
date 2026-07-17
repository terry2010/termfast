// TitleBar — platform-specific title bar (§4.4)
// macOS: traffic lights, content extends to top
// Windows: standard or custom title bar

import { platform as tauriPlatform } from "@tauri-apps/plugin-os";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";
import { useEffect, useRef, useState } from "react";
import { useServerStore } from "@/stores/serverStore";

interface TitleBarProps {
  onOpenSettings?: () => void;
  onOpenTemplates?: () => void;
  onOpenOnboarding?: () => void;
}

/** Detect platform safely — falls back to userAgent in browser/dev mode */
function detectPlatform(): string {
  try {
    const p = tauriPlatform();
    if (p) return p;
  } catch {
    // Tauri API not available (browser dev mode)
  }
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes("mac")) return "macos";
  if (ua.includes("win")) return "windows";
  return "linux";
}

export function TitleBar(props: TitleBarProps) {
  const currentPlatform = detectPlatform();

  if (currentPlatform === "macos") {
    return <MacTitleBar {...props} />;
  }
  if (currentPlatform === "windows") {
    return <WinTitleBar {...props} />;
  }
  return null;
}

// === SECTION 1 END ===

/** Connection status summary for title bar */
function ConnectionSummary() {
  const { t } = useTranslation();
  const servers = useServerStore((s) => s.servers);
  const connected = servers.filter((s) => s.current_status === "connected");
  const abnormal = servers.filter(
    (s) => s.current_status === "auth_failed" || s.current_status === "reconnecting" || s.current_status === "offline"
  );
  const activeClients = servers
    .filter((s) => s.proxy_running)
    .reduce((sum, s) => sum + (s.active_channels || 0), 0);
  const totalBytesIn = servers
    .filter((s) => s.proxy_running)
    .reduce((sum, s) => sum + (s.bytes_in || 0), 0);
  const totalBytesOut = servers
    .filter((s) => s.proxy_running)
    .reduce((sum, s) => sum + (s.bytes_out || 0), 0);

  const speedRef = useRef({ in: 0, out: 0, time: Date.now(), speedIn: 0, speedOut: 0 });
  const [, forceUpdate] = useState(0);
  useEffect(() => {
    if (activeClients === 0) {
      speedRef.current = { in: 0, out: 0, time: Date.now(), speedIn: 0, speedOut: 0 };
      return;
    }
    const now = Date.now();
    const dt = (now - speedRef.current.time) / 1000;
    if (dt > 0.5) {
      const dIn = totalBytesIn - speedRef.current.in;
      const dOut = totalBytesOut - speedRef.current.out;
      speedRef.current = {
        in: totalBytesIn,
        out: totalBytesOut,
        time: now,
        speedIn: Math.max(0, dIn / dt),
        speedOut: Math.max(0, dOut / dt),
      };
    }
  }, [totalBytesIn, totalBytesOut, activeClients]);

  useEffect(() => {
    if (activeClients === 0) return;
    const id = setInterval(() => forceUpdate((v) => v + 1), 1000);
    return () => clearInterval(id);
  }, [activeClients]);

  const fmtSpeed = (bytesPerSec: number) => {
    if (bytesPerSec < 1024) return `${bytesPerSec.toFixed(0)} B/s`;
    if (bytesPerSec < 1024 * 1024) return `${(bytesPerSec / 1024).toFixed(1)} KB/s`;
    return `${(bytesPerSec / 1024 / 1024).toFixed(1)} MB/s`;
  };

  return (
    <div className="flex items-center gap-3 text-xs text-gray-600 dark:text-gray-300" data-tauri-drag-region>
      <span className="font-medium" data-tauri-drag-region>termfast</span>
      {connected.length > 0 && (
        <span className="flex items-center gap-1" data-tauri-drag-region>
          <span className="w-2 h-2 rounded-full bg-green-500" />
          {connected.length} {t("server.connected_count")}
        </span>
      )}
      {abnormal.length > 0 && (
        <span className="flex items-center gap-1" data-tauri-drag-region>
          <span className="w-2 h-2 rounded-full bg-red-500" />
          {abnormal.length} {t("server.connected_count")}
        </span>
      )}
      {activeClients > 0 && (speedRef.current.speedIn > 0 || speedRef.current.speedOut > 0) && (
        <span className="text-blue-500" data-tauri-drag-region>
          ↑ {fmtSpeed(speedRef.current.speedIn)} ↓ {fmtSpeed(speedRef.current.speedOut)}
        </span>
      )}
    </div>
  );
}

async function handleClose() {
  try {
    await getCurrentWindow().close();
  } catch (e) {
    console.error("close failed:", e);
  }
}

async function handleMinimize() {
  try {
    await getCurrentWindow().minimize();
  } catch (e) {
    console.error("minimize failed:", e);
  }
}

async function handleToggleMaximize() {
  try {
    const win = getCurrentWindow();
    if (await win.isMaximized()) {
      await win.unmaximize();
    } else {
      await win.maximize();
    }
  } catch (e) {
    console.error("maximize failed:", e);
  }
}

function MacTitleBar(_: TitleBarProps) {
  return (
    <div
      className="flex items-center h-8 px-3 select-none"
      data-tauri-drag-region
    >
      <div className="flex items-center gap-2" data-tauri-drag-region>
        <button
          className="w-3 h-3 shrink-0 aspect-square rounded-full bg-red-500 hover:bg-red-400"
          style={{ minWidth: "12px", maxWidth: "12px", minHeight: "12px", maxHeight: "12px" }}
          aria-label="close"
          onClick={handleClose}
        />
        <button
          className="w-3 h-3 shrink-0 aspect-square rounded-full bg-yellow-500 hover:bg-yellow-400"
          style={{ minWidth: "12px", maxWidth: "12px", minHeight: "12px", maxHeight: "12px" }}
          aria-label="minimize"
          onClick={handleMinimize}
        />
        <button
          className="w-3 h-3 shrink-0 aspect-square rounded-full bg-green-500 hover:bg-green-400"
          style={{ minWidth: "12px", maxWidth: "12px", minHeight: "12px", maxHeight: "12px" }}
          aria-label="maximize"
          onClick={handleToggleMaximize}
        />
      </div>
      <div
        className="flex-1 flex items-center justify-center gap-4"
        data-tauri-drag-region
      >
        <ConnectionSummary />
      </div>
    </div>
  );
}

function WinTitleBar(_: TitleBarProps) {
  return (
    <div
      className="flex items-center justify-between h-9 px-3 bg-gray-100 dark:bg-gray-800 select-none"
      data-tauri-drag-region
    >
      <div className="flex items-center gap-4" data-tauri-drag-region>
        <ConnectionSummary />
      </div>
      <div className="flex gap-1">
        <button
          className="w-8 h-7 hover:bg-gray-300 dark:hover:bg-gray-700"
          aria-label="minimize"
          onClick={handleMinimize}
        >─</button>
        <button
          className="w-8 h-7 hover:bg-gray-300 dark:hover:bg-gray-700"
          aria-label="maximize"
          onClick={handleToggleMaximize}
        >□</button>
        <button
          className="w-8 h-7 hover:bg-red-500 hover:text-white"
          aria-label="close"
          onClick={handleClose}
        >✕</button>
      </div>
    </div>
  );
}

// === SECTION 2 END ===
