//! Socket protector — FP-2.1
//!
//! Platform-agnostic hook for protecting a socket before `connect()`.
//! On Android, the implementation calls `VpnService.protect(fd)` via JNI
//! so the SSH control traffic is not routed back into the TUN interface.

use socket2::Socket;

/// Hook called after a socket is created but before `connect()` is issued.
///
/// Implementors receive the raw socket and may perform platform-specific
/// work (e.g. `VpnService.protect(fd)` on Android). The socket is still
/// non-blocking at this point and has not connected yet.
pub trait SocketProtector: Send + Sync {
    fn protect_socket(&self, socket: &Socket) -> std::io::Result<()>;
}

/// No-op protector used on desktop platforms.
#[derive(Debug, Clone, Copy)]
pub struct NoOpSocketProtector;

impl SocketProtector for NoOpSocketProtector {
    fn protect_socket(&self, _socket: &Socket) -> std::io::Result<()> {
        Ok(())
    }
}
