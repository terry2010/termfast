//! Cloud sync pure logic — conflict detection and rollback detection.
//!
//! This module is NOT gated to `target_os = "android"` so that the pure
//! functions can be unit-tested on any platform (including the host machine
//! during development). The rest of cloud_sync.rs is android-only because
//! it depends on JNI state.

use termfast_cloud_sync::{sync_crypto, RemoteFileInfo};

/// Check for upload conflict (4 branches, design doc 6.1).
/// Returns Some(conflict_json) if conflict detected, None if safe.
///
/// - remote doesn't exist → None (safe, first upload)
/// - remote exists, hash matches local cache → None (safe, cloud unchanged)
/// - remote exists, hash differs from local cache → conflict (cloud_changed)
/// - remote exists, no local cache → conflict (cloud_exists_no_cache)
/// - remote exists but no hash from provider → None (can't detect, proceed)
pub fn check_upload_conflict(
    remote_info: &RemoteFileInfo,
    local_hash: Option<&str>,
) -> Option<serde_json::Value> {
    if !remote_info.exists {
        return None;
    }
    let remote_hash = remote_info.hash.as_deref();
    match (remote_hash, local_hash) {
        (Some(rh), Some(lh)) if rh == lh => None,
        (Some(_), Some(_)) => Some(serde_json::json!({
            "conflict": true,
            "reason": "cloud_changed",
            "message": "网盘文件被其他客户端改过，强行覆盖会丢失对方改动。",
        })),
        (Some(_), None) => Some(serde_json::json!({
            "conflict": true,
            "reason": "cloud_exists_no_cache",
            "message": "网盘已有数据文件，是否强行覆盖云端？",
        })),
        (None, _) => None,
    }
}

/// Check for rollback attack by comparing cloud updated_at with local last_updated_at.
/// Returns Some(rollback_json) if rollback detected, None if safe.
///
/// Design doc 6.2.1: if cloud updated_at < local last_updated_at, the cloud file
/// is older than what we last synced — likely a rollback attack.
///
/// Timestamps are parsed as RFC3339 DateTime for correct chronological comparison
/// (handles timezone offsets and fractional seconds that string comparison would
/// misorder).
pub fn check_rollback(
    payload: &sync_crypto::SyncPayload,
    last_updated_at: Option<&str>,
) -> Option<serde_json::Value> {
    let last = last_updated_at?;
    // Parse both timestamps as RFC3339 for correct chronological comparison.
    // Fall back to string comparison if either fails to parse (e.g. non-standard
    // format from older versions), preserving backward compatibility.
    let is_older = match (
        chrono::DateTime::parse_from_rfc3339(&payload.updated_at),
        chrono::DateTime::parse_from_rfc3339(last),
    ) {
        (Ok(cloud_dt), Ok(local_dt)) => cloud_dt < local_dt,
        _ => {
            tracing::warn!(
                "check_rollback: failed to parse timestamps as RFC3339, falling back to string compare: cloud={}, local={}",
                payload.updated_at, last
            );
            payload.updated_at.as_str() < last
        }
    };
    if is_older {
        Some(serde_json::json!({
            "ok": false,
            "reason": "rollback_detected",
            "message": "云端文件时间戳比上次同步更旧，可能是回滚攻击",
            "cloud_updated_at": payload.updated_at,
            "last_updated_at": last,
            "device_name": payload.device_name,
            "config": payload.config,
        }))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termfast_cloud_sync::RemoteFileInfo;
    use termfast_cloud_sync::sync_crypto::SyncPayload;

    fn make_remote(exists: bool, hash: Option<&str>) -> RemoteFileInfo {
        RemoteFileInfo {
            exists,
            size: Some(1024),
            hash: hash.map(|s| s.to_string()),
            modified: Some("2026-07-21T10:00:00Z".to_string()),
        }
    }

    fn make_payload(updated_at: &str, device: &str) -> SyncPayload {
        SyncPayload {
            config: serde_json::json!({}),
            device_name: device.to_string(),
            updated_at: updated_at.to_string(),
        }
    }

    // === 4-branch conflict detection ===

    #[test]
    fn test_conflict_remote_not_exists() {
        let remote = make_remote(false, None);
        let result = check_upload_conflict(&remote, None);
        assert!(result.is_none(), "remote not exists → safe");
    }

    #[test]
    fn test_conflict_hash_match() {
        let remote = make_remote(true, Some("abc123"));
        let result = check_upload_conflict(&remote, Some("abc123"));
        assert!(result.is_none(), "hash match → safe");
    }

    #[test]
    fn test_conflict_hash_mismatch() {
        let remote = make_remote(true, Some("abc123"));
        let result = check_upload_conflict(&remote, Some("different"));
        assert!(result.is_some());
        let v = result.unwrap();
        assert_eq!(v["conflict"], true);
        assert_eq!(v["reason"], "cloud_changed");
    }

    #[test]
    fn test_conflict_no_local_cache() {
        let remote = make_remote(true, Some("abc123"));
        let result = check_upload_conflict(&remote, None);
        assert!(result.is_some());
        let v = result.unwrap();
        assert_eq!(v["reason"], "cloud_exists_no_cache");
    }

    #[test]
    fn test_conflict_no_remote_hash() {
        let remote = make_remote(true, None);
        let result = check_upload_conflict(&remote, Some("local_hash"));
        assert!(result.is_none(), "no remote hash → can't detect, proceed");
    }

    // === Rollback detection ===

    #[test]
    fn test_rollback_detected() {
        let payload = make_payload("2026-07-20T10:00:00Z", "old-device");
        let last = Some("2026-07-21T10:00:00Z");
        let result = check_rollback(&payload, last);
        assert!(result.is_some());
        let v = result.unwrap();
        assert_eq!(v["reason"], "rollback_detected");
        assert_eq!(v["cloud_updated_at"], "2026-07-20T10:00:00Z");
        assert_eq!(v["last_updated_at"], "2026-07-21T10:00:00Z");
        assert_eq!(v["device_name"], "old-device");
    }

    #[test]
    fn test_rollback_not_detected_newer() {
        let payload = make_payload("2026-07-22T10:00:00Z", "new-device");
        let last = Some("2026-07-21T10:00:00Z");
        let result = check_rollback(&payload, last);
        assert!(result.is_none(), "cloud newer → safe");
    }

    #[test]
    fn test_rollback_equal_timestamp_safe() {
        let payload = make_payload("2026-07-21T10:00:00Z", "same-device");
        let last = Some("2026-07-21T10:00:00Z");
        let result = check_rollback(&payload, last);
        assert!(result.is_none(), "equal timestamp → safe (not strictly less)");
    }

    #[test]
    fn test_rollback_no_local_history() {
        let payload = make_payload("2020-01-01T00:00:00Z", "any-device");
        let result = check_rollback(&payload, None);
        assert!(result.is_none(), "no local history → skip rollback check");
    }
}

// === SECTION cloud_sync_logic END ===
