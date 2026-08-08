import { ipcInvoke } from "@/hooks/useIpc";

/**
 * Get this desktop's device_id (hostname-username-xxxx format).
 * D9: Includes 4-digit random hex suffix to prevent device_id collisions.
 * Used to filter ListDevices to only this desktop's pairings.
 */
export async function getDesktopDeviceId(): Promise<string> {
  const info = await ipcInvoke<any>("ipc_get_local_info");
  const hostname = info?.hostname || "unknown";
  const username = info?.username || "unknown";
  const suffix = info?.device_suffix || "";
  if (suffix) {
    return `${hostname}-${username}-${suffix}`;
  }
  return `${hostname}-${username}`;
}

/**
 * Fetch paired devices, filtered to only this desktop's pairings.
 * Returns empty array if desktop_device_id can't be determined.
 */
export async function fetchPairedDevices(token: string): Promise<any[]> {
  try {
    const deviceId = await getDesktopDeviceId();
    const r = await ipcInvoke<any>("ipc_pairing_list_devices", {
      token,
      desktop_device_id: deviceId,
    });
    return r.devices || [];
  } catch {
    return [];
  }
}
