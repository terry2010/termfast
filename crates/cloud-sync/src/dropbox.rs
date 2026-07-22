//! Dropbox cloud provider — PKCE OAuth 2.0 flow.
//!
//! No client_secret needed: Dropbox supports PKCE for native apps.
//! The app_key is a public identifier; embedding it in the binary is safe.
//!
//! API docs: https://www.dropbox.com/developers/documentation/http/documentation

use crate::{
    CloudProvider, CloudProviderTrait, CloudSyncError, OAuthToken, RemoteFileInfo,
    compute_code_challenge, generate_code_verifier,
};
use serde::Deserialize;

/// Dropbox API base URLs
const API_BASE: &str = "https://api.dropboxapi.com/2";
const CONTENT_BASE: &str = "https://content.dropboxapi.com/2";

/// Dropbox provider. No app_key or secret stored in the binary —
/// all OAuth operations go through the cloud sync proxy server.
pub struct DropboxProvider {
    proxy_mode: crate::proxy::ProxyMode,
}

impl DropboxProvider {
    pub fn new() -> Self {
        Self {
            proxy_mode: crate::proxy::ProxyMode::Auto,
        }
    }

    pub fn with_proxy_mode(proxy_mode: crate::proxy::ProxyMode) -> Self {
        Self { proxy_mode }
    }
}

impl Default for DropboxProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl CloudProviderTrait for DropboxProvider {
    fn provider_type(&self) -> CloudProvider {
        CloudProvider::Dropbox
    }

    fn proxy_mode(&self) -> &crate::proxy::ProxyMode {
        &self.proxy_mode
    }

    fn auth_url(&self, redirect_uri: &str) -> (String, Option<String>) {
        let code_verifier = generate_code_verifier();
        let code_challenge = compute_code_challenge(&code_verifier);

        // Request auth URL from proxy server (server holds app_key)
        let url = format!(
            "{}?action=auth_url&provider=dropbox&redirect_uri={}&code_challenge={}",
            crate::CLOUD_SYNC_SERVER,
            urlencoding::encode(redirect_uri),
            code_challenge,
        );

        (url, Some(code_verifier))
    }

    async fn exchange_code(
        &self,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
        _state: &str,
    ) -> Result<OAuthToken, CloudSyncError> {
        let client = crate::proxy::build_client(&self.proxy_mode, std::time::Duration::from_secs(30), std::time::Duration::from_secs(10));

        let body = serde_json::json!({
            "provider": "dropbox",
            "code": code,
            "code_verifier": code_verifier,
            "redirect_uri": redirect_uri,
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

        let token_resp: DropboxTokenResponse = resp.json().await?;
        Ok(token_resp.into())
    }

    // === SECTION dropbox_1 END ===

    async fn refresh_token(&self, token: &OAuthToken) -> Result<OAuthToken, CloudSyncError> {
        let refresh_token = token
            .refresh_token
            .as_ref()
            .ok_or(CloudSyncError::TokenExpired)?;

        let client = crate::proxy::build_client(&self.proxy_mode, std::time::Duration::from_secs(30), std::time::Duration::from_secs(10));

        let body = serde_json::json!({
            "provider": "dropbox",
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

        let token_resp: DropboxTokenResponse = resp.json().await?;
        Ok(token_resp.into())
    }

    async fn upload(
        &self,
        token: &OAuthToken,
        path: &str,
        data: &[u8],
    ) -> Result<(), CloudSyncError> {
        let client = crate::proxy::build_client(&self.proxy_mode, std::time::Duration::from_secs(30), std::time::Duration::from_secs(10));
        let api_arg = serde_json::json!({
            "path": path,
            "mode": "overwrite",
            "autorename": false,
            "mute": true,
        });

        let resp = client
            .post(format!("{}/files/upload", CONTENT_BASE))
            .header(
                "Authorization",
                format!("Bearer {}", token.access_token),
            )
            .header("Dropbox-API-Arg", api_arg.to_string())
            .header("Content-Type", "application/octet-stream")
            .body(data.to_vec())
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(CloudSyncError::Api(format!(
                "upload failed ({}): {}",
                status, body
            )));
        }

        Ok(())
    }

    async fn download(
        &self,
        token: &OAuthToken,
        path: &str,
    ) -> Result<Vec<u8>, CloudSyncError> {
        let client = crate::proxy::build_client(&self.proxy_mode, std::time::Duration::from_secs(30), std::time::Duration::from_secs(10));
        let api_arg = serde_json::json!({ "path": path });

        let resp = client
            .post(format!("{}/files/download", CONTENT_BASE))
            .header(
                "Authorization",
                format!("Bearer {}", token.access_token),
            )
            .header("Dropbox-API-Arg", api_arg.to_string())
            .send()
            .await?;

        if resp.status() == reqwest::StatusCode::CONFLICT
            || resp.status() == reqwest::StatusCode::NOT_FOUND
        {
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
        let client = crate::proxy::build_client(&self.proxy_mode, std::time::Duration::from_secs(30), std::time::Duration::from_secs(10));

        let resp = client
            .post(format!("{}/files/get_metadata", API_BASE))
            .header(
                "Authorization",
                format!("Bearer {}", token.access_token),
            )
            .json(&serde_json::json!({ "path": path }))
            .send()
            .await?;

        if resp.status() == reqwest::StatusCode::CONFLICT {
            // File doesn't exist
            return Ok(RemoteFileInfo {
                exists: false,
                size: None,
                hash: None,
                modified: None,
            });
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(CloudSyncError::Api(format!(
                "get_metadata failed ({}): {}",
                status, body
            )));
        }

        let meta: DropboxMetadata = resp.json().await?;
        Ok(RemoteFileInfo {
            exists: true,
            size: Some(meta.size),
            hash: Some(meta.content_hash),
            modified: Some(meta.server_modified),
        })
    }

    async fn delete(&self, token: &OAuthToken, path: &str) -> Result<(), CloudSyncError> {
        let client = crate::proxy::build_client(&self.proxy_mode, std::time::Duration::from_secs(30), std::time::Duration::from_secs(10));

        let resp = client
            .post(format!("{}/files/delete_v2", API_BASE))
            .header(
                "Authorization",
                format!("Bearer {}", token.access_token),
            )
            .json(&serde_json::json!({ "path": path }))
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

// === SECTION dropbox_2 END ===

/// Dropbox token exchange response
#[derive(Debug, Deserialize)]
struct DropboxTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    token_type: String,
}

impl From<DropboxTokenResponse> for OAuthToken {
    fn from(r: DropboxTokenResponse) -> Self {
        let expires_at = r.expires_in.map(|secs| {
            chrono::Utc::now().timestamp() + secs as i64
        });
        OAuthToken {
            access_token: r.access_token,
            refresh_token: r.refresh_token,
            expires_at,
            token_type: r.token_type,
        }
    }
}

/// Dropbox file metadata response
#[derive(Debug, Deserialize)]
struct DropboxMetadata {
    #[serde(default)]
    size: u64,
    #[serde(default)]
    content_hash: String,
    #[serde(default)]
    server_modified: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_url_goes_through_server() {
        let provider = DropboxProvider::new();
        let (url, verifier) = provider.auth_url("http://localhost:12345/callback");
        // URL should point to our proxy server
        assert!(url.contains("termfast.xisj.com"));
        assert!(url.contains("action=auth_url"));
        assert!(url.contains("provider=dropbox"));
        assert!(url.contains("code_challenge="));
        assert!(verifier.is_some());
        assert!(verifier.unwrap().len() >= 43);
    }

    #[test]
    fn test_code_verifier_length() {
        let v = generate_code_verifier();
        assert!(v.len() >= 43 && v.len() <= 128);
    }

    #[test]
    fn test_code_challenge_is_base64url() {
        let v = "test_verifier_1234567890";
        let c = compute_code_challenge(v);
        // Base64url without padding
        assert!(!c.contains('='));
        assert!(!c.contains('+'));
        assert!(!c.contains('/'));
    }

    #[test]
    fn test_dropbox_token_response_has_refresh() {
        let resp = DropboxTokenResponse {
            access_token: "at123".into(),
            refresh_token: Some("rt456".into()),
            expires_in: Some(14400),
            token_type: "bearer".into(),
        };
        let token: OAuthToken = resp.into();
        assert_eq!(token.access_token, "at123");
        assert_eq!(token.refresh_token.as_deref(), Some("rt456"));
        assert!(token.expires_at.is_some());
    }

    #[test]
    fn test_dropbox_token_response_no_refresh() {
        let resp = DropboxTokenResponse {
            access_token: "at789".into(),
            refresh_token: None,
            expires_in: None,
            token_type: "bearer".into(),
        };
        let token: OAuthToken = resp.into();
        assert_eq!(token.access_token, "at789");
        assert!(token.refresh_token.is_none());
        assert!(token.expires_at.is_none());
        assert_eq!(token.token_type, "bearer");
    }
}

// === SECTION dropbox_3 END ===
