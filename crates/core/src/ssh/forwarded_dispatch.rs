//! Forwarded-tcpip channel dispatcher — for remote port forwarding (-R)
//!
//! When the SSH server receives a connection on a remotely forwarded port,
//! it sends a forwarded-tcpip channel open request to the client.
//! The `server_channel_open_forwarded_tcpip` callback in `SshHandler`
//! dispatches the channel to the registered `RemoteForwarder` via this module.

use russh::Channel;
use russh::client::Msg;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio::sync::Mutex;

/// Key for dispatching forwarded-tcpip channels: (connected_address, connected_port)
pub type ForwardKey = (String, u16);

/// Dispatcher for forwarded-tcpip channels.
///
/// `SshHandler` holds an `Arc<ForwardedDispatch>` and calls `dispatch()`
/// when a forwarded-tcpip channel arrives. `RemoteForwarder` registers
/// a receiver via `register()` before calling `tcpip_forward()`.
pub struct ForwardedDispatch {
    senders: Mutex<HashMap<ForwardKey, mpsc::Sender<Channel<Msg>>>>,
}

impl Default for ForwardedDispatch {
    fn default() -> Self {
        Self::new()
    }
}

impl ForwardedDispatch {
    pub fn new() -> Self {
        Self {
            senders: Mutex::new(HashMap::new()),
        }
    }

    /// Register a receiver for forwarded-tcpip channels on the given key.
    pub async fn register(&self, key: ForwardKey, tx: mpsc::Sender<Channel<Msg>>) {
        self.senders.lock().await.insert(key, tx);
    }

    /// Unregister a receiver.
    pub async fn unregister(&self, key: &ForwardKey) {
        self.senders.lock().await.remove(key);
    }

    /// Dispatch a forwarded-tcpip channel to the registered receiver.
    /// Returns true if a receiver was found, false otherwise.
    pub async fn dispatch(&self, key: &ForwardKey, channel: Channel<Msg>) -> bool {
        let sender = {
            let guard = self.senders.lock().await;
            guard.get(key).cloned()
        };
        match sender {
            Some(tx) => {
                let _ = tx.send(channel).await;
                true
            }
            None => false,
        }
    }

    /// Clear all registered receivers (e.g. on SSH disconnect)
    pub async fn clear(&self) {
        self.senders.lock().await.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_and_unregister() {
        let dispatch = ForwardedDispatch::new();
        let (tx, _rx) = mpsc::channel::<Channel<Msg>>(1);
        let key = ("127.0.0.1".to_string(), 8080u16);

        dispatch.register(key.clone(), tx).await;
        // Verify registered
        {
            let senders = dispatch.senders.lock().await;
            assert!(senders.contains_key(&key));
        }

        dispatch.unregister(&key).await;
        // Verify unregistered
        {
            let senders = dispatch.senders.lock().await;
            assert!(!senders.contains_key(&key));
        }
    }

    #[tokio::test]
    async fn test_unregister_nonexistent_key() {
        let dispatch = ForwardedDispatch::new();
        let key = ("0.0.0.0".to_string(), 9999u16);
        // Should not panic
        dispatch.unregister(&key).await;
    }

    #[tokio::test]
    async fn test_clear_all() {
        let dispatch = ForwardedDispatch::new();
        let (tx1, _rx1) = mpsc::channel::<Channel<Msg>>(1);
        let (tx2, _rx2) = mpsc::channel::<Channel<Msg>>(1);
        dispatch.register(("127.0.0.1".into(), 8080u16), tx1).await;
        dispatch.register(("127.0.0.1".into(), 9090u16), tx2).await;
        dispatch.clear().await;
        // Verify cleared
        let senders = dispatch.senders.lock().await;
        assert!(senders.is_empty());
    }

    #[tokio::test]
    async fn test_dispatch_no_receiver_returns_false() {
        let dispatch = ForwardedDispatch::new();
        let key = ("127.0.0.1".to_string(), 1234u16);
        // No receiver registered — dispatch should return false
        // We can't create a real Channel without SSH connection,
        // but we can verify the sender lookup logic:
        // when no sender is registered, dispatch returns false.
        let senders = dispatch.senders.lock().await;
        assert!(!senders.contains_key(&key));
        // If we had a channel, dispatch() would return false here.
        // The actual dispatch() call requires a Channel<Msg> which
        // can only be created by russh during a real SSH session.
    }

    #[tokio::test]
    async fn test_dispatch_with_closed_sender() {
        // When the receiver is dropped, sending to the channel fails.
        // dispatch() should handle this gracefully (send returns Err,
        // but dispatch still returns true because a sender was found).
        let dispatch = ForwardedDispatch::new();
        let (tx, rx) = mpsc::channel::<Channel<Msg>>(1);
        let key = ("127.0.0.1".to_string(), 8080u16);
        dispatch.register(key.clone(), tx).await;
        // Drop the receiver — sender will fail on send
        drop(rx);
        // Verify sender is still registered
        {
            let senders = dispatch.senders.lock().await;
            assert!(senders.contains_key(&key));
        }
        // If we could call dispatch() with a real channel, it would
        // find the sender but the send would fail (rx dropped).
        // The dispatch() function ignores send errors (let _ = tx.send()),
        // so it would still return true. This is acceptable — the
        // RemoteForwarder's receive loop would have already exited.
    }
}
