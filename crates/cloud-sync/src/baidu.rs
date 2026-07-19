//! Baidu Netdisk cloud provider — implicit grant OAuth 2.0.
//!
//! No client_secret needed: uses implicit grant flow (response_type=token).
//! The app_key is a public identifier from Baidu Netdisk Open Platform.
//!
//! Third-party apps are sandboxed to /apps/<app_name>/ directory.
//!
//! API docs: https://pan.baidu.com/union/doc/

use crate::{
    CloudProvider, CloudProviderTrait, CloudSyncError, OAuthToken, RemoteFileInfo,
};
use serde::Deserialize;

/// Baidu API base URLs
const AUTH_URL: &str = "https://openapi.baidu.com/oauth/2.0/authorize";
const PCS_BASE: &str = "https://d.pcs.baidu.com";
const PAN_BASE: &str = "https://pan.baidu.com";

/// Baidu provider. app_key is the public API Key from Baidu Netdisk
/// Open Platform console. No secret is stored.
pub struct BaiduProvider {
    app_key: String,
}

impl BaiduProvider {
    pub fn new(app_key: String) -> Self {
        Self { app_key }
    }

    /// Parse access_token from the redirect URL fragment.
    /// Baidu implicit grant returns: `#access_token=xxx&expires_in=2592000`
    pub fn parse_token_from_fragment(fragment: &str) -> Result<OAuthToken, CloudSyncError> {
        let fragment = fragment.trim_start_matches('#');
        let mut access_token = None;
        let mut expires_in: Option<u64> = None;

        for pair in fragment.split('&') {
            let mut kv = pair.splitn(2, '=');
            let key = kv.next().unwrap_or("");
            let val = kv.next().unwrap_or("");
            match key {
                "access_token" => access_token = Some(val.to_string()),
                "expires_in" => expires_in = val.parse().ok(),
                _ => {}
            }
        }

        let access_token =
            access_token.ok_or_else(|| CloudSyncError::OAuth("no access_token in fragment".into()))?;

        let expires_at = expires_in.map(|secs| chrono::Utc::now().timestamp() + secs as i64);

        Ok(OAuthToken {
            access_token,
            refresh_token: None, // implicit grant has no refresh_token
            expires_at,
            token_type: "bearer".into(),
        })
    }
}

#[async_trait::async_trait]
impl CloudProviderTrait for BaiduProvider {
    fn provider_type(&self) -> CloudProvider {
        CloudProvider::Baidu
    }

    fn auth_url(&self, _redirect_uri: &str) -> (String, Option<String>) {
        // Implicit grant: response_type=token, redirect_uri=oob
        // No PKCE needed — token is returned directly in URL fragment
        let url = format!(
            "{}?response_type=token&client_id={}&redirect_uri=oob&scope=basic,netdisk&display=mobile",
            AUTH_URL,
            urlencoding::encode(&self.app_key),
        );
        (url, None)
    }

    async fn exchange_code(
        &self,
        _code: &str,
        _code_verifier: &str,
        _redirect_uri: &str,
    ) -> Result<OAuthToken, CloudSyncError> {
        // Implicit grant doesn't use code exchange — token is obtained
        // directly from the redirect URL fragment.
        Err(CloudSyncError::OAuth(
            "Baidu uses implicit grant, not code exchange".into(),
        ))
    }

    async fn refresh_token(&self, _token: &OAuthToken) -> Result<OAuthToken, CloudSyncError> {
        // Implicit grant has no refresh_token — user must re-authorize
        Err(CloudSyncError::TokenExpired)
    }

    // === SECTION baidu_1 END ===

    async fn upload(
        &self,
        token: &OAuthToken,
        path: &str,
        data: &[u8],
    ) -> Result<(), CloudSyncError> {
        // Baidu uses a 3-step upload: precreate → upload slices → create
        let client = reqwest::Client::new();
        let access_token = &token.access_token;

        // Step 1: precreate
        let block_list = vec![md5_hex(data)];
        let precreate_form: Vec<(&str, String)> = vec![
            ("path", path.to_string()),
            ("size", data.len().to_string()),
            ("isdir", "0".to_string()),
            ("autoinit", "1".to_string()),
            ("block_list", serde_json::to_string(&block_list).unwrap()),
            ("content-md5", block_list[0].clone()),
        ];

        let precreate_url = format!(
            "{}/rest/2.0/pcs/file?method=precreate&access_token={}",
            PAN_BASE, access_token
        );

        let resp = client
            .post(&precreate_url)
            .form(&precreate_form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(CloudSyncError::Api(format!(
                "precreate failed ({}): {}",
                status, body
            )));
        }

        let precreate_resp: BaiduPrecreateResponse = resp.json().await?;

        if precreate_resp.errno != 0 {
            return Err(CloudSyncError::Api(format!(
                "precreate errno: {}",
                precreate_resp.errno
            )));
        }

        let uploadid = precreate_resp.uploadid;

        // Step 2: upload single slice (whole file as one slice for small files)
        let upload_url = format!(
            "{}/rest/2.0/pcs/superfile2?method=upload&access_token={}&type=tmpfile&path={}&uploadid={}&partseq=0",
            PCS_BASE, access_token,
            urlencoding::encode(path),
            uploadid
        );

        let part = reqwest::multipart::Part::bytes(data.to_vec())
            .file_name("file")
            .mime_str("application/octet-stream")
            .unwrap();

        let form = reqwest::multipart::Form::new().part("file", part);

        let resp = client.post(&upload_url).multipart(form).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(CloudSyncError::Api(format!(
                "slice upload failed ({}): {}",
                status, body
            )));
        }

        // Step 3: create (finalize)
        let create_form: Vec<(&str, String)> = vec![
            ("path", path.to_string()),
            ("size", data.len().to_string()),
            ("isdir", "0".to_string()),
            ("block_list", serde_json::to_string(&block_list).unwrap()),
            ("content-md5", block_list[0].clone()),
            ("uploadid", uploadid.clone()),
        ];

        let create_url = format!(
            "{}/rest/2.0/pcs/file?method=create&access_token={}",
            PAN_BASE, access_token
        );

        let resp = client
            .post(&create_url)
            .form(&create_form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(CloudSyncError::Api(format!(
                "create failed ({}): {}",
                status, body
            )));
        }

        let create_resp: BaiduCreateResponse = resp.json().await?;
        if create_resp.errno != 0 {
            return Err(CloudSyncError::Api(format!(
                "create errno: {}",
                create_resp.errno
            )));
        }

        Ok(())
    }

    // === SECTION baidu_2 END ===

    async fn download(
        &self,
        token: &OAuthToken,
        path: &str,
    ) -> Result<Vec<u8>, CloudSyncError> {
        let client = reqwest::Client::new();
        let url = format!(
            "{}/rest/2.0/pcs/file?method=download&access_token={}&path={}",
            PCS_BASE,
            token.access_token,
            urlencoding::encode(path),
        );

        let resp = client.get(&url).send().await?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(CloudSyncError::NotFound(path.into()));
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(CloudSyncError::Api(format!(
                "download failed ({}): {}",
                status, body
            )));
        }

        let bytes = resp.bytes().await?;
        Ok(bytes.to_vec())
    }

    async fn file_info(
        &self,
        token: &OAuthToken,
        path: &str,
    ) -> Result<RemoteFileInfo, CloudSyncError> {
        let client = reqwest::Client::new();
        let url = format!(
            "{}/rest/2.0/pcs/file?method=meta&access_token={}&path={}",
            PAN_BASE,
            token.access_token,
            urlencoding::encode(path),
        );

        let resp = client.get(&url).send().await?;

        if !resp.status().is_success() {
            // File doesn't exist
            return Ok(RemoteFileInfo {
                exists: false,
                size: None,
                hash: None,
                modified: None,
            });
        }

        let meta: BaiduFileMeta = resp.json().await?;

        if meta.errno != 0 {
            return Ok(RemoteFileInfo {
                exists: false,
                size: None,
                hash: None,
                modified: None,
            });
        }

        let info = meta.list.first();
        Ok(RemoteFileInfo {
            exists: info.is_some(),
            size: info.map(|f| f.size),
            hash: info.and_then(|f| f.dlink.clone()),
            modified: info.map(|f| f.local_mtime.to_string()),
        })
    }

    async fn delete(&self, token: &OAuthToken, path: &str) -> Result<(), CloudSyncError> {
        let client = reqwest::Client::new();
        let url = format!(
            "{}/rest/2.0/pcs/file?method=delete&access_token={}",
            PAN_BASE, token.access_token,
        );

        let resp = client
            .post(&url)
            .form(&[("path", path)])
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(CloudSyncError::Api(format!(
                "delete failed ({}): {}",
                status, body
            )));
        }

        Ok(())
    }
}

// === SECTION baidu_3 END ===

/// Baidu API response structs
#[derive(Debug, Deserialize)]
struct BaiduPrecreateResponse {
    errno: i64,
    #[serde(default)]
    uploadid: String,
}

#[derive(Debug, Deserialize)]
struct BaiduCreateResponse {
    errno: i64,
}

#[derive(Debug, Deserialize)]
struct BaiduFileMeta {
    errno: i64,
    #[serde(default)]
    list: Vec<BaiduFileInfo>,
}

#[derive(Debug, Deserialize)]
struct BaiduFileInfo {
    size: u64,
    #[serde(default)]
    dlink: Option<String>,
    local_mtime: i64,
}

/// Compute MD5 hex hash of data (for Baidu block_list)
fn md5_hex(data: &[u8]) -> String {
    use md5::{Digest, Md5};
    let hash = Md5::digest(data);
    hex::encode(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_url_implicit_grant() {
        let provider = BaiduProvider::new("test_app_key".into());
        let (url, verifier) = provider.auth_url("http://localhost:12345/callback");
        assert!(url.contains("response_type=token"));
        assert!(url.contains("client_id=test_app_key"));
        assert!(url.contains("redirect_uri=oob"));
        assert!(url.contains("display=mobile"));
        assert!(verifier.is_none()); // No PKCE for implicit grant
    }

    #[test]
    fn test_parse_token_from_fragment() {
        let fragment = "access_token=abc123&expires_in=2592000";
        let token = BaiduProvider::parse_token_from_fragment(fragment).unwrap();
        assert_eq!(token.access_token, "abc123");
        assert!(token.expires_at.is_some());
        assert!(token.refresh_token.is_none());
    }

    #[test]
    fn test_parse_token_missing() {
        let fragment = "expires_in=2592000";
        let result = BaiduProvider::parse_token_from_fragment(fragment);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_token_with_hash_prefix() {
        let fragment = "#access_token=xyz789&expires_in=3600";
        let token = BaiduProvider::parse_token_from_fragment(fragment).unwrap();
        assert_eq!(token.access_token, "xyz789");
    }
}

// === SECTION baidu_4 END ===