//! SocketProtector implementation for Android.
//!
//! On Android the protector calls back into `VpnService.protect(fd)` via JNI.
//! On desktop it is a no-op.

use socket2::Socket;
use termfast_core::ssh::SocketProtector;

/// No-op protector for desktop builds and tests.
#[derive(Debug, Clone, Copy)]
pub struct AndroidSocketProtector;

impl SocketProtector for AndroidSocketProtector {
    fn protect_socket(&self, _socket: &Socket) -> std::io::Result<()> {
        Ok(())
    }
}
