//! SocketProtector implementation for Android.
//!
//! On Android the protector calls back into `VpnService.protect(fd)` via JNI
//! so the SSH control traffic is not routed back into the TUN interface.
//! On desktop it is a no-op.

use socket2::Socket;
use std::sync::OnceLock;
use std::sync::Mutex;
use termfast_core::ssh::SocketProtector;

/// Type of the protect callback: receives a raw fd, returns true if protected.
type ProtectCallback = Box<dyn Fn(i32) -> bool + Send + Sync>;

static PROTECT_CALLBACK: OnceLock<Mutex<Option<ProtectCallback>>> = OnceLock::new();

fn protect_callback() -> &'static Mutex<Option<ProtectCallback>> {
    PROTECT_CALLBACK.get_or_init(|| Mutex::new(None))
}

/// Set the protect callback (called from JNI when VpnService is established).
/// The callback typically calls `VpnService.protect(fd)` via JNI.
pub fn set_protect_callback(cb: ProtectCallback) {
    *protect_callback().lock().unwrap() = Some(cb);
}

/// Clear the protect callback (called when VpnService is destroyed).
pub fn clear_protect_callback() {
    *protect_callback().lock().unwrap() = None;
}

/// Android socket protector — calls `VpnService.protect(fd)` via callback.
#[derive(Debug, Clone, Copy)]
pub struct AndroidSocketProtector;

#[cfg(unix)]
impl SocketProtector for AndroidSocketProtector {
    fn protect_socket(&self, socket: &Socket) -> std::io::Result<()> {
        use std::os::unix::io::AsRawFd;
        let fd = socket.as_raw_fd();
        let lock = protect_callback().lock().unwrap();
        if let Some(ref cb) = *lock {
            if cb(fd) {
                Ok(())
            } else {
                Err(std::io::Error::other("VpnService.protect(fd) returned false"))
            }
        } else {
            // No callback set — allow the connection (no VPN active)
            Ok(())
        }
    }
}

#[cfg(not(unix))]
impl SocketProtector for AndroidSocketProtector {
    fn protect_socket(&self, _socket: &Socket) -> std::io::Result<()> {
        // Non-Unix (Windows): no-op, VpnService is Android-only
        Ok(())
    }
}
