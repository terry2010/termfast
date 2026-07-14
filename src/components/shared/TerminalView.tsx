// TerminalView — xterm.js wrapper for interactive SSH terminal
// Connects to a backend PTY session via IPC

import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { ipcInvoke } from "@/hooks/useIpc";
import "@xterm/xterm/css/xterm.css";

// === SECTION 1 END ===

interface TerminalViewProps {
  sessionId: string;
  serverId: string;
  active: boolean;
  initialOutput?: string;
}

export function TerminalView({ sessionId, serverId, active, initialOutput }: TerminalViewProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const sessionIdRef = useRef(sessionId);
  // Keep sessionIdRef updated so event listeners (which close over the ref) see the latest value
  sessionIdRef.current = sessionId;

  useEffect(() => {
    if (!containerRef.current) return;

    const term = new Terminal({
      cursorBlink: true,
      fontSize: 13,
      fontFamily: "'Menlo', 'Monaco', 'Courier New', monospace",
      theme: {
        background: "#1e1e2e",
        foreground: "#cdd6f4",
        cursor: "#f5e0dc",
      },
      allowProposedApi: true,
    });
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(containerRef.current);
    fitAddon.fit();

    termRef.current = term;
    fitRef.current = fitAddon;

    // Write initial output (MOTD/prompt) captured at terminal open time.
    // This avoids a race condition where the backend's read_task emits
    // "terminal:output" events before this component's listener is registered.
    if (initialOutput) {
      term.write(initialOutput);
    }

    // Send initial resize to backend
    const cols = term.cols;
    const rows = term.rows;
    ipcInvoke("ipc_terminal_resize", {
      session_id: sessionIdRef.current,
      cols,
      rows,
    }).catch(() => {});

    // User input → backend
    // With a PTY the remote shell echoes characters itself, so we do NOT
    // perform local echo here — doing so would double-print every keystroke.
    // Input bytes (including \r from Enter) are sent raw; the remote tty
    // line discipline handles CR/LF translation and line editing.
    const inputDisposable = term.onData((data) => {
      console.log("[TerminalView] onData:", JSON.stringify(data));
      ipcInvoke("ipc_terminal_input", {
        session_id: sessionIdRef.current,
        data,
      }).catch(() => {});
    });

    // Resize → backend
    const resizeDisposable = term.onResize(({ cols, rows }) => {
      ipcInvoke("ipc_terminal_resize", {
        session_id: sessionIdRef.current,
        cols,
        rows,
      }).catch(() => {});
    });

    // Listen for terminal output events from backend
    let unlistenOutput: UnlistenFn | undefined;
    listen<{ sessionId: string; data: string; stderr: boolean }>(
      "terminal:output",
      (event) => {
        console.log("[TerminalView] terminal:output", { sid: event.payload.sessionId, len: event.payload.data.length, match: event.payload.sessionId === sessionIdRef.current });
        if (event.payload.sessionId === sessionIdRef.current) {
          term.write(event.payload.data);
        }
      }
    ).then((fn) => {
      unlistenOutput = fn;
      console.log("[TerminalView] output listener registered for session", sessionId);
    });

    // Listen for terminal closed event (EOF/Close/channel dropped)
    let unlistenClosed: UnlistenFn | undefined;
    listen<{ sessionId: string }>("terminal:closed", (event) => {
      if (event.payload.sessionId === sessionIdRef.current) {
        term.write("\r\n[Connection closed]\r\n");
      }
    }).then((fn) => {
      unlistenClosed = fn;
    });

    // Window resize handler
    const handleResize = () => {
      try {
        fitAddon.fit();
      } catch {
        // ignore — container may not be visible
      }
    };
    window.addEventListener("resize", handleResize);
    const resizeObserver = new ResizeObserver(() => handleResize());
    resizeObserver.observe(containerRef.current);

    // Focus the terminal
    term.focus();

    return () => {
      inputDisposable.dispose();
      resizeDisposable.dispose();
      if (unlistenOutput) unlistenOutput();
      if (unlistenClosed) unlistenClosed();
      window.removeEventListener("resize", handleResize);
      resizeObserver.disconnect();
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);

  // When this tab becomes active, re-fit and focus
  useEffect(() => {
    if (!active || !termRef.current || !fitRef.current) return;
    try {
      fitRef.current.fit();
      const cols = termRef.current.cols;
      const rows = termRef.current.rows;
      ipcInvoke("ipc_terminal_resize", {
        session_id: sessionIdRef.current,
        cols,
        rows,
      }).catch(() => {});
      termRef.current.focus();
    } catch {
      // ignore
    }
  }, [active]);

  // Note: we do NOT close the session on unmount here. The tab is closed
  // explicitly via ServerDetail.handleCloseTerminal. Closing on unmount would
  // accidentally terminate the session whenever React remounts the component.

  return (
    <div
      ref={containerRef}
      className="w-full h-full bg-[#1e1e2e] overflow-hidden"
    />
  );
}

// === SECTION 2 END ===
