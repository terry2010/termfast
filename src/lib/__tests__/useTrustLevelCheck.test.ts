// useTrustLevelCheck hook tests — D7: trust level conditional UI hiding
import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";

const mockIpcInvoke = vi.fn();
vi.mock("@/hooks/useIpc", () => ({
  ipcInvoke: (...args: unknown[]) => mockIpcInvoke(...args),
}));

import { useTrustLevelCheck } from "@/lib/useTrustLevelCheck";

beforeEach(() => {
  mockIpcInvoke.mockReset();
});

describe("useTrustLevelCheck", () => {
  it("returns hasLocalOnlyPairing=false when token is null", () => {
    const { result } = renderHook(() => useTrustLevelCheck(null));
    expect(result.current.hasLocalOnlyPairing).toBe(false);
    expect(result.current.loading).toBe(false);
  });

  it("returns hasLocalOnlyPairing=false when no pairings have local_only", async () => {
    mockIpcInvoke.mockImplementation((cmd: string) => {
      if (cmd === "ipc_get_local_info") return Promise.resolve({ hostname: "mac", username: "terry", device_suffix: "a3f7" });
      if (cmd === "ipc_pairing_list_devices") return Promise.resolve({
        devices: [
          { pairing_type: "mobile", trust_level: "full" },
          { pairing_type: "desktop", trust_level: "full" },
        ],
      });
      return Promise.resolve(undefined);
    });

    const { result } = renderHook(() => useTrustLevelCheck("jwt-token"));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });
    expect(result.current.hasLocalOnlyPairing).toBe(false);
  });

  it("returns hasLocalOnlyPairing=true when a mobile pairing has local_only", async () => {
    mockIpcInvoke.mockImplementation((cmd: string) => {
      if (cmd === "ipc_get_local_info") return Promise.resolve({ hostname: "mac", username: "terry", device_suffix: "a3f7" });
      if (cmd === "ipc_pairing_list_devices") return Promise.resolve({
        devices: [
          { pairing_type: "mobile", trust_level: "full" },
          { pairing_type: "mobile", trust_level: "local_only" },
        ],
      });
      return Promise.resolve(undefined);
    });

    const { result } = renderHook(() => useTrustLevelCheck("jwt-token"));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });
    expect(result.current.hasLocalOnlyPairing).toBe(true);
  });

  it("ignores desktop pairings with local_only (only checks mobile)", async () => {
    mockIpcInvoke.mockImplementation((cmd: string) => {
      if (cmd === "ipc_get_local_info") return Promise.resolve({ hostname: "mac", username: "terry", device_suffix: "a3f7" });
      if (cmd === "ipc_pairing_list_devices") return Promise.resolve({
        devices: [
          { pairing_type: "desktop", trust_level: "local_only" },
        ],
      });
      return Promise.resolve(undefined);
    });

    const { result } = renderHook(() => useTrustLevelCheck("jwt-token"));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });
    expect(result.current.hasLocalOnlyPairing).toBe(false);
  });

  it("returns false on fetch error", async () => {
    mockIpcInvoke.mockImplementation((cmd: string) => {
      if (cmd === "ipc_get_local_info") return Promise.resolve({ hostname: "mac", username: "terry", device_suffix: "a3f7" });
      if (cmd === "ipc_pairing_list_devices") return Promise.reject(new Error("Network error"));
      return Promise.resolve(undefined);
    });

    const { result } = renderHook(() => useTrustLevelCheck("jwt-token"));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });
    expect(result.current.hasLocalOnlyPairing).toBe(false);
  });
});
