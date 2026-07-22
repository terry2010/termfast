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

/// Translate a Baidu errno code to a user-friendly Chinese message.
/// Source: Baidu Netdisk Open Platform error codes + community docs.
/// Returns (short_msg) suitable for displaying to end users.
fn baidu_errno_msg(errno: i64) -> &'static str {
    match errno {
        0 => "成功",
        // Negative codes — auth/session related
        -1 => "分享功能被禁用（文件违规）",
        -2 => "用户不存在，请重新登录",
        -3 => "文件不存在",
        -4 => "登录信息有误，请重新登录",
        -5 => "登录信息有误，请重新登录",
        -6 => "登录已过期，请重新连接百度网盘",
        -7 => "该分享已删除或已取消",
        -8 => "该分享已经过期",
        -9 => "访问密码错误",
        -10 => "分享外链已达上限",
        -11 => "验证 cookie 无效，请重新登录",
        -16 => "该文件已限制操作",
        -30 => "文件已存在",
        -31 => "文件保存失败",
        -32 => "网盘空间不足",
        -33 => "一次操作数量超限（最多 999 个）",
        -62 => "密码输入次数达到上限",
        // Positive codes — API errors
        2 => "参数错误（请检查路径或文件名）",
        3 => "未登录或帐号无效，请重新连接",
        4 => "存储服务异常，请稍后重试",
        12 => "批量处理错误",
        14 => "网络错误，请稍后重试",
        15 => "操作失败，请稍后重试",
        16 => "网络错误，请稍后重试",
        108 => "文件名含敏感词，请重命名",
        111 => "外链转存失败",
        112 => "页面已过期，请重试",
        // PCS-specific large codes
        31045 => "用户不存在，请重新登录",
        31061 => "文件已存在",
        31062 => "文件名不合法",
        31063 => "文件名太长",
        31064 => "目录名太长",
        31066 => "文件或目录不存在",
        31079 => "秒传文件失败",
        // Token errors
        110 => "Token 已过期，请重新连接",
        _ => "未知错误",
    }
}

/// Format an errno into a user-friendly error string.
fn baidu_error(prefix: &str, errno: i64) -> CloudSyncError {
    CloudSyncError::Api(format!("{}：{}（错误码 {}）", prefix, baidu_errno_msg(errno), errno))
}

/// Format an HTTP error response into a user-friendly message.
/// If the body is HTML (error page), don't expose it to the user.
fn http_error(prefix: &str, status: reqwest::StatusCode, body: String) -> CloudSyncError {
    // If body looks like HTML, don't show it to the user
    let msg = if body.trim_start().starts_with('<') || body.is_empty() {
        format!("HTTP {}", status.as_u16())
    } else if body.len() > 200 {
        // Truncate long JSON error bodies
        format!("HTTP {}：{}...", status.as_u16(), &body[..200])
    } else {
        format!("HTTP {}：{}", status.as_u16(), body)
    };
    CloudSyncError::Api(format!("{}：{}", prefix, msg))
}

/// Build a reqwest client with timeouts to prevent indefinite hangs.
fn reqwest_client_with_proxy(
    mode: &crate::proxy::ProxyMode,
) -> reqwest::Client {
    crate::proxy::build_client(mode, Duration::from_secs(30), Duration::from_secs(10))
}

/// Build a reqwest client with a long timeout for file uploads.
/// Uploads can take a while depending on file size and network speed.
fn reqwest_upload_client_with_proxy(
    mode: &crate::proxy::ProxyMode,
) -> reqwest::Client {
    crate::proxy::build_client(mode, Duration::from_secs(300), Duration::from_secs(10))
}

/// Baidu provider. No app_key or secret stored in the binary —
/// all OAuth operations go through the cloud sync proxy server.
/// Uses Authorization Code flow (via server) which provides refresh_token
/// (10-year validity), unlike implicit grant which had no refresh.
pub struct BaiduProvider {
    proxy_mode: crate::proxy::ProxyMode,
}

impl BaiduProvider {
    pub fn new() -> Self {
        Self {
            proxy_mode: crate::proxy::ProxyMode::Auto,
        }
    }

    pub fn with_proxy_mode(proxy_mode: crate::proxy::ProxyMode) -> Self {
        Self { proxy_mode }
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

    fn proxy_mode(&self) -> &crate::proxy::ProxyMode {
        &self.proxy_mode
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
        let client = reqwest_client_with_proxy(&self.proxy_mode);

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

        let client = reqwest_client_with_proxy(&self.proxy_mode);

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
                "token refresh failed ({}): {}",
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
        let client = reqwest_client_with_proxy(&self.proxy_mode);
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
            // rtype=3: overwrite if file already exists (avoids errno -6)
            ("rtype", "3".to_string()),
        ];

        let precreate_url = format!(
            "{}/rest/2.0/xpan/file?method=precreate&access_token={}",
            PAN_BASE,
            urlencoding::encode(access_token),
        );

        let resp = client
            .post(&precreate_url)
            .form(&precreate_form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(http_error(
                "预上传失败",
                status, body
            ));
        }

        let precreate_resp: BaiduPrecreateResponse = resp.json().await?;

        if precreate_resp.errno != 0 {
            return Err(baidu_error("预上传失败", precreate_resp.errno));
        }

        let uploadid = precreate_resp.uploadid;

        // Step 2: upload single slice (whole file as one slice for small files)
        let upload_url = format!(
            "{}/rest/2.0/pcs/superfile2?method=upload&type=tmpfile&path={}&uploadid={}&partseq=0&access_token={}",
            PCS_BASE,
            urlencoding::encode(&path),
            uploadid,
            urlencoding::encode(access_token),
        );

        let part = reqwest::multipart::Part::bytes(data.to_vec())
            .file_name("file")
            .mime_str("application/octet-stream")
            .unwrap();

        let form = reqwest::multipart::Form::new().part("file", part);

        let resp = reqwest_upload_client_with_proxy(&self.proxy_mode)
            .post(&upload_url)
            .multipart(form)
            .send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(http_error(
                "分片上传失败",
                status, body
            ));
        }

        // Step 3: create (finalize)
        let create_form: Vec<(&str, String)> = vec![
            ("path", path.to_string()),
            ("size", data.len().to_string()),
            ("isdir", "0".to_string()),
            ("block_list", serde_json::to_string(&block_list).unwrap()),
            ("content-md5", block_list[0].clone()),
            ("uploadid", uploadid.clone()),
            // rtype=3: overwrite if file already exists (must match precreate)
            ("rtype", "3".to_string()),
        ];

        let create_url = format!(
            "{}/rest/2.0/xpan/file?method=create&access_token={}",
            PAN_BASE,
            urlencoding::encode(access_token),
        );

        let resp = client
            .post(&create_url)
            .form(&create_form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(http_error(
                "创建文件失败",
                status, body
            ));
        }

        let create_resp: BaiduCreateResponse = resp.json().await?;
        if create_resp.errno != 0 {
            return Err(baidu_error("创建文件失败", create_resp.errno));
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
        let client = reqwest_client_with_proxy(&self.proxy_mode);
        let url = format!(
            "{}/rest/2.0/pcs/file?method=download&path={}&access_token={}",
            PCS_BASE,
            urlencoding::encode(&path),
            urlencoding::encode(&token.access_token),
        );

        let resp = client
            .get(&url)
            .send().await?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(CloudSyncError::NotFound(path.into()));
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(http_error(
                "下载失败",
                status, body
            ));
        }

        let bytes = resp.bytes().await?;
        Ok(bytes.to_vec())
    }

    async fn file_info(
        &self,
        token: &OAuthToken,
        path: &str,
    ) -> Result<RemoteFileInfo, CloudSyncError> {
        let full_path = baidu_path(path);
        tracing::debug!("baidu file_info: full_path={}", full_path);
        // Split into parent dir + filename, then use method=list to find the file.
        // (method=meta requires fsids, not path — unusable for path-based lookup.)
        let (dir, filename) = match full_path.rsplit_once('/') {
            Some((d, f)) if !d.is_empty() && !f.is_empty() => (d, f),
            _ => {
                tracing::warn!("baidu file_info: cannot split path '{}'", full_path);
                return Ok(RemoteFileInfo {
                    exists: false,
                    size: None,
                    hash: None,
                    modified: None,
                });
            }
        };
        tracing::debug!("baidu file_info: dir='{}' filename='{}'", dir, filename);

        let client = reqwest_client_with_proxy(&self.proxy_mode);
        let url = format!(
            "{}/rest/2.0/xpan/file?method=list&dir={}&access_token={}",
            PAN_BASE,
            urlencoding::encode(dir),
            urlencoding::encode(&token.access_token),
        );

        let resp = client
            .get(&url)
            .send().await?;

        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            tracing::warn!("baidu file_info: HTTP 404");
            return Ok(RemoteFileInfo {
                exists: false,
                size: None,
                hash: None,
                modified: None,
            });
        }

        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            tracing::warn!("baidu file_info: HTTP {} body={}", status, &body[..body.len().min(200)]);
            return Err(http_error(
                "查询文件信息失败",
                status, body
            ));
        }

        let body_text = resp.text().await.unwrap_or_default();
        tracing::debug!("baidu file_info: response body={}", &body_text[..body_text.len().min(500)]);

        let meta: BaiduFileMeta = serde_json::from_str(&body_text)
            .map_err(|e| {
                tracing::warn!("baidu file_info: JSON parse error: {}", e);
                CloudSyncError::Api(format!("file_info JSON parse: {}", e))
            })?;

        if meta.errno != 0 {
            tracing::warn!("baidu file_info: errno={}", meta.errno);
            return Ok(RemoteFileInfo {
                exists: false,
                size: None,
                hash: None,
                modified: None,
            });
        }

        tracing::debug!("baidu file_info: list has {} entries", meta.list.len());
        for f in &meta.list {
            tracing::debug!("baidu file_info: entry name={:?} size={}", f.name(), f.size);
        }

        // Find the file matching our target filename in the directory listing
        let info = meta.list.iter().find(|f| f.name() == Some(filename));
        let result = Ok(RemoteFileInfo {
            exists: info.is_some(),
            size: info.map(|f| f.size),
            hash: info.and_then(|f| f.md5.clone()),
            // Use server_mtime (upload time) as the authoritative cloud time.
            // Fallback to local_mtime if server_mtime is missing.
            modified: info.and_then(|f| {
                f.server_mtime
                    .or(f.local_mtime)
                    .map(|t| t.to_string())
            }),
        });
        tracing::debug!("baidu file_info: result exists={}", info.is_some());
        result
    }

    async fn delete(&self, token: &OAuthToken, path: &str) -> Result<(), CloudSyncError> {
        let path = baidu_path(path);
        let client = reqwest_client_with_proxy(&self.proxy_mode);
        let url = format!(
            "{}/rest/2.0/xpan/file?method=delete&access_token={}",
            PAN_BASE,
            urlencoding::encode(&token.access_token),
        );

        let resp = client
            .post(&url)
            .form(&[("path", &path)])
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(http_error(
                "删除文件失败",
                status, body
            ));
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
    /// Server-side modification time (when file was uploaded to Baidu).
    /// This is the authoritative "cloud upload time".
    #[serde(default)]
    server_mtime: Option<i64>,
    /// Client-side modification time (file's local mtime on the uploader's device).
    #[serde(default)]
    #[allow(dead_code)]
    local_mtime: Option<i64>,
    /// Baidu list API returns server_filename; meta API returns filename.
    /// We try both, plus fallback to extracting from path.
    #[serde(default)]
    server_filename: Option<String>,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

impl BaiduFileInfo {
    /// Get the filename from whichever field is available.
    fn name(&self) -> Option<&str> {
        self.server_filename
            .as_deref()
            .or(self.filename.as_deref())
            .or(self.path.as_deref().and_then(|p| p.rsplit('/').next()))
    }
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

    /// 验证 file_info 用 method=list + filename 匹配的逻辑：
    /// 目录下有多个文件时，只匹配目标 filename 的那个
    #[test]
    fn test_baidu_file_info_list_filename_match() {
        // method=list 返回 server_filename 和 path，不返回 filename
        let json = r#"{
            "errno": 0,
            "list": [
                {"size": 100, "server_filename": "other.bin", "path": "/apps/云盘备份/TermFast/other.bin", "local_mtime": 1000},
                {"size": 2048, "server_filename": "config.enc", "path": "/apps/云盘备份/TermFast/config.enc", "md5": "abc123", "local_mtime": 1721568000}
            ]
        }"#;
        let meta: BaiduFileMeta = serde_json::from_str(json).unwrap();
        assert_eq!(meta.errno, 0);
        // 模拟 file_info 里 find(name == "config.enc") 的逻辑
        let target = "config.enc";
        let info = meta.list.iter().find(|f| f.name() == Some(target));
        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.size, 2048);
        assert_eq!(info.md5.as_deref(), Some("abc123"));
        assert_eq!(info.name(), Some("config.enc"));
        // 确认不会误匹配 other.bin
        let wrong = meta.list.iter().find(|f| f.name() == Some("other.bin"));
        assert!(wrong.is_some());
        assert_ne!(wrong.unwrap().size, 2048);
    }

    /// 验证 server_filename 缺失时从 path 提取 filename 的 fallback 逻辑
    #[test]
    fn test_baidu_file_info_name_from_path_fallback() {
        let json = r#"{
            "errno": 0,
            "list": [
                {"size": 512, "path": "/apps/云盘备份/TermFast/config.enc", "local_mtime": 1000}
            ]
        }"#;
        let meta: BaiduFileMeta = serde_json::from_str(json).unwrap();
        let info = meta.list.first().unwrap();
        // server_filename 和 filename 都缺失，应从 path 提取
        assert_eq!(info.server_filename, None);
        assert_eq!(info.filename, None);
        assert_eq!(info.name(), Some("config.enc"));
    }

    /// 验证百度错误码翻译为用户友好文案
    #[test]
    fn test_baidu_errno_msg() {
        assert_eq!(baidu_errno_msg(0), "成功");
        assert_eq!(baidu_errno_msg(-6), "登录已过期，请重新连接百度网盘");
        assert_eq!(baidu_errno_msg(-32), "网盘空间不足");
        assert_eq!(baidu_errno_msg(2), "参数错误（请检查路径或文件名）");
        assert_eq!(baidu_errno_msg(31066), "文件或目录不存在");
        // 未知错误码返回通用文案
        assert_eq!(baidu_errno_msg(99999), "未知错误");
    }

    /// 验证 baidu_error 格式化包含中文文案和错误码
    #[test]
    fn test_baidu_error_format() {
        let err = baidu_error("预上传失败", -6);
        match err {
            CloudSyncError::Api(msg) => {
                assert!(msg.contains("预上传失败"), "msg = {}", msg);
                assert!(msg.contains("登录已过期"), "msg = {}", msg);
                assert!(msg.contains("-6"), "msg = {}", msg);
            }
            _ => panic!("expected CloudSyncError::Api"),
        }
    }
}

// === SECTION baidu_4 END ===