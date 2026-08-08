// D8: WebSocket notification listener — desktop connects to /notifications
// to receive join_batch_pending events in real-time.
import { useEffect, useRef, useState, useCallback } from "react";
import { getDesktopDeviceId } from "./pairing";

export interface JoinBatchPendingNotification {
  type: "join_batch_pending";
  batch_id: string;
}

export type NotificationMessage = JoinBatchPendingNotification | { type: string; [key: string]: unknown };

interface UseNotificationListenerOptions {
  token: string | null;
  backendUrl: string;
  onJoinBatchPending?: (batchId: string) => void;
}

interface NotificationListenerState {
  connected: boolean;
  error: string | null;
}

/**
 * React hook that connects to the backend's /notifications WebSocket endpoint
 * and listens for join_batch_pending notifications.
 *
 * When a notification is received, calls onJoinBatchPending with the batch_id.
 * Automatically reconnects on disconnect with exponential backoff.
 */
export function useNotificationListener({
  token,
  backendUrl,
  onJoinBatchPending,
}: UseNotificationListenerOptions): NotificationListenerState {
  const [connected, setConnected] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const reconnectAttemptsRef = useRef(0);
  const mountedRef = useRef(true);
  const onJoinBatchPendingRef = useRef(onJoinBatchPending);

  // Keep callback ref updated without triggering reconnect
  useEffect(() => {
    onJoinBatchPendingRef.current = onJoinBatchPending;
  }, [onJoinBatchPending]);

  const connect = useCallback(async () => {
    if (!token || !mountedRef.current) {
      setConnected(false);
      return;
    }

    try {
      const deviceId: string = await getDesktopDeviceId();
      if (!mountedRef.current) return;
      const wsUrl = backendUrl.replace(/^http/, "ws") + "/notifications?token=" + token + "&device_id=" + deviceId;
      const ws = new WebSocket(wsUrl);
      wsRef.current = ws;

      ws.onopen = () => {
        if (!mountedRef.current) return;
        setConnected(true);
        setError(null);
        reconnectAttemptsRef.current = 0;
      };

      ws.onmessage = (event) => {
        if (!mountedRef.current) return;
        try {
          const msg = JSON.parse(event.data) as NotificationMessage;
          if (msg.type === "join_batch_pending" && "batch_id" in msg) {
            const batchId = (msg as { batch_id: string }).batch_id;
            onJoinBatchPendingRef.current?.(batchId);
          }
        } catch {
          // Ignore malformed messages
        }
      };

      ws.onerror = () => {
        if (!mountedRef.current) return;
        setError("WebSocket error");
      };

      ws.onclose = () => {
        if (!mountedRef.current) return;
        setConnected(false);
        wsRef.current = null;
        // Reconnect with exponential backoff (max 30 seconds)
        const attempts = reconnectAttemptsRef.current++;
        const delay = Math.min(1000 * Math.pow(2, attempts), 30000);
        reconnectTimerRef.current = setTimeout(() => {
          connect();
        }, delay);
      };
    } catch (err) {
      if (!mountedRef.current) return;
      setError(err instanceof Error ? err.message : "Connection failed");
      setConnected(false);
    }
  }, [token, backendUrl]);

  useEffect(() => {
    mountedRef.current = true;
    connect();

    return () => {
      mountedRef.current = false;
      if (reconnectTimerRef.current) {
        clearTimeout(reconnectTimerRef.current);
      }
      if (wsRef.current) {
        wsRef.current.close();
      }
    };
  }, [connect]);

  return { connected, error };
}
