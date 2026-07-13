// ServerList component tests — FP-8.2
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { ServerList } from "@/components/shared/ServerList";
import { useServerStore } from "@/stores/serverStore";
import type { ServerState } from "@/stores/serverStore";

vi.mock("@/hooks/useIpc", () => ({
  ipcInvoke: vi.fn().mockImplementation(() => {
    const servers = useServerStore.getState().servers;
    return Promise.resolve({ servers });
  }),
}));

function mockServer(id: string, name: string, status: ServerState["current_status"] = "disconnected"): ServerState {
  return {
    id, name,
    ssh: { host: "1.2.3.4", port: 22, user: "root", auth_method: "password", key_path: "", key_auto_generated: false, connection_mode: "single", skip_hostkey_verify: false },
    proxy: { enabled: false, socks5_port: 1080, http_port: 8080, mixed_port: 0, max_channels: 64, channel_idle_timeout: 300 },
    reconnect: { heartbeat_interval: 15, max_attempts: 10, initial_backoff_secs: 1, max_backoff_secs: 300 },
    ip_check: { enabled: false, interval_secs: 300 },
    last_known_ip: null, triggers: [], suppress_firewall_badge: false,
    current_status: status, current_ip: null,
    client_ip: null, connected_since: null,
    reconnect_count: 0, max_attempts: 10, proxy_running: false, active_channels: 0, bytes_in: 0, bytes_out: 0,
  };
}

beforeEach(() => {
  useServerStore.setState({ servers: [], selected_server_id: null });
});

describe("ServerList", () => {
  it("renders without crashing when no servers", () => {
    const { container } = render(<ServerList />);
    expect(container).toBeTruthy();
  });

  it("renders server names when servers exist", async () => {
    useServerStore.setState({
      servers: [mockServer("s1", "Tokyo VPS"), mockServer("s2", "US West")],
      selected_server_id: null,
    });
    render(<ServerList />);
    await waitFor(() => expect(screen.getByText("Tokyo VPS")).toBeInTheDocument());
    expect(screen.getByText("US West")).toBeInTheDocument();
  });

  it("shows port chip with socks5 port", async () => {
    useServerStore.setState({
      servers: [mockServer("s1", "Test VPS")],
      selected_server_id: null,
    });
    render(<ServerList />);
    await waitFor(() => expect(screen.getByText(":1080")).toBeInTheDocument());
    expect(screen.getByText(":1080")).toBeInTheDocument();
  });

  it("calls selectServer when clicking a server", async () => {
    useServerStore.setState({
      servers: [mockServer("s1", "Click Me")],
      selected_server_id: null,
    });
    render(<ServerList />);
    await waitFor(() => expect(screen.getByText("Click Me")).toBeInTheDocument());
    fireEvent.click(screen.getByText("Click Me"));
    expect(useServerStore.getState().selected_server_id).toBe("s1");
  });

  it("pins abnormal servers to top", async () => {
    useServerStore.setState({
      servers: [
        mockServer("s1", "Normal", "connected"),
        mockServer("s2", "Abnormal", "auth_failed"),
      ],
      selected_server_id: null,
    });
    render(<ServerList />);
    await waitFor(() => expect(screen.getByText("Abnormal")).toBeInTheDocument());
    const items = screen.getAllByRole("listitem");
    expect(items[0]).toHaveTextContent("Abnormal");
  });
});
