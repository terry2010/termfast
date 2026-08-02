// Shared terminal IPC helpers — used by ServerList, ServerDetail, and tray menu.
//
// All terminal open calls MUST go through openTerminalWithChannel so that
// a binary Channel is registered for terminal:output events.  Calling
// ipc_terminal_open without a Channel silently drops all terminal output.

import { Channel } from "@tauri-apps/api/core";
import { ipcInvoke } from "@/hooks/useIpc";
import { dispatchTerminalOutput } from "@/components/shared/TerminalView";

export interface TerminalOpenResult {
  session_id: string;
  initial_output: string;
}

export interface TerminalOpenOptions {
  backend?: "ssh" | "local";
  shell?: string;
  triggerOverrides?: Record<string, boolean>;
}

/**
 * Open a terminal session with a binary Channel for output.
 *
 * The Channel receives raw ArrayBuffer from the Rust backend; we convert to
 * Uint8Array and dispatch to the registered TerminalView callback.
 *
 * Always use this helper instead of calling ipc_terminal_open directly —
 * without a Channel, terminal output is silently dropped.
 */
export async function openTerminalWithChannel(
  serverId: string,
  cols = 80,
  rows = 24,
  options?: TerminalOpenOptions,
): Promise<TerminalOpenResult> {
  // session_id is assigned after ipc_terminal_open returns; the closure
  // captures it by reference so onmessage can dispatch once set.
  let sessionId = "";
  const onOutput = new Channel<ArrayBuffer>();
  onOutput.onmessage = (data: ArrayBuffer) => {
    if (sessionId) {
      dispatchTerminalOutput(sessionId, new Uint8Array(data), false);
    }
  };
  const result = await ipcInvoke<TerminalOpenResult>("ipc_terminal_open", {
    server_id: serverId,
    cols,
    rows,
    on_output: onOutput,
    backend: options?.backend ?? "ssh",
    shell: options?.shell,
    trigger_overrides: options?.triggerOverrides,
  });
  sessionId = result.session_id;
  return result;
}
