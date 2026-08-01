// PortForwardPanel component tests
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { PortForwardPanel } from "@/components/shared/PortForwardPanel";

// Mock IPC
const mockIpcInvoke = vi.fn();
vi.mock("@/hooks/useIpc", () => ({
  ipcInvoke: (...args: any[]) => mockIpcInvoke(...args),
  formatIpcError: (e: unknown) => String(e),
}));

// Mock toast
vi.mock("@/components/ui/toast", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
  },
}));

beforeEach(() => {
  mockIpcInvoke.mockReset();
});

describe("PortForwardPanel", () => {
  it("renders empty state when no rules", async () => {
    mockIpcInvoke.mockResolvedValue({ rules: [] });
    render(<PortForwardPanel serverId="srv1" />);
    await waitFor(() => {
      // i18n not initialized in tests — t() returns the key
      expect(screen.getByText("port_forward.empty")).toBeInTheDocument();
    });
  });

  it("renders rules list", async () => {
    mockIpcInvoke.mockResolvedValue({
      rules: [
        {
          id: "pf_1",
          name: "MySQL Tunnel",
          type: "local",
          local_host: "127.0.0.1",
          local_port: 13306,
          remote_host: "127.0.0.1",
          remote_port: 3306,
          enabled: true,
          auto_start: false,
          running: false,
          active_connections: 0,
          bytes_in: 0,
          bytes_out: 0,
        },
      ],
    });
    render(<PortForwardPanel serverId="srv1" />);
    await waitFor(() => {
      expect(screen.getByText("MySQL Tunnel")).toBeInTheDocument();
      expect(screen.getByText(/127.0.0.1:13306/)).toBeInTheDocument();
    });
  });

  it("shows running indicator for running rules", async () => {
    mockIpcInvoke.mockResolvedValue({
      rules: [
        {
          id: "pf_2",
          name: "Web Forward",
          type: "remote",
          local_host: "0.0.0.0",
          local_port: 8080,
          remote_host: "127.0.0.1",
          remote_port: 80,
          enabled: true,
          auto_start: true,
          running: true,
          active_connections: 3,
          bytes_in: 1024,
          bytes_out: 2048,
          error: null,
        },
      ],
    });
    const { container } = render(<PortForwardPanel serverId="srv1" />);
    await waitFor(() => {
      expect(screen.getByText("Web Forward")).toBeInTheDocument();
      // Verify the full text content includes connections and traffic
      const text = container.textContent || "";
      expect(text).toMatch(/3/); // 3 connections
      expect(text).toMatch(/1\.0K/); // bytes_in formatted
      expect(text).toMatch(/2\.0K/); // bytes_out formatted
    });
  });

  it("shows error state when rule has error", async () => {
    mockIpcInvoke.mockResolvedValue({
      rules: [
        {
          id: "pf_err",
          name: "Failed Forward",
          type: "local",
          local_host: "127.0.0.1",
          local_port: 9999,
          remote_host: "127.0.0.1",
          remote_port: 80,
          enabled: true,
          auto_start: false,
          running: false,
          active_connections: 0,
          bytes_in: 0,
          bytes_out: 0,
          error: "Connection refused",
        },
      ],
    });
    const { container } = render(<PortForwardPanel serverId="srv1" />);
    await waitFor(() => {
      expect(screen.getByText("Failed Forward")).toBeInTheDocument();
      // Error label should be rendered (t("port_forward.error") returns the key in tests)
      const text = container.textContent || "";
      expect(text).toContain("port_forward.error");
    });
  });

  it("calls ipc_start_port_forward when start clicked", async () => {
    mockIpcInvoke.mockResolvedValue({
      rules: [
        {
          id: "pf_3",
          name: "Test",
          type: "local",
          local_host: "127.0.0.1",
          local_port: 1080,
          remote_host: "127.0.0.1",
          remote_port: 80,
          enabled: true,
          auto_start: false,
          running: false,
          active_connections: 0,
          bytes_in: 0,
          bytes_out: 0,
        },
      ],
    });
    render(<PortForwardPanel serverId="srv1" />);
    await waitFor(() => {
      expect(screen.getByText("Test")).toBeInTheDocument();
    });

    // Click start button — t("common.start") returns "common.start" in tests
    const startBtn = screen.getByText("common.start");
    fireEvent.click(startBtn);

    await waitFor(() => {
      expect(mockIpcInvoke).toHaveBeenCalledWith("ipc_start_port_forward", {
        server_id: "srv1",
        rule_id: "pf_3",
      });
    });
  });

  it("shows bulk action buttons when rules have running or startable rules", async () => {
    mockIpcInvoke.mockResolvedValue({
      rules: [
        { id: "pf_1", name: "Test", type: "local", running: true, enabled: true },
      ],
    });
    render(<PortForwardPanel serverId="srv1" />);
    await waitFor(() => {
      expect(screen.getByText("port_forward.stop_all")).toBeInTheDocument();
    });
  });
});
