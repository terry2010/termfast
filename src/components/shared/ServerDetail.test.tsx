// ServerDetail component tests
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { ServerDetail } from "@/components/shared/ServerDetail";
import { useServerStore } from "@/stores/serverStore";
import type { ServerState } from "@/stores/serverStore";

vi.mock("@/hooks/useIpc", () => ({
  ipcInvoke: vi.fn().mockResolvedValue(null),
}));

function mockServer(id: string, name: string): ServerState {
  return {
    id, name,
    ssh: { host: "1.2.3.4", port: 22, user: "root", auth_method: "password", key_path: "", key_auto_generated: false, connection_mode: "single", skip_hostkey_verify: false },
    proxy: { enabled: false, socks5_port: 1080, http_port: 8080, mixed_port: 0, max_channels: 64, channel_idle_timeout: 300 },
    reconnect: { heartbeat_interval: 15, max_attempts: 10, initial_backoff_secs: 1, max_backoff_secs: 300 },
    ip_check: { enabled: false, interval_secs: 300 },
    last_known_ip: null, triggers: [], suppress_firewall_badge: false,
    current_status: "disconnected", current_ip: null,
    client_ip: null, connected_since: null,
    reconnect_count: 0, max_attempts: 10, proxy_running: false, active_channels: 0, bytes_in: 0, bytes_out: 0,
  };
}

beforeEach(() => {
  useServerStore.setState({ servers: [], selected_server_id: null });
});

describe("ServerDetail", () => {
  it("renders without crashing when no server selected", () => {
    const { container } = render(<ServerDetail />);
    expect(container).toBeTruthy();
  });

  it("renders server name when server selected", () => {
    useServerStore.setState({
      servers: [mockServer("s1", "Test VPS")],
      selected_server_id: "s1",
    });
    render(<ServerDetail />);
    expect(screen.getByText("Test VPS")).toBeInTheDocument();
  });
});
