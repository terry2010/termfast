//! Channel manager — FP-3.1
//! Manages SSH channel lifecycle with concurrency limits and idle timeouts.

use crate::error::Result;
use crate::ssh::channel_opener::ChannelOpener;
use russh::client;
use russh::Channel;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Semaphore;

/// Manages SSH direct-tcpip channels with concurrency limits.
pub struct ChannelManager {
    opener: Arc<dyn ChannelOpener>,
    semaphore: Arc<Semaphore>,
    idle_timeout: Duration,
    max_channels: usize,
    /// Number of active client connections (incremented on each open, decremented on drop)
    active_clients: Arc<AtomicU32>,
    /// Total bytes received from clients (upload)
    bytes_in: Arc<AtomicU64>,
    /// Total bytes sent to clients (download)
    bytes_out: Arc<AtomicU64>,
}

impl ChannelManager {
    pub fn new(opener: Arc<dyn ChannelOpener>, max_channels: usize, idle_timeout_secs: u64) -> Self {
        Self {
            opener,
            semaphore: Arc::new(Semaphore::new(max_channels)),
            idle_timeout: Duration::from_secs(idle_timeout_secs),
            max_channels,
            active_clients: Arc::new(AtomicU32::new(0)),
            bytes_in: Arc::new(AtomicU64::new(0)),
            bytes_out: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Open a channel to the target host:port
    pub async fn open(&self, host: &str, port: u16) -> Result<ManagedChannel> {
        let permit = self.semaphore.clone().acquire_owned().await.map_err(|e| {
            crate::error::Error::Other(format!("semaphore error: {}", e))
        })?;

        // Log warning when at 80% capacity
        if self.is_near_capacity() {
            tracing::warn!(
                "proxy channel near capacity: {}/{} active",
                self.active_channel_count(),
                self.max_channels
            );
        }

        let channel = self.opener.open_channel(host, port).await?;
        self.active_clients.fetch_add(1, Ordering::Relaxed);
        Ok(ManagedChannel {
            channel,
            _permit: permit,
            idle_timeout: self.idle_timeout,
            active_clients: self.active_clients.clone(),
            bytes_in: self.bytes_in.clone(),
            bytes_out: self.bytes_out.clone(),
        })
    }

    /// Get the number of available permits
    pub fn available(&self) -> usize {
        self.semaphore.available_permits()
    }

    /// Get max channels
    pub fn max_channels(&self) -> usize {
        self.max_channels
    }

    /// Get the number of currently active channels (= active client connections)
    pub fn active_channel_count(&self) -> u32 {
        (self.max_channels - self.semaphore.available_permits()) as u32
    }

    /// Get the number of active client connections
    pub fn active_clients(&self) -> u32 {
        self.active_clients.load(Ordering::Relaxed)
    }

    /// Get total bytes received from clients (upload)
    pub fn bytes_in(&self) -> u64 {
        self.bytes_in.load(Ordering::Relaxed)
    }

    /// Get total bytes sent to clients (download)
    pub fn bytes_out(&self) -> u64 {
        self.bytes_out.load(Ordering::Relaxed)
    }

    /// Check if at 80% capacity (for logging warnings)
    pub fn is_near_capacity(&self) -> bool {
        self.available() < (self.max_channels / 5)
    }
}

/// A managed channel with a semaphore permit
pub struct ManagedChannel {
    pub channel: Channel<client::Msg>,
    _permit: tokio::sync::OwnedSemaphorePermit,
    idle_timeout: Duration,
    active_clients: Arc<AtomicU32>,
    bytes_in: Arc<AtomicU64>,
    bytes_out: Arc<AtomicU64>,
}

impl ManagedChannel {
    pub fn idle_timeout(&self) -> Duration {
        self.idle_timeout
    }

    /// Get a clone of the active_clients counter for later decrement.
    pub fn active_clients_clone(&self) -> Arc<AtomicU32> {
        self.active_clients.clone()
    }

    /// Get clones of byte counters for tracking data transfer.
    pub fn byte_counters(&self) -> (Arc<AtomicU64>, Arc<AtomicU64>) {
        (self.bytes_in.clone(), self.bytes_out.clone())
    }
}

/// Counting wrapper for AsyncRead — increments counter by bytes read.
pub struct CountingReader<R> {
    pub inner: R,
    pub counter: Arc<AtomicU64>,
}

impl<R: AsyncRead + Unpin> AsyncRead for CountingReader<R> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let pin = std::pin::Pin::new(&mut this.inner);
        let result = pin.poll_read(cx, buf);
        if let std::task::Poll::Ready(Ok(())) = &result {
            let after = buf.filled().len();
            if after > before {
                this.counter.fetch_add((after - before) as u64, Ordering::Relaxed);
            }
        }
        result
    }
}

/// Counting wrapper for AsyncWrite — increments counter by bytes written.
pub struct CountingWriter<W> {
    pub inner: W,
    pub counter: Arc<AtomicU64>,
}

impl<W: AsyncWrite + Unpin> AsyncWrite for CountingWriter<W> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        let pin = std::pin::Pin::new(&mut this.inner);
        let result = pin.poll_write(cx, buf);
        if let std::task::Poll::Ready(Ok(n)) = &result {
            this.counter.fetch_add(*n as u64, Ordering::Relaxed);
        }
        result
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.get_mut();
        std::pin::Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.get_mut();
        std::pin::Pin::new(&mut this.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::channel_opener::SshChannelOpener;

    #[tokio::test]
    async fn test_channel_manager_creation() {
        let opener = Arc::new(SshChannelOpener::empty());
        let mgr = ChannelManager::new(opener, 64, 300);
        assert_eq!(mgr.max_channels(), 64);
        assert_eq!(mgr.available(), 64);
        assert!(!mgr.is_near_capacity());
    }
}
