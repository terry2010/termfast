// src-tauri/src/pairing_store.rs — Persistent pairing key store
//
// Persists (pairing_id, pairing_key_hex, relay_url) for each paired phone so
// tunnels can be restored after desktop restart. Without this, the pairing_key
// (generated randomly and embedded in the QR code) is lost on restart and the
// desktop cannot re-establish tunnels for existing pairings — the phone would
// connect to the relay but find no desktop.
//
// Stored as JSON in the app data directory (same dir as config.json):
//   macOS:   ~/Library/Application Support/termfast/pairings.json
//   Windows: %APPDATA%\termfast\pairings.json
//   Linux:   ~/.local/share/termfast/pairings.json

use std::fs;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

/// One persisted pairing entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredPairing {
    pub pairing_id: String,
    pub pairing_key_hex: String,
    pub relay_url: String,
}

/// On-disk JSON structure.
#[derive(Debug, Default, Serialize, Deserialize)]
struct PairingsFile {
    pairings: Vec<StoredPairing>,
}

/// Returns the platform-appropriate data directory for termfast.
/// Matches the path used by `FileConfigStorage::default_path` in core.
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

fn pairings_path() -> PathBuf {
    data_dir().join("pairings.json")
}

/// Load all persisted pairings. Returns empty vec if file missing or invalid.
pub fn load() -> Vec<StoredPairing> {
    let path = pairings_path();
    match fs::read_to_string(&path) {
        Ok(content) => {
            match serde_json::from_str::<PairingsFile>(&content) {
                Ok(f) => f.pairings,
                Err(e) => {
                    tracing::warn!("pairings.json parse error: {}, ignoring", e);
                    Vec::new()
                }
            }
        }
        Err(_) => Vec::new(), // file missing — not an error
    }
}

/// Save or update a pairing entry (upsert by pairing_id).
pub fn save(pairing: StoredPairing) {
    let mut all = load();
    all.retain(|p| p.pairing_id != pairing.pairing_id);
    all.push(pairing);
    write_all(&all);
}

/// Remove a pairing entry by pairing_id.
pub fn remove(pairing_id: &str) {
    let mut all = load();
    all.retain(|p| p.pairing_id != pairing_id);
    write_all(&all);
}

fn write_all(pairings: &[StoredPairing]) {
    let path = pairings_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let file = PairingsFile { pairings: pairings.to_vec() };
    match serde_json::to_string_pretty(&file) {
        Ok(json) => {
            if let Err(e) = fs::write(&path, json) {
                tracing::warn!("failed to write pairings.json: {}", e);
            }
        }
        Err(e) => tracing::warn!("failed to serialize pairings: {}", e),
    }
}
