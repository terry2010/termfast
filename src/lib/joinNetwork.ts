// D4: Signing logic for ApproveJoin — UI code path only, no IPC/CLI proxy signing.
import { ipcInvoke } from "@/hooks/useIpc";
import { getDesktopDeviceId } from "./pairing";

/**
 * Canonical JSON serialization: keys sorted, no whitespace, UTF-8 direct.
 * Matches the backend's EncodeApproveJoinPayload canonical JSON format.
 */
export function canonicalJSON(value: unknown): string {
  if (value === null || value === undefined) return "null";
  if (typeof value === "boolean") return value ? "true" : "false";
  if (typeof value === "number") return String(value);
  if (typeof value === "string") return JSON.stringify(value);
  if (Array.isArray(value)) {
    return "[" + value.map(canonicalJSON).join(",") + "]";
  }
  if (typeof value === "object") {
    const obj = value as Record<string, unknown>;
    const keys = Object.keys(obj).sort();
    return "{" + keys.map((k) => JSON.stringify(k) + ":" + canonicalJSON(obj[k])).join(",") + "}";
  }
  return "null";
}

/**
 * Base64-encode a string using UTF-8 encoding (handles non-ASCII characters).
 * btoa() only handles Latin-1, so we use TextEncoder for proper UTF-8.
 */
function base64EncodeUtf8(str: string): string {
  const bytes = new TextEncoder().encode(str);
  let binary = "";
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
}

/**
 * Build the ApproveJoin payload (canonical JSON, base64-encoded).
 * Payload fields: {join_batch_id, approver_user_id, approver_device_id, server_nonce, batch_content_hash}
 */
export function buildApproveJoinPayload(params: {
  joinBatchId: string;
  approverUserId: number;
  approverDeviceId: string;
  serverNonce: string; // base64
  batchContentHash: string; // base64
}): string {
  const payload = {
    join_batch_id: params.joinBatchId,
    approver_user_id: params.approverUserId,
    approver_device_id: params.approverDeviceId,
    server_nonce: params.serverNonce,
    batch_content_hash: params.batchContentHash,
  };
  const json = canonicalJSON(payload);
  // base64-encode the UTF-8 canonical JSON
  return base64EncodeUtf8(json);
}

/**
 * Sign the ApproveJoin payload with the device private key.
 * Calls the Rust IPC command ipc_sign_device_payload, which decodes
 * the base64 payload and signs the raw canonical JSON bytes with the
 * ECDSA P-256 private key.
 * Returns base64-encoded DER signature.
 *
 * Security: This function is only called from UI handler code path
 * (user clicks "Approve" button). Not exposed via IPC/CLI for proxy signing.
 */
export async function signApproveJoinPayload(payloadBase64: string): Promise<string> {
  // Pass base64 directly to Rust, which decodes and signs the raw bytes.
  // This avoids encoding ambiguity (atob returns Latin-1, Rust expects UTF-8).
  return ipcInvoke<string>("ipc_sign_device_payload", { payloadBase64 });
}

/**
 * Full ApproveJoin flow: build payload, sign, and submit to backend.
 * Called when user clicks "Approve" in the D1/D2 approval dialog.
 */
export async function approveJoin(params: {
  batchId: string;
  approverUserId: number;
  serverNonce: string; // base64
  batchContentHash: string; // base64
  token: string; // user JWT
}): Promise<{ receivedApprovals: number; requiredApprovals: number; status: string }> {
  const approverDeviceId = await getDesktopDeviceId();

  // 1. Build canonical JSON payload
  const payloadBase64 = buildApproveJoinPayload({
    joinBatchId: params.batchId,
    approverUserId: params.approverUserId,
    approverDeviceId,
    serverNonce: params.serverNonce,
    batchContentHash: params.batchContentHash,
  });

  // 2. Sign the payload with device private key
  const signature = await signApproveJoinPayload(payloadBase64);

  // 3. Submit to backend via Rust IPC (which calls the backend API)
  const result = await ipcInvoke<{
    batch_id: string;
    received_approvals: number;
    required_approvals: number;
    status: string;
  }>("ipc_approve_join", {
    token: params.token,
    batch_id: params.batchId,
    approver_device_id: approverDeviceId,
    payload: payloadBase64,
    signature,
  });

  return {
    receivedApprovals: result.received_approvals,
    requiredApprovals: result.required_approvals,
    status: result.status,
  };
}

/**
 * BatchInfo — response from GetBatchInfo API.
 */
export interface BatchInfo {
  batch_id: string;
  flow_type: string;
  required_approvals: number;
  received_approvals: number;
  server_nonce: string;
  batch_content_hash: string;
  expires_at: string;
  status: string;
  target_members: Array<{ user_id: number; device_id: string; device_name: string }>;
  initiator_user_id: number;
}

/**
 * Get batch info from the backend (for D1/D2 approval dialog).
 */
export async function getBatchInfo(params: {
  batchId: string;
  token: string;
}): Promise<BatchInfo> {
  const deviceId = await getDesktopDeviceId();
  return ipcInvoke("ipc_get_batch_info", {
    token: params.token,
    batch_id: params.batchId,
    device_id: deviceId,
  });
}
