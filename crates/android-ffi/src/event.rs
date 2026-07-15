//! JSON event serialization and callback dispatch.
//!
//! Events are sent to Kotlin via a callback object that implements
//! `com.termfast.app.RustEventListener.onEvent(String json)`.

use serde::Serialize;

#[derive(Serialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum RustEvent {
    #[serde(rename = "log:entry")]
    LogEntry { entry: serde_json::Value },
    #[serde(rename = "server:status_changed")]
    ServerStatusChanged {
        server_id: String,
        status: String,
        exit_ip: Option<String>,
        latency_ms: Option<u32>,
    },
    #[serde(rename = "proxy:status_changed")]
    ProxyStatusChanged {
        server_id: String,
        proxy_running: bool,
        active_channels: u32,
    },
    #[serde(rename = "vpn:status_changed")]
    VpnStatusChanged {
        server_id: String,
        vpn_running: bool,
    },
}

impl RustEvent {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}
