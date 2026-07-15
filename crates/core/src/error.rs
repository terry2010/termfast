//! Error type system — FP-1.1
//!
//! Backend returns language-agnostic `ErrorCode` + English `detail`.
//! Frontend renders translated message via i18next based on `ErrorCode`.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Language-agnostic error code enum (§10.0).
/// Frontend uses `t('errors.' + code)` to render translated message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ErrorCode {
    /// Port already in use by another server
    PortConflict,
    /// SSH authentication failed (wrong password/key)
    AuthFailed,
    /// SSH TCP connection failed
    SshConnectFailed,
    /// SSH connection dropped (not主动断开)
    SshDisconnected,
    /// HostKey fingerprint mismatch
    HostKeyMismatch,
    /// Config file corrupted
    ConfigCorrupt,
    /// Config schema migration failed
    ConfigMigrationFailed,
    /// Keychain entry not found
    CredentialNotFound,
    /// Keychain write failed
    CredentialStoreFailed,
    /// Template not found
    TemplateNotFound,
    /// Trigger not found
    TriggerNotFound,
    /// Server not found
    ServerNotFound,
    /// Proxy port in use by system
    ProxyPortInUse,
    /// Needs admin privilege
    NeedsPrivilege,
    /// Import failed (bad JSON or invalid data)
    ImportFailed,
    /// Decryption failed (wrong master password or corrupted file)
    DecryptionFailed,
    /// Trigger command execution failed (non-zero exit or timeout)
    TriggerCommandFailed,
    /// Internal unexpected error
    Internal,
    /// Invalid IPC request parameters
    InvalidParams,
}

/// IPC error structure — serialized and sent to frontend/CLI.
/// Only `code` + `detail`, no translated `message` field (§3.6).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcError {
    /// Language-agnostic error code
    pub code: ErrorCode,
    /// English debug detail, e.g. "port 1080 already used by srv_uswest"
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

impl std::fmt::Display for IpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.detail)
    }
}

impl std::error::Error for IpcError {}

/// Internal error type for core library.
/// Distinguishes between IPC-serializable errors and internal errors.
#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Ipc(#[from] IpcError),
    #[error("SSH error: {0}")]
    Ssh(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Config error: {0}")]
    Config(String),
    #[error("Crypto error: {0}")]
    Crypto(String),
    #[error("Serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("{0}")]
    Other(String),
}

impl Error {
    /// Convert to IpcError for IPC transmission
    pub fn to_ipc(&self) -> IpcError {
        match self {
            Error::Ipc(ipc) => ipc.clone(),
            Error::Ssh(msg) => IpcError::new(ErrorCode::SshConnectFailed, msg.clone()),
            Error::Io(e) => IpcError::new(ErrorCode::Internal, e.to_string()),
            Error::Config(msg) => IpcError::new(ErrorCode::ConfigCorrupt, msg.clone()),
            Error::Crypto(msg) => IpcError::new(ErrorCode::DecryptionFailed, msg.clone()),
            Error::Serde(e) => IpcError::new(ErrorCode::ImportFailed, e.to_string()),
            Error::Other(msg) => IpcError::new(ErrorCode::Internal, msg.clone()),
        }
    }
}

impl From<Error> for IpcError {
    fn from(e: Error) -> Self {
        e.to_ipc()
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_code_serialization() {
        let code = ErrorCode::PortConflict;
        let json = serde_json::to_string(&code).unwrap();
        assert_eq!(json, "\"PortConflict\"");
        let de: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(de, code);
    }

    #[test]
    fn test_all_error_codes_round_trip() {
        let codes = vec![
            ErrorCode::PortConflict,
            ErrorCode::AuthFailed,
            ErrorCode::SshConnectFailed,
            ErrorCode::SshDisconnected,
            ErrorCode::HostKeyMismatch,
            ErrorCode::ConfigCorrupt,
            ErrorCode::ConfigMigrationFailed,
            ErrorCode::CredentialNotFound,
            ErrorCode::CredentialStoreFailed,
            ErrorCode::TemplateNotFound,
            ErrorCode::TriggerNotFound,
            ErrorCode::ServerNotFound,
            ErrorCode::ProxyPortInUse,
            ErrorCode::NeedsPrivilege,
            ErrorCode::ImportFailed,
            ErrorCode::DecryptionFailed,
            ErrorCode::TriggerCommandFailed,
            ErrorCode::Internal,
        ];
        for code in codes {
            let json = serde_json::to_string(&code).unwrap();
            let de: ErrorCode = serde_json::from_str(&json).unwrap();
            assert_eq!(de, code, "round-trip failed for {:?}", code);
        }
    }

    #[test]
    fn test_ipc_error_json_format() {
        let err = IpcError::new(
            ErrorCode::PortConflict,
            "port 1080 already used by srv_uswest",
        );
        let json = serde_json::to_string(&err).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["code"], "PortConflict");
        assert_eq!(v["detail"], "port 1080 already used by srv_uswest");
        // No "message" field
        assert!(v.get("message").is_none());
    }

    #[test]
    fn test_ipc_error_no_message_field() {
        let err = IpcError::new(ErrorCode::AuthFailed, "wrong password");
        let json = serde_json::to_string(&err).unwrap();
        assert!(!json.contains("message"));
    }

    #[test]
    fn test_error_to_ipc_conversion() {
        let err = Error::Ssh("connection refused".into());
        let ipc = err.to_ipc();
        assert_eq!(ipc.code, ErrorCode::SshConnectFailed);
        assert_eq!(ipc.detail, "connection refused");
    }
}
