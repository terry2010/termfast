//! Config migration — FP-1.4
//!
//! Chain migration: v1→v2→v3... (§17.5)
//! Each step: backup → migrate → validate → rollback on failure.

use crate::error::{Error, ErrorCode, IpcError, Result};
use serde_json::Value;
use std::path::PathBuf;

/// Current config schema version supported by this software
pub const CURRENT_VERSION: u32 = 1;

/// Migrate config JSON from `from_version` to current version.
/// Chain migration: v1→v2→v3... (§17.5)
pub fn migrate(config: &mut Value, from_version: u32) -> Result<()> {
    if from_version > CURRENT_VERSION {
        return Err(Error::Ipc(IpcError::new(
            ErrorCode::ConfigMigrationFailed,
            format!(
                "config version {} is higher than supported version {}",
                from_version, CURRENT_VERSION
            ),
        )));
    }

    let mut current = from_version;
    while current < CURRENT_VERSION {
        let backup = config.clone();
        // v1→v2: future migration placeholder
        // 1 => migrate_v1_to_v2(config),
        let result: Result<()> = Ok(());

        if let Err(e) = result {
            tracing::error!("migration v{}→v{} failed: {}, rolling back", current, current + 1, e);
            *config = backup; // rollback
            return Err(Error::Ipc(IpcError::new(
                ErrorCode::ConfigMigrationFailed,
                format!("migration v{}→v{} failed: {}", current, current + 1, e),
            )));
        }
        current += 1;
    }

    // Update version field
    if let Some(version) = config.get_mut("version") {
        *version = serde_json::Value::from(CURRENT_VERSION);
    }

    Ok(())
}

/// Handle corrupt config file: backup as `config.json.corrupt.{timestamp}` (§17.4)
pub fn backup_corrupt_config(config_path: &std::path::Path) -> Result<PathBuf> {
    if !config_path.exists() {
        return Ok(PathBuf::new());
    }

    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let backup_name = format!("config.json.corrupt.{}", timestamp);
    let backup_path = config_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(backup_name);

    std::fs::copy(config_path, &backup_path).map_err(Error::Io)?;
    tracing::warn!("backed up corrupt config to {:?}", backup_path);
    Ok(backup_path)
}

/// Load config with migration support.
/// Handles: missing file → default, corrupt JSON → backup + default, version mismatch → migrate.
pub fn load_config_with_migration(
    config_path: &std::path::Path,
) -> Result<crate::config::Config> {
    use crate::config::Config;

    // File doesn't exist → create default
    if !config_path.exists() {
        tracing::info!("config file not found, creating default");
        return Ok(Config::default());
    }

    let content = std::fs::read_to_string(config_path).map_err(|e| {
        Error::Ipc(IpcError::new(
            ErrorCode::ConfigCorrupt,
            format!("read error: {}", e),
        ))
    })?;

    // Try parse as JSON Value first (to check version before full deserialize)
    let mut value: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            // JSON parse failed → backup corrupt file and return default (§17.4)
            tracing::error!("config JSON parse error: {}", e);
            let _ = backup_corrupt_config(config_path);
            return Ok(Config::default());
        }
    };

    // Check version
    let file_version = value
        .get("version")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or(1);

    if file_version > CURRENT_VERSION {
        return Err(Error::Ipc(IpcError::new(
            ErrorCode::ConfigMigrationFailed,
            format!(
                "config version {} > supported {}",
                file_version, CURRENT_VERSION
            ),
        )));
    }

    // Migrate if needed
    if file_version < CURRENT_VERSION {
        tracing::info!("migrating config from v{} to v{}", file_version, CURRENT_VERSION);
        migrate(&mut value, file_version)?;
    }

    // Deserialize final config
    let config: Config = serde_json::from_value(value).map_err(|e| {
        Error::Ipc(IpcError::new(
            ErrorCode::ConfigCorrupt,
            format!("deserialize error: {}", e),
        ))
    })?;

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migrate_same_version_noop() {
        let mut config = serde_json::json!({"version": 1, "servers": []});
        migrate(&mut config, 1).unwrap();
        assert_eq!(config["version"], 1);
    }

    #[test]
    fn test_migrate_version_too_high() {
        let mut config = serde_json::json!({"version": 99});
        let result = migrate(&mut config, 99);
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Ipc(ipc) => assert_eq!(ipc.code, ErrorCode::ConfigMigrationFailed),
            _ => panic!("expected ConfigMigrationFailed"),
        }
    }

    #[test]
    fn test_backup_corrupt_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{ invalid }").unwrap();

        let backup = backup_corrupt_config(&path).unwrap();
        assert!(backup.exists());
        assert!(backup.to_string_lossy().contains("corrupt"));
    }

    #[test]
    fn test_load_missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        let config = load_config_with_migration(&path).unwrap();
        assert_eq!(config.version, 1);
        assert!(config.servers.is_empty());
    }

    #[test]
    fn test_load_corrupt_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{ invalid json }").unwrap();

        let config = load_config_with_migration(&path).unwrap();
        assert_eq!(config.version, 1);
        // Corrupt file should have been backed up
        let entries = std::fs::read_dir(dir.path()).unwrap();
        let has_backup = entries
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains("corrupt"));
        assert!(has_backup);
    }
}
