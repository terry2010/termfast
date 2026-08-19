// Remote desktop store — manages desktop-to-desktop pairing state (FP-7)
// Peers loaded from ipc_list_desktop_pairings, connection state from events.

import { create } from "zustand";
import { ipcInvoke } from "@/hooks/useIpc";

export interface RemotePeer {
  pairingId: string;
  pairingKeyHex: string;
  relayUrl: string;
  jwt: string;
  peerName: string;
  peerRole: string; // "client" or "server"
  online: boolean;
}

export interface RemoteTerminal {
  terminalId: number;
  name: string;
}

interface RemoteDesktopStore {
  peers: RemotePeer[];
  loading: boolean;
  error: string | null;
  // Currently connected pairing_id
  activeConnection: string | null;
  // Remote terminals for the active connection
  remoteTerminals: RemoteTerminal[];
  // Load desktop pairings from the local store
  loadPeers: () => Promise<void>;
  // Set online status for a peer (from remote_client_state event)
  setPeerOnline: (pairingId: string, online: boolean) => void;
  // Set active connection
  setActiveConnection: (pairingId: string | null) => void;
  // Set remote terminals
  setRemoteTerminals: (terminals: RemoteTerminal[]) => void;
}

export const useRemoteDesktopStore = create<RemoteDesktopStore>((set) => ({
  peers: [],
  loading: false,
  error: null,
  activeConnection: null,
  remoteTerminals: [],

  loadPeers: async () => {
    set({ loading: true, error: null });
    try {
      const data = await ipcInvoke<{ pairings: RemotePeer[] }>(
        "ipc_list_desktop_pairings"
      );
      const pairings = (data.pairings || []).map((p: any) => ({
        pairingId: p.pairing_id,
        pairingKeyHex: p.pairing_key_hex,
        relayUrl: p.relay_url,
        jwt: p.jwt,
        peerName: p.peer_name,
        peerRole: p.peer_role,
        online: !!p.is_online,
      }));
      set({ peers: pairings, loading: false });
    } catch (e: any) {
      set({ error: String(e?.message || e), loading: false });
    }
  },

  setPeerOnline: (pairingId, online) =>
    set((state) => ({
      peers: state.peers.map((p) =>
        p.pairingId === pairingId ? { ...p, online } : p
      ),
    })),

  setActiveConnection: (pairingId) =>
    set({ activeConnection: pairingId, remoteTerminals: [] }),

  setRemoteTerminals: (terminals) => set({ remoteTerminals: terminals }),
}));
