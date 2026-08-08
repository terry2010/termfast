// Unit tests for getDesktopDeviceId — D9 device_id random suffix
import { describe, it, expect, vi, beforeEach } from "vitest";

// Mock ipcInvoke before importing the module under test
vi.mock("@/hooks/useIpc", () => ({
  ipcInvoke: vi.fn(),
}));

import { ipcInvoke } from "@/hooks/useIpc";
import { getDesktopDeviceId } from "../pairing";

const mockIpcInvoke = ipcInvoke as unknown as ReturnType<typeof vi.fn>;

beforeEach(() => {
  mockIpcInvoke.mockReset();
});

describe("getDesktopDeviceId", () => {
  it("returns hostname-username-xxxx when device_suffix is present", async () => {
    mockIpcInvoke.mockResolvedValue({
      hostname: "my-mac",
      username: "terry",
      device_suffix: "a3f7",
    });

    const id = await getDesktopDeviceId();
    expect(id).toBe("my-mac-terry-a3f7");
  });

  it("falls back to hostname-username when device_suffix is empty", async () => {
    mockIpcInvoke.mockResolvedValue({
      hostname: "my-mac",
      username: "terry",
      device_suffix: "",
    });

    const id = await getDesktopDeviceId();
    expect(id).toBe("my-mac-terry");
  });

  it("falls back to hostname-username when device_suffix is missing", async () => {
    mockIpcInvoke.mockResolvedValue({
      hostname: "my-mac",
      username: "terry",
      // no device_suffix field
    });

    const id = await getDesktopDeviceId();
    expect(id).toBe("my-mac-terry");
  });

  it("uses 'unknown' for missing hostname/username", async () => {
    mockIpcInvoke.mockResolvedValue({
      device_suffix: "b2c4",
    });

    const id = await getDesktopDeviceId();
    expect(id).toBe("unknown-unknown-b2c4");
  });
});
