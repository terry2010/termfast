// cleanupRemoteDesktopConnection tests — FP-7 modal close cleanup logic
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useRemoteDesktopStore } from "@/stores/remoteDesktopStore";
import { invoke } from "@tauri-apps/api/core";

const mockInvoke = vi.mocked(invoke);

// Import after mocks are set up (mocks in setup.ts handle @tauri-apps/api)
import { cleanupRemoteDesktopConnection } from "@/components/remote-desktop/RemoteDesktopPanel";

describe("cleanupRemoteDesktopConnection", () => {
  beforeEach(() => {
    useRemoteDesktopStore.setState({
      peers: [],
      loading: false,
      error: null,
      activeConnection: null,
      remoteTerminals: [],
    });
    mockInvoke.mockReset();
  });

  it("does nothing when there is no active connection", async () => {
    await cleanupRemoteDesktopConnection();
    expect(mockInvoke).not.toHaveBeenCalled();
    expect(useRemoteDesktopStore.getState().activeConnection).toBeNull();
  });

  it("disconnects and clears active connection when one exists", async () => {
    useRemoteDesktopStore.getState().setActiveConnection("dpair-close-1");
    mockInvoke.mockResolvedValueOnce(undefined);

    await cleanupRemoteDesktopConnection();

    expect(mockInvoke).toHaveBeenCalledTimes(1);
    expect(mockInvoke).toHaveBeenCalledWith("ipc_remote_client_disconnect", {
      pairingId: "dpair-close-1",
    });
    expect(useRemoteDesktopStore.getState().activeConnection).toBeNull();
  });

  it("clears active connection even if disconnect IPC fails", async () => {
    useRemoteDesktopStore.getState().setActiveConnection("dpair-close-2");
    mockInvoke.mockRejectedValueOnce(new Error("IPC error"));

    await cleanupRemoteDesktopConnection();

    // Should still have attempted disconnect
    expect(mockInvoke).toHaveBeenCalledTimes(1);
    // Should still clear the connection despite error
    expect(useRemoteDesktopStore.getState().activeConnection).toBeNull();
  });
});
