//! Android cloud sync — bridges Kotlin JNI calls to `termfast-cloud-sync`.
//!
//! Mirrors the daemon handler's cloud sync logic but adapted for the FFI
//! environment: no IPC, no `directories::BaseDirs` (uses the app's data dir
//! passed from Kotlin), and OAuth uses a server-side relay callback
//! (`cloud-sync-callback.php` → `termfast://oauth/callback`) instead of a
//! localhost HTTP server.
//!
//! All async work runs on the shared tokio runtime (`crate::runtime::runtime()`).
//! JNI functions block on the runtime to return synchronous results to Kotlin.

#![cfg(target_os = "android")]

use std::path::PathBuf;
use std::sync::OnceLock;
use termfast_cloud_sync::{
    sync_crypto, sync_state, token_store, CloudProvider, CloudProviderTrait, OAuthToken,
    SYNC_FILE_PATH,
};
use termfast_cloud_sync::token_store::StoredToken;
use termfast_core::migration::FullExportData;
use termfast_credential::CredentialStore;

// Re-export pure logic functions (testable on any platform via cloud_sync_logic module)
pub use crate::cloud_sync_logic::{check_upload_conflict, check_rollback};

/// Mobile OAuth redirect URI — the server-side relay script.
/// The provider redirects here with ?code=xxx&state=xxx, then the script
/// responds with an HTML page that redirects the browser to
/// `termfast://oauth/callback?code=xxx&state=xxx`, which Android catches
/// via a deep-link intent filter.
pub const MOBILE_REDIRECT_URI: &str =
    "https://termfast.xisj.com/tools/cloud-sync-callback.php";

/// Pending OAuth state — stored between `nativeCloudSyncAuthUrl` (which
/// generates the code_verifier) and `nativeCloudSyncExchangeCode` (which
/// consumes it). Only one flow can be in progress at a time.
static PENDING_AUTH: OnceLock<std::sync::Mutex<PendingAuth>> = OnceLock::new();

#[derive(Default, Clone)]
struct PendingAuth {
    /// PKCE code_verifier (Dropbox only; empty for Baidu)
    code_verifier: String,
    /// OAuth state (Baidu only; empty for Dropbox)
    state: String,
    /// Provider name ("dropbox" or "baidu")
    provider: String,
}

fn pending_auth() -> &'static std::sync::Mutex<PendingAuth> {
    PENDING_AUTH.get_or_init(|| std::sync::Mutex::new(PendingAuth::default()))
}

/// Get the app's data directory (set by `nativeSetDataDir`).
fn data_dir() -> PathBuf {
    let st = crate::jni::state().lock().unwrap();
    PathBuf::from(&st.data_dir)
}

/// Path to the cloud token store (plaintext JSON, 0600).
pub fn token_file_path() -> PathBuf {
    data_dir().join("cloud_tokens.json")
}

/// Path to the encrypted sync state file (TFSS format).
pub fn sync_state_path() -> PathBuf {
    data_dir().join("sync_state.enc")
}

/// Path to the local config.json file.
fn local_config_path() -> PathBuf {
    data_dir().join("config.json")
}

/// Get the mtime (unix epoch seconds) of the local config.json file.
/// Returns None if the file doesn't exist or mtime can't be read.
fn local_config_mtime() -> Option<String> {
    let path = local_config_path();
    let meta = std::fs::metadata(&path).ok()?;
    let mtime = meta.modified().ok()?;
    let secs = mtime
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(secs.to_string())
}

/// Build a provider instance from the provider type string.
fn build_provider(
    provider: &str,
) -> Result<Box<dyn CloudProviderTrait>, String> {
    match provider {
        "dropbox" => Ok(Box::new(
            termfast_cloud_sync::dropbox::DropboxProvider::new(),
        )),
        "baidu" => Ok(Box::new(
            termfast_cloud_sync::baidu::BaiduProvider::new(),
        )),
        _ => Err(format!("unknown provider: {}", provider)),
    }
}

/// Get the sync file path on cloud storage (default or custom).
fn sync_path_from_params(params: &serde_json::Value) -> String {
    params["sync_path"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| SYNC_FILE_PATH.to_string())
}

// === SECTION cloud_sync_1 END ===

/// Generate the OAuth authorization URL for the user to open in a browser.
/// Stores the PKCE code_verifier (Dropbox) and state (Baidu) in a global
/// pending-auth slot for later use by `exchange_code`.
///
/// Returns a JSON string: `{"auth_url":"...","provider":"..."}`
pub fn auth_url(provider: &str) -> Result<String, String> {
    let p = build_provider(provider)?;
    let (proxy_url, code_verifier) = p.auth_url(MOBILE_REDIRECT_URI);

    // Fetch the real auth URL from the proxy server (synchronous blocking)
    let rt = crate::runtime::runtime();
    let result = rt.block_on(async {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| format!("http client: {}", e))?;
        let resp = client
            .get(&proxy_url)
            .send()
            .await
            .map_err(|e| format!("proxy request: {}", e))?;
        let json: serde_json::Value =
            resp.json().await.map_err(|e| format!("proxy parse: {}", e))?;
        let real_url = json["auth_url"]
            .as_str()
            .ok_or("proxy returned no auth_url")?
            .to_string();
        let state = json["state"].as_str().unwrap_or("").to_string();
        Ok::<_, String>((real_url, state))
    })?;

    let (real_url, state) = result;

    // Store pending auth state
    {
        let mut pa = pending_auth().lock().unwrap();
        pa.code_verifier = code_verifier.unwrap_or_default();
        pa.state = state;
        pa.provider = provider.to_string();
    }

    Ok(serde_json::json!({
        "auth_url": real_url,
        "provider": provider,
    })
    .to_string())
}

/// Exchange an OAuth authorization code for a token.
/// Uses the stored code_verifier / state from the pending auth slot.
///
/// `code` comes from the deep-link callback (`termfast://oauth/callback?code=...`).
/// Returns a JSON string with the token fields, or an error string.
pub fn exchange_code(code: &str) -> Result<String, String> {
    let (provider, code_verifier, state) = {
        let pa = pending_auth().lock().unwrap();
        (pa.provider.clone(), pa.code_verifier.clone(), pa.state.clone())
    };
    if provider.is_empty() {
        return Err("no pending OAuth flow".into());
    }

    let p = build_provider(&provider)?;
    let rt = crate::runtime::runtime();
    let token = rt.block_on(async {
        p.exchange_code(code, &code_verifier, MOBILE_REDIRECT_URI, &state)
            .await
            .map_err(|e| format!("OAuth exchange: {}", e))
    })?;

    // Clear pending auth
    {
        let mut pa = pending_auth().lock().unwrap();
        *pa = PendingAuth::default();
    }

    Ok(serde_json::json!({
        "access_token": token.access_token,
        "refresh_token": token.refresh_token,
        "expires_at": token.expires_at,
        "token_type": token.token_type,
        "provider": provider,
    })
    .to_string())
}

/// Save a token to the token store (called after successful exchange).
/// `token_json` is the JSON returned by `exchange_code`.
pub fn save_token(token_json: &str) -> Result<(), String> {
    let token_data: serde_json::Value =
        serde_json::from_str(token_json).map_err(|e| format!("parse token json: {}", e))?;
    let provider = token_data["provider"]
        .as_str()
        .ok_or("missing provider in token json")?;
    let access_token = token_data["access_token"]
        .as_str()
        .ok_or("missing access_token")?;

    let provider_type: CloudProvider = provider
        .parse::<CloudProvider>()
        .map_err(|e| e.to_string())?;

    let token = OAuthToken {
        access_token: access_token.to_string(),
        refresh_token: token_data["refresh_token"].as_str().map(|s| s.to_string()),
        expires_at: token_data["expires_at"].as_i64(),
        token_type: token_data["token_type"]
            .as_str()
            .unwrap_or("bearer")
            .to_string(),
    };

    let path = token_file_path();
    let mut data = if token_store::token_file_exists(&path) {
        token_store::load_tokens(&path).unwrap_or_default()
    } else {
        token_store::TokenStoreData::default()
    };
    data.tokens.insert(
        provider.to_string(),
        StoredToken {
            provider: provider_type,
            token,
            stored_at: chrono::Utc::now().timestamp(),
        },
    );
    token_store::save_tokens(&path, &data).map_err(|e| format!("save token: {}", e))?;
    Ok(())
}

/// Check if a provider is authenticated (has a stored token).
/// Returns JSON: `{"authenticated":true,"expires_at":...}` or `{"authenticated":false}`.
pub fn load_token(provider: &str) -> Result<String, String> {
    let path = token_file_path();
    if !token_store::token_file_exists(&path) {
        return Ok(r#"{"authenticated":false}"#.to_string());
    }
    let data = token_store::load_tokens(&path).map_err(|e| format!("load token: {}", e))?;
    let stored = data.tokens.get(provider);
    let json = serde_json::json!({
        "authenticated": stored.is_some(),
        "expires_at": stored.as_ref().and_then(|s| s.token.expires_at),
    });
    Ok(json.to_string())
}

// === SECTION cloud_sync_2 END ===

/// Export the full config + credentials as a FullExportData struct.
/// Mirrors daemon handler's `export_full_data` but uses the FFI state.
fn export_full_data() -> Result<FullExportData, String> {
    let st = crate::jni::state().lock().unwrap();
    let cm = st.config_manager.as_ref().ok_or("config not initialized")?;
    let config = cm.get_blocking();

    let store = crate::credential::android_credential_store();
    let mut passwords = std::collections::HashMap::new();
    let mut key_passphrases = std::collections::HashMap::new();
    let mut key_files = std::collections::HashMap::new();

    for server in &config.servers {
        let sid = &server.id;
        if server.ssh.auth_method == "password" {
            let pwd_key =
                termfast_credential::make_key(sid, termfast_credential::cred_type::PASSWORD);
            if let Ok(pwd) = store.load(&pwd_key) {
                passwords.insert(sid.clone(), pwd);
            }
        } else if server.ssh.auth_method == "key" {
            let pass_key = termfast_credential::make_key(
                sid,
                termfast_credential::cred_type::KEY_PASSPHRASE,
            );
            if let Ok(pass) = store.load(&pass_key) {
                key_passphrases.insert(sid.clone(), pass);
            }
            if !server.ssh.key_path.is_empty() {
                if let Ok(content) = std::fs::read_to_string(&server.ssh.key_path) {
                    key_files.insert(sid.clone(), content);
                }
            }
        }
    }

    Ok(FullExportData {
        config,
        passwords,
        key_passphrases,
        key_files,
    })
}

/// Apply a FullExportData to the local config + credentials.
/// Mirrors daemon handler's `apply_full_export`.
fn apply_full_export(export_data: &FullExportData) -> Result<(), String> {
    // Apply config
    {
        let st = crate::jni::state().lock().unwrap();
        let cm = st.config_manager.as_ref().ok_or("config not initialized")?;
        let rt = crate::runtime::runtime();
        rt.block_on(cm.modify(|config| {
            *config = export_data.config.clone();
        }))
        .map_err(|e| format!("apply config: {}", e))?;
    }

    // Restore credentials
    let store = crate::credential::android_credential_store();
    for (server_id, pwd) in &export_data.passwords {
        let key =
            termfast_credential::make_key(server_id, termfast_credential::cred_type::PASSWORD);
        let _ = store.save(&key, pwd);
    }
    for (server_id, pass) in &export_data.key_passphrases {
        let key = termfast_credential::make_key(
            server_id,
            termfast_credential::cred_type::KEY_PASSPHRASE,
        );
        let _ = store.save(&key, pass);
    }
    // Key files: on Android, key_path is within the app's private storage;
    // we write the content back to the path specified in the config.
    for (server_id, content) in &export_data.key_files {
        let st = crate::jni::state().lock().unwrap();
        let cm = st.config_manager.as_ref().ok_or("config not initialized")?;
        let config = cm.get_blocking();
        if let Some(server) = config.servers.iter().find(|s| s.id == *server_id) {
            if server.ssh.key_path.is_empty() {
                continue;
            }
            // Only write to paths within the app's data dir or temp dir (path traversal guard)
            let key_path = std::path::Path::new(&server.ssh.key_path);
            let data_dir = data_dir();
            let temp_dir = std::env::temp_dir().join("termfast");
            let is_safe = key_path.starts_with(&data_dir) || key_path.starts_with(&temp_dir);
            if !is_safe {
                continue;
            }
            if let Err(e) = std::fs::write(key_path, content) {
                log::warn!("apply_full_export: failed to write key file {}: {}", key_path.display(), e);
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(key_path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
    Ok(())
}

// === SECTION cloud_sync_3 END ===

/// Upload encrypted config to cloud.
/// `params_json`: `{"provider":"dropbox","master_password":"xxx","force":false}`
/// Returns JSON with `ok`/`conflict`/`reason` fields.
pub fn upload(params_json: &str) -> Result<String, String> {
    let params: serde_json::Value =
        serde_json::from_str(params_json).map_err(|e| format!("parse params: {}", e))?;
    let provider = params["provider"]
        .as_str()
        .ok_or("missing provider")?
        .to_string();
    let master_password = params["master_password"]
        .as_str()
        .ok_or("missing master_password")?
        .to_string();
    let force = params["force"].as_bool().unwrap_or(false);

    // Block upload if credential store is in pending mode (no master password
    // set). Uploading would export unencrypted in-memory credentials wrapped
    // with whatever password the user typed — but the local data isn't
    // properly encrypted yet, so this is likely a mistake.
    let store = crate::credential::android_credential_store();
    if store.is_pending() {
        return Ok(serde_json::json!({
            "ok": false,
            "reason": "not_initialized",
            "message": "请先设置主密码后再上传到云端",
        }).to_string());
    }

    // Verify the password can unlock the local credential store.
    // If not, tell the frontend to prompt the user to change their password.
    if let Err(_) = store.unlock(&master_password) {
        return Ok(serde_json::json!({
            "ok": false,
            "reason": "wrong_password",
            "message": "输入的主密码与本地主密码不一致，请先修改主密码后再上传",
        }).to_string());
    }

    // Password change detection: compare input password hash with stored hash.
    // If they differ, return password_mismatch so the frontend can ask the
    // user to confirm the cloud password change.
    let hash_path = data_dir().join("sync_hash.dat");
    let input_hash = sync_crypto::password_hash(&master_password);
    if !force {
        if let Some(stored_hash) = sync_crypto::load_password_hash(&hash_path) {
            if stored_hash != input_hash {
                return Ok(serde_json::json!({
                    "ok": false,
                    "reason": "password_mismatch",
                    "message": "输入的密码与上次云同步使用的密码不一致。\n继续上传将用新密码加密云端数据，其他设备需要使用新密码才能下载。\n是否更换云端密码？",
                }).to_string());
            }
        }
    }

    // Load token
    let path = token_file_path();
    let data = token_store::load_tokens(&path).map_err(|e| format!("load token: {}", e))?;
    let stored = data
        .tokens
        .get(provider.as_str())
        .ok_or("not authenticated to cloud")?;

    let p = build_provider(&provider)?;
    let sync_path = sync_path_from_params(&params);
    let rt = crate::runtime::runtime();

    // Check remote file info
    let remote_info = rt
        .block_on(p.file_info(&stored.token, &sync_path))
        .map_err(|e| format!("file_info: {}", e))?;

    // Load local sync state (encrypted)
    let state_path = sync_state_path();
    let mp_for_state = master_password.clone();
    let sync_state = std::thread::scope(|s| {
        let h = s.spawn(move || sync_state::load_state(&state_path, &mp_for_state));
        h.join().map_err(|e| format!("load_state thread: {:?}", e))
    }).map_err(|e| e)?;
    let sync_state = sync_state;
    let local_hash = sync_state.last_hash(&provider);

    // Conflict detection (unless force=true)
    if !force {
        if let Some(conflict) = check_upload_conflict(&remote_info, local_hash) {
            return Ok(conflict.to_string());
        }
    }

    // Export + encrypt
    let export_data = export_full_data()?;
    let device_name = sync_crypto::device_name();
    let updated_at = chrono::Utc::now().to_rfc3339();
    let payload = sync_crypto::SyncPayload {
        config: serde_json::to_value(&export_data)
            .map_err(|e| format!("serialize config: {}", e))?,
        device_name: device_name.clone(),
        updated_at: updated_at.clone(),
    };

    let mp = master_password.clone();
    let blob = std::thread::scope(|s| {
        s.spawn(move || sync_crypto::encrypt_config(&mp, &payload))
            .join()
            .map_err(|e| format!("encrypt thread: {:?}", e))
    })?
    .map_err(|e| format!("encrypt: {}", e))?;

    // Debug: log blob header
    if blob.len() >= 5 {
        log::info!(
            "cloud sync upload: blob_size={}, magic={:02x?}, version={}",
            blob.len(),
            &blob[..4],
            blob[4]
        );
    }

    // Upload
    rt.block_on(p.upload(&stored.token, &sync_path, &blob))
        .map_err(|e| format!("upload: {}", e))?;

    // Re-fetch file_info for hash
    let new_info = rt
        .block_on(p.file_info(&stored.token, &sync_path))
        .map_err(|e| format!("file_info after upload: {}", e))?;
    let new_hash = new_info.hash.unwrap_or_default();

    // Update sync state — record local config mtime so we can detect
    // local modifications on future downloads.
    let state_path = sync_state_path();
    let config_path = local_config_path();
    let mp = master_password.clone();
    let prov = provider.clone();
    let dn = device_name.clone();
    let ua = updated_at.clone();
    let hash_path_clone = hash_path.clone();
    let input_hash_clone = input_hash;
    let _ = std::thread::scope(|s| {
        s.spawn(move || {
            let mut st = sync_state::load_state(&state_path, &mp);
            let local_mtime = std::fs::metadata(&config_path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs().to_string());
            st.set_sync_info(&prov, new_hash, dn, ua, local_mtime);
            sync_state::save_state(&state_path, &mp, &st)
        })
        .join()
    });

    // Save password hash so future uploads can detect password changes.
    sync_crypto::save_password_hash(&hash_path, &input_hash);

    Ok(serde_json::json!({ "ok": true, "size": blob.len() }).to_string())
}

// === SECTION cloud_sync_4 END ===

/// Download and apply encrypted config from cloud.
/// `params_json`: `{"provider":"dropbox","master_password":"xxx","force_download":false}`
/// Returns JSON with `ok`/`reason`/`device_name`/`updated_at`/`size` fields.
pub fn download(params_json: &str) -> Result<String, String> {
    let params: serde_json::Value =
        serde_json::from_str(params_json).map_err(|e| format!("parse params: {}", e))?;
    let provider = params["provider"]
        .as_str()
        .ok_or("missing provider")?
        .to_string();
    let master_password = params["master_password"]
        .as_str()
        .ok_or("missing master_password")?
        .to_string();
    let force_download = params["force_download"].as_bool().unwrap_or(false);

    // If credential store is initialized (not pending), verify the password
    // can unlock it before proceeding with download.
    let store = crate::credential::android_credential_store();
    if !store.is_pending() && store.is_initialized() {
        if let Err(_) = store.unlock(&master_password) {
            return Ok(serde_json::json!({
                "ok": false,
                "reason": "wrong_password",
                "message": "输入的主密码与本地主密码不一致，请先修改主密码后再下载",
            }).to_string());
        }
    }

    // Load token
    let path = token_file_path();
    let data = token_store::load_tokens(&path).map_err(|e| format!("load token: {}", e))?;
    let stored = data
        .tokens
        .get(provider.as_str())
        .ok_or("not authenticated to cloud")?;

    let p = build_provider(&provider)?;
    let sync_path = sync_path_from_params(&params);
    let rt = crate::runtime::runtime();

    // Check remote file info
    let remote_info = rt
        .block_on(p.file_info(&stored.token, &sync_path))
        .map_err(|e| format!("file_info: {}", e))?;

    if !remote_info.exists {
        return Ok(serde_json::json!({
            "ok": false,
            "reason": "no_remote_data",
            "message": "云端没有同步数据",
        })
        .to_string());
    }

    // Load local sync state
    let state_path = sync_state_path();
    let mp_for_state = master_password.clone();
    let sync_state = std::thread::scope(|s| {
        let h = s.spawn(move || sync_state::load_state(&state_path, &mp_for_state));
        h.join().map_err(|e| format!("load_state thread: {:?}", e))
    }).map_err(|e| e)?;
    let sync_state = sync_state;
    let local_hash = sync_state.last_hash(&provider);
    let last_local_mtime = sync_state.last_local_mtime(&provider).map(String::from);
    let current_local_mtime = local_config_mtime();

    // If both cloud and local are unchanged since last sync, no update needed.
    // force_download=true skips this check (user confirmed overwrite).
    // Checking local mtime prevents false "no_update" when user has edited
    // local data (new node, changed password, etc.) since last sync.
    if !force_download {
        if let (Some(rh), Some(lh)) = (&remote_info.hash, local_hash) {
            if rh == lh {
                // Cloud unchanged — now check if local is also unchanged
                let local_unchanged = match (&last_local_mtime, &current_local_mtime) {
                    (Some(last), Some(cur)) => last == cur,
                    _ => false,  // missing mtime info → allow download (safe default)
                };
                if local_unchanged {
                    return Ok(serde_json::json!({
                        "ok": false,
                        "reason": "no_update",
                        "message": "云端无更新",
                        "cloud_updated_at": remote_info.modified,
                        "local_updated_at": current_local_mtime,
                    })
                    .to_string());
                }

                // Cloud unchanged but local changed → local is newer than cloud.
                // Downloading would overwrite newer local data — ask user to confirm.
                let local_changed = match (&last_local_mtime, &current_local_mtime) {
                    (Some(last), Some(cur)) => last != cur,
                    _ => false,
                };
                if local_changed {
                    return Ok(serde_json::json!({
                        "ok": false,
                        "reason": "local_newer",
                        "message": "本地数据比云端新，下载将覆盖本地改动",
                        "cloud_updated_at": remote_info.modified,
                        "local_updated_at": current_local_mtime,
                    })
                    .to_string());
                }
            }
        }
    }

    // Download
    let blob = rt
        .block_on(p.download(&stored.token, &sync_path))
        .map_err(|e| format!("download: {}", e))?;
    let blob_size = blob.len();

    // Decrypt
    let mp = master_password.clone();
    let decrypt_result = std::thread::scope(|s| {
        s.spawn(move || sync_crypto::decrypt_config(&mp, &blob))
            .join()
            .map_err(|e| format!("decrypt thread: {:?}", e))
    })?;

    let payload = match decrypt_result {
        Ok(p) => p,
        Err(_) => {
            return Ok(serde_json::json!({
                "ok": false,
                "reason": "decrypt_failed",
                "message": "解密失败，主密码与云端不一致或数据损坏",
            })
            .to_string());
        }
    };

    // Rollback detection
    if !force_download {
        let last_updated = sync_state
            .get(&provider)
            .last_updated_at
            .as_deref()
            .map(String::from);
        if let Some(rollback) = check_rollback(&payload, last_updated.as_deref()) {
            return Ok(rollback.to_string());
        }
    }

    // Apply
    let export_data: FullExportData = serde_json::from_value(payload.config.clone())
        .map_err(|e| format!("parse config: {}", e))?;

    // Before applying, ensure the credential store is unlocked with the
    // download password. This handles two cases:
    // 1. Store in pending mode (no credentials.enc) → initialize with
    //    download password, so apply_full_export's save() calls write
    //    to encrypted credentials.enc instead of plaintext JSON.
    // 2. Store locked (credentials.enc exists but not unlocked) → unlock
    //    with download password, so save() calls succeed and persist.
    //    (If download password ≠ old local password, unlock fails — but
    //    apply_full_export saves to in-memory store which is lost on
    //    restart. User would need to re-enter passwords. This is
    //    acceptable because mismatched passwords are an edge case.)
    {
        let store = crate::credential::android_credential_store();
        if store.is_pending() {
            let _ = store.initialize(&master_password);
        } else if !store.is_unlocked() {
            // Try to unlock with download password. If it fails (wrong
            // password), reset to pending and re-initialize with the
            // download password so credentials persist encrypted.
            if store.unlock(&master_password).is_err() {
                let _ = store.reset();
                let _ = store.initialize(&master_password);
            }
        }
    }

    apply_full_export(&export_data)?;

    // Update sync state — record config.json mtime AFTER apply, so that
    // on next download we can detect if local config has been modified.
    let new_hash = remote_info.hash.unwrap_or_default();
    let device_name = payload.device_name.clone();
    let updated_at = payload.updated_at.clone();
    let state_path = sync_state_path();
    let config_path = local_config_path();
    let mp = master_password.clone();
    let prov = provider.clone();
    let _ = std::thread::scope(|s| {
        s.spawn(move || {
            let mut st = sync_state::load_state(&state_path, &mp);
            // Read config.json mtime after apply (it was just written, so mtime = now)
            let local_mtime = std::fs::metadata(&config_path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs().to_string());
            st.set_sync_info(&prov, new_hash, device_name, updated_at, local_mtime);
            sync_state::save_state(&state_path, &mp, &st)
        })
        .join()
    });

    // Download success — update stored password hash to the download password,
    // so future uploads don't warn about password mismatch.
    let hash_path = data_dir().join("sync_hash.dat");
    sync_crypto::save_password_hash(&hash_path, &sync_crypto::password_hash(&master_password));

    Ok(serde_json::json!({
        "ok": true,
        "device_name": payload.device_name,
        "updated_at": payload.updated_at,
        "size": blob_size,
    })
    .to_string())
}

/// Get cloud sync status for a provider.
/// Returns JSON: `{"authenticated":true,"has_remote":true,"remote_size":1234,"remote_modified":"...","last_synced":"..."}`
pub fn status(provider: &str) -> Result<String, String> {
    // Check if authenticated
    let path = token_file_path();
    if !token_store::token_file_exists(&path) {
        return Ok(serde_json::json!({
            "authenticated": false,
            "has_remote": false,
        })
        .to_string());
    }
    let data = token_store::load_tokens(&path).map_err(|e| format!("load token: {}", e))?;
    let stored = match data.tokens.get(provider) {
        Some(s) => s,
        None => {
            return Ok(serde_json::json!({
                "authenticated": false,
                "has_remote": false,
            })
            .to_string());
        }
    };
    let _ = stored;  // token exists, user is authenticated

    // Load local sync state for last_synced info.
    // NOTE: We intentionally do NOT call file_info (network request) here,
    // because status() is called from the UI main thread via Compose
    // remember{}. A blocking network call here causes ANR.
    // has_remote is set to true (token exists → likely has data);
    // the actual remote check happens when user clicks download/upload.
    let state_path = sync_state_path();
    // Use empty password if store is locked — status should work without unlock
    let mp = String::new();
    let sync_state = std::thread::scope(|s| {
        let h = s.spawn(move || sync_state::load_state(&state_path, &mp));
        h.join().unwrap_or_default()
    });
    let prov_state = sync_state.get(provider);

    Ok(serde_json::json!({
        "authenticated": true,
        "has_remote": true,
        "remote_size": null,
        "remote_modified": null,
        "last_synced": prov_state.last_updated_at,
        "last_device": prov_state.last_device_name,
    })
    .to_string())
}

/// Remove a provider's token (disconnect).
pub fn disconnect(provider: &str) -> Result<(), String> {
    let path = token_file_path();
    if !token_store::token_file_exists(&path) {
        return Ok(());
    }
    let mut data = token_store::load_tokens(&path).map_err(|e| format!("load token: {}", e))?;
    data.tokens.remove(provider);
    token_store::save_tokens(&path, &data).map_err(|e| format!("save token: {}", e))?;
    Ok(())
}

// === SECTION cloud_sync_5 END ===




