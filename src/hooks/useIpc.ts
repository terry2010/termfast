// IPC wrapper for Tauri invoke + event listen — §4.2
// Type-safe wrapper around @tauri-apps/api
//
// Tauri commands return Result<serde_json::Value, String>:
// - Ok(value) → tauriInvoke resolves with the bare Value
// - Err(string) → tauriInvoke rejects with the error string

import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useState, useCallback } from "react";
import type { IpcError } from "@/types";

/** Invoke a Tauri command (IPC to daemon) */
// Convert snake_case keys to camelCase for Tauri command params
function toCamelCase(params: Record<string, unknown>): Record<string, unknown> {
  const result: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(params)) {
    const camelKey = key.replace(/_([a-z])/g, (_, c) => c.toUpperCase());
    result[camelKey] = value;
  }
  return result;
}

export async function ipcInvoke<T = unknown>(
  action: string,
  params?: Record<string, unknown>
): Promise<T> {
  // Tauri commands return bare Value on success, throw string on error
  try {
    const result = await tauriInvoke<unknown>(action, toCamelCase(params || {}));
    return result as T;
  } catch (e) {
    // Tauri errors come as strings or { code, detail } objects
    if (typeof e === "string") {
      throw new IpcErrorImpl({ code: "Internal", detail: e });
    }
    if (e && typeof e === "object" && "code" in e) {
      throw new IpcErrorImpl(e as IpcError);
    }
    throw new IpcErrorImpl({ code: "Internal", detail: String(e) });
  }
}

/** IpcError wrapper class */
export class IpcErrorImpl extends Error {
  code: string;
  detail: string;

  constructor(error: IpcError) {
    super(`${error.code}: ${error.detail}`);
    this.code = error.code;
    this.detail = error.detail;
    this.name = "IpcError";
  }
}

// === SECTION 1 END ===

/** Hook to listen for Tauri events (daemon → frontend) */
export function useTauriEvent<T = unknown>(
  eventName: string,
  handler: (data: T) => void
): void {
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let mounted = true;

    listen<T>(eventName, (event) => {
      if (mounted) {
        handler(event.payload);
      }
    }).then((unlistenFn) => {
      unlisten = unlistenFn;
    });

    return () => {
      mounted = false;
      if (unlisten) {
        unlisten();
      }
    };
  }, [eventName, handler]);
}

/** Hook for async IPC call with loading/error state */
export function useIpcCall<T>(
  action: string,
  options?: {
    immediate?: boolean;
    params?: Record<string, unknown>;
  }
): {
  data: T | null;
  loading: boolean;
  error: IpcErrorImpl | null;
  execute: (params?: Record<string, unknown>) => Promise<T>;
  reset: () => void;
} {
  const [data, setData] = useState<T | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<IpcErrorImpl | null>(null);

  const execute = useCallback(
    async (callParams?: Record<string, unknown>) => {
      setLoading(true);
      setError(null);
      try {
        const result = await ipcInvoke<T>(
          action,
          callParams || options?.params
        );
        setData(result);
        return result;
      } catch (e) {
        if (e instanceof IpcErrorImpl) {
          setError(e);
        } else {
          setError(new IpcErrorImpl({ code: "Internal", detail: String(e) }));
        }
        throw e;
      } finally {
        setLoading(false);
      }
    },
    [action, options?.params]
  );

  const reset = useCallback(() => {
    setData(null);
    setError(null);
    setLoading(false);
  }, []);

  useEffect(() => {
    if (options?.immediate) {
      execute().catch(() => {});
    }
  }, [options?.immediate, execute]);

  return { data, loading, error, execute, reset };
}

// === SECTION 2 END ===