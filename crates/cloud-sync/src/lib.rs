//! Cloud sync — upload/download encrypted config to cloud providers.
//!
//! Supports Dropbox (PKCE OAuth) and Baidu Netdisk (implicit grant).
//! Neither requires embedding client_secret in the app binary.
//!
//! Sync data is always encrypted with the user's master password before
//! upload, so the cloud provider only sees ciphertext.

pub mod baidu;
pub mod dropbox;
pub mod token_store;

use serde::{Deserialize, Serialize};

/// Cloud provider type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudProvider {
    Dropbox,
    Baidu,
}

impl std::fmt::Display for CloudProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CloudProvider::Dropbox => write!(f, "dropbox"),
            CloudProvider::Baidu => write!(f, "baidu"),
        }
    }
}

impl std::str::FromStr for CloudProvider {
    type Err = CloudSyncError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "dropbox" => Ok(CloudProvider::Dropbox),
            "baidu" => Ok(CloudProvider::Baidu),
            _ => Err(CloudSyncError::UnknownProvider(s.into())),
        }
    }
}

/// OAuth token bundle returned from authorization flow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Unix timestamp (seconds) when access_token expires.
    pub expires_at: Option<i64>,
    pub token_type: String,
}

/// Result of an OAuth authorization flow — either a token (implicit/simple)
/// or an authorization code that needs to be exchanged (PKCE).
#[derive(Debug, Clone)]
pub enum AuthResult {
    /// Got access_token directly (implicit grant / simple mode)
    Token(OAuthToken),
    /// Got authorization code, needs to be exchanged for token via PKCE
    AuthCode {
        code: String,
        code_verifier: String,
        redirect_uri: String,
    },
}

/// Metadata about a remote sync file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteFileInfo {
    pub exists: bool,
    pub size: Option<u64>,
    /// Server-side hash (provider-specific format)
    pub hash: Option<String>,
    /// Last modified time (RFC 3339)
    pub modified: Option<String>,
}

/// Trait for cloud storage providers.
#[async_trait::async_trait]
pub trait CloudProviderTrait: Send + Sync {
    /// Provider type identifier
    fn provider_type(&self) -> CloudProvider;

    /// Generate the OAuth authorization URL for the user to open in a browser.
    /// For PKCE flows, returns the URL + code_verifier (caller must save it).
    /// For implicit flows, returns just the URL.
    fn auth_url(&self, redirect_uri: &str) -> (String, Option<String>);

    /// Exchange an authorization code for an OAuth token (PKCE flow).
    /// Returns error for implicit-only providers.
    async fn exchange_code(
        &self,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
    ) -> Result<OAuthToken, CloudSyncError>;

    /// Refresh an expired access token using a refresh token.
    async fn refresh_token(&self, token: &OAuthToken) -> Result<OAuthToken, CloudSyncError>;

    /// Upload a file to the sync path. Data should already be encrypted.
    async fn upload(
        &self,
        token: &OAuthToken,
        path: &str,
        data: &[u8],
    ) -> Result<(), CloudSyncError>;

    /// Download a file from the sync path. Returns the encrypted blob.
    async fn download(
        &self,
        token: &OAuthToken,
        path: &str,
    ) -> Result<Vec<u8>, CloudSyncError>;

    /// Check if a file exists at the sync path and get metadata.
    async fn file_info(
        &self,
        token: &OAuthToken,
        path: &str,
    ) -> Result<RemoteFileInfo, CloudSyncError>;

    /// Delete a file at the sync path.
    async fn delete(&self, token: &OAuthToken, path: &str) -> Result<(), CloudSyncError>;
}

/// Cloud sync errors
#[derive(Debug, thiserror::Error)]
pub enum CloudSyncError {
    #[error("unknown cloud provider: {0}")]
    UnknownProvider(String),

    #[error("OAuth error: {0}")]
    OAuth(String),

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("API error: {0}")]
    Api(String),

    #[error("file not found at path: {0}")]
    NotFound(String),

    #[error("token expired, re-authorization required")]
    TokenExpired,

    #[error("not authenticated")]
    NotAuthenticated,

    #[error("IO error: {0}")]
    Io(String),

    #[error("config error: {0}")]
    Config(String),
}

impl From<reqwest::Error> for CloudSyncError {
    fn from(e: reqwest::Error) -> Self {
        CloudSyncError::Http(e.to_string())
    }
}

impl From<serde_json::Error> for CloudSyncError {
    fn from(e: serde_json::Error) -> Self {
        CloudSyncError::Api(format!("JSON parse error: {}", e))
    }
}

/// Default sync file path on cloud storage
pub const SYNC_FILE_PATH: &str = "/TermFast/config.enc";

/// Helper: generate a random PKCE code_verifier (43-128 chars, RFC 7636)
pub fn generate_code_verifier() -> String {
    use rand::RngExt;
    const CHARSET: &[u8] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let mut rng = rand::rng();
    (0..64)
        .map(|_| {
            let idx = rng.random_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// Helper: compute PKCE code_challenge from code_verifier (S256 method)
pub fn compute_code_challenge(verifier: &str) -> String {
    use base64::Engine;
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash)
}

// === SECTION 1 END ===
