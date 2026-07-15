//! VPN / tun2proxy integration stub.
//!
//! Full implementation will call `tun2proxy::general_run_async` with the
//! raw TUN fd received from `VpnService.establish()`.

#[cfg(target_os = "android")]
pub async fn start_tun2proxy_stub(_tun_fd: i32, _mtu: u16, _socks5_port: u16) -> std::io::Result<()> {
    Ok(())
}

#[cfg(not(target_os = "android"))]
pub async fn start_tun2proxy_stub(_tun_fd: i32, _mtu: u16, _socks5_port: u16) -> std::io::Result<()> {
    Ok(())
}
