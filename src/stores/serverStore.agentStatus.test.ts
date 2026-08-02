// Unit tests for setTerminalTabAgentStatus — serverStore agent status updates
import { describe, it, expect, beforeEach } from "vitest";
import { useServerStore } from "@/stores/serverStore";
import type { ServerState } from "@/stores/serverStore";

function mockServer(id: string): ServerState {
  return {
    id,
    name: id,
    ssh: {
      host: "1.2.3.4", port: 22, user: "root", auth_method: "password",
      key_path: "", key_auto_generated: false, connection_mode: "single",
      skip_hostkey_verify: false,
    },
    proxy: {
      enabled: false, socks5_port: 1080, http_port: 8080, mixed_port: 0,
      max_channels: 100, channel_idle_timeout: 300,
    },
    reconnect: {
      auto_reconnect: true, heartbeat_interval: 15, max_attempts: 10,
      reconnect_timeout_secs: 86400, initial_backoff_secs: 1, max_backoff_secs: 300,
    },
    ip_check: { enabled: true, interval_secs: 300 },
    last_known_ip: null,
    triggers: [],
    suppress_firewall_badge: false,
    current_status: "disconnected",
    current_ip: null, client_ip: null, connected_since: null,
    reconnect_count: 0, max_attempts: 10,
    proxy_running: false, active_channels: 0, bytes_in: 0, bytes_out: 0,
    auth_banner: null, rz_available: false,
  };
}

function mockTab(id: string, sessionId: string) {
  return {
    id, sessionId, label: `Terminal ${id}`, defaultLabel: `Terminal ${id}`,
    initialOutput: "", disconnected: false, agentStatus: null as AgentStatus | null,
  };
}

import type { AgentStatus } from "@/hooks/agentStateMachine";

describe("setTerminalTabAgentStatus", () => {
  beforeEach(() => {
    useServerStore.setState({
      servers: [],
      selected_server_id: null,
      loading: false,
      terminal_tabs_by_server: {},
      active_terminal_tab_by_server: {},
    });
  });

  it("updates tab agentStatus to working", () => {
    const server = mockServer("srv_1");
    useServerStore.getState().addServer(server);
    useServerStore.getState().addTerminalTab("srv_1", mockTab("term:s1", "s1"));

    useServerStore.getState().setTerminalTabAgentStatus("srv_1", "term:s1", "working");

    const tabs = useServerStore.getState().terminal_tabs_by_server["srv_1"];
    expect(tabs[0].agentStatus).toBe("working");
  });

  it("updates tab agentStatus to blocked", () => {
    const server = mockServer("srv_1");
    useServerStore.getState().addServer(server);
    useServerStore.getState().addTerminalTab("srv_1", mockTab("term:s1", "s1"));

    useServerStore.getState().setTerminalTabAgentStatus("srv_1", "term:s1", "blocked");

    const tabs = useServerStore.getState().terminal_tabs_by_server["srv_1"];
    expect(tabs[0].agentStatus).toBe("blocked");
  });

  it("skips update when status is unchanged (returns same state reference)", () => {
    const server = mockServer("srv_1");
    useServerStore.getState().addServer(server);
    useServerStore.getState().addTerminalTab("srv_1", mockTab("term:s1", "s1"));

    // Set to working first
    useServerStore.getState().setTerminalTabAgentStatus("srv_1", "term:s1", "working");
    const stateBefore = useServerStore.getState();

    // Set to working again — should be a no-op
    useServerStore.getState().setTerminalTabAgentStatus("srv_1", "term:s1", "working");
    const stateAfter = useServerStore.getState();

    // The terminal_tabs_by_server reference should be unchanged (same state)
    expect(stateAfter.terminal_tabs_by_server).toBe(stateBefore.terminal_tabs_by_server);
  });

  it("does nothing when serverId has no tabs", () => {
    const server = mockServer("srv_1");
    useServerStore.getState().addServer(server);

    // No tabs added for srv_1 — should not throw
    useServerStore.getState().setTerminalTabAgentStatus("srv_1", "term:s1", "working");
    expect(useServerStore.getState().terminal_tabs_by_server["srv_1"]).toBeUndefined();
  });

  it("does nothing when tabId not found", () => {
    const server = mockServer("srv_1");
    useServerStore.getState().addServer(server);
    useServerStore.getState().addTerminalTab("srv_1", mockTab("term:s1", "s1"));

    // Try to update a non-existent tab
    useServerStore.getState().setTerminalTabAgentStatus("srv_1", "term:nonexistent", "working");

    const tabs = useServerStore.getState().terminal_tabs_by_server["srv_1"];
    expect(tabs[0].agentStatus).toBeNull(); // unchanged
  });
});
