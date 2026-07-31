//! Config storage trait + file implementation — FP-1.3
//!
//! Platform-agnostic storage abstraction. File implementation uses JSON.
//! Platform-specific storage (keychain etc.) is in credential crate.

use super::config::Config;
use super::migration::load_config_with_migration;
use crate::error::{Error, ErrorCode, IpcError, Result};
use std::io::Write;
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
            std::fs::create_dir_all(parent).map_err(Error::Io)?;
        }
        Ok(())
    }
}

impl ConfigStorage for FileConfigStorage {
    fn load(&self) -> Result<Config> {
        // Delegate to the migration-aware loader which handles version
        // checks, chain migration, and corrupt file recovery.
        load_config_with_migration(&self.config_path)
    }

    fn save(&self, config: &Config) -> Result<()> {
        self.ensure_parent_dir()?;

        // Note: the "refuse to save empty config over non-empty" guard was
        // moved to ConfigManager::save() where it can distinguish user-initiated
        // modify() from automatic save() after a corrupt load.

        let json = serde_json::to_string_pretty(config)?;
        let json = json + "\n"; // trailing newline

        // Crash-safe atomic write: write to temp file, fsync, rename, fsync dir.
        // Without fsync, a crash after rename can lose data because the OS
        // may not have flushed page cache to disk yet.
        let tmp_path = self.config_path.with_extension("json.tmp");

        // 1. Write data to temp file
        let mut file = std::fs::File::create(&tmp_path).map_err(Error::Io)?;
        file.write_all(json.as_bytes()).map_err(Error::Io)?;

        // 2. fsync the temp file — ensures file content is on disk before rename
        file.sync_all().map_err(Error::Io)?;
        // Close the file handle before rename (Windows requires it)
        drop(file);

        // Set file permissions to 600 on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            let _ = std::fs::set_permissions(&tmp_path, perms);
        }

        // 3. Atomic rename
        std::fs::rename(&tmp_path, &self.config_path).map_err(|e| {
            // Clean up temp file on rename failure
            let _ = std::fs::remove_file(&tmp_path);
            Error::Io(e)
        })?;

        // 4. fsync the parent directory — ensures the rename is persisted
        //    Without this, a crash can leave the old file name pointing to
        //    stale data (the new content was fsync'd but the directory entry
        //    update was not).
        #[cfg(unix)]
        {
            if let Some(parent) = self.config_path.parent() {
                if let Ok(dir) = std::fs::File::open(parent) {
                    let _ = dir.sync_all();
                    // macOS: sync_all only flushes to disk cache, not to platter.
                    // F_FULLFSYNC forces a physical write to the storage device.
                    #[cfg(target_os = "macos")]
                    {
                        use std::os::fd::AsRawFd;
                        unsafe {
                            libc::fcntl(dir.as_raw_fd(), libc::F_FULLFSYNC);
                        }
                    }
                }
            }
        }

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
                let config: Config = serde_json::from_str(json).map_err(|e| {
                    Error::Ipc(IpcError::new(ErrorCode::ConfigCorrupt, e.to_string()))
                })?;
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
        assert_eq!(config.version, 2);
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
        // Corrupt JSON → backup file + return default config (not an error)
        let config = storage.load().unwrap();
        assert!(config.servers.is_empty());
        // Corrupt file should have been backed up
        let entries = std::fs::read_dir(dir.path()).unwrap();
        let has_backup = entries
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains("corrupt"));
        assert!(has_backup);
    }
}
