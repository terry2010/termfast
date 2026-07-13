//! IPC protocol definitions — FP-1.7
//!
//! Message format: [4-byte length (big-endian u32)][JSON payload]
//! Shared between daemon, GUI (Tauri), and CLI.

use serde::{Deserialize, Serialize};
use vps_guard_core::error::ErrorCode;

/// Client → daemon request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    /// UUID for matching response
    pub id: String,
    /// Command to execute
    pub action: Action,
    /// Command parameters (varies by action)
    #[serde(default)]
    pub params: serde_json::Value,
}

/// daemon → client response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Response {
    Ok {
        id: String,
        data: serde_json::Value,
    },
    Err {
        id: String,
        error: IpcError,
    },
    /// Server-pushed event (no request id)
    Event {
        event: String,
        data: serde_json::Value,
    },
}

/// IPC error (mirrors core::IpcError but in daemon proto for convenience)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcError {
    pub code: ErrorCode,
    pub detail: String,
}

impl IpcError {
    pub fn new(code: ErrorCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

impl From<vps_guard_core::error::IpcError> for IpcError {
    fn from(e: vps_guard_core::error::IpcError) -> Self {
        Self {
            code: e.code,
            detail: e.detail,
        }
    }
}

/// All available IPC commands (§10.1-10.5)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    // Server management (§10.1)
    AddServer,
    RemoveServer,
    UpdateServer,
    ConnectServer,
    DisconnectServer,
    GetServerStatus,
    ListServers,
    ExportServers,
    ImportServers,
    ExportFull,
    ImportFull,
    CleanupAuthorizedKeys,
    ReorderServers,

    // Proxy control (§10.2)
    ToggleProxy,
    ToggleProxyAdvanced,
    GetProxyStatus,
    TestProxy,
    SetProxyAuth,
    ClearProxyAuth,
    SetSystemProxy,
    ClearSystemProxy,
    GetSystemProxy,

    // Trigger management (§10.3)
    AddTrigger,
    RemoveTrigger,
    UpdateTrigger,
    SyncTriggerFromTemplate,
    ManualFireTrigger,
    PauseAllTriggers,
    ResumeAllTriggers,
    PauseServerTriggers,
    ResumeServerTriggers,

    // Template management (§10.4)
    ListTemplates,
    CreateTemplate,
    UpdateTemplate,
    DeleteTemplate,
    SaveTriggerAsTemplate,
    ImportTemplates,
    ExportTemplates,

    // Credential management (§10.5)
    SaveCredential,
    HasCredential,
    DeleteCredential,
    ConfigureKeyAuth,
    SwitchAuthMethod,

    // Config
    GetConfig,
    UpdateGeneralConfig,

    // Logs
    GetLogs,
    ClearLogs,
    ExportLogs,

    // Daemon control
    Shutdown,
    GetDaemonStatus,

    // Onboarding (FP-8.1)
    DetectFirewall,
}

/// Event types (daemon → all clients broadcast, §10.6)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    /// { server_id, status, ip? }
    ServerStatusChanged,
    /// { server_id, enabled }
    ProxyStatusChanged,
    /// { server_id, trigger_id, trigger_name, type, total_commands }
    TriggerFired,
    /// { server_id, trigger_id, command_index, total_commands, command, output, success }
    TriggerCommandExecuted,
    /// { server_id, trigger_id, success, executed_commands, total_commands }
    TriggerCompleted,
    /// { server_id, level, kind, message, timestamp, data? }
    LogEntry,
}

impl EventType {
    /// Get the event name string for serialization
    pub fn as_str(&self) -> &'static str {
        match self {
            EventType::ServerStatusChanged => "server:status_changed",
            EventType::ProxyStatusChanged => "proxy:status_changed",
            EventType::TriggerFired => "trigger:fired",
            EventType::TriggerCommandExecuted => "trigger:command_executed",
            EventType::TriggerCompleted => "trigger:completed",
            EventType::LogEntry => "log:entry",
        }
    }
}

/// Helper to create a request
impl Request {
    pub fn new(action: Action, params: serde_json::Value) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            action,
            params,
        }
    }

    pub fn new_simple(action: Action) -> Self {
        Self::new(action, serde_json::Value::Null)
    }
}

/// Helper to create an OK response
impl Response {
    pub fn ok(id: impl Into<String>, data: serde_json::Value) -> Self {
        Response::Ok {
            id: id.into(),
            data,
        }
    }

    pub fn err(id: impl Into<String>, error: IpcError) -> Self {
        Response::Err {
            id: id.into(),
            error,
        }
    }

    pub fn event(event: impl Into<String>, data: serde_json::Value) -> Self {
        Response::Event {
            event: event.into(),
            data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let req = Request::new(Action::ListServers, serde_json::Value::Null);
        let json = serde_json::to_string(&req).unwrap();
        let de: Request = serde_json::from_str(&json).unwrap();
        assert_eq!(de.id, req.id);
        assert!(matches!(de.action, Action::ListServers));
    }

    #[test]
    fn test_response_ok_serialization() {
        let resp = Response::ok("req_123", serde_json::json!({"servers": []}));
        let json = serde_json::to_string(&resp).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "Ok");
        assert_eq!(v["id"], "req_123");
    }

    #[test]
    fn test_response_err_serialization() {
        let resp = Response::err(
            "req_123",
            IpcError::new(ErrorCode::AuthFailed, "wrong password"),
        );
        let json = serde_json::to_string(&resp).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "Err");
        assert_eq!(v["error"]["code"], "AuthFailed");
    }

    #[test]
    fn test_response_event_serialization() {
        let resp = Response::event("server:status_changed", serde_json::json!({"server_id": "srv_1"}));
        let json = serde_json::to_string(&resp).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "Event");
        assert_eq!(v["event"], "server:status_changed");
    }

    #[test]
    fn test_all_actions_round_trip() {
        let actions = vec![
            Action::AddServer, Action::RemoveServer, Action::UpdateServer,
            Action::ConnectServer, Action::DisconnectServer, Action::GetServerStatus,
            Action::ListServers, Action::ExportServers, Action::ImportServers,
            Action::ExportFull, Action::ImportFull, Action::CleanupAuthorizedKeys,
            Action::ToggleProxy, Action::ToggleProxyAdvanced, Action::GetProxyStatus,
            Action::TestProxy, Action::SetProxyAuth, Action::ClearProxyAuth,
            Action::SetSystemProxy, Action::ClearSystemProxy, Action::GetSystemProxy,
            Action::AddTrigger, Action::RemoveTrigger, Action::UpdateTrigger,
            Action::SyncTriggerFromTemplate, Action::ManualFireTrigger,
            Action::PauseAllTriggers, Action::ResumeAllTriggers,
            Action::PauseServerTriggers, Action::ResumeServerTriggers,
            Action::ListTemplates, Action::CreateTemplate, Action::UpdateTemplate,
            Action::DeleteTemplate, Action::SaveTriggerAsTemplate,
            Action::ImportTemplates, Action::ExportTemplates,
            Action::SaveCredential, Action::HasCredential, Action::DeleteCredential,
            Action::ConfigureKeyAuth, Action::SwitchAuthMethod,
            Action::GetConfig, Action::UpdateGeneralConfig,
            Action::GetLogs, Action::ClearLogs, Action::ExportLogs,
            Action::Shutdown, Action::GetDaemonStatus,
            Action::DetectFirewall,
        ];
        for action in actions {
            let json = serde_json::to_string(&action).unwrap();
            let de: Action = serde_json::from_str(&json).unwrap();
            // Just verify round-trip doesn't panic
            let _ = format!("{:?}", de);
        }
    }

    #[test]
    fn test_event_type_as_str() {
        assert_eq!(EventType::ServerStatusChanged.as_str(), "server:status_changed");
        assert_eq!(EventType::ProxyStatusChanged.as_str(), "proxy:status_changed");
        assert_eq!(EventType::TriggerFired.as_str(), "trigger:fired");
        assert_eq!(EventType::LogEntry.as_str(), "log:entry");
    }
}
