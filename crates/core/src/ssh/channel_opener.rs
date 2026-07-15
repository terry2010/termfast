//! Channel opener trait + SSH implementation — FP-2.4
//!
//! Abstracts the opening of direct-tcpip channels for proxy use.
//! Allows proxy layer to be independent of SSH implementation details.

use crate::error::{Error, Result};
use russh::client;
use russh::Channel;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Trait for opening SSH direct-tcpip channels.
/// Proxy layer uses this without knowing about SSH specifics.
#[async_trait::async_trait]
pub trait ChannelOpener: Send + Sync {
    /// Open a direct-tcpip channel to the target host:port
    async fn open_channel(&self, host: &str, port: u16) -> Result<Channel<client::Msg>>;
}

/// SSH implementation of ChannelOpener
pub struct SshChannelOpener {
    handle: Mutex<Option<Arc<client::Handle<super::client::SshHandler>>>>,
}

impl SshChannelOpener {
    pub fn new(handle: Arc<client::Handle<super::client::SshHandler>>) -> Self {
        Self {
            handle: Mutex::new(Some(handle)),
        }
    }

    pub fn empty() -> Self {
        Self {
            handle: Mutex::new(None),
        }
    }

    pub async fn set_handle(&self, handle: Arc<client::Handle<super::client::SshHandler>>) {
        *self.handle.lock().await = Some(handle);
    }

    pub async fn clear_handle(&self) {
        *self.handle.lock().await = None;
    }

    pub async fn is_available(&self) -> bool {
        let guard = self.handle.lock().await;
        guard.as_ref().map(|h| !h.is_closed()).unwrap_or(false)
    }
}

#[async_trait::async_trait]
impl ChannelOpener for SshChannelOpener {
    async fn open_channel(&self, host: &str, port: u16) -> Result<Channel<client::Msg>> {
        let guard = self.handle.lock().await;
        let handle = guard
            .as_ref()
            .ok_or_else(|| Error::Ssh("SSH connection not available".into()))?;
        handle
            .channel_open_direct_tcpip(host, port as u32, "127.0.0.1", 0)
            .await
            .map_err(|e| Error::Ssh(format!("failed to open direct-tcpip channel: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_empty_channel_opener_not_available() {
        let opener = SshChannelOpener::empty();
        assert!(!opener.is_available().await);
    }

    #[tokio::test]
    async fn test_empty_channel_opener_open_fails() {
        let opener = SshChannelOpener::empty();
        let result = opener.open_channel("example.com", 80).await;
        assert!(result.is_err());
    }
}
