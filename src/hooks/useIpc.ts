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
import i18n from "@/i18n/config";

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
    // Tauri errors come as strings (JSON from Rust) or { code, detail } objects
    if (typeof e === "string") {
      // Try to parse as JSON { code, detail } from the Rust backend
      try {
        const parsed = JSON.parse(e);
        if (parsed && typeof parsed === "object" && "code" in parsed) {
          throw new IpcErrorImpl(parsed as IpcError);
        }
      } catch (parseErr) {
        // Not JSON — fall through to plain string error
        if (parseErr instanceof SyntaxError) {
          // Not JSON, use as plain detail
        } else {
          throw parseErr;
        }
      }
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

/**
 * Parse the English `detail` field from an IpcError and return a
 * user-friendly, localized explanation.  The backend sends language-agnostic
 * ErrorCode + English detail; this function translates the common detail
 * patterns so the user never sees raw English technical text.
 */
function localizeDetail(code: string, detail: string): string | undefined {
  const d = detail.toLowerCase();

  // --- SshConnectFailed ---
  if (code === "SshConnectFailed") {
    if (d.includes("timed out") || d.includes("timeout"))
      return i18n.t("errors.detail.connect_timeout");
    if (d.includes("connection refused"))
      return i18n.t("errors.detail.connect_refused");
    if (d.includes("unreachable") || d.includes("noroutetohost"))
      return i18n.t("errors.detail.network_unreachable");
    if (d.includes("dns") || d.includes("name or service not known"))
      return i18n.t("errors.detail.dns_failed");
    if (d.includes("reset") || d.includes("broken pipe"))
      return i18n.t("errors.detail.connection_reset");
    if (d.includes("banner") || d.includes("protocol"))
      return i18n.t("errors.detail.protocol_error");
    return i18n.t("errors.detail.connect_failed");
  }

  // --- AuthFailed ---
  if (code === "AuthFailed") {
    if (d.includes("rejected by server"))
      return i18n.t("errors.detail.auth_rejected");
    if (d.includes("key file not found"))
      return i18n.t("errors.detail.key_not_found");
    if (d.includes("failed to load key"))
      return i18n.t("errors.detail.key_load_failed");
    if (d.includes("password auth error"))
      return i18n.t("errors.detail.auth_rejected");
    if (d.includes("key auth error"))
      return i18n.t("errors.detail.auth_rejected");
    return i18n.t("errors.detail.auth_rejected");
  }

  // --- HostKeyMismatch ---
  if (code === "HostKeyMismatch") {
    return i18n.t("errors.detail.hostkey_mismatch");
  }

  // --- CredentialNotFound ---
  if (code === "CredentialNotFound") {
    if (d.includes("key file"))
      return i18n.t("errors.detail.key_not_found");
    return i18n.t("errors.detail.credential_not_found");
  }

  // --- PortConflict / ProxyPortInUse ---
  if (code === "PortConflict" || code === "ProxyPortInUse") {
    return i18n.t("errors.detail.port_in_use", { detail });
  }

  // --- NeedsPrivilege ---
  if (code === "NeedsPrivilege") {
    return i18n.t("errors.detail.needs_privilege");
  }

  // --- SshDisconnected ---
  if (code === "SshDisconnected") {
    if (d.includes("reset") || d.includes("broken pipe"))
      return i18n.t("errors.detail.connection_reset");
    if (d.includes("timeout") || d.includes("timed out"))
      return i18n.t("errors.detail.connection_timeout");
    return i18n.t("errors.detail.disconnected");
  }

  return undefined;
}

/**
 * Format an IPC error into a translated message using i18n.
 * Falls back to the raw error string if no code is available.
 */
export function formatIpcError(e: unknown): string {
  if (e instanceof IpcErrorImpl) {
    const localized = localizeDetail(e.code, e.detail);
    if (localized) return localized;
    // Fallback: use the generic errors.<code> template with raw detail
    const key = `errors.${e.code}`;
    const translated = i18n.t(key, { detail: e.detail });
    if (translated !== key) return translated;
    return e.detail || e.message;
  }
  if (e instanceof Error) return e.message;
  return String(e);
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