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
    /// JWT for relay auth. For mobile pairing: user JWT. For desktop pairing (client): pairing JWT.
    #[serde(default)]
    pub jwt: String,
    /// "mobile" or "desktop"
    #[serde(default = "default_pairing_type")]
    pub pairing_type: String,
    /// Peer desktop name (for desktop pairings)
    #[serde(default)]
    pub peer_name: String,
    /// "server" (this desktop is server B) or "client" (this desktop is client A)
    #[serde(default)]
    pub peer_role: String,
}

fn default_pairing_type() -> String {
    "mobile".to_string()
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
/// Prefers SQLCipher DB if the global storage singleton is initialized.
///
/// **Self-healing migration**: if SQLCipher DB returns 0 pairings but
/// pairings.json exists with data (e.g. DB was recreated after data loss),
/// the file-based pairings are migrated into the DB so future loads read
/// from DB. This handles the scenario where the DB file was deleted or
/// recreated empty while pairings.json still had valid data.
pub fn load() -> Vec<StoredPairing> {
    // Try SQLCipher first
    if let Some(storage) = crate::storage_singleton::get_storage() {
        let from_db = load_from_storage(storage);
        if !from_db.is_empty() {
            return from_db;
        }
        // DB is empty — check if pairings.json has data to migrate
        let from_file = load_from_file();
        if !from_file.is_empty() {
            tracing::info!(
                "pairing_store: DB empty but pairings.json has {} entry(s) — migrating to DB",
                from_file.len()
            );
            for p in &from_file {
                save_to_storage(storage, p);
            }
            tracing::info!("pairing_store: migration complete, {} pairings imported to DB", from_file.len());
        }
        return from_file;
    }
    // Fallback to file-based storage
    load_from_file()
}

/// Load pairings from the JSON file (pairings.json).
fn load_from_file() -> Vec<StoredPairing> {
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

/// Load pairings from SqlCipherStorage.
fn load_from_storage(
    storage: &std::sync::Arc<termfast_core::config::SqlCipherStorage>,
) -> Vec<StoredPairing> {
    match storage.list_pairings() {
        Ok(rows) => {
            let mut result = Vec::new();
            for (id, data) in rows {
                match serde_json::from_str::<StoredPairing>(&data) {
                    Ok(p) => result.push(p),
                    Err(e) => tracing::warn!("failed to deserialize pairing {}: {}", id, e),
                }
            }
            result
        }
        Err(e) => {
            tracing::warn!("failed to load pairings from DB: {}", e);
            Vec::new()
        }
    }
}

/// Save or update a pairing entry (upsert by pairing_id).
pub fn save(pairing: StoredPairing) {
    // Try SQLCipher first
    if let Some(storage) = crate::storage_singleton::get_storage() {
        save_to_storage(storage, &pairing);
        return;
    }
    // Fallback to file-based storage
    let mut all = load();
    all.retain(|p| p.pairing_id != pairing.pairing_id);
    all.push(pairing);
    write_all(&all);
}

/// Save pairing to SqlCipherStorage.
fn save_to_storage(
    storage: &std::sync::Arc<termfast_core::config::SqlCipherStorage>,
    pairing: &StoredPairing,
) {
    match serde_json::to_string(pairing) {
        Ok(json) => {
            if let Err(e) = storage.upsert_pairing(&pairing.pairing_id, &json) {
                tracing::warn!("failed to save pairing to DB: {}", e);
            }
        }
        Err(e) => tracing::warn!("failed to serialize pairing: {}", e),
    }
}

/// Remove a pairing entry by pairing_id.
pub fn remove(pairing_id: &str) {
    // Try SQLCipher first
    if let Some(storage) = crate::storage_singleton::get_storage() {
        if let Err(e) = storage.delete_pairing(pairing_id) {
            tracing::warn!("failed to delete pairing from DB: {}", e);
        }
        return;
    }
    // Fallback to file-based storage
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Test StoredPairing with all new fields serializes correctly
    #[test]
    fn test_stored_pairing_full_serialization() {
        let pairing = StoredPairing {
            pairing_id: "dpair-123".to_string(),
            pairing_key_hex: "ab".repeat(32),
            relay_url: "wss://relay.example.com/tunnel".to_string(),
            jwt: "jwt-token".to_string(),
            pairing_type: "desktop".to_string(),
            peer_name: "Desktop-B".to_string(),
            peer_role: "server".to_string(),
        };
        let json = serde_json::to_string(&pairing).unwrap();
        let decoded: StoredPairing = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.pairing_id, "dpair-123");
        assert_eq!(decoded.pairing_key_hex, "ab".repeat(32));
        assert_eq!(decoded.relay_url, "wss://relay.example.com/tunnel");
        assert_eq!(decoded.jwt, "jwt-token");
        assert_eq!(decoded.pairing_type, "desktop");
        assert_eq!(decoded.peer_name, "Desktop-B");
        assert_eq!(decoded.peer_role, "server");
    }

    /// Test backward compatibility: old JSON without new fields deserializes with defaults
    #[test]
    fn test_stored_pairing_backward_compatibility() {
        let old_json = format!(
            r#"{{
            "pairing_id": "pair-old",
            "pairing_key_hex": "{}",
            "relay_url": "wss://old.relay.com/tunnel"
        }}"#,
            "cd".repeat(32)
        );
        let decoded: StoredPairing = serde_json::from_str(&old_json).unwrap();
        assert_eq!(decoded.pairing_id, "pair-old");
        assert_eq!(decoded.relay_url, "wss://old.relay.com/tunnel");
        // New fields should have defaults
        assert_eq!(decoded.jwt, "");
        assert_eq!(decoded.pairing_type, "mobile");
        assert_eq!(decoded.peer_name, "");
        assert_eq!(decoded.peer_role, "");
    }

    /// Test PairingsFile serialization with desktop and mobile pairings
    #[test]
    fn test_pairings_file_mixed_types() {
        let file = PairingsFile {
            pairings: vec![
                StoredPairing {
                    pairing_id: "mobile-1".to_string(),
                    pairing_key_hex: "ab".repeat(32),
                    relay_url: "wss://relay.com/tunnel".to_string(),
                    jwt: "user-jwt".to_string(),
                    pairing_type: "mobile".to_string(),
                    peer_name: "".to_string(),
                    peer_role: "".to_string(),
                },
                StoredPairing {
                    pairing_id: "desktop-1".to_string(),
                    pairing_key_hex: "cd".repeat(32),
                    relay_url: "wss://relay.com/tunnel".to_string(),
                    jwt: "pairing-jwt".to_string(),
                    pairing_type: "desktop".to_string(),
                    peer_name: "Desktop-A".to_string(),
                    peer_role: "client".to_string(),
                },
            ],
        };
        let json = serde_json::to_string(&file).unwrap();
        let decoded: PairingsFile = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.pairings.len(), 2);
        assert_eq!(decoded.pairings[0].pairing_type, "mobile");
        assert_eq!(decoded.pairings[1].pairing_type, "desktop");
        assert_eq!(decoded.pairings[1].peer_role, "client");
    }

    /// Test default_pairing_type function
    #[test]
    fn test_default_pairing_type() {
        assert_eq!(default_pairing_type(), "mobile");
    }
}
