//! Config migration — FP-1.4
//!
//! Chain migration: v1→v2→v3... (§17.5)
//! Each step: backup → migrate → validate → rollback on failure.

use crate::error::{Error, ErrorCode, IpcError, Result};
use serde_json::Value;
use std::path::PathBuf;

/// Current config schema version supported by this software
pub const CURRENT_VERSION: u32 = 2;

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
        let result: Result<()> = match current {
            // v1→v2: upgrade reconnect config to new defaults
            1 => migrate_v1_to_v2(config),
            _ => Ok(()),
        };

        if let Err(e) = result {
            tracing::error!(
                "migration v{}→v{} failed: {}, rolling back",
                current,
                current + 1,
                e
            );
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

/// v1→v2: Upgrade all servers' reconnect config to new defaults.
/// Old defaults had heartbeat_interval=15-30, max_attempts=5-10.
/// New defaults: heartbeat_interval=10, max_attempts=999, max_backoff_secs=60.
fn migrate_v1_to_v2(config: &mut Value) -> Result<()> {
    if let Some(servers) = config.get_mut("servers").and_then(|s| s.as_array_mut()) {
        for server in servers.iter_mut() {
            if let Some(reconnect) = server.get_mut("reconnect") {
                // heartbeat_interval: old 15-30 → 10
                if let Some(hi) = reconnect.get("heartbeat_interval").and_then(|v| v.as_u64()) {
                    if hi > 10 {
                        reconnect["heartbeat_interval"] = serde_json::Value::from(10u64);
                    }
                }
                // max_attempts: old 5-10 → 999 (effectively unlimited)
                if let Some(ma) = reconnect.get("max_attempts").and_then(|v| v.as_u64()) {
                    if ma < 999 {
                        reconnect["max_attempts"] = serde_json::Value::from(999u64);
                    }
                }
                // max_backoff_secs: old 30-300 → 60
                if let Some(mb) = reconnect.get("max_backoff_secs").and_then(|v| v.as_u64()) {
                    if mb > 60 {
                        reconnect["max_backoff_secs"] = serde_json::Value::from(60u64);
                    }
                }
                // reconnect_timeout_secs: add if missing (default 24h = 86400)
                if reconnect.get("reconnect_timeout_secs").is_none() {
                    reconnect["reconnect_timeout_secs"] = serde_json::Value::from(86400u64);
                }
            }
        }
    }
    tracing::info!("migrated v1→v2: upgraded reconnect config for all servers");
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
/// Returns (Config, is_fallback). is_fallback=true means the config was not
/// loaded from a valid file (missing, corrupt, or migration failed) — the
/// caller should set the corrupt_load flag to prevent overwriting the
/// backed-up original.
pub fn load_config_with_migration_fallback(
    config_path: &std::path::Path,
) -> (crate::config::Config, bool) {
    match load_config_with_migration(config_path) {
        Ok(c) => {
            // Distinguish "corrupt JSON → replaced with Config::default()" from
            // "valid config that legitimately has 0 servers" by comparing the
            // loaded config against Config::default(). If the config is exactly
            // the default AND the file exists, it's likely a corrupt fallback
            // (the corrupt file was backed up and replaced with default).
            // A real config with 0 servers would have non-default general
            // settings (theme, language, etc.), so it won't match default.
            let file_exists = config_path.exists();
            let is_fallback = c == crate::config::Config::default() && file_exists;
            (c, is_fallback)
        }
        Err(e) => {
            tracing::error!(
                "load_config_with_migration failed: {} — starting with empty config",
                e
            );
            (crate::config::Config::default(), true)
        }
    }
}

/// Load config with migration support.
/// Handles: missing file → default, corrupt JSON → backup + default, version mismatch → migrate.
pub fn load_config_with_migration(config_path: &std::path::Path) -> Result<crate::config::Config> {
    use crate::config::Config;

    // File doesn't exist → create default
    if !config_path.exists() {
        // Check for .tmp recovery (crash during atomic write).
        // write_atomic uses `path.with_extension("tmp")` which replaces the
        // last extension: `config.json` → `config.tmp`.
        let tmp_path = config_path.with_extension("tmp");
        if tmp_path.exists() {
            tracing::warn!("config file missing but .tmp exists — attempting recovery");
            if let Err(e) = std::fs::rename(&tmp_path, config_path) {
                tracing::error!("failed to recover config from .tmp: {}", e);
            }
        }
    }

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
        tracing::info!(
            "migrating config from v{} to v{}",
            file_version,
            CURRENT_VERSION
        );
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
        let mut config = serde_json::json!({"version": 2, "servers": []});
        migrate(&mut config, 2).unwrap();
        assert_eq!(config["version"], 2);
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
        assert_eq!(config.version, 2);
        assert!(config.servers.is_empty());
    }

    #[test]
    fn test_load_corrupt_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{ invalid json }").unwrap();

        let config = load_config_with_migration(&path).unwrap();
        assert_eq!(config.version, 2);
        // Corrupt file should have been backed up
        let entries = std::fs::read_dir(dir.path()).unwrap();
        let has_backup = entries
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains("corrupt"));
        assert!(has_backup);
    }

    #[test]
    fn test_load_old_config_without_local_triggers() {
        // 模拟旧配置文件（没有 local_triggers 字段）——验证 serde(default) 生效
        // 用一个完整的真实 config.json（从 config.json.bak 格式简化）
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        // 读取真实备份文件
        let bak_path = "/Users/terry/Library/Application Support/termfast/config.json.bak";
        if !std::path::Path::new(bak_path).exists() {
            eprintln!("skipping test: config.json.bak not found");
            return;
        }
        let content = std::fs::read_to_string(bak_path).unwrap();
        std::fs::write(&path, &content).unwrap();

        let config = load_config_with_migration(&path).unwrap();
        assert!(!config.servers.is_empty(), "should load servers from old config");
        assert_eq!(config.local_triggers.len(), 0, "local_triggers should default to empty");
        assert_ne!(config, crate::config::Config::default(), "should not equal default (has servers)");
    }

    /// Bug 1 regression test: corrupt JSON → fallback → corrupt_load must be true.
    /// Previously ConfigManager::load used storage.load() which returns Ok(default)
    /// for corrupt JSON, causing corrupt_load=false and subsequent modify() to
    /// overwrite the backed-up original with an empty config.
    #[test]
    fn test_fallback_detects_corrupt_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{ invalid json }").unwrap();

        let (config, is_fallback) = load_config_with_migration_fallback(&path);
        assert!(is_fallback, "corrupt JSON must set is_fallback=true");
        assert!(config.servers.is_empty());
        // Corrupt file should have been backed up
        let entries = std::fs::read_dir(dir.path()).unwrap();
        let has_backup = entries
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains("corrupt"));
        assert!(has_backup, "corrupt file should be backed up");
    }

    /// Bug 1 regression test: valid config with 0 servers must NOT be flagged
    /// as fallback (would incorrectly block saving).
    #[test]
    fn test_fallback_does_not_flag_valid_empty_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        // A valid config with 0 servers but non-default general settings
        let config_json = serde_json::json!({
            "version": 2,
            "servers": [],
            "general": {
                "theme": "dark",
                "language": "en"
            }
        });
        std::fs::write(&path, serde_json::to_string_pretty(&config_json).unwrap()).unwrap();

        let (config, is_fallback) = load_config_with_migration_fallback(&path);
        assert!(!is_fallback, "valid config with non-default settings must not be flagged as fallback");
        assert!(config.servers.is_empty());
    }

    /// Bug 2 regression test: write_atomic creates `config.tmp` (not `config.json.tmp`).
    /// The recovery code must look for the correct tmp filename.
    #[test]
    fn test_tmp_recovery_uses_correct_extension() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        // Simulate a crash: write_atomic wrote config.tmp but rename didn't happen
        let tmp_path = path.with_extension("tmp"); // config.tmp — what write_atomic creates
        assert_eq!(tmp_path.file_name().unwrap(), "config.tmp");
        let config_json = serde_json::json!({
            "version": 2,
            "servers": [],
            "general": { "theme": "dark" }
        });
        std::fs::write(&tmp_path, serde_json::to_string_pretty(&config_json).unwrap()).unwrap();

        // config.json doesn't exist, but config.tmp does → recovery should kick in
        assert!(!path.exists());
        assert!(tmp_path.exists());

        let config = load_config_with_migration(&path).unwrap();
        // Recovery should have renamed config.tmp → config.json
        assert!(path.exists(), "config.json should exist after tmp recovery");
        assert!(!tmp_path.exists(), "config.tmp should be gone after recovery");
        assert_eq!(config.general.theme, "dark");
    }
}
