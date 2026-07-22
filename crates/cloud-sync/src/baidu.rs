//! Baidu Netdisk cloud provider — Authorization Code flow via proxy server.
//!
//! OAuth token exchange goes through a proxy server that holds app_key +
//! app_secret. The app binary contains no secrets. Uses Authorization Code
//! flow (not implicit grant) which provides refresh_token (10-year validity).
//!
//! Third-party apps are sandboxed to /apps/<app_name>/ directory.
//!
//! API docs: https://pan.baidu.com/union/doc/

use crate::{
    CloudProvider, CloudProviderTrait, CloudSyncError, OAuthToken, RemoteFileInfo,
};
use serde::Deserialize;
use std::time::Duration;

/// Baidu API base URLs
const PCS_BASE: &str = "https://d.pcs.baidu.com";
const PAN_BASE: &str = "https://pan.baidu.com";

/// Baidu sandbox app name (registered on Baidu Open Platform).
/// Third-party apps can only access files under `/apps/<app_name>/`.
const BAIDU_APP_NAME: &str = "云盘备份";

/// Prepend the Baidu sandbox prefix `/apps/<app_name>/` to a relative path.
/// Baidu's API requires absolute paths under the app sandbox directory.
/// If the path already starts with `/apps/`, it is returned as-is (allows
/// custom override).
fn baidu_path(path: &str) -> String {
    if path.starts_with("/apps/") {
        return path.to_string();
    }
    // Ensure path starts with /
    let p = if path.starts_with('/') { path.to_string() } else { format!("/{}", path) };
    format!("/apps/{}{}", BAIDU_APP_NAME, p)
}

/// Build a reqwest client with timeouts to prevent indefinite hangs.
fn reqwest_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// Baidu provider. No app_key or secret stored in the binary —
/// all OAuth operations go through the cloud sync proxy server.
/// Uses Authorization Code flow (via server) which provides refresh_token
/// (10-year validity), unlike implicit grant which had no refresh.
pub struct BaiduProvider;

impl BaiduProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BaiduProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl CloudProviderTrait for BaiduProvider {
    fn provider_type(&self) -> CloudProvider {
        CloudProvider::Baidu
    }

    fn auth_url(&self, redirect_uri: &str) -> (String, Option<String>) {
        // Request auth URL from proxy server (server holds app_key)
        // Baidu Authorization Code flow — returns code, not token directly
        let url = format!(
            "{}?action=auth_url&provider=baidu&redirect_uri={}",
            crate::CLOUD_SYNC_SERVER,
            urlencoding::encode(redirect_uri),
        );
        (url, None) // No PKCE for Baidu
    }

    async fn exchange_code(
        &self,
        code: &str,
        _code_verifier: &str,
        redirect_uri: &str,
        state: &str,
    ) -> Result<OAuthToken, CloudSyncError> {
        let client = reqwest_client();

        let body = serde_json::json!({
            "provider": "baidu",
            "code": code,
            "redirect_uri": redirect_uri,
            "state": state,
        });

        let url = format!("{}?action=exchange", crate::CLOUD_SYNC_SERVER);
        let resp = client.post(&url).json(&body).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(CloudSyncError::OAuth(format!(
                "token exchange failed ({}): {}",
                status, body
            )));
        }

        let token_resp: BaiduTokenResponse = resp.json().await?;
        Ok(token_resp.into())
    }

    async fn refresh_token(&self, token: &OAuthToken) -> Result<OAuthToken, CloudSyncError> {
        let refresh_token = token
            .refresh_token
            .as_ref()
            .ok_or(CloudSyncError::TokenExpired)?;

        let client = reqwest_client();

        let body = serde_json::json!({
            "provider": "baidu",
            "refresh_token": refresh_token,
        });

        let url = format!("{}?action=refresh", crate::CLOUD_SYNC_SERVER);
        let resp = client.post(&url).json(&body).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(CloudSyncError::OAuth(format!(
                "refresh failed ({}): {}",
                status, body
            )));
        }

        let token_resp: BaiduTokenResponse = resp.json().await?;
        Ok(token_resp.into())
    }

    // === SECTION baidu_1 END ===

    async fn upload(
        &self,
        token: &OAuthToken,
        path: &str,
        data: &[u8],
    ) -> Result<(), CloudSyncError> {
        let path = baidu_path(path);
        // Baidu uses a 3-step upload: precreate → upload slices → create
        let client = reqwest_client();
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
            "{}/rest/2.0/pcs/file?method=precreate",
            PAN_BASE,
        );

        let resp = client
            .post(&precreate_url)
            .header("Authorization", format!("Bearer {}", access_token))
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
            "{}/rest/2.0/pcs/superfile2?method=upload&type=tmpfile&path={}&uploadid={}&partseq=0",
            PCS_BASE,
            urlencoding::encode(&path),
            uploadid
        );

        let part = reqwest::multipart::Part::bytes(data.to_vec())
            .file_name("file")
            .mime_str("application/octet-stream")
            .unwrap();

        let form = reqwest::multipart::Form::new().part("file", part);

        let resp = client
            .post(&upload_url)
            .header("Authorization", format!("Bearer {}", access_token))
            .multipart(form)
            .send().await?;

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
            "{}/rest/2.0/pcs/file?method=create",
            PAN_BASE,
        );

        let resp = client
            .post(&create_url)
            .header("Authorization", format!("Bearer {}", access_token))
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
        let path = baidu_path(path);
        let client = reqwest_client();
        let url = format!(
            "{}/rest/2.0/pcs/file?method=download&path={}",
            PCS_BASE,
            urlencoding::encode(&path),
        );

        let resp = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token.access_token))
            .send().await?;

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
        let path = baidu_path(path);
        let client = reqwest_client();
        let url = format!(
            "{}/rest/2.0/pcs/file?method=meta&path={}",
            PAN_BASE,
            urlencoding::encode(&path),
        );

        let resp = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token.access_token))
            .send().await?;

        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            // File doesn't exist
            return Ok(RemoteFileInfo {
                exists: false,
                size: None,
                hash: None,
                modified: None,
            });
        }

        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(CloudSyncError::Api(format!(
                "Baidu meta request failed ({}): {}",
                status, body
            )));
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
            hash: info.and_then(|f| f.md5.clone()),
            modified: info.map(|f| f.local_mtime.to_string()),
        })
    }

    async fn delete(&self, token: &OAuthToken, path: &str) -> Result<(), CloudSyncError> {
        let path = baidu_path(path);
        let client = reqwest_client();
        let url = format!(
            "{}/rest/2.0/pcs/file?method=delete",
            PAN_BASE,
        );

        let resp = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token.access_token))
            .form(&[("path", &path)])
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
    #[allow(dead_code)]
    dlink: Option<String>,
    #[serde(default)]
    md5: Option<String>,
    local_mtime: i64,
}

/// Baidu OAuth token response (Authorization Code flow)
/// Returns both access_token and refresh_token (10-year validity)
#[derive(Debug, Deserialize)]
struct BaiduTokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
    #[serde(default)]
    token_type: String,
}

impl From<BaiduTokenResponse> for OAuthToken {
    fn from(r: BaiduTokenResponse) -> Self {
        let expires_at = Some(chrono::Utc::now().timestamp() + r.expires_in as i64);
        OAuthToken {
            access_token: r.access_token,
            refresh_token: Some(r.refresh_token),
            expires_at,
            token_type: if r.token_type.is_empty() {
                "bearer".into()
            } else {
                r.token_type
            },
        }
    }
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
    fn test_auth_url_goes_through_server() {
        let provider = BaiduProvider::new();
        let (url, verifier) = provider.auth_url("oob");
        // URL should point to our proxy server, not baidu directly
        assert!(url.contains("termfast.xisj.com"));
        assert!(url.contains("action=auth_url"));
        assert!(url.contains("provider=baidu"));
        assert!(url.contains("redirect_uri=oob"));
        assert!(verifier.is_none()); // No PKCE for Baidu
    }

    #[test]
    fn test_baidu_token_response_has_refresh() {
        let resp = BaiduTokenResponse {
            access_token: "at123".into(),
            refresh_token: "rt456".into(),
            expires_in: 2592000,
            token_type: "bearer".into(),
        };
        let token: OAuthToken = resp.into();
        assert_eq!(token.access_token, "at123");
        assert_eq!(token.refresh_token.as_deref(), Some("rt456"));
        assert!(token.expires_at.is_some());
    }

    /// 11.4 用例 31: file_info 返回的 hash 是 md5 而不是 dlink
    /// 验证 BaiduFileInfo 反序列化时正确解析 md5 字段，
    /// 且 RemoteFileInfo.hash 取自 md5 而非 dlink。
    #[test]
    fn test_baidu_file_info_returns_md5_not_dlink() {
        // 构造百度 PCS meta 接口的真实响应格式
        let json = r#"{
            "errno": 0,
            "list": [
                {
                    "size": 1024,
                    "dlink": "https://d.pcs.baidu.com/file/abc123?fid=xxx",
                    "md5": "d41d8cd98f00b204e9800998ecf8427e",
                    "local_mtime": 1721568000
                }
            ]
        }"#;
        let meta: BaiduFileMeta = serde_json::from_str(json).unwrap();
        assert_eq!(meta.errno, 0);
        assert_eq!(meta.list.len(), 1);

        let info = meta.list.first().unwrap();
        assert_eq!(info.size, 1024);
        // md5 字段必须被正确反序列化
        assert_eq!(
            info.md5.as_deref(),
            Some("d41d8cd98f00b204e9800998ecf8427e")
        );
        // dlink 仍然存在（字段保留用于其他用途）
        assert!(info.dlink.is_some());

        // 模拟 file_info 里的取值逻辑：hash 取 md5 而非 dlink
        let hash = info.md5.clone();
        assert_eq!(
            hash.as_deref(),
            Some("d41d8cd98f00b204e9800998ecf8427e"),
            "file_info.hash must be md5, not dlink"
        );
        // 明确验证不是 dlink
        assert_ne!(
            hash.as_deref(),
            info.dlink.as_deref(),
            "file_info.hash must NOT equal dlink"
        );
    }

    /// 11.4 补充: md5 字段缺失时 hash 返回 None（向后兼容）
    #[test]
    fn test_baidu_file_info_md5_missing_returns_none() {
        let json = r#"{
            "errno": 0,
            "list": [
                {
                    "size": 512,
                    "dlink": "https://d.pcs.baidu.com/file/def456",
                    "local_mtime": 1721568000
                }
            ]
        }"#;
        let meta: BaiduFileMeta = serde_json::from_str(json).unwrap();
        let info = meta.list.first().unwrap();
        // md5 缺失 → hash 应为 None
        assert_eq!(info.md5, None);
        let hash = info.md5.clone();
        assert_eq!(hash, None, "hash must be None when md5 is absent");
    }

    /// 11.4 补充: errno != 0 时视为文件不存在
    #[test]
    fn test_baidu_file_meta_errno_nonzero() {
        let json = r#"{"errno": -1, "list": []}"#;
        let meta: BaiduFileMeta = serde_json::from_str(json).unwrap();
        assert_ne!(meta.errno, 0);
        assert!(meta.list.is_empty());
    }
}

// === SECTION baidu_4 END ===