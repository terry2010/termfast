//! Envelope encryption — KEK wraps DEK, DEK encrypts data.
//!
//! File format v2 (all little-endian):
//! ```text
//! offset  size    field
//! 0       4B      magic (e.g. b"TCRE" or b"TFSC")
//! 4       1B      version = 2
//! 5       4B      argon2_m_cost (u32)
//! 9       4B      argon2_t_cost (u32)
//! 13      2B      argon2_p_cost (u16)
//! 15      16B     salt (Argon2id)
//! 31      12B     nonce_kek (AES-GCM nonce for DEK wrapping)
//! 43      48B     wrapped_dek (32B DEK + 16B GCM auth tag)
//! 91      12B     nonce_data (AES-GCM nonce for data encryption)
//! 103     N B     ciphertext (includes 16B GCM auth tag at the end)
//! ```
//!
//! The header (bytes 0..43) is used as AAD for both AES-GCM operations
//! (DEK wrapping and data encryption), so any tampering with the header
//! invalidates everything.
//!
//! Envelope encryption benefits:
//! - Change password: only re-wrap 32B DEK, no need to re-encrypt data
//! - Argon2 params stored in header: future upgrades don't break old files

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use argon2::Argon2;
use rand::Rng;
use rand::rng;
use unicode_normalization::UnicodeNormalization;
use zeroize::Zeroize;

/// Current envelope format version.
const FORMAT_VERSION: u8 = 2;
/// Argon2id salt length in bytes.
const SALT_LEN: usize = 16;
/// AES-GCM nonce length in bytes.
const NONCE_LEN: usize = 12;
/// DEK length (AES-256 key).
const DEK_LEN: usize = 32;
/// Wrapped DEK length: 32B DEK + 16B GCM auth tag.
const WRAPPED_DEK_LEN: usize = DEK_LEN + 16;
/// Argon2 params size: m_cost(4) + t_cost(4) + p_cost(2).
const PARAMS_LEN: usize = 4 + 4 + 2;
/// Header size: magic(4) + version(1) + params(10) + salt(16) + nonce_kek(12).
pub const HEADER_LEN: usize = 4 + 1 + PARAMS_LEN + SALT_LEN + NONCE_LEN;
/// Full prefix before ciphertext: header + wrapped_dek + nonce_data.
pub const PREFIX_LEN: usize = HEADER_LEN + WRAPPED_DEK_LEN + NONCE_LEN;
/// Derived key length (AES-256).
const KEY_LEN: usize = 32;
/// Max password byte length after NFKC normalization.
const MAX_PASSWORD_LEN: usize = 1024;

/// Default Argon2id parameters for desktop platforms.
pub const DESKTOP_M_COST: u32 = 65536; // 64 MiB
/// Default Argon2id parameters for mobile platforms.
pub const MOBILE_M_COST: u32 = 32768; // 32 MiB
/// Default t_cost and p_cost (shared across platforms).
pub const DEFAULT_T_COST: u32 = 3;
pub const DEFAULT_P_COST: u16 = 1;

/// Argon2id parameters stored in the file header.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Argon2Params {
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u16,
}

impl Argon2Params {
    /// Desktop defaults: 64 MiB / 3 iterations / 1 lane.
    pub fn desktop() -> Self {
        Self {
            m_cost: DESKTOP_M_COST,
            t_cost: DEFAULT_T_COST,
            p_cost: DEFAULT_P_COST,
        }
    }

    /// Mobile defaults: 32 MiB / 3 iterations / 1 lane.
    pub fn mobile() -> Self {
        Self {
            m_cost: MOBILE_M_COST,
            t_cost: DEFAULT_T_COST,
            p_cost: DEFAULT_P_COST,
        }
    }

    /// Platform-appropriate defaults.
    pub fn default_for_platform() -> Self {
        #[cfg(target_os = "android")]
        { Self::mobile() }
        #[cfg(not(target_os = "android"))]
        { Self::desktop() }
    }

    /// Serialize to 10 bytes (m_cost:4 + t_cost:4 + p_cost:2).
    fn to_bytes(self) -> [u8; PARAMS_LEN] {
        let mut buf = [0u8; PARAMS_LEN];
        buf[0..4].copy_from_slice(&self.m_cost.to_le_bytes());
        buf[4..8].copy_from_slice(&self.t_cost.to_le_bytes());
        buf[8..10].copy_from_slice(&self.p_cost.to_le_bytes());
        buf
    }

    /// Deserialize from 10 bytes.
    fn from_bytes(data: &[u8]) -> Self {
        let m_cost = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let t_cost = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let p_cost = u16::from_le_bytes([data[8], data[9]]);
        Self { m_cost, t_cost, p_cost }
    }

    /// Validate that the params are within safe bounds.
    /// Prevents DoS via crafted encrypted files with extremely high m_cost
    /// or t_cost (which would cause Argon2 to allocate excessive memory
    /// or run for a very long time before GCM authentication fails).
    /// Limits are generous enough to allow future upgrades.
    fn validate(&self) -> Result<(), EnvelopeError> {
        // m_cost is in KiB. Max 256 MiB (262144 KiB) — well above current
        // desktop default of 64 MiB, but prevents 4 TB allocations.
        const MAX_M_COST: u32 = 256 * 1024;
        // t_cost: max 100 iterations (current default is 3).
        const MAX_T_COST: u32 = 100;
        // p_cost: max 16 lanes (current default is 1).
        const MAX_P_COST: u16 = 16;
        // m_cost must be at least 8 KiB (Argon2 minimum).
        const MIN_M_COST: u32 = 8;

        if self.m_cost < MIN_M_COST || self.m_cost > MAX_M_COST {
            return Err(EnvelopeError::Crypto(format!(
                "invalid argon2 m_cost: {} (must be {}-{})",
                self.m_cost, MIN_M_COST, MAX_M_COST
            )));
        }
        if self.t_cost == 0 || self.t_cost > MAX_T_COST {
            return Err(EnvelopeError::Crypto(format!(
                "invalid argon2 t_cost: {} (must be 1-{})",
                self.t_cost, MAX_T_COST
            )));
        }
        if self.p_cost == 0 || self.p_cost > MAX_P_COST {
            return Err(EnvelopeError::Crypto(format!(
                "invalid argon2 p_cost: {} (must be 1-{})",
                self.p_cost, MAX_P_COST
            )));
        }
        Ok(())
    }
}

/// Errors from envelope crypto operations.
#[derive(Debug, thiserror::Error)]
pub enum EnvelopeError {
    #[error("password too long after normalization: {0} bytes (max {1})")]
    PasswordTooLong(usize, usize),
    #[error("data too short: {0} bytes (need at least {1})")]
    TooShort(usize, usize),
    #[error("invalid magic bytes")]
    InvalidMagic,
    #[error("unsupported format version: {0}")]
    UnsupportedVersion(u8),
    #[error("decryption failed: wrong password or corrupted data")]
    DecryptFailed,
    #[error("crypto error: {0}")]
    Crypto(String),
}

/// Normalize password with NFKC and validate length.
fn normalize_password(password: &str) -> Result<zeroize::Zeroizing<String>, EnvelopeError> {
    let normalized: String = password.nfkc().collect();
    let byte_len = normalized.len();
    if byte_len > MAX_PASSWORD_LEN {
        return Err(EnvelopeError::PasswordTooLong(byte_len, MAX_PASSWORD_LEN));
    }
    Ok(zeroize::Zeroizing::new(normalized))
}

/// Derive a 32-byte KEK from password + salt using Argon2id with given params.
fn derive_kek(
    password: &str,
    salt: &[u8],
    params: Argon2Params,
) -> Result<[u8; KEY_LEN], EnvelopeError> {
    let normalized = normalize_password(password)?;
    let argon2 = Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        argon2::Params::new(params.m_cost, params.t_cost, params.p_cost as u32, Some(KEY_LEN))
            .map_err(|e| EnvelopeError::Crypto(format!("invalid argon2 params: {}", e)))?,
    );
    let mut out = [0u8; KEY_LEN];
    argon2
        .hash_password_into(normalized.as_bytes(), salt, &mut out)
        .map_err(|e| EnvelopeError::Crypto(format!("argon2 key derivation failed: {}", e)))?;
    Ok(out)
}

/// Generate a random 32-byte DEK.
fn generate_dek() -> [u8; DEK_LEN] {
    let mut dek = [0u8; DEK_LEN];
    rng().fill_bytes(&mut dek);
    dek
}

// === SECTION 1 END ===

/// Encrypt plaintext with envelope encryption.
///
/// Returns the full blob: [header][wrapped_dek][nonce_data][ciphertext].
/// The header (bytes 0..HEADER_LEN) is used as AAD for both AES-GCM ops.
///
/// `magic` identifies the file type (e.g. b"TCRE" for credentials, b"TFSC" for cloud sync).
/// `salt` is the Argon2id salt (random, stored in header).
/// `salt_extra` is optional additional salt mixed into KEK derivation
/// (e.g. hw_id for local credentials, empty for cloud sync).
pub fn encrypt(
    magic: &[u8; 4],
    password: &str,
    salt: &[u8; SALT_LEN],
    salt_extra: &[u8],
    params: Argon2Params,
    plaintext: &[u8],
) -> Result<Vec<u8>, EnvelopeError> {
    // Derive KEK from password + salt (+ optional salt_extra for hw_id binding)
    let kek_salt = if salt_extra.is_empty() {
        salt.to_vec()
    } else {
        // Length-prefix salt_extra to prevent collision
        let len_prefix = (salt_extra.len() as u64).to_le_bytes();
        [&len_prefix, salt_extra, salt].concat()
    };
    let mut kek = derive_kek(password, &kek_salt, params)?;

    // Generate DEK
    let mut dek = generate_dek();

    // Generate nonces
    let mut nonce_kek = [0u8; NONCE_LEN];
    let mut nonce_data = [0u8; NONCE_LEN];
    rng().fill_bytes(&mut nonce_kek);
    rng().fill_bytes(&mut nonce_data);

    // Build header (AAD for both encryptions)
    let mut header = Vec::with_capacity(HEADER_LEN);
    header.extend_from_slice(magic);
    header.push(FORMAT_VERSION);
    header.extend_from_slice(&params.to_bytes());
    header.extend_from_slice(salt);
    header.extend_from_slice(&nonce_kek);

    // Wrap DEK with KEK (AAD = full header)
    let kek_cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&kek));
    let wrapped_dek = kek_cipher
        .encrypt(Nonce::from_slice(&nonce_kek), Payload { msg: &dek, aad: &header })
        .map_err(|e| EnvelopeError::Crypto(format!("aes-gcm wrap DEK failed: {}", e)))?;

    // Data encryption AAD = magic + version + nonce_data
    // (NOT the full header — so password change doesn't require re-encrypting data.
    // Header tampering is still detected via DEK unwrapping failure.)
    let mut data_aad = Vec::with_capacity(4 + 1 + NONCE_LEN);
    data_aad.extend_from_slice(magic);
    data_aad.push(FORMAT_VERSION);
    data_aad.extend_from_slice(&nonce_data);

    // Encrypt data with DEK
    let dek_cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&dek));
    let ciphertext = dek_cipher
        .encrypt(Nonce::from_slice(&nonce_data), Payload { msg: plaintext, aad: &data_aad })
        .map_err(|e| EnvelopeError::Crypto(format!("aes-gcm encrypt data failed: {}", e)))?;

    // Assemble final blob
    let mut blob = Vec::with_capacity(PREFIX_LEN + ciphertext.len());
    blob.extend_from_slice(&header);
    blob.extend_from_slice(&wrapped_dek);
    blob.extend_from_slice(&nonce_data);
    blob.extend_from_slice(&ciphertext);

    // Zeroize keys
    kek.zeroize();
    dek.zeroize();

    Ok(blob)
}

/// Decrypt an envelope-encrypted blob.
///
/// `salt_extra` must match what was used during encryption (e.g. hw_id).
/// Returns the plaintext on success.
pub fn decrypt(
    magic: &[u8; 4],
    password: &str,
    salt_extra: &[u8],
    blob: &[u8],
) -> Result<Vec<u8>, EnvelopeError> {
    if blob.len() < PREFIX_LEN {
        return Err(EnvelopeError::TooShort(blob.len(), PREFIX_LEN));
    }
    if &blob[..4] != magic {
        return Err(EnvelopeError::InvalidMagic);
    }
    let version = blob[4];
    if version != FORMAT_VERSION {
        return Err(EnvelopeError::UnsupportedVersion(version));
    }

    // Parse header
    let params = Argon2Params::from_bytes(&blob[5..5 + PARAMS_LEN]);
    // Security: validate params before Argon2 derivation to prevent DoS
    // via crafted files with extremely high m_cost/t_cost.
    params.validate()?;
    let salt = &blob[5 + PARAMS_LEN..5 + PARAMS_LEN + SALT_LEN];
    let nonce_kek = &blob[5 + PARAMS_LEN + SALT_LEN..HEADER_LEN];
    let wrapped_dek = &blob[HEADER_LEN..HEADER_LEN + WRAPPED_DEK_LEN];
    let nonce_data = &blob[HEADER_LEN + WRAPPED_DEK_LEN..HEADER_LEN + WRAPPED_DEK_LEN + NONCE_LEN];
    let ciphertext = &blob[PREFIX_LEN..];

    // Derive KEK
    let kek_salt = if salt_extra.is_empty() {
        salt.to_vec()
    } else {
        let len_prefix = (salt_extra.len() as u64).to_le_bytes();
        [&len_prefix, salt_extra, salt].concat()
    };
    let mut kek = derive_kek(password, &kek_salt, params)?;

    // Reconstruct header AAD
    let mut header_aad = Vec::with_capacity(HEADER_LEN);
    header_aad.extend_from_slice(magic);
    header_aad.push(version);
    header_aad.extend_from_slice(&params.to_bytes());
    header_aad.extend_from_slice(salt);
    header_aad.extend_from_slice(nonce_kek);

    // Unwrap DEK
    let kek_cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&kek));
    let mut dek = kek_cipher
        .decrypt(Nonce::from_slice(nonce_kek), Payload { msg: wrapped_dek, aad: &header_aad })
        .map_err(|_| EnvelopeError::DecryptFailed)?;

    if dek.len() != DEK_LEN {
        dek.zeroize();
        return Err(EnvelopeError::DecryptFailed);
    }
    let mut dek_arr = [0u8; DEK_LEN];
    dek_arr.copy_from_slice(&dek);
    dek.zeroize();
    kek.zeroize();

    // Reconstruct data AAD = magic + version + nonce_data
    let mut data_aad = Vec::with_capacity(4 + 1 + NONCE_LEN);
    data_aad.extend_from_slice(magic);
    data_aad.push(version);
    data_aad.extend_from_slice(nonce_data);

    // Decrypt data
    let dek_cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&dek_arr));
    let plaintext = dek_cipher
        .decrypt(Nonce::from_slice(nonce_data), Payload { msg: ciphertext, aad: &data_aad })
        .map_err(|_| EnvelopeError::DecryptFailed)?;

    dek_arr.zeroize();

    Ok(plaintext)
}

/// Re-wrap DEK with a new password (envelope encryption core benefit).
///
/// Decrypts the wrapped DEK with the old password, then re-wraps it with
/// the new password. Does NOT re-encrypt the data — only the 32B DEK is
/// re-encrypted, making password change instantaneous.
///
/// Generates a new salt and new nonce_kek. The data ciphertext and nonce_data
/// are preserved as-is (the DEK hasn't changed, and data AAD only includes
/// magic + version + nonce_data, which are unchanged).
///
/// `salt_extra` is the optional hw_id binding (same for old and new).
pub fn change_password(
    magic: &[u8; 4],
    old_password: &str,
    new_password: &str,
    salt_extra: &[u8],
    blob: &[u8],
) -> Result<Vec<u8>, EnvelopeError> {
    if blob.len() < PREFIX_LEN {
        return Err(EnvelopeError::TooShort(blob.len(), PREFIX_LEN));
    }
    if &blob[..4] != magic {
        return Err(EnvelopeError::InvalidMagic);
    }
    let version = blob[4];
    if version != FORMAT_VERSION {
        return Err(EnvelopeError::UnsupportedVersion(version));
    }

    // Parse old header
    let old_params = Argon2Params::from_bytes(&blob[5..5 + PARAMS_LEN]);
    // Security: validate params before Argon2 derivation to prevent DoS.
    old_params.validate()?;
    let old_salt = &blob[5 + PARAMS_LEN..5 + PARAMS_LEN + SALT_LEN];
    let old_nonce_kek = &blob[5 + PARAMS_LEN + SALT_LEN..HEADER_LEN];
    let wrapped_dek = &blob[HEADER_LEN..HEADER_LEN + WRAPPED_DEK_LEN];
    let nonce_data = &blob[HEADER_LEN + WRAPPED_DEK_LEN..HEADER_LEN + WRAPPED_DEK_LEN + NONCE_LEN];
    let ciphertext = &blob[PREFIX_LEN..];

    // Unwrap DEK with old password
    let old_kek_salt = if salt_extra.is_empty() {
        old_salt.to_vec()
    } else {
        let len_prefix = (salt_extra.len() as u64).to_le_bytes();
        [&len_prefix, salt_extra, old_salt].concat()
    };
    let mut old_kek = derive_kek(old_password, &old_kek_salt, old_params)?;

    let mut old_header_aad = Vec::with_capacity(HEADER_LEN);
    old_header_aad.extend_from_slice(magic);
    old_header_aad.push(version);
    old_header_aad.extend_from_slice(&old_params.to_bytes());
    old_header_aad.extend_from_slice(old_salt);
    old_header_aad.extend_from_slice(old_nonce_kek);

    let kek_cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&old_kek));
    let mut dek = kek_cipher
        .decrypt(Nonce::from_slice(old_nonce_kek), Payload { msg: wrapped_dek, aad: &old_header_aad })
        .map_err(|_| EnvelopeError::DecryptFailed)?;
    old_kek.zeroize();

    if dek.len() != DEK_LEN {
        dek.zeroize();
        return Err(EnvelopeError::DecryptFailed);
    }

    // Generate new salt + new nonce_kek
    let mut new_salt = [0u8; SALT_LEN];
    let mut new_nonce_kek = [0u8; NONCE_LEN];
    rng().fill_bytes(&mut new_salt);
    rng().fill_bytes(&mut new_nonce_kek);

    // Use same params (or could upgrade here)
    let new_params = old_params;

    // Derive new KEK
    let new_kek_salt = if salt_extra.is_empty() {
        new_salt.to_vec()
    } else {
        let len_prefix = (salt_extra.len() as u64).to_le_bytes();
        [&len_prefix, salt_extra, &new_salt].concat()
    };
    let mut new_kek = derive_kek(new_password, &new_kek_salt, new_params)?;

    // Build new header
    let mut new_header = Vec::with_capacity(HEADER_LEN);
    new_header.extend_from_slice(magic);
    new_header.push(FORMAT_VERSION);
    new_header.extend_from_slice(&new_params.to_bytes());
    new_header.extend_from_slice(&new_salt);
    new_header.extend_from_slice(&new_nonce_kek);

    // Re-wrap DEK with new KEK (AAD = new header)
    let new_kek_cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&new_kek));
    let new_wrapped_dek = new_kek_cipher
        .encrypt(Nonce::from_slice(&new_nonce_kek), Payload { msg: &dek, aad: &new_header })
        .map_err(|e| EnvelopeError::Crypto(format!("aes-gcm re-wrap DEK failed: {}", e)))?;
    new_kek.zeroize();
    dek.zeroize();

    // Assemble new blob: new header + new wrapped_dek + SAME nonce_data + SAME ciphertext
    // Data doesn't need re-encryption because:
    // - DEK is unchanged (same key for data decryption)
    // - Data AAD (magic + version + nonce_data) is unchanged
    let mut new_blob = Vec::with_capacity(PREFIX_LEN + ciphertext.len());
    new_blob.extend_from_slice(&new_header);
    new_blob.extend_from_slice(&new_wrapped_dek);
    new_blob.extend_from_slice(nonce_data);
    new_blob.extend_from_slice(ciphertext);

    Ok(new_blob)
}

/// Check if a blob is envelope format (version 2).
pub fn is_envelope_format(blob: &[u8]) -> bool {
    blob.len() >= 5 && blob[4] == FORMAT_VERSION
}

/// Get the Argon2 params from a blob header (without decrypting).
pub fn read_params(blob: &[u8]) -> Result<Argon2Params, EnvelopeError> {
    if blob.len() < 5 + PARAMS_LEN {
        return Err(EnvelopeError::TooShort(blob.len(), 5 + PARAMS_LEN));
    }
    Ok(Argon2Params::from_bytes(&blob[5..5 + PARAMS_LEN]))
}

/// Parsed envelope blob components (for incremental re-encryption).
pub struct ParsedBlob<'a> {
    pub magic: &'a [u8; 4],
    pub params: Argon2Params,
    pub salt: &'a [u8],
    pub nonce_kek: &'a [u8],
    pub wrapped_dek: &'a [u8],
    pub nonce_data: &'a [u8],
    pub ciphertext: &'a [u8],
}

/// Parse an envelope blob into its components (without decrypting).
pub fn parse_blob<'a>(magic: &[u8; 4], blob: &'a [u8]) -> Result<ParsedBlob<'a>, EnvelopeError> {
    if blob.len() < PREFIX_LEN {
        return Err(EnvelopeError::TooShort(blob.len(), PREFIX_LEN));
    }
    if &blob[..4] != magic {
        return Err(EnvelopeError::InvalidMagic);
    }
    let version = blob[4];
    if version != FORMAT_VERSION {
        return Err(EnvelopeError::UnsupportedVersion(version));
    }
    let magic_ref: &[u8; 4] = blob[..4].try_into().unwrap();
    let params = Argon2Params::from_bytes(&blob[5..5 + PARAMS_LEN]);
    let salt = &blob[5 + PARAMS_LEN..5 + PARAMS_LEN + SALT_LEN];
    let nonce_kek = &blob[5 + PARAMS_LEN + SALT_LEN..HEADER_LEN];
    let wrapped_dek = &blob[HEADER_LEN..HEADER_LEN + WRAPPED_DEK_LEN];
    let nonce_data = &blob[HEADER_LEN + WRAPPED_DEK_LEN..HEADER_LEN + WRAPPED_DEK_LEN + NONCE_LEN];
    let ciphertext = &blob[PREFIX_LEN..];
    Ok(ParsedBlob { magic: magic_ref, params, salt, nonce_kek, wrapped_dek, nonce_data, ciphertext })
}

/// Decrypt data using a raw DEK (32 bytes) and the blob's nonce_data.
/// Used for incremental saves where the DEK is already cached.
pub fn decrypt_data_with_dek(
    magic: &[u8; 4],
    dek: &[u8; DEK_LEN],
    nonce_data: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, EnvelopeError> {
    let mut data_aad = Vec::with_capacity(4 + 1 + NONCE_LEN);
    data_aad.extend_from_slice(magic);
    data_aad.push(FORMAT_VERSION);
    data_aad.extend_from_slice(nonce_data);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(dek));
    cipher
        .decrypt(Nonce::from_slice(nonce_data), Payload { msg: ciphertext, aad: &data_aad })
        .map_err(|_| EnvelopeError::DecryptFailed)
}

/// Encrypt data using a raw DEK, producing just the (nonce_data, ciphertext) portion.
/// Used for incremental saves — the header + wrapped_dek are preserved from the existing file.
pub fn encrypt_data_with_dek(
    magic: &[u8; 4],
    dek: &[u8; DEK_LEN],
    plaintext: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), EnvelopeError> {
    let mut nonce_data = [0u8; NONCE_LEN];
    rng().fill_bytes(&mut nonce_data);
    let mut data_aad = Vec::with_capacity(4 + 1 + NONCE_LEN);
    data_aad.extend_from_slice(magic);
    data_aad.push(FORMAT_VERSION);
    data_aad.extend_from_slice(&nonce_data);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(dek));
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_data), Payload { msg: plaintext, aad: &data_aad })
        .map_err(|e| EnvelopeError::Crypto(format!("aes-gcm encrypt data failed: {}", e)))?;
    Ok((nonce_data.to_vec(), ciphertext))
}

/// Assemble a full blob from header components + data.
/// Used after incremental re-encryption: header + wrapped_dek are preserved,
/// nonce_data + ciphertext are new.
pub fn assemble_blob(
    magic: &[u8; 4],
    params: Argon2Params,
    salt: &[u8],
    nonce_kek: &[u8],
    wrapped_dek: &[u8],
    nonce_data: &[u8],
    ciphertext: &[u8],
) -> Vec<u8> {
    let mut blob = Vec::with_capacity(PREFIX_LEN + ciphertext.len());
    blob.extend_from_slice(magic);
    blob.push(FORMAT_VERSION);
    blob.extend_from_slice(&params.to_bytes());
    blob.extend_from_slice(salt);
    blob.extend_from_slice(nonce_kek);
    blob.extend_from_slice(wrapped_dek);
    blob.extend_from_slice(nonce_data);
    blob.extend_from_slice(ciphertext);
    blob
}

/// Unwrap DEK from a parsed blob using the password.
/// Returns the raw 32-byte DEK.
pub fn unwrap_dek(
    password: &str,
    salt_extra: &[u8],
    parsed: &ParsedBlob,
) -> Result<[u8; DEK_LEN], EnvelopeError> {
    let kek_salt = if salt_extra.is_empty() {
        parsed.salt.to_vec()
    } else {
        let len_prefix = (salt_extra.len() as u64).to_le_bytes();
        [&len_prefix, salt_extra, parsed.salt].concat()
    };
    let mut kek = derive_kek(password, &kek_salt, parsed.params)?;

    // Reconstruct header AAD (bytes 0..HEADER_LEN of the original blob)
    let mut header_aad = Vec::with_capacity(HEADER_LEN);
    header_aad.extend_from_slice(parsed.magic);
    header_aad.push(FORMAT_VERSION);
    header_aad.extend_from_slice(&parsed.params.to_bytes());
    header_aad.extend_from_slice(parsed.salt);
    header_aad.extend_from_slice(parsed.nonce_kek);

    let kek_cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&kek));
    let mut dek = kek_cipher
        .decrypt(Nonce::from_slice(parsed.nonce_kek), Payload { msg: parsed.wrapped_dek, aad: &header_aad })
        .map_err(|_| EnvelopeError::DecryptFailed)?;
    kek.zeroize();

    if dek.len() != DEK_LEN {
        dek.zeroize();
        return Err(EnvelopeError::DecryptFailed);
    }
    let mut dek_arr = [0u8; DEK_LEN];
    dek_arr.copy_from_slice(&dek);
    dek.zeroize();
    Ok(dek_arr)
}

// === SECTION 2 END ===

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_MAGIC: &[u8; 4] = b"TEST";

    fn make_salt() -> [u8; SALT_LEN] {
        let mut salt = [0u8; SALT_LEN];
        rng().fill_bytes(&mut salt);
        salt
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let salt = make_salt();
        let plaintext = b"hello world secret data";
        let blob = encrypt(TEST_MAGIC, "password123", &salt, &[], Argon2Params::mobile(), plaintext).unwrap();
        let decrypted = decrypt(TEST_MAGIC, "password123", &[], &blob).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_decrypt_with_hw_id() {
        let salt = make_salt();
        let hw_id = "IOPlatformUUID-1234";
        let plaintext = b"credentials with hw binding";
        let blob = encrypt(TEST_MAGIC, "password123", &salt, hw_id.as_bytes(), Argon2Params::mobile(), plaintext).unwrap();
        // Correct hw_id → success
        let decrypted = decrypt(TEST_MAGIC, "password123", hw_id.as_bytes(), &blob).unwrap();
        assert_eq!(decrypted, plaintext);
        // Wrong hw_id → fail
        let result = decrypt(TEST_MAGIC, "password123", b"wrong-hw-id", &blob);
        assert!(matches!(result, Err(EnvelopeError::DecryptFailed)));
    }

    #[test]
    fn test_wrong_password_fails() {
        let salt = make_salt();
        let blob = encrypt(TEST_MAGIC, "correctPassword", &salt, &[], Argon2Params::mobile(), b"data").unwrap();
        let result = decrypt(TEST_MAGIC, "wrongPassword", &[], &blob);
        assert!(matches!(result, Err(EnvelopeError::DecryptFailed)));
    }

    #[test]
    fn test_tamper_header_fails() {
        let salt = make_salt();
        let mut blob = encrypt(TEST_MAGIC, "pw1234567890", &salt, &[], Argon2Params::mobile(), b"secret").unwrap();
        // Tamper with argon2 params (byte 5 = m_cost LSB)
        blob[5] ^= 0xff;
        let result = decrypt(TEST_MAGIC, "pw1234567890", &[], &blob);
        assert!(matches!(result, Err(EnvelopeError::DecryptFailed)));
    }

    #[test]
    fn test_tamper_magic_rejected() {
        let salt = make_salt();
        let mut blob = encrypt(TEST_MAGIC, "pw1234567890", &salt, &[], Argon2Params::mobile(), b"secret").unwrap();
        blob[0] = b'X';
        let result = decrypt(TEST_MAGIC, "pw1234567890", &[], &blob);
        assert!(matches!(result, Err(EnvelopeError::InvalidMagic)));
    }

    #[test]
    fn test_tamper_version_rejected() {
        let salt = make_salt();
        let mut blob = encrypt(TEST_MAGIC, "pw1234567890", &salt, &[], Argon2Params::mobile(), b"secret").unwrap();
        blob[4] = 99;
        let result = decrypt(TEST_MAGIC, "pw1234567890", &[], &blob);
        assert!(matches!(result, Err(EnvelopeError::UnsupportedVersion(99))));
    }

    #[test]
    fn test_tamper_wrapped_dek_fails() {
        let salt = make_salt();
        let mut blob = encrypt(TEST_MAGIC, "pw1234567890", &salt, &[], Argon2Params::mobile(), b"secret").unwrap();
        // Tamper with wrapped DEK (starts at HEADER_LEN)
        blob[HEADER_LEN] ^= 0xff;
        let result = decrypt(TEST_MAGIC, "pw1234567890", &[], &blob);
        assert!(matches!(result, Err(EnvelopeError::DecryptFailed)));
    }

    #[test]
    fn test_tamper_ciphertext_fails() {
        let salt = make_salt();
        let mut blob = encrypt(TEST_MAGIC, "pw1234567890", &salt, &[], Argon2Params::mobile(), b"secret").unwrap();
        // Tamper with ciphertext (starts at PREFIX_LEN)
        blob[PREFIX_LEN] ^= 0xff;
        let result = decrypt(TEST_MAGIC, "pw1234567890", &[], &blob);
        assert!(matches!(result, Err(EnvelopeError::DecryptFailed)));
    }

    #[test]
    fn test_change_password_roundtrip() {
        let salt = make_salt();
        let plaintext = b"important credentials data";
        let blob = encrypt(TEST_MAGIC, "oldPassword", &salt, &[], Argon2Params::mobile(), plaintext).unwrap();
        // Change password
        let new_blob = change_password(TEST_MAGIC, "oldPassword", "newPassword", &[], &blob).unwrap();
        // Old password can't decrypt new blob
        let result = decrypt(TEST_MAGIC, "oldPassword", &[], &new_blob);
        assert!(matches!(result, Err(EnvelopeError::DecryptFailed)));
        // New password can decrypt new blob
        let decrypted = decrypt(TEST_MAGIC, "newPassword", &[], &new_blob).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_change_password_with_hw_id() {
        let salt = make_salt();
        let hw_id = "machine-uuid-5678";
        let plaintext = b"hw-bound credentials";
        let blob = encrypt(TEST_MAGIC, "oldPw", &salt, hw_id.as_bytes(), Argon2Params::mobile(), plaintext).unwrap();
        let new_blob = change_password(TEST_MAGIC, "oldPw", "newPw", hw_id.as_bytes(), &blob).unwrap();
        let decrypted = decrypt(TEST_MAGIC, "newPw", hw_id.as_bytes(), &new_blob).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_change_password_wrong_old_fails() {
        let salt = make_salt();
        let blob = encrypt(TEST_MAGIC, "oldPassword", &salt, &[], Argon2Params::mobile(), b"data").unwrap();
        let result = change_password(TEST_MAGIC, "wrongOld", "newPw", &[], &blob);
        assert!(matches!(result, Err(EnvelopeError::DecryptFailed)));
    }

    #[test]
    fn test_different_params_in_header() {
        let salt = make_salt();
        let desktop_params = Argon2Params::desktop();
        let blob = encrypt(TEST_MAGIC, "pw", &salt, &[], desktop_params, b"data").unwrap();
        // Read params from header
        let read_params_result = read_params(&blob).unwrap();
        assert_eq!(read_params_result, desktop_params);
        // Decrypt reads params from header, not hardcoded
        let decrypted = decrypt(TEST_MAGIC, "pw", &[], &blob).unwrap();
        assert_eq!(decrypted, b"data");
    }

    #[test]
    fn test_nonce_randomness() {
        let salt = make_salt();
        let blob1 = encrypt(TEST_MAGIC, "pw", &salt, &[], Argon2Params::mobile(), b"data").unwrap();
        let blob2 = encrypt(TEST_MAGIC, "pw", &salt, &[], Argon2Params::mobile(), b"data").unwrap();
        // Nonces should differ
        let nonce_kek_1 = &blob1[5 + PARAMS_LEN + SALT_LEN..HEADER_LEN];
        let nonce_kek_2 = &blob2[5 + PARAMS_LEN + SALT_LEN..HEADER_LEN];
        assert_ne!(nonce_kek_1, nonce_kek_2);
        // Full blobs should differ
        assert_ne!(blob1, blob2);
    }

    #[test]
    fn test_nfkc_normalization() {
        let salt = make_salt();
        let nfc_pw = "pass\u{00E9}word12345";
        let nfd_pw = "pass\u{0065}\u{0301}word12345";
        let blob = encrypt(TEST_MAGIC, nfc_pw, &salt, &[], Argon2Params::mobile(), b"data").unwrap();
        let result = decrypt(TEST_MAGIC, nfd_pw, &[], &blob);
        assert!(result.is_ok(), "NFC encrypt + NFD decrypt should succeed with NFKC normalization");
    }

    #[test]
    fn test_password_too_long() {
        let salt = make_salt();
        let long_pw = "a".repeat(1025);
        let result = encrypt(TEST_MAGIC, &long_pw, &salt, &[], Argon2Params::mobile(), b"data");
        assert!(matches!(result, Err(EnvelopeError::PasswordTooLong(_, _))));
    }

    #[test]
    fn test_is_envelope_format() {
        let salt = make_salt();
        let blob = encrypt(TEST_MAGIC, "pw", &salt, &[], Argon2Params::mobile(), b"data").unwrap();
        assert!(is_envelope_format(&blob));
        // v1 format would have version=1
        let mut v1_blob = blob.clone();
        v1_blob[4] = 1;
        assert!(!is_envelope_format(&v1_blob));
    }

    #[test]
    fn test_large_data() {
        let salt = make_salt();
        let plaintext = vec![0xABu8; 100_000]; // 100KB
        let blob = encrypt(TEST_MAGIC, "pw", &salt, &[], Argon2Params::mobile(), &plaintext).unwrap();
        let decrypted = decrypt(TEST_MAGIC, "pw", &[], &blob).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_parse_blob_and_incremental_reencrypt() {
        let salt = make_salt();
        let plaintext1 = b"original data";
        let blob = encrypt(TEST_MAGIC, "pw", &salt, &[], Argon2Params::mobile(), plaintext1).unwrap();

        // Parse the blob
        let parsed = parse_blob(TEST_MAGIC, &blob).unwrap();

        // Unwrap DEK
        let dek = unwrap_dek("pw", &[], &parsed).unwrap();

        // Decrypt data with DEK
        let decrypted = decrypt_data_with_dek(TEST_MAGIC, &dek, parsed.nonce_data, parsed.ciphertext).unwrap();
        assert_eq!(decrypted, plaintext1);

        // Re-encrypt new data with same DEK (incremental save)
        let plaintext2 = b"new data after edit";
        let (new_nonce_data, new_ciphertext) = encrypt_data_with_dek(TEST_MAGIC, &dek, plaintext2).unwrap();

        // Assemble new blob (preserving header + wrapped_dek)
        let new_blob = assemble_blob(
            TEST_MAGIC, parsed.params, parsed.salt, parsed.nonce_kek,
            parsed.wrapped_dek, &new_nonce_data, &new_ciphertext,
        );

        // Decrypt new blob with password — should get new data
        let decrypted2 = decrypt(TEST_MAGIC, "pw", &[], &new_blob).unwrap();
        assert_eq!(decrypted2, plaintext2);
    }

    #[test]
    fn test_unwrap_dek_wrong_password_fails() {
        let salt = make_salt();
        let blob = encrypt(TEST_MAGIC, "correctPw", &salt, &[], Argon2Params::mobile(), b"data").unwrap();
        let parsed = parse_blob(TEST_MAGIC, &blob).unwrap();
        let result = unwrap_dek("wrongPw", &[], &parsed);
        assert!(matches!(result, Err(EnvelopeError::DecryptFailed)));
    }

    #[test]
    fn test_assemble_blob_roundtrip() {
        let salt = make_salt();
        let blob = encrypt(TEST_MAGIC, "pw", &salt, &[], Argon2Params::mobile(), b"test").unwrap();
        let parsed = parse_blob(TEST_MAGIC, &blob).unwrap();
        // Reassemble with same components → should produce identical blob
        let reassembled = assemble_blob(
            TEST_MAGIC, parsed.params, parsed.salt, parsed.nonce_kek,
            parsed.wrapped_dek, parsed.nonce_data, parsed.ciphertext,
        );
        assert_eq!(reassembled, blob);
    }

    #[test]
    fn test_argon2_params_validation_rejects_extreme_values() {
        // m_cost too high (would cause OOM)
        assert!(Argon2Params { m_cost: u32::MAX, t_cost: 3, p_cost: 1 }.validate().is_err());
        // m_cost too low (below Argon2 minimum)
        assert!(Argon2Params { m_cost: 1, t_cost: 3, p_cost: 1 }.validate().is_err());
        // t_cost too high
        assert!(Argon2Params { m_cost: 32768, t_cost: u32::MAX, p_cost: 1 }.validate().is_err());
        // t_cost zero
        assert!(Argon2Params { m_cost: 32768, t_cost: 0, p_cost: 1 }.validate().is_err());
        // p_cost too high
        assert!(Argon2Params { m_cost: 32768, t_cost: 3, p_cost: u16::MAX }.validate().is_err());
        // p_cost zero
        assert!(Argon2Params { m_cost: 32768, t_cost: 3, p_cost: 0 }.validate().is_err());
        // Valid params should pass
        assert!(Argon2Params::desktop().validate().is_ok());
        assert!(Argon2Params::mobile().validate().is_ok());
    }

    #[test]
    fn test_decrypt_rejects_extreme_argon2_params() {
        let salt = make_salt();
        let blob = encrypt(TEST_MAGIC, "pw", &salt, &[], Argon2Params::mobile(), b"data").unwrap();
        // Tamper with m_cost in the header to an extreme value
        let mut tampered = blob.clone();
        // m_cost is at offset 5..9 (4 bytes, little-endian)
        tampered[5..9].copy_from_slice(&u32::MAX.to_le_bytes());
        // Decrypt should fail with Crypto error (params validation), not attempt Argon2
        let result = decrypt(TEST_MAGIC, "pw", &[], &tampered);
        assert!(matches!(result, Err(EnvelopeError::Crypto(_))));
    }
}

// === SECTION 3 END ===


