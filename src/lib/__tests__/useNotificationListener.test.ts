// Unit tests for useNotificationListener — D8 WebSocket notification listener
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// Mock ipcInvoke before importing
vi.mock("@/hooks/useIpc", () => ({
  ipcInvoke: vi.fn(),
}));

// Mock WebSocket
class MockWebSocket {
  static instances: MockWebSocket[] = [];
  static LAST: MockWebSocket | null = null;

  url: string;
  readyState: number = 0;
  onopen: ((ev: Event) => void) | null = null;
  onmessage: ((ev: MessageEvent) => void) | null = null;
  onerror: ((ev: Event) => void) | null = null;
  onclose: ((ev: CloseEvent) => void) | null = null;

  constructor(url: string) {
    this.url = url;
    MockWebSocket.instances.push(this);
    MockWebSocket.LAST = this;
  }

  close() {
    this.readyState = 3;
    if (this.onclose) {
      this.onclose(new CloseEvent("close"));
    }
  }

  send(data: string) {
    // no-op
  }

  // Test helpers
  simulateOpen() {
    this.readyState = 1;
    this.onopen?.(new Event("open"));
  }

  simulateMessage(data: unknown) {
    this.onmessage?.({ data: JSON.stringify(data) } as MessageEvent);
  }

  simulateError() {
    this.onerror?.(new Event("error"));
  }

  simulateClose() {
    this.readyState = 3;
    this.onclose?.(new CloseEvent("close"));
  }
}

// Replace global WebSocket
(globalThis as unknown as { WebSocket: typeof WebSocket }).WebSocket = MockWebSocket as unknown as typeof WebSocket;

import { ipcInvoke } from "@/hooks/useIpc";
import { renderHook, act, waitFor } from "@testing-library/react";
import { useNotificationListener } from "../useNotificationListener";

const mockIpcInvoke = ipcInvoke as unknown as ReturnType<typeof vi.fn>;

beforeEach(() => {
  mockIpcInvoke.mockReset();
  MockWebSocket.instances = [];
  MockWebSocket.LAST = null;
});

afterEach(() => {
  vi.clearAllTimers();
});

describe("useNotificationListener", () => {
  it("does not connect when token is null", async () => {
    mockIpcInvoke.mockResolvedValue({ hostname: "mac", username: "terry", device_suffix: "a3f7" });

    const { result } = renderHook(() =>
      useNotificationListener({ token: null, backendUrl: "http://localhost:8080" })
    );

    expect(result.current.connected).toBe(false);
    expect(MockWebSocket.instances).toHaveLength(0);
  });

  it("connects to WebSocket with token and device_id", async () => {
    mockIpcInvoke.mockResolvedValue({ hostname: "mac", username: "terry", device_suffix: "a3f7" });

    renderHook(() =>
      useNotificationListener({ token: "jwt-token", backendUrl: "http://localhost:8080" })
    );

    await waitFor(() => {
      expect(MockWebSocket.instances).toHaveLength(1);
    });

    const ws = MockWebSocket.LAST!;
    expect(ws.url).toContain("ws://localhost:8080/notifications");
    expect(ws.url).toContain("token=jwt-token");
    expect(ws.url).toContain("device_id=mac-terry-a3f7");
  });

  it("sets connected=true on open", async () => {
    mockIpcInvoke.mockResolvedValue({ hostname: "mac", username: "terry", device_suffix: "a3f7" });

    const { result } = renderHook(() =>
      useNotificationListener({ token: "jwt", backendUrl: "http://localhost:8080" })
    );

    await waitFor(() => expect(MockWebSocket.LAST).toBeTruthy());

    act(() => {
      MockWebSocket.LAST!.simulateOpen();
    });

    expect(result.current.connected).toBe(true);
    expect(result.current.error).toBeNull();
  });

  it("calls onJoinBatchPending when join_batch_pending message received", async () => {
    mockIpcInvoke.mockResolvedValue({ hostname: "mac", username: "terry", device_suffix: "a3f7" });

    const onJoinBatchPending = vi.fn();

    renderHook(() =>
      useNotificationListener({ token: "jwt", backendUrl: "http://localhost:8080", onJoinBatchPending })
    );

    await waitFor(() => expect(MockWebSocket.LAST).toBeTruthy());

    act(() => {
      MockWebSocket.LAST!.simulateOpen();
      MockWebSocket.LAST!.simulateMessage({ type: "join_batch_pending", batch_id: "batch-123" });
    });

    expect(onJoinBatchPending).toHaveBeenCalledWith("batch-123");
  });

  it("ignores non join_batch_pending messages", async () => {
    mockIpcInvoke.mockResolvedValue({ hostname: "mac", username: "terry", device_suffix: "a3f7" });

    const onJoinBatchPending = vi.fn();

    renderHook(() =>
      useNotificationListener({ token: "jwt", backendUrl: "http://localhost:8080", onJoinBatchPending })
    );

    await waitFor(() => expect(MockWebSocket.LAST).toBeTruthy());

    act(() => {
      MockWebSocket.LAST!.simulateOpen();
      MockWebSocket.LAST!.simulateMessage({ type: "connected" });
      MockWebSocket.LAST!.simulateMessage({ type: "other_event", data: "test" });
    });

    expect(onJoinBatchPending).not.toHaveBeenCalled();
  });

  it("sets error on WebSocket error", async () => {
    mockIpcInvoke.mockResolvedValue({ hostname: "mac", username: "terry", device_suffix: "a3f7" });

    const { result } = renderHook(() =>
      useNotificationListener({ token: "jwt", backendUrl: "http://localhost:8080" })
    );

    await waitFor(() => expect(MockWebSocket.LAST).toBeTruthy());

    act(() => {
      MockWebSocket.LAST!.simulateError();
    });

    expect(result.current.error).toBe("WebSocket error");
  });

  it("reconnects with exponential backoff after close", async () => {
    mockIpcInvoke.mockResolvedValue({ hostname: "mac", username: "terry", device_suffix: "a3f7" });

    // Use fake timers only for this test
    vi.useFakeTimers();

    try {
      renderHook(() =>
        useNotificationListener({ token: "jwt", backendUrl: "http://localhost:8080" })
      );

      // Wait for initial connection (flush microtasks + pending promises)
      await act(async () => {
        await vi.runAllTimersAsync();
      });
      expect(MockWebSocket.instances).toHaveLength(1);

      // Simulate close
      act(() => {
        MockWebSocket.LAST!.simulateClose();
      });

      // No new connection immediately (backoff = 1s)
      expect(MockWebSocket.instances).toHaveLength(1);

      // Advance 1 second → first reconnect attempt
      await act(async () => {
        vi.advanceTimersByTime(1000);
        await vi.runAllTimersAsync();
      });
      expect(MockWebSocket.instances).toHaveLength(2);

      // Simulate second close
      act(() => {
        MockWebSocket.LAST!.simulateClose();
      });

      // Advance 2 seconds → second reconnect (backoff = 2s)
      await act(async () => {
        vi.advanceTimersByTime(2000);
        await vi.runAllTimersAsync();
      });
      expect(MockWebSocket.instances).toHaveLength(3);
    } finally {
      vi.useRealTimers();
    }
  });

  it("stops reconnecting after unmount", async () => {
    mockIpcInvoke.mockResolvedValue({ hostname: "mac", username: "terry", device_suffix: "a3f7" });

    vi.useFakeTimers();

    try {
      const { unmount } = renderHook(() =>
        useNotificationListener({ token: "jwt", backendUrl: "http://localhost:8080" })
      );

      await act(async () => {
        await vi.runAllTimersAsync();
      });
      expect(MockWebSocket.instances).toHaveLength(1);

      // Unmount
      unmount();

      // Simulate close (should not trigger reconnect since unmounted)
      const wsBeforeUnmount = MockWebSocket.LAST;
      act(() => {
        wsBeforeUnmount?.simulateClose();
      });

      // Advance time — no new connections should be made
      await act(async () => {
        vi.advanceTimersByTime(5000);
        await vi.runAllTimersAsync();
      });

      expect(MockWebSocket.instances).toHaveLength(1);
    } finally {
      vi.useRealTimers();
    }
  });
});
