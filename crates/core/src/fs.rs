//! Crash-safe atomic file write utilities.
//!
//! All persistent data writes in TermFast should use `write_atomic` to ensure
//! crash safety: a power loss or process kill at any point leaves either the
//! old file intact or the new file fully written — never a truncated/corrupt
//! file.
//!
//! The sequence is:
//! 1. Write data to `<path>.tmp`
//! 2. `fsync` the temp file (content is on disk)
//! 3. Close the file handle (Windows requires this before rename)
//! 4. Set permissions (Unix: 0600 for sensitive files)
//! 5. `rename` temp → final (atomic on POSIX)
//! 6. `fsync` the parent directory (rename is persisted)
//!
//! Step 6 is often missed but is critical: without it, a crash after rename
//! can leave the directory entry pointing to the old inode (new content was
//! fsync'd but the directory update was not).

use std::io::Write;
use std::path::Path;

/// Write `data` to `path` atomically with fsync.
///
/// Creates parent directories if needed. Sets 0600 permissions on Unix
/// when `restrictive_perms` is true (use for credential/key files).
#[allow(unused_variables)]
pub fn write_atomic(path: &Path, data: &[u8], restrictive_perms: bool) -> std::io::Result<()> {
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let tmp_path = path.with_extension("tmp");

    // 1. Write data to temp file
    let mut file = std::fs::File::create(&tmp_path)?;
    file.write_all(data)?;

    // 2. fsync the temp file — ensures content is on disk before rename
    file.sync_all()?;
    // 3. Close the file handle before rename (Windows requires it)
    drop(file);

    // 4. Set permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if restrictive_perms { 0o600 } else { 0o644 };
        if let Err(e) = std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(mode)) {
            // Security: for sensitive files, permission failure is a real risk.
            // Log warning and clean up temp file.
            tracing::warn!("write_atomic: failed to set permissions on {:?}: {}", tmp_path, e);
            if restrictive_perms {
                let _ = std::fs::remove_file(&tmp_path);
                return Err(e);
            }
        }
    }

    // 5. Atomic rename
    if let Err(e) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }

    // 6. fsync the parent directory — ensures the rename is persisted
    #[cfg(unix)]
    {
        if let Some(parent) = path.parent() {
            if let Ok(dir) = std::fs::File::open(parent) {
                if let Err(e) = dir.sync_all() {
                    tracing::warn!("write_atomic: parent dir fsync failed for {:?}: {}", parent, e);
                }
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
