//! Proxy control API stub.
//!
//! Full implementation will start/stop `Socks5Server`/`MixedProxyServer`
//! from `termfast-core::proxy`.

pub fn start_socks5_stub(port: u16) -> u16 {
    port
}

pub fn stop_socks5_stub() {}
