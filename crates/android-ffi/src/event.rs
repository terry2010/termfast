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
        #[serde(skip_serializing_if = "Option::is_none")]
        error_code: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error_detail: Option<String>,
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
    #[serde(rename = "ip:changed")]
    IpChanged {
        server_id: String,
        server_name: String,
        old_ip: Option<String>,
        new_ip: String,
    },
    #[serde(rename = "TerminalData")]
    TerminalData {
        session_id: String,
        data: String,
    },
    #[serde(rename = "TerminalClosed")]
    TerminalClosed {
        session_id: String,
    },
    #[serde(rename = "TerminalError")]
    TerminalError {
        session_id: String,
        error: String,
    },
}

impl RustEvent {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Send a log event to Kotlin (if callback is set).
    /// Safe to call from any thread.
    #[cfg(target_os = "android")]
    pub fn log(level: &str, tag: &str, message: &str) {
        let entry = serde_json::json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "level": level,
            "tag": tag,
            "message": message,
        });
        let json = RustEvent::LogEntry { entry }.to_json();
        crate::jni::dispatch_event_to_kotlin(&json);
    }
}
