import { useEffect, useState, useRef } from "react";
import { listen } from "@tauri-apps/api/event";

const IDLE_THRESHOLD_MS = 60_000; // 60 秒，硬编码

export interface IdleState {
  idle: boolean;       // 用户是否空闲（超过阈值）
  idleSeconds: number; // 空闲了多少秒
  locked: boolean;     // 屏幕是否锁定
}

/**
 * 桌面端系统空闲检测 hook。
 *
 * 使用 tauri-plugin-idlemonitor 的事件驱动 API：
 * - `system:idle` 事件：用户空闲超过阈值 / 从空闲恢复
 * - `system:lock` 事件：屏幕锁定 / 解锁
 *
 * 阈值固定 60 秒（IDLE_THRESHOLD_MS），在插件 start() 时传入。
 *
 * 注意：插件需要在 Rust 端注册（lib.rs 的 tauri::Builder::plugin()），
 * 并且 capabilities/default.json 需要包含 "idlemonitor:default" 权限。
 */
export function useUserIdle(): IdleState {
  const [state, setState] = useState<IdleState>({
    idle: false,
    idleSeconds: 0,
    locked: false,
  });

  // Track whether monitoring has been started
  const startedRef = useRef(false);

  useEffect(() => {
    if (startedRef.current) return;
    startedRef.current = true;

    let unlistenIdle: (() => void) | null = null;
    let unlistenLock: (() => void) | null = null;

    (async () => {
      // Start monitoring with 60-second idle threshold
      // The plugin emits `system:idle` and `system:lock` events
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        // 先 stop 之前可能用错误阈值启动的 monitor
        try { await invoke("plugin:idlemonitor|stop"); } catch {}
        // Start the idle monitor plugin with our threshold
        await invoke("plugin:idlemonitor|start", {
          options: { idleThresholdSecs: Math.floor(IDLE_THRESHOLD_MS / 1000) },
        });
      } catch (e) {
        console.error("useUserIdle: failed to start idlemonitor", e);
        return;
      }

      // Listen for idle state changes
      unlistenIdle = await listen<{ idle: boolean; seconds?: number }>(
        "system:idle",
        (event) => {
          const { idle, seconds } = event.payload;
          setState((prev) => ({
            ...prev,
            idle,
            idleSeconds: seconds ?? (idle ? Math.floor(IDLE_THRESHOLD_MS / 1000) : 0),
          }));
        }
      );

      // Listen for screen lock/unlock
      unlistenLock = await listen<{ locked: boolean }>(
        "system:lock",
        (event) => {
          const { locked } = event.payload;
          setState((prev) => ({
            ...prev,
            locked,
            // When screen locks, consider user as idle
            idle: locked ? true : prev.idle,
          }));
        }
      );
    })();

    return () => {
      unlistenIdle?.();
      unlistenLock?.();
      // Stop monitoring when no more consumers
      (async () => {
        try {
          const { invoke } = await import("@tauri-apps/api/core");
          await invoke("plugin:idlemonitor|stop");
        } catch {
          // ignore — app may be shutting down
        }
      })();
      startedRef.current = false;
    };
  }, []);

  return state;
}
