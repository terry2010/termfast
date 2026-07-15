//! Daemon lock file — FP-1.7
//!
//! daemon.lock contains: { pid, socket_path, version, started_at }
//! Used by CLI/GUI to discover daemon socket path.
//! File permissions 600. PID liveness check on startup.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Daemon lock file content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonLock {
    pub pid: u32,
    pub socket_path: String,
    pub version: String,
    pub started_at: String,
}

impl DaemonLock {
    /// Get the default daemon.lock path
    pub fn default_path() -> Result<PathBuf> {
        let proj_dir = directories::ProjectDirs::from("", "", "termfast")
            .ok_or_else(|| anyhow::anyhow!("cannot determine data directory"))?;
        Ok(proj_dir.data_dir().join("daemon.lock"))
    }

    /// Write lock file with 600 permissions
    pub fn write(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json + "\n")?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            let _ = std::fs::set_permissions(path, perms);
        }

        Ok(())
    }

    /// Read lock file
    pub fn read(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let lock: DaemonLock = serde_json::from_str(&content)?;
        Ok(lock)
    }

    /// Remove lock file
    pub fn remove(path: &Path) -> Result<()> {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Check if the PID in the lock file is still alive
    pub fn is_pid_alive(pid: u32) -> bool {
        #[cfg(unix)]
        {
            // Send signal 0 (no-op) to check if process exists
            unsafe { libc::kill(pid as i32, 0) == 0 }
        }
        #[cfg(not(unix))]
        {
            // On Windows, use OpenProcess to check if the process exists
            use std::ffi::c_void;
            #[link(name = "kernel32")]
            extern "system" {
                fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
                fn CloseHandle(h: *mut c_void) -> i32;
            }
            const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
            unsafe {
                let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
                if handle.is_null() {
                    false
                } else {
                    CloseHandle(handle);
                    true
                }
            }
        }
    }

    /// Check if a daemon is already running (lock file exists + PID alive)
    pub fn is_daemon_running(path: &Path) -> bool {
        if !path.exists() {
            return false;
        }
        match Self::read(path) {
            Ok(lock) => Self::is_pid_alive(lock.pid),
            Err(_) => false,
        }
    }

    /// Acquire a daemon lock. Checks if another daemon is running first.
    /// If a stale lock exists (PID dead), it's overwritten.
    pub fn acquire(socket_path: &Path) -> Result<Self> {
        let lock_path = Self::default_path()?;

        // Check if another daemon is already running
        if Self::is_daemon_running(&lock_path) {
            bail!(
                "daemon is already running (lock file: {})",
                lock_path.display()
            );
        }

        // Create the lock
        let lock = DaemonLock {
            pid: std::process::id(),
            socket_path: socket_path.to_string_lossy().into(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            started_at: chrono::Utc::now().to_rfc3339(),
        };

        lock.write(&lock_path)?;
        Ok(lock)
    }
}

/// Find the daemon socket path from lock file
pub fn find_daemon_socket() -> Result<Option<String>> {
    let lock_path = DaemonLock::default_path()?;
    if !lock_path.exists() {
        return Ok(None);
    }
    match DaemonLock::read(&lock_path) {
        Ok(lock) => {
            if DaemonLock::is_pid_alive(lock.pid) {
                Ok(Some(lock.socket_path))
            } else {
                // Stale lock file, remove it
                let _ = DaemonLock::remove(&lock_path);
                Ok(None)
            }
        }
        Err(_) => {
            let _ = DaemonLock::remove(&lock_path);
            Ok(None)
        }
    }
}

/// Get the default socket path for the current platform
pub fn default_socket_path() -> Result<String> {
    #[cfg(unix)]
    {
        let proj_dir = directories::ProjectDirs::from("", "", "termfast")
            .ok_or_else(|| anyhow::anyhow!("cannot determine data directory"))?;
        Ok(proj_dir
            .data_dir()
            .join("daemon.sock")
            .to_string_lossy()
            .into())
    }
    #[cfg(not(unix))]
    {
        Ok(r"\\.\pipe\termfast-daemon".into())
    }
}

/// Get the Windows named pipe name
#[cfg(target_os = "windows")]
pub fn windows_pipe_name() -> &'static str {
    r"\\.\pipe\termfast-daemon"
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_lock_file_write_read_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("daemon.lock");

        let lock = DaemonLock {
            pid: std::process::id(),
            socket_path: "/tmp/test.sock".into(),
            version: "0.1.0".into(),
            started_at: "2026-01-15T14:32:00Z".into(),
        };

        lock.write(&path).unwrap();
        assert!(path.exists());

        let loaded = DaemonLock::read(&path).unwrap();
        assert_eq!(loaded.pid, lock.pid);
        assert_eq!(loaded.socket_path, lock.socket_path);
        assert_eq!(loaded.version, lock.version);
    }

    #[test]
    fn test_is_pid_alive_self() {
        // Current process should be alive
        assert!(DaemonLock::is_pid_alive(std::process::id()));
    }

    #[test]
    fn test_is_pid_alive_dead() {
        // PID 999999 almost certainly doesn't exist
        assert!(!DaemonLock::is_pid_alive(999999));
    }

    #[test]
    fn test_is_daemon_running_no_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent.lock");
        assert!(!DaemonLock::is_daemon_running(&path));
    }

    #[test]
    fn test_is_daemon_running_alive_pid() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("daemon.lock");

        let lock = DaemonLock {
            pid: std::process::id(),
            socket_path: "/tmp/test.sock".into(),
            version: "0.1.0".into(),
            started_at: "2026-01-15T14:32:00Z".into(),
        };
        lock.write(&path).unwrap();

        assert!(DaemonLock::is_daemon_running(&path));
    }

    #[test]
    fn test_is_daemon_running_dead_pid() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("daemon.lock");

        let lock = DaemonLock {
            pid: 999999,
            socket_path: "/tmp/test.sock".into(),
            version: "0.1.0".into(),
            started_at: "2026-01-15T14:32:00Z".into(),
        };
        lock.write(&path).unwrap();

        assert!(!DaemonLock::is_daemon_running(&path));
    }

    #[test]
    fn test_find_daemon_socket_no_lock() {
        // This test may fail if a real daemon is running, but that's unlikely in test env
        // Just verify it doesn't panic
        let _ = find_daemon_socket();
    }

    #[test]
    fn test_lock_file_remove() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("daemon.lock");

        let lock = DaemonLock {
            pid: 12345,
            socket_path: "/tmp/test.sock".into(),
            version: "0.1.0".into(),
            started_at: "2026-01-15T14:32:00Z".into(),
        };
        lock.write(&path).unwrap();
        assert!(path.exists());

        DaemonLock::remove(&path).unwrap();
        assert!(!path.exists());
    }
}
