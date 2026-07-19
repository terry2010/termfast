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
const AUTH_BASE: &str = "https://www.dropbox.com/oauth2/authorize";
const TOKEN_URL: &str = "https://api.dropboxapi.com/oauth2/token";

/// Dropbox provider. app_key is the public app identifier from the
/// Dropbox Developer Console. No secret is stored.
pub struct DropboxProvider {
    app_key: String,
}

impl DropboxProvider {
    pub fn new(app_key: String) -> Self {
        Self { app_key }
    }
}

#[async_trait::async_trait]
impl CloudProviderTrait for DropboxProvider {
    fn provider_type(&self) -> CloudProvider {
        CloudProvider::Dropbox
    }

    fn auth_url(&self, redirect_uri: &str) -> (String, Option<String>) {
        let code_verifier = generate_code_verifier();
        let code_challenge = compute_code_challenge(&code_verifier);

        let url = format!(
            "{}?client_id={}&response_type=code&code_challenge={}&code_challenge_method=S256&redirect_uri={}&token_access_type=offline",
            AUTH_BASE,
            urlencoding::encode(&self.app_key),
            code_challenge,
            urlencoding::encode(redirect_uri),
        );

        (url, Some(code_verifier))
    }

    async fn exchange_code(
        &self,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
    ) -> Result<OAuthToken, CloudSyncError> {
        let client = reqwest::Client::new();

        let params = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("code_verifier", code_verifier),
            ("client_id", &self.app_key),
            ("redirect_uri", redirect_uri),
        ];

        let resp = client
            .post(TOKEN_URL)
            .form(&params)
            .send()
            .await?;

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

        let client = reqwest::Client::new();
        let params = [
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", &self.app_key),
        ];

        let resp = client.post(TOKEN_URL).form(&params).send().await?;

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
        let client = reqwest::Client::new();
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
        let client = reqwest::Client::new();
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
        let client = reqwest::Client::new();

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
        let client = reqwest::Client::new();

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
    fn test_auth_url_contains_pkce_params() {
        let provider = DropboxProvider::new("test_app_key".into());
        let (url, verifier) = provider.auth_url("http://localhost:12345/callback");
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=test_app_key"));
        assert!(url.contains("token_access_type=offline"));
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
}

// === SECTION dropbox_3 END ===
