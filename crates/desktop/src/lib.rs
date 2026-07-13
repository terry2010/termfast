//! VPS Guard Desktop — Tauri shell
//!
//! Desktop-specific functionality: tray, autostart, platform adapter,
//! notifications, offline detection, window effects.

pub mod platform;
pub mod notification;
pub mod tray;
pub mod autostart;
pub mod network;

pub use platform::{PlatformAdapter, SystemProxyConfig, SetProxyResult, get_platform_adapter};
pub use notification::{NotificationPrefs, NotificationCategory, NotificationLevel, NotificationEvent};
pub use tray::{TrayMenu, TrayMenuItem, TrayIconColor, build_tray_menu, calculate_icon_color, calculate_badge};
pub use autostart::{AutostartManager, StubAutostartManager};
pub use network::{NetworkMonitor, NetworkState};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
