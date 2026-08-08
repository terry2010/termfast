// Unit tests for joinNetwork — D4 signing logic
import { describe, it, expect, vi, beforeEach } from "vitest";

// Mock ipcInvoke before importing the module under test
vi.mock("@/hooks/useIpc", () => ({
  ipcInvoke: vi.fn(),
}));

import { ipcInvoke } from "@/hooks/useIpc";
import {
  canonicalJSON,
  buildApproveJoinPayload,
  signApproveJoinPayload,
  approveJoin,
  getBatchInfo,
} from "../joinNetwork";

const mockIpcInvoke = ipcInvoke as unknown as ReturnType<typeof vi.fn>;

beforeEach(() => {
  mockIpcInvoke.mockReset();
});

describe("canonicalJSON", () => {
  it("serializes object with sorted keys", () => {
    const result = canonicalJSON({ b: 2, a: 1, c: 3 });
    expect(result).toBe('{"a":1,"b":2,"c":3}');
  });

  it("serializes string with proper escaping", () => {
    expect(canonicalJSON("hello")).toBe('"hello"');
    expect(canonicalJSON('a"b')).toBe('"a\\"b"');
  });

  it("serializes numbers", () => {
    expect(canonicalJSON(42)).toBe("42");
    expect(canonicalJSON(3.14)).toBe("3.14");
  });

  it("serializes booleans and null", () => {
    expect(canonicalJSON(true)).toBe("true");
    expect(canonicalJSON(false)).toBe("false");
    expect(canonicalJSON(null)).toBe("null");
  });

  it("serializes arrays", () => {
    expect(canonicalJSON([1, 2, 3])).toBe("[1,2,3]");
  });

  it("serializes nested objects with sorted keys", () => {
    const result = canonicalJSON({ outer: { z: 1, a: 2 } });
    expect(result).toBe('{"outer":{"a":2,"z":1}}');
  });

  it("produces no whitespace", () => {
    const result = canonicalJSON({ a: 1, b: 2 });
    expect(result).not.toContain(" ");
  });
});

describe("buildApproveJoinPayload", () => {
  it("builds base64-encoded canonical JSON with all 5 fields", () => {
    const payloadBase64 = buildApproveJoinPayload({
      joinBatchId: "batch-123",
      approverUserId: 42,
      approverDeviceId: "MacBook-terry-a3f7",
      serverNonce: "bm9uY2U=", // base64("nonce")
      batchContentHash: "aGFzaA==", // base64("hash")
    });

    // Decode and verify
    const json = atob(payloadBase64);
    const parsed = JSON.parse(json);

    expect(parsed.join_batch_id).toBe("batch-123");
    expect(parsed.approver_user_id).toBe(42);
    expect(parsed.approver_device_id).toBe("MacBook-terry-a3f7");
    expect(parsed.server_nonce).toBe("bm9uY2U=");
    expect(parsed.batch_content_hash).toBe("aGFzaA==");
  });

  it("produces canonical JSON with sorted keys", () => {
    const payloadBase64 = buildApproveJoinPayload({
      joinBatchId: "b1",
      approverUserId: 1,
      approverDeviceId: "dev1",
      serverNonce: "n1",
      batchContentHash: "h1",
    });

    const json = atob(payloadBase64);
    // Keys should be in alphabetical order: approver_device_id, approver_user_id, batch_content_hash, join_batch_id, server_nonce
    const expected = '{"approver_device_id":"dev1","approver_user_id":1,"batch_content_hash":"h1","join_batch_id":"b1","server_nonce":"n1"}';
    expect(json).toBe(expected);
  });
});

describe("signApproveJoinPayload", () => {
  it("calls ipc_sign_device_payload with base64 payload", async () => {
    mockIpcInvoke.mockResolvedValue("DER_SIGNATURE_BASE64");

    const payloadBase64 = "eyJ0ZXN0IjoidmFsdWUifQ=="; // base64 of {"test":"value"}
    const sig = await signApproveJoinPayload(payloadBase64);

    expect(sig).toBe("DER_SIGNATURE_BASE64");
    expect(mockIpcInvoke).toHaveBeenCalledWith("ipc_sign_device_payload", { payloadBase64 });
  });

  it("propagates IPC errors", async () => {
    mockIpcInvoke.mockRejectedValue(new Error("signing failed"));

    await expect(signApproveJoinPayload("dGVzdA==")).rejects.toThrow("signing failed");
  });
});

describe("approveJoin", () => {
  it("builds payload, signs, and submits to backend", async () => {
    // Mock getDesktopDeviceId (called via ipc_get_local_info)
    // and ipc_sign_device_payload and ipc_approve_join
    mockIpcInvoke
      .mockResolvedValueOnce({ hostname: "mac", username: "terry", device_suffix: "a3f7" }) // getDesktopDeviceId
      .mockResolvedValueOnce("SIGNATURE_B64") // signApproveJoinPayload
      .mockResolvedValueOnce({ // ipc_approve_join
        batch_id: "batch-1",
        received_approvals: 1,
        required_approvals: 2,
        status: "pending_approval",
      });

    const result = await approveJoin({
      batchId: "batch-1",
      approverUserId: 42,
      serverNonce: "bm9uY2U=",
      batchContentHash: "aGFzaA==",
      token: "jwt-token",
    });

    expect(result.receivedApprovals).toBe(1);
    expect(result.requiredApprovals).toBe(2);
    expect(result.status).toBe("pending_approval");

    // Verify 3 IPC calls were made
    expect(mockIpcInvoke).toHaveBeenCalledTimes(3);

    // First call: getDesktopDeviceId (ipc_get_local_info)
    expect(mockIpcInvoke.mock.calls[0][0]).toBe("ipc_get_local_info");

    // Second call: sign payload
    expect(mockIpcInvoke.mock.calls[1][0]).toBe("ipc_sign_device_payload");

    // Third call: approve join
    expect(mockIpcInvoke.mock.calls[2][0]).toBe("ipc_approve_join");
    const approveArgs = mockIpcInvoke.mock.calls[2][1] as Record<string, string>;
    expect(approveArgs.token).toBe("jwt-token");
    expect(approveArgs.batch_id).toBe("batch-1");
    expect(approveArgs.approver_device_id).toBe("mac-terry-a3f7");
    expect(approveArgs.signature).toBe("SIGNATURE_B64");
    // payload should be base64-encoded canonical JSON
    expect(typeof approveArgs.payload).toBe("string");
    expect(approveArgs.payload.length).toBeGreaterThan(0);
  });
});

describe("getBatchInfo", () => {
  it("calls ipc_get_batch_info with token, batch_id, device_id", async () => {
    mockIpcInvoke
      .mockResolvedValueOnce({ hostname: "mac", username: "terry", device_suffix: "a3f7" }) // getDesktopDeviceId
      .mockResolvedValueOnce({
        batch_id: "batch-1",
        flow_type: "initial",
        required_approvals: 2,
        received_approvals: 0,
        server_nonce: "bm9uY2U=",
        batch_content_hash: "aGFzaA==",
        expires_at: "2024-01-01T00:00:00Z",
        status: "pending_approval",
        target_members: [
          { user_id: 1, device_id: "DEV-A", device_name: "Desktop A" },
          { user_id: 2, device_id: "DEV-B", device_name: "Desktop B" },
        ],
        initiator_user_id: 3,
      });

    const result = await getBatchInfo({ batchId: "batch-1", token: "jwt" });

    expect(result.batch_id).toBe("batch-1");
    expect(result.target_members).toHaveLength(2);
    expect(mockIpcInvoke).toHaveBeenCalledTimes(2);
    expect(mockIpcInvoke.mock.calls[1][0]).toBe("ipc_get_batch_info");
    const args = mockIpcInvoke.mock.calls[1][1] as Record<string, string>;
    expect(args.batch_id).toBe("batch-1");
    expect(args.device_id).toBe("mac-terry-a3f7");
    expect(args.token).toBe("jwt");
  });
});
