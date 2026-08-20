// RemoteTerminalView — xterm.js wrapper for remote desktop terminal (FP-7)
// Receives output via remote_client_frame events, sends input via IPC.

import { useEffect, useRef, useCallback } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { ipcInvoke } from "@/hooks/useIpc";
import { getTerminalTheme } from "@/lib/terminalThemes";
import { useConfigStore } from "@/stores/configStore";
import "@xterm/xterm/css/xterm.css";

interface RemoteTerminalViewProps {
  pairingId: string;
  terminalId: number;
  terminalName: string;
}

interface RemoteClientFrameEvent {
  pairing_id: string;
  frame_type: number;
  terminal_id: number;
  data: string; // base64 encoded
}

// Frame type constants (must match remote_frame.rs)
const OUTPUT = 0x05;
const HISTORY = 0x0A;

function decodeBase64(b64: string): Uint8Array {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

export function RemoteTerminalView({
  pairingId,
  terminalId,
  terminalName,
}: RemoteTerminalViewProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const config = useConfigStore((s) => s.config);

  // Initialize xterm.js
  useEffect(() => {
    if (!containerRef.current) return;

    const themePreset = getTerminalTheme(config?.general.terminal_theme || "catppuccin-mocha");
    const term = new Terminal({
      cols: 80,
      rows: 24,
      fontFamily: config?.general.terminal_font_family || "Menlo, Monaco, 'Courier New', monospace",
      fontSize: config?.general.terminal_font_size || 14,
      theme: themePreset.theme,
      cursorBlink: true,
      allowProposedApi: true,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon());
    term.open(containerRef.current);
    fit.fit();

    termRef.current = term;
    fitRef.current = fit;

    // Subscribe to the terminal on mount (ensures output is sent to us)
    ipcInvoke("ipc_remote_client_subscribe", {
      pairing_id: pairingId,
      terminal_id: terminalId,
    }).catch(() => {});

    // Send resize to remote
    const sendResize = (cols: number, rows: number) => {
      ipcInvoke("ipc_remote_client_send_resize", {
        pairing_id: pairingId,
        terminal_id: terminalId,
        cols,
        rows,
      }).catch(() => {});
    };
    sendResize(term.cols, term.rows);

    // Handle user input → send to remote
    const inputDisposable = term.onData((data) => {
      const encoder = new TextEncoder();
      const bytes = Array.from(encoder.encode(data));
      ipcInvoke("ipc_remote_client_send_input", {
        pairing_id: pairingId,
        terminal_id: terminalId,
        data: bytes,
      }).catch(() => {});
    });

    // Handle resize
    const resizeDisposable = term.onResize(({ cols, rows }) => {
      sendResize(cols, rows);
    });

    // Fit on window resize
    const handleResize = () => {
      if (fitRef.current) {
        fitRef.current.fit();
      }
    };
    window.addEventListener("resize", handleResize);

    return () => {
      // Unsubscribe on cleanup — without this, React StrictMode's
      // mount-unmount-mount cycle leaves two subscribers active,
      // causing duplicated output and doubled input characters.
      ipcInvoke("ipc_remote_client_unsubscribe", {
        pairing_id: pairingId,
        terminal_id: terminalId,
      }).catch(() => {});
      inputDisposable.dispose();
      resizeDisposable.dispose();
      window.removeEventListener("resize", handleResize);
      term.dispose();
      termRef.current = null;
    };
  }, [pairingId, terminalId, config]);

  // Listen for remote_client_frame events
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;

    listen<RemoteClientFrameEvent>("remote_client_frame", (event) => {
      const payload = event.payload;
      if (payload.pairing_id !== pairingId || payload.terminal_id !== terminalId) {
        return;
      }
      const term = termRef.current;
      if (!term) return;

      const bytes = decodeBase64(payload.data);
      if (payload.frame_type === OUTPUT) {
        term.write(bytes);
      } else if (payload.frame_type === HISTORY) {
        // HISTORY payload = [seq:4][is_last:1][data] — skip the 5-byte header
        if (bytes.length > 5) {
          term.write(bytes.subarray(5));
        }
      }
    }).then((unlistenFn) => {
      unlisten = unlistenFn;
    });

    return () => {
      if (unlisten) unlisten();
    };
  }, [pairingId, terminalId]);

  // Unsubscribe on unmount
  useEffect(() => {
    return () => {
      ipcInvoke("ipc_remote_client_unsubscribe", {
        pairing_id: pairingId,
        terminal_id: terminalId,
      }).catch(() => {});
    };
  }, [pairingId, terminalId]);

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center gap-2 px-3 py-2 border-b border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-800">
        <span className="text-sm font-medium">
          {terminalName || `Remote #${terminalId}`}
        </span>
        <span className="text-xs text-gray-500">({pairingId.slice(0, 8)}...)</span>
      </div>
      <div ref={containerRef} className="flex-1 overflow-hidden bg-black" />
    </div>
  );
}
