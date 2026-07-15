//! TermFast Desktop — Tauri shell
//!
//! Desktop-specific functionality: tray, autostart, platform adapter,
//! notifications, offline detection, window effects.

pub mod autostart;
pub mod network;
pub mod notification;
pub mod platform;
pub mod tray;

pub use autostart::{AutostartManager, StubAutostartManager};
pub use network::{NetworkMonitor, NetworkState};
pub use notification::{
    NotificationCategory, NotificationEvent, NotificationLevel, NotificationPrefs,
};
pub use platform::{get_platform_adapter, PlatformAdapter, SetProxyResult, SystemProxyConfig};
pub use tray::{
    build_tray_menu, calculate_badge, calculate_icon_color, TrayIconColor, TrayMenu, TrayMenuItem,
};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
