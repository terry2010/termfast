// remoteDesktopStore tests — FP-7
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useRemoteDesktopStore } from "@/stores/remoteDesktopStore";
import { invoke } from "@tauri-apps/api/core";

const mockInvoke = vi.mocked(invoke);

describe("remoteDesktopStore", () => {
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

  it("starts with empty peers", () => {
    expect(useRemoteDesktopStore.getState().peers).toEqual([]);
    expect(useRemoteDesktopStore.getState().activeConnection).toBeNull();
  });

  it("loadPeers maps backend pairing fields to RemotePeer", async () => {
    mockInvoke.mockResolvedValueOnce({
      pairings: [
        {
          pairing_id: "dpair-1",
          pairing_key_hex: "aabb".repeat(32),
          relay_url: "wss://relay.example.com/tunnel",
          jwt: "jwt-token-1",
          peer_name: "WinPC",
          peer_role: "client",
        },
        {
          pairing_id: "dpair-2",
          pairing_key_hex: "ccdd".repeat(32),
          relay_url: "wss://relay.example.com/tunnel",
          jwt: "jwt-token-2",
          peer_name: "LinuxBox",
          peer_role: "server",
        },
      ],
    });

    await useRemoteDesktopStore.getState().loadPeers();

    const state = useRemoteDesktopStore.getState();
    expect(state.peers).toHaveLength(2);
    expect(state.peers[0]).toEqual({
      pairingId: "dpair-1",
      pairingKeyHex: "aabb".repeat(32),
      relayUrl: "wss://relay.example.com/tunnel",
      jwt: "jwt-token-1",
      peerName: "WinPC",
      peerRole: "client",
      online: false,
    });
    expect(state.peers[1].peerRole).toBe("server");
    expect(state.loading).toBe(false);
    expect(state.error).toBeNull();
  });

  it("loadPeers handles empty pairings", async () => {
    mockInvoke.mockResolvedValueOnce({ pairings: [] });
    await useRemoteDesktopStore.getState().loadPeers();
    expect(useRemoteDesktopStore.getState().peers).toEqual([]);
  });

  it("loadPeers handles missing pairings field", async () => {
    mockInvoke.mockResolvedValueOnce({});
    await useRemoteDesktopStore.getState().loadPeers();
    expect(useRemoteDesktopStore.getState().peers).toEqual([]);
  });

  it("loadPeers sets error on IPC failure", async () => {
    mockInvoke.mockRejectedValueOnce(new Error("IPC error"));
    await useRemoteDesktopStore.getState().loadPeers();
    const state = useRemoteDesktopStore.getState();
    expect(state.loading).toBe(false);
    expect(state.error).toBeTruthy();
    expect(state.peers).toEqual([]);
  });

  it("setPeerOnline updates only the matching peer", () => {
    useRemoteDesktopStore.setState({
      peers: [
        {
          pairingId: "dpair-1",
          pairingKeyHex: "aa",
          relayUrl: "wss://r",
          jwt: "j1",
          peerName: "A",
          peerRole: "client",
          online: false,
        },
        {
          pairingId: "dpair-2",
          pairingKeyHex: "bb",
          relayUrl: "wss://r",
          jwt: "j2",
          peerName: "B",
          peerRole: "server",
          online: false,
        },
      ],
    });

    useRemoteDesktopStore.getState().setPeerOnline("dpair-2", true);

    const peers = useRemoteDesktopStore.getState().peers;
    expect(peers[0].online).toBe(false);
    expect(peers[1].online).toBe(true);
  });

  it("setActiveConnection sets and clears connection", () => {
    useRemoteDesktopStore.getState().setActiveConnection("dpair-1");
    expect(useRemoteDesktopStore.getState().activeConnection).toBe("dpair-1");
    expect(useRemoteDesktopStore.getState().remoteTerminals).toEqual([]);

    useRemoteDesktopStore.getState().setActiveConnection(null);
    expect(useRemoteDesktopStore.getState().activeConnection).toBeNull();
  });

  it("setRemoteTerminals updates the list", () => {
    const terminals = [
      { terminalId: 1, name: "bash" },
      { terminalId: 2, name: "vim" },
    ];
    useRemoteDesktopStore.getState().setRemoteTerminals(terminals);
    expect(useRemoteDesktopStore.getState().remoteTerminals).toEqual(terminals);
  });
});
