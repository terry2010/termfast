// src-tauri/src/device_id_store.rs — Persistent device_id random suffix
//
// D9: device_id 防冲突
// 首次启动生成 4 位随机十六进制后缀，持久化存储在 app data 目录。
// device_id 格式改为 `hostname-username-xxxx`。
// 丢失恢复时后缀也重新生成（见 §6.7）。
//
// 存储位置：
//   macOS:   ~/Library/Application Support/termfast/device_id.json
//   Windows: %APPDATA%\termfast\device_id.json
//   Linux:   ~/.local/share/termfast/device_id.json

use std::fs;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

/// On-disk JSON structure for device_id suffix.
#[derive(Debug, Serialize, Deserialize)]
struct DeviceIdFile {
    suffix: String,
}

/// Returns the platform-appropriate data directory for termfast.
/// Matches the path used by `pairing_store::data_dir`.
fn data_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join("Library/Application Support/termfast")
    }
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(appdata).join("termfast")
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            PathBuf::from(xdg).join("termfast")
        } else {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".local/share/termfast")
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        PathBuf::from(".")
    }
}

fn device_id_path() -> PathBuf {
    data_dir().join("device_id.json")
}

/// Generate a 4-digit random hexadecimal suffix (e.g. "a3f7").
fn generate_suffix() -> String {
    // Use system random via std::time + simple hash for portability.
    // For security purposes, this only needs to be unique, not cryptographically random.
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // Mix with process ID for extra entropy
    let pid = std::process::id() as u128;
    let mixed = (nanos ^ (pid << 32)).wrapping_mul(0x517cc1b727220a95);
    let hex = format!("{:x}", mixed);
    // Take last 4 chars
    let len = hex.len();
    if len >= 4 {
        hex[len - 4..].to_string()
    } else {
        format!("{:0>4}", hex)
    }
}

/// Get the persisted device_id suffix, generating and persisting it on first call.
/// Returns the 4-digit hex suffix (e.g. "a3f7").
pub fn get_or_create_suffix() -> String {
    let path = device_id_path();
    get_or_create_suffix_at(&path)
}

/// Get or create suffix at a specific path (for testing with temp directories).
pub fn get_or_create_suffix_at(path: &std::path::Path) -> String {
    // Try to load existing suffix
    if let Ok(content) = fs::read_to_string(path) {
        if let Ok(f) = serde_json::from_str::<DeviceIdFile>(&content) {
            if is_valid_suffix(&f.suffix) {
                return f.suffix;
            }
        }
    }

    // Generate new suffix
    let suffix = generate_suffix();

    // Persist
    let file = DeviceIdFile {
        suffix: suffix.clone(),
    };
    if let Ok(json) = serde_json::to_string(&file) {
        // Ensure parent dir exists
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, json);
    }

    suffix
}

/// Regenerate the suffix (for lost-key recovery).
/// Overwrites the existing suffix with a new random one.
/// Used by D3 key recovery flow (will be called when device key is lost).
#[allow(dead_code)]
pub fn regenerate_suffix() -> String {
    let path = device_id_path();
    regenerate_suffix_at(&path)
}

/// Regenerate suffix at a specific path (for testing).
#[allow(dead_code)]
pub fn regenerate_suffix_at(path: &std::path::Path) -> String {
    let suffix = generate_suffix();
    let file = DeviceIdFile {
        suffix: suffix.clone(),
    };
    if let Ok(json) = serde_json::to_string(&file) {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, json);
    }
    suffix
}

/// Validate that a suffix is a 4-character hex string.
fn is_valid_suffix(s: &str) -> bool {
    s.len() == 4 && s.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_suffix_is_4_hex_chars() {
        let suffix = generate_suffix();
        assert_eq!(suffix.len(), 4);
        assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_generate_suffix_is_random() {
        let s1 = generate_suffix();
        // Sleep a tiny bit to ensure different timestamp
        std::thread::sleep(std::time::Duration::from_millis(1));
        let s2 = generate_suffix();
        // Extremely unlikely to be the same
        assert_ne!(s1, s2, "suffixes should differ: {} vs {}", s1, s2);
    }

    #[test]
    fn test_is_valid_suffix() {
        assert!(is_valid_suffix("a3f7"));
        assert!(is_valid_suffix("0000"));
        assert!(is_valid_suffix("ffff"));
        assert!(!is_valid_suffix("a3f"));
        assert!(!is_valid_suffix("a3f70"));
        assert!(!is_valid_suffix("g3f7"));
        assert!(!is_valid_suffix(""));
    }

    #[test]
    fn test_get_or_create_suffix_first_call_generates_and_persists() {
        let dir = std::env::temp_dir().join(format!(
            "termfast_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("device_id.json");

        // First call — should generate and persist
        let suffix1 = get_or_create_suffix_at(&path);
        assert!(is_valid_suffix(&suffix1));
        assert!(path.exists(), "device_id.json should be created");

        // Clean up
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_get_or_create_suffix_second_call_reads_same_suffix() {
        let dir = std::env::temp_dir().join(format!(
            "termfast_test_persist_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("device_id.json");

        // First call — generates
        let suffix1 = get_or_create_suffix_at(&path);

        // Second call — should read back the same suffix
        let suffix2 = get_or_create_suffix_at(&path);
        assert_eq!(suffix1, suffix2, "second call should return persisted suffix");

        // Clean up
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_get_or_create_suffix_corrupted_file_regenerates() {
        let dir = std::env::temp_dir().join(format!(
            "termfast_test_corrupt_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("device_id.json");

        // Write corrupted JSON
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "not valid json").unwrap();

        // Call — should regenerate (not crash, not return invalid)
        let suffix = get_or_create_suffix_at(&path);
        assert!(is_valid_suffix(&suffix), "should regenerate valid suffix");

        // Clean up
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_get_or_create_suffix_invalid_suffix_format_regenerates() {
        let dir = std::env::temp_dir().join(format!(
            "termfast_test_invalid_fmt_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("device_id.json");

        // Write valid JSON but invalid suffix format (too short)
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, r#"{"suffix":"abc"}"#).unwrap();

        // Call — should regenerate because "abc" is not 4 hex chars
        let suffix = get_or_create_suffix_at(&path);
        assert!(is_valid_suffix(&suffix), "should regenerate valid suffix");
        assert_ne!(suffix, "abc");

        // Clean up
        let _ = std::fs::remove_dir_all(&dir);
    }
}
