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
            if let Err(e) = std::fs::set_permissions(path, perms) {
                // Security: lock file contains PID + socket path — not secrets,
                // but failing to set 600 means other users on the host could
                // read it. Log a warning (don't fail) since the lock file is
                // still functional without restrictive permissions.
                tracing::warn!(
                    "failed to set 600 permissions on lock file {:?}: {}",
                    path,
                    e
                );
            }
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
            Ok(lock) => Self::is_daemon_process_alive(lock.pid, &default_process_checker()),
            Err(_) => false,
        }
    }

    /// Check if the PID in the lock file belongs to a live termfast process.
    ///
    /// On Windows, PIDs are aggressively reused, so a bare `is_pid_alive`
    /// check can return true for an unrelated process that happened to reuse
    /// the stale lock's PID.  We additionally verify the process image name
    /// contains "termfast" to avoid false positives that would permanently
    /// block daemon startup.
    ///
    /// Accepts a `ProcessChecker` trait so the decision logic can be unit
    /// tested with a fake checker (the Windows FFI path itself is not
    /// mockable, but the AND-combination of pid-alive + image-name-check is).
    fn is_daemon_process_alive(pid: u32, checker: &dyn ProcessChecker) -> bool {
        if !checker.is_pid_alive(pid) {
            return false;
        }
        #[cfg(not(unix))]
        {
            checker.process_image_contains(pid, "termfast")
        }
        #[cfg(unix)]
        {
            let _ = checker;
            true
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

// === SECTION 1 END ===

/// Abstraction over OS process queries used by `is_daemon_process_alive`.
///
/// Exists so the decision logic (PID alive AND image name matches) can be
/// unit tested without touching real OS APIs — the Windows FFI path calls
/// `kernel32!QueryFullProcessImageNameW` which cannot be mocked directly.
pub trait ProcessChecker {
    /// Returns true if `pid` identifies a currently running process.
    fn is_pid_alive(&self, pid: u32) -> bool;

    /// Returns true if the running process at `pid` has an image path that
    /// contains `needle` (case-insensitive).  On Unix this is unused.
    fn process_image_contains(&self, pid: u32, needle: &str) -> bool;
}

/// Default `ProcessChecker` that delegates to the real OS APIs.
struct DefaultProcessChecker;

impl ProcessChecker for DefaultProcessChecker {
    fn is_pid_alive(&self, pid: u32) -> bool {
        DaemonLock::is_pid_alive(pid)
    }

    #[cfg(not(unix))]
    fn process_image_contains(&self, pid: u32, needle: &str) -> bool {
        process_image_contains_windows(pid, needle)
    }

    #[cfg(unix)]
    fn process_image_contains(&self, _pid: u32, _needle: &str) -> bool {
        true
    }
}

fn default_process_checker() -> DefaultProcessChecker {
    DefaultProcessChecker
}

/// Windows-only: query the full image path of `pid` and check whether it
/// contains `needle` (case-insensitive).  Returns false if the PID doesn't
/// exist or can't be queried.
#[cfg(not(unix))]
fn process_image_contains_windows(pid: u32, needle: &str) -> bool {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn CloseHandle(h: *mut c_void) -> i32;
        fn QueryFullProcessImageNameW(
            handle: *mut c_void,
            flags: u32,
            lpexename: *mut u16,
            size: *mut u32,
        ) -> i32;
    }
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut buf = [0u16; 512];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size);
        CloseHandle(handle);
        if ok == 0 {
            return false;
        }
        let path = String::from_utf16_lossy(&buf[..size as usize]);
        path.to_lowercase().contains(needle)
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
            if DaemonLock::is_daemon_process_alive(lock.pid, &default_process_checker()) {
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

    // --- Regression tests for the Windows PID-reuse bug ---
    //
    // These use a fake `ProcessChecker` to verify the decision logic in
    // `is_daemon_process_alive` without touching real OS APIs.

    /// Fake checker that returns canned answers for testing.
    struct FakeChecker {
        pid_alive: bool,
        image_contains: bool,
    }
    impl ProcessChecker for FakeChecker {
        fn is_pid_alive(&self, _pid: u32) -> bool {
            self.pid_alive
        }
        fn process_image_contains(&self, _pid: u32, _needle: &str) -> bool {
            self.image_contains
        }
    }

    #[test]
    fn test_is_daemon_process_alive_pid_dead() {
        // PID not alive → false regardless of image check
        let checker = FakeChecker { pid_alive: false, image_contains: true };
        assert!(!DaemonLock::is_daemon_process_alive(999999, &checker));
    }

    #[test]
    fn test_is_daemon_process_alive_pid_alive_image_matches() {
        // PID alive + image contains "termfast" → true (normal happy path)
        let checker = FakeChecker { pid_alive: true, image_contains: true };
        assert!(DaemonLock::is_daemon_process_alive(50612, &checker));
    }

    #[cfg(not(unix))]
    #[test]
    fn test_is_daemon_process_alive_pid_reuse_by_non_termfast() {
        // Regression for the original bug: PID is alive (reused by another
        // program) but the image does not contain "termfast" → must return
        // false so the stale lock is treated as dead and overwritten.
        let checker = FakeChecker { pid_alive: true, image_contains: false };
        assert!(!DaemonLock::is_daemon_process_alive(50612, &checker));
    }

    #[cfg(unix)]
    #[test]
    fn test_is_daemon_process_alive_unix_no_image_check() {
        // On Unix, PID liveness alone is sufficient (no PID-reuse issue).
        let checker = FakeChecker { pid_alive: true, image_contains: false };
        assert!(DaemonLock::is_daemon_process_alive(50612, &checker));
    }
}
