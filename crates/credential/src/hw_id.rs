//! Hardware ID retrieval for local credential binding.
//!
//! The hw_id is used as `salt_extra` in envelope encryption to bind
//! the local credential store to the current machine. If the file is
//! copied to another machine, the hw_id will differ and decryption
//! will fail (preventing data theft via disk cloning).
//!
//! Platform sources:
//! - macOS:   IOPlatformUUID (via ioread)
//! - Windows: MachineGuid (from registry)
//! - Linux:   /etc/machine-id
//! - Android: ANDROID_ID (Settings.Secure) — set via `set_hw_id()` from FFI
//!
//! Security properties:
//! - Steal data disk only → cannot get hw_id → cannot decrypt ✅
//! - Steal full disk → can get hw_id → but master_pw not on disk → still safe ✅
//! - Full disk + master_pw → can decrypt (unavoidable for any scheme)

use std::sync::OnceLock;

/// Process-global override for hw_id, set by FFI layers (e.g. Android JNI).
static HW_ID_OVERRIDE: OnceLock<String> = OnceLock::new();

/// Set the hardware ID from an external source (e.g. Android ANDROID_ID via JNI).
/// Must be called once during initialization, before any credential operations.
/// Subsequent calls are ignored (the first value wins).
pub fn set_hw_id(id: String) {
    let _ = HW_ID_OVERRIDE.set(id);
}

/// Get the hardware ID for the current platform.
/// Returns a string that uniquely identifies this machine.
///
/// Priority: FFI override (set_hw_id) > env var (TERMFAST_HW_ID) > platform native.
/// Falls back to a constant only if all methods fail (hardware binding weakened).
pub fn get_hw_id() -> String {
    // 1. Check FFI override (set by Android JNI layer)
    if let Some(id) = HW_ID_OVERRIDE.get() {
        return id.clone();
    }
    // 2. Check env var (for testing / standalone Rust code)
    if let Ok(id) = std::env::var("TERMFAST_HW_ID") {
        return id;
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(id) = get_macos_uuid() {
            return id;
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(id) = get_windows_machine_guid() {
            return id;
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(id) = get_linux_machine_id() {
            return id;
        }
    }
    // Fallback: constant (hardware binding disabled, but app still works)
    // This should only happen on Android if FFI layer didn't call set_hw_id.
    tracing::warn!("get_hw_id: falling back to constant — hardware binding disabled");
    "unknown-hw-id".to_string()
}

#[cfg(target_os = "macos")]
fn get_macos_uuid() -> Option<String> {
    let output = std::process::Command::new("ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Parse "IOPlatformUUID" = "XXXX-XXXX-..."
    for line in stdout.lines() {
        if line.contains("IOPlatformUUID") {
            if let Some(uuid) = line.split('"').nth(3) {
                return Some(uuid.to_string());
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn get_windows_machine_guid() -> Option<String> {
    let output = std::process::Command::new("reg")
        .args(["query", r"HKLM\SOFTWARE\Microsoft\Cryptography", "/v", "MachineGuid"])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains("MachineGuid") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(guid) = parts.last() {
                return Some(guid.to_string());
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn get_linux_machine_id() -> Option<String> {
    // Try /etc/machine-id first (systemd)
    if let Ok(id) = std::fs::read_to_string("/etc/machine-id") {
        let id = id.trim();
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }
    // Fall back to /var/lib/dbus/machine-id
    if let Ok(id) = std::fs::read_to_string("/var/lib/dbus/machine-id") {
        let id = id.trim();
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_hw_id_not_empty() {
        let id = get_hw_id();
        assert!(!id.is_empty(), "hw_id should not be empty");
    }

    #[test]
    fn test_get_hw_id_consistent() {
        let id1 = get_hw_id();
        let id2 = get_hw_id();
        assert_eq!(id1, id2, "hw_id should be consistent within a session");
    }
}
