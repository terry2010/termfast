//! Config storage trait + file implementation — FP-1.3
//!
//! Platform-agnostic storage abstraction. File implementation uses JSON.
//! Platform-specific storage (keychain etc.) is in credential crate.

use super::config::Config;
use super::migration::backup_corrupt_config;
use crate::error::{Error, ErrorCode, IpcError, Result};
use std::path::{Path, PathBuf};

/// Trait for config storage backends.
pub trait ConfigStorage: Send + Sync {
    fn load(&self) -> Result<Config>;
    fn save(&self, config: &Config) -> Result<()>;
    fn exists(&self) -> bool;
    fn backup(&self) -> Result<PathBuf>;
}

/// File-based config storage (JSON).
pub struct FileConfigStorage {
    config_path: PathBuf,
}

impl FileConfigStorage {
    /// Create with explicit path
    pub fn new(config_path: impl AsRef<Path>) -> Self {
        Self {
            config_path: config_path.as_ref().to_path_buf(),
        }
    }

    /// Create with platform default path
    pub fn default_path() -> Result<PathBuf> {
        let proj_dir = directories::ProjectDirs::from("", "", "termfast")
            .ok_or_else(|| Error::Config("cannot determine config directory".into()))?;
        let data_dir = proj_dir.data_dir();
        Ok(data_dir.join("config.json"))
    }

    /// Create with platform default path
    pub fn with_default_path() -> Result<Self> {
        let path = Self::default_path()?;
        Ok(Self::new(path))
    }

    /// Get the config file path
    pub fn path(&self) -> &Path {
        &self.config_path
    }

    /// Ensure parent directory exists
    fn ensure_parent_dir(&self) -> Result<()> {
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(Error::Io)?;
        }
        Ok(())
    }
}

impl ConfigStorage for FileConfigStorage {
    fn load(&self) -> Result<Config> {
        // If the main config file doesn't exist but a .tmp file does, it means
        // a crash happened during atomic write (after write(tmp), before rename).
        // Try to recover by renaming .tmp → config.json first.
        if !self.config_path.exists() {
            let tmp_path = self.config_path.with_extension("json.tmp");
            if tmp_path.exists() {
                tracing::warn!("config file missing but .tmp exists — attempting recovery");
                if let Err(e) = std::fs::rename(&tmp_path, &self.config_path) {
                    tracing::error!("failed to recover config from .tmp: {}", e);
                }
            }
        }

        if !self.config_path.exists() {
            tracing::info!("config file not found, using default config");
            return Ok(Config::default());
        }

        let content = std::fs::read_to_string(&self.config_path).map_err(|e| {
            Error::Ipc(IpcError::new(
                ErrorCode::ConfigCorrupt,
                format!("failed to read config: {}", e),
            ))
        })?;

        let config: Config = serde_json::from_str(&content).map_err(|e| {
            tracing::error!("config parse error: {}", e);
            // Back up the corrupt file before returning the error so the
            // user's data is not lost when a caller falls back to defaults
            // and then saves (which would overwrite the original file).
            let _ = backup_corrupt_config(&self.config_path);
            Error::Ipc(IpcError::new(
                ErrorCode::ConfigCorrupt,
                format!("config JSON parse error: {}", e),
            ))
        })?;

        Ok(config)
    }

    fn save(&self, config: &Config) -> Result<()> {
        self.ensure_parent_dir()?;

        // Safety guard: if the new config has zero servers but the existing
        // file on disk has servers, refuse to overwrite. This prevents a
        // corrupt-load → empty-config → save cycle from wiping all user data
        // after a crash. The user can still explicitly delete servers one by
        // one (which goes through modify() with a non-empty starting config).
        if config.servers.is_empty() && self.config_path.exists() {
            if let Ok(existing) = std::fs::read_to_string(&self.config_path) {
                if let Ok(old) = serde_json::from_str::<Config>(&existing) {
                    if !old.servers.is_empty() {
                        tracing::error!(
                            "refusing to save empty config over non-empty config ({} servers on disk) \
                             — likely a corrupt-load fallback. Save skipped to prevent data loss.",
                            old.servers.len()
                        );
                        // Back up the on-disk config so the user can recover
                        let _ = backup_corrupt_config(&self.config_path);
                        return Err(Error::Config(
                            "refusing to overwrite non-empty config with empty config — \
                             possible corrupt-load fallback. Original config backed up.".into(),
                        ));
                    }
                }
            }
        }

        let json = serde_json::to_string_pretty(config)?;
        let json = json + "\n"; // trailing newline

        // Write atomically: write to temp file then rename
        let tmp_path = self.config_path.with_extension("json.tmp");
        std::fs::write(&tmp_path, &json).map_err(Error::Io)?;

        // Set file permissions to 600 on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            let _ = std::fs::set_permissions(&tmp_path, perms);
        }

        std::fs::rename(&tmp_path, &self.config_path).map_err(|e| {
            // Clean up temp file on rename failure
            let _ = std::fs::remove_file(&tmp_path);
            Error::Io(e)
        })?;

        Ok(())
    }

    fn exists(&self) -> bool {
        self.config_path.exists()
    }

    fn backup(&self) -> Result<PathBuf> {
        if !self.config_path.exists() {
            return Err(Error::Config("config file does not exist".into()));
        }

        let backup_path = self.config_path.with_extension("json.bak");
        std::fs::copy(&self.config_path, &backup_path).map_err(Error::Io)?;
        Ok(backup_path)
    }
}

/// In-memory config storage (for testing).
pub struct InMemoryConfigStorage {
    data: std::sync::Mutex<Option<String>>,
}

impl InMemoryConfigStorage {
    pub fn new() -> Self {
        Self {
            data: std::sync::Mutex::new(None),
        }
    }
}

impl Default for InMemoryConfigStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigStorage for InMemoryConfigStorage {
    fn load(&self) -> Result<Config> {
        let data = self.data.lock().unwrap();
        match data.as_ref() {
            Some(json) => {
                let config: Config = serde_json::from_str(json)
                    .map_err(|e| Error::Ipc(IpcError::new(ErrorCode::ConfigCorrupt, e.to_string())))?;
                Ok(config)
            }
            None => Err(Error::Ipc(IpcError::new(
                ErrorCode::Internal,
                "no config in memory",
            ))),
        }
    }

    fn save(&self, config: &Config) -> Result<()> {
        let json = serde_json::to_string_pretty(config)
            .map_err(|e| Error::Ipc(IpcError::new(ErrorCode::Internal, e.to_string())))?;
        *self.data.lock().unwrap() = Some(json);
        Ok(())
    }

    fn exists(&self) -> bool {
        self.data.lock().unwrap().is_some()
    }

    fn backup(&self) -> Result<PathBuf> {
        // No-op for in-memory storage
        Ok(PathBuf::from("/dev/null"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_file_config_storage_save_load_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.json");
        let storage = FileConfigStorage::new(&path);

        let config = Config::default();
        storage.save(&config).unwrap();
        assert!(path.exists());

        let loaded = storage.load().unwrap();
        assert_eq!(loaded.version, config.version);
        assert_eq!(loaded.servers.len(), config.servers.len());
        assert_eq!(
            loaded.trigger_templates.len(),
            config.trigger_templates.len()
        );
    }

    #[test]
    fn test_file_config_storage_load_missing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        let storage = FileConfigStorage::new(&path);

        let config = storage.load().unwrap();
        assert_eq!(config.version, 1);
        assert!(config.servers.is_empty());
    }

    #[test]
    fn test_file_config_storage_exists() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.json");
        let storage = FileConfigStorage::new(&path);

        assert!(!storage.exists());
        storage.save(&Config::default()).unwrap();
        assert!(storage.exists());
    }

    #[test]
    fn test_file_config_storage_backup() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.json");
        let storage = FileConfigStorage::new(&path);

        storage.save(&Config::default()).unwrap();
        let backup_path = storage.backup().unwrap();
        assert!(backup_path.exists());
    }

    #[test]
    fn test_file_config_storage_corrupt_json() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{ invalid json }").unwrap();

        let storage = FileConfigStorage::new(&path);
        let result = storage.load();
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            Error::Ipc(ipc) => assert_eq!(ipc.code, ErrorCode::ConfigCorrupt),
            _ => panic!("expected IpcError with ConfigCorrupt"),
        }
    }
}
