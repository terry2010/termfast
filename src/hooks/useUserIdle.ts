import { useEffect, useState, useRef } from "react";
import { listen } from "@tauri-apps/api/event";

const DEFAULT_IDLE_THRESHOLD_MS = 60_000; // 60 秒，默认值

export interface IdleState {
  idle: boolean;       // 用户是否空闲（超过阈值）
  idleSeconds: number; // 空闲了多少秒
  locked: boolean;     // 屏幕是否锁定
}

// === 全局单例：所有 useUserIdle 实例共享同一个 idle 状态 ===
// 避免多个 TerminalView tab 各自 start/stop 插件导致冲突。
let globalState: IdleState = { idle: false, idleSeconds: 0, locked: false };
let globalThresholdMs = DEFAULT_IDLE_THRESHOLD_MS;
let refCount = 0;
let unlistenIdle: (() => void) | null = null;
let unlistenLock: (() => void) | null = null;
let started = false;
const subscribers = new Set<(s: IdleState) => void>();

function notifyAll() {
  for (const fn of subscribers) fn(globalState);
}

async function startMonitor(thresholdMs: number) {
  if (started) return;
  started = true;
  globalThresholdMs = thresholdMs;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    try { await invoke("plugin:idlemonitor|stop"); } catch {}
    console.log(`%c[useUserIdle] starting idlemonitor with threshold=${Math.floor(thresholdMs / 1000)}s`, "color:red;font-weight:bold");
    await invoke("plugin:idlemonitor|start", {
      options: { idleThresholdSecs: Math.floor(thresholdMs / 1000) },
    });
    console.log(`%c[useUserIdle] idlemonitor started successfully`, "color:red;font-weight:bold");
  } catch (e) {
    console.error("useUserIdle: failed to start idlemonitor", e);
    started = false;
    return;
  }

  unlistenIdle = await listen<{ idle: boolean; seconds?: number }>(
    "system:idle",
    (event) => {
      const { idle, seconds } = event.payload;
      const prevIdle = globalState.idle;
      globalState = {
        ...globalState,
        idle,
        idleSeconds: seconds ?? (idle ? Math.floor(globalThresholdMs / 1000) : 0),
      };
      if (prevIdle !== idle) {
        console.log(`%c[useUserIdle] idle: ${prevIdle}→${idle} (threshold=${Math.floor(globalThresholdMs / 1000)}s)`, "color:red;font-weight:bold");
      }
      notifyAll();
    }
  );

  unlistenLock = await listen<{ locked: boolean }>(
    "system:lock",
    (event) => {
      const { locked } = event.payload;
      const prevLocked = globalState.locked;
      globalState = {
        ...globalState,
        locked,
        idle: locked ? true : globalState.idle,
      };
      if (prevLocked !== locked) {
        console.log(`%c[useUserIdle] locked: ${prevLocked}→${locked}`, "color:red;font-weight:bold");
      }
      notifyAll();
    }
  );
}

async function stopMonitor() {
  unlistenIdle?.();
  unlistenLock?.();
  unlistenIdle = null;
  unlistenLock = null;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("plugin:idlemonitor|stop");
  } catch {}
  started = false;
}

async function restartWithThreshold(thresholdMs: number) {
  globalThresholdMs = thresholdMs;
  await stopMonitor();
  await startMonitor(thresholdMs);
}

/**
 * 桌面端系统空闲检测 hook（全局单例）。
 *
 * 多个组件调用时共享同一个 idlemonitor 插件实例，
 * 用引用计数管理生命周期，避免 start/stop 冲突。
 *
 * @param thresholdSecs 空闲阈值秒数，0 或 undefined 时用默认 60 秒
 */
export function useUserIdle(thresholdSecs?: number): IdleState {
  const idleThresholdMs = thresholdSecs && thresholdSecs > 0
    ? thresholdSecs * 1000
    : DEFAULT_IDLE_THRESHOLD_MS;
  const [state, setState] = useState<IdleState>(globalState);
  const thresholdRef = useRef(idleThresholdMs);
  thresholdRef.current = idleThresholdMs;

  useEffect(() => {
    // 订阅全局状态
    subscribers.add(setState);

    // 首个消费者启动插件
    refCount++;
    if (refCount === 1) {
      startMonitor(idleThresholdMs);
    } else if (idleThresholdMs !== globalThresholdMs) {
      // 阈值变了，重启插件
      restartWithThreshold(idleThresholdMs);
    }

    return () => {
      subscribers.delete(setState);
      refCount--;
      if (refCount === 0) {
        stopMonitor();
      }
    };
  }, [idleThresholdMs]);

  return state;
}
