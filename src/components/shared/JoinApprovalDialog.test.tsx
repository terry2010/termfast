// JoinApprovalDialog component tests — D1/D2 approval dialog
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

// Mock ipcInvoke
const mockIpcInvoke = vi.fn();
vi.mock("@/hooks/useIpc", () => ({
  ipcInvoke: (...args: unknown[]) => mockIpcInvoke(...args),
}));

// Mock i18n to return keys as-is
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, fallback?: string) => fallback ?? key,
  }),
}));

import { JoinApprovalDialog } from "@/components/shared/JoinApprovalDialog";

const mockBatchInfo = {
  batch_id: "batch-123",
  flow_type: "merge",
  required_approvals: 2,
  received_approvals: 0,
  server_nonce: "bm9uY2U=",
  batch_content_hash: "aGFzaA==",
  expires_at: "2025-01-01T00:00:00Z",
  status: "pending_approval",
  target_members: [
    { user_id: 1, device_id: "DEV-A", device_name: "MacBook Pro" },
    { user_id: 2, device_id: "DEV-B", device_name: "MacBook Air" },
  ],
  initiator_user_id: 3,
};

const mockLocalInfo = { hostname: "mac", username: "terry", device_suffix: "a3f7" };

beforeEach(() => {
  mockIpcInvoke.mockReset();
  // Default: return local info for ipc_get_local_info
  mockIpcInvoke.mockImplementation((cmd: string) => {
    if (cmd === "ipc_get_local_info") return Promise.resolve(mockLocalInfo);
    return Promise.resolve(undefined);
  });
});

describe("JoinApprovalDialog", () => {
  it("shows loading spinner while fetching batch info", () => {
    // Never resolve batch info
    mockIpcInvoke.mockImplementation((cmd: string) => {
      if (cmd === "ipc_get_local_info") return Promise.resolve(mockLocalInfo);
      return new Promise(() => {}); // never resolves
    });
    render(
      <JoinApprovalDialog
        batchId="batch-123"
        token="jwt"
        approverUserId={1}
        onClose={vi.fn()}
      />
    );
    expect(document.querySelector(".animate-spin")).toBeTruthy();
  });

  it("displays batch info after loading", async () => {
    mockIpcInvoke.mockImplementation((cmd: string) => {
      if (cmd === "ipc_get_local_info") return Promise.resolve(mockLocalInfo);
      if (cmd === "ipc_get_batch_info") return Promise.resolve(mockBatchInfo);
      return Promise.resolve(undefined);
    });
    render(
      <JoinApprovalDialog
        batchId="batch-123"
        token="jwt"
        approverUserId={1}
        onClose={vi.fn()}
      />
    );

    await waitFor(() => {
      expect(screen.getByText("网络合并")).toBeTruthy();
    });

    // Initiator
    expect(screen.getByText(/用户 #3/)).toBeTruthy();

    // Target members
    expect(screen.getByText(/MacBook Pro/)).toBeTruthy();
    expect(screen.getByText(/MacBook Air/)).toBeTruthy();

    // Approval progress
    expect(screen.getByText("0 / 2")).toBeTruthy();
  });

  it("displays new_device flow type for D2", async () => {
    mockIpcInvoke.mockImplementation((cmd: string) => {
      if (cmd === "ipc_get_local_info") return Promise.resolve(mockLocalInfo);
      if (cmd === "ipc_get_batch_info") return Promise.resolve({ ...mockBatchInfo, flow_type: "new_device" });
      return Promise.resolve(undefined);
    });
    render(
      <JoinApprovalDialog
        batchId="batch-123"
        token="jwt"
        approverUserId={1}
        onClose={vi.fn()}
      />
    );

    await waitFor(() => {
      expect(screen.getByText("新设备加入")).toBeTruthy();
    });
  });

  it("shows error message when batch info fetch fails", async () => {
    mockIpcInvoke.mockImplementation((cmd: string) => {
      if (cmd === "ipc_get_local_info") return Promise.resolve(mockLocalInfo);
      if (cmd === "ipc_get_batch_info") return Promise.reject(new Error("Network error"));
      return Promise.resolve(undefined);
    });
    render(
      <JoinApprovalDialog
        batchId="batch-123"
        token="jwt"
        approverUserId={1}
        onClose={vi.fn()}
      />
    );

    await waitFor(() => {
      expect(screen.getByText("Network error")).toBeTruthy();
    });
  });

  it("calls approveJoin when approve button clicked", async () => {
    // Call sequence: getDesktopDeviceId (local_info) → getBatchInfo → approve: getDesktopDeviceId → sign → approve_join
    mockIpcInvoke.mockImplementation((cmd: string) => {
      if (cmd === "ipc_get_local_info") return Promise.resolve(mockLocalInfo);
      if (cmd === "ipc_get_batch_info") return Promise.resolve(mockBatchInfo);
      if (cmd === "ipc_sign_device_payload") return Promise.resolve("signature-base64");
      if (cmd === "ipc_approve_join") return Promise.resolve({
        batch_id: "batch-123",
        received_approvals: 1,
        required_approvals: 2,
        status: "pending_approval",
      });
      return Promise.resolve(undefined);
    });

    const onApproved = vi.fn();
    render(
      <JoinApprovalDialog
        batchId="batch-123"
        token="jwt"
        approverUserId={1}
        onClose={vi.fn()}
        onApproved={onApproved}
      />
    );

    // Wait for batch info to load
    await waitFor(() => {
      expect(screen.getByText("网络合并")).toBeTruthy();
    });

    // Click approve
    fireEvent.click(screen.getByText("批准"));

    // Wait for success
    await waitFor(() => {
      expect(screen.getByText("已批准。等待其他设备签名...")).toBeTruthy();
    });

    expect(onApproved).toHaveBeenCalledWith("batch-123");
  });

  it("shows error when approval fails", async () => {
    mockIpcInvoke.mockImplementation((cmd: string) => {
      if (cmd === "ipc_get_local_info") return Promise.resolve(mockLocalInfo);
      if (cmd === "ipc_get_batch_info") return Promise.resolve(mockBatchInfo);
      if (cmd === "ipc_sign_device_payload") return Promise.reject(new Error("Signature verification failed"));
      return Promise.resolve(undefined);
    });

    render(
      <JoinApprovalDialog
        batchId="batch-123"
        token="jwt"
        approverUserId={1}
        onClose={vi.fn()}
      />
    );

    await waitFor(() => {
      expect(screen.getByText("网络合并")).toBeTruthy();
    });

    fireEvent.click(screen.getByText("批准"));

    await waitFor(() => {
      expect(screen.getByText("Signature verification failed")).toBeTruthy();
    });
  });

  it("calls onClose when reject button clicked and does not call any approval API", async () => {
    mockIpcInvoke.mockImplementation((cmd: string) => {
      if (cmd === "ipc_get_local_info") return Promise.resolve(mockLocalInfo);
      if (cmd === "ipc_get_batch_info") return Promise.resolve(mockBatchInfo);
      return Promise.resolve(undefined);
    });
    const onClose = vi.fn();
    render(
      <JoinApprovalDialog
        batchId="batch-123"
        token="jwt"
        approverUserId={1}
        onClose={onClose}
      />
    );

    await waitFor(() => {
      expect(screen.getByText("网络合并")).toBeTruthy();
    });

    // Clear mock calls before reject (only batch-info + local-info should have been called)
    mockIpcInvoke.mockClear();

    fireEvent.click(screen.getByText("拒绝"));
    expect(onClose).toHaveBeenCalled();

    // Verify no approval-related IPC was called
    const calls = mockIpcInvoke.mock.calls.map((c) => c[0]);
    expect(calls).not.toContain("ipc_approve_join");
    expect(calls).not.toContain("ipc_sign_device_payload");
  });

  it("switches footer to close button after successful approval", async () => {
    mockIpcInvoke.mockImplementation((cmd: string) => {
      if (cmd === "ipc_get_local_info") return Promise.resolve(mockLocalInfo);
      if (cmd === "ipc_get_batch_info") return Promise.resolve(mockBatchInfo);
      if (cmd === "ipc_sign_device_payload") return Promise.resolve("signature-base64");
      if (cmd === "ipc_approve_join") return Promise.resolve({
        batch_id: "batch-123",
        received_approvals: 1,
        required_approvals: 2,
        status: "pending_approval",
      });
      return Promise.resolve(undefined);
    });

    render(
      <JoinApprovalDialog
        batchId="batch-123"
        token="jwt"
        approverUserId={1}
        onClose={vi.fn()}
      />
    );

    await waitFor(() => {
      expect(screen.getByText("网络合并")).toBeTruthy();
    });

    // Click approve
    fireEvent.click(screen.getByText("批准"));

    // Wait for success message
    await waitFor(() => {
      expect(screen.getByText("已批准。等待其他设备签名...")).toBeTruthy();
    });

    // Footer should now show "关闭" button, not "拒绝"/"批准"
    expect(screen.getByText("关闭")).toBeTruthy();
    expect(screen.queryByText("拒绝")).toBeNull();
    expect(screen.queryByText("批准")).toBeNull();
  });
});
