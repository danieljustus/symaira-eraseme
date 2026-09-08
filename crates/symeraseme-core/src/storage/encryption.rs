//! Decryption for the shipped Python-final V1/V2/V3 event-store envelopes.
//!
//! V1 and V2 use PBKDF2-HMAC-SHA256 followed by a standard Fernet token.
//! V3 uses HKDF-SHA256 followed by the same standard Fernet token. This module
//! intentionally implements only the read contracts needed by CRY-001,
//! CRY-002, and CRY-003; writes and legacy Go payloads are separate migration
//! slices.

use aes::Aes128;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE;
use cbc::Decryptor;
use cbc::cipher::block_padding::Pkcs7;
use cbc::cipher::{BlockModeDecrypt, KeyIvInit};
use hmac::{Hmac, KeyInit, Mac};
use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;
use std::fmt;

/// The V1 event-store envelope header, including its trailing newline.
pub const V1_HEADER: &[u8] = b"SYMERASEME_ENCv1\n";

/// The V2 event-store envelope header, including its trailing newline.
pub const V2_HEADER: &[u8] = b"SYMERASEME_ENCv2\n";

/// The V3 event-store envelope header, including its trailing newline.
pub const V3_HEADER: &[u8] = b"SYMERASEME_ENCv3\n";

/// The per-file V2 salt length.
pub const V2_SALT_LEN: usize = 16;

/// The per-file V3 salt length.
pub const V3_SALT_LEN: usize = 16;

/// The fixed salt used by Python-final and the Go V1 compatibility path.
pub const V1_FIXED_SALT: &[u8] = b"symeraseme-db-encryption-v1";

/// The V1 PBKDF2 work factor.
pub const PBKDF2_ITERATIONS: u32 = 600_000;

/// The V3 HKDF info label.
pub const V3_HKDF_INFO: &[u8] = b"symeraseme-db-encryption-v3";

const MASTER_KEY_LEN: usize = 32;
const FERNET_KEY_LEN: usize = 32;
const FERNET_VERSION: u8 = 0x80;
const FERNET_IV_LEN: usize = 16;
const FERNET_MAC_LEN: usize = 32;
const FERNET_BLOCK_LEN: usize = 16;
const FERNET_MIN_FRAME_LEN: usize =
    1 + std::mem::size_of::<u64>() + FERNET_IV_LEN + FERNET_BLOCK_LEN + FERNET_MAC_LEN;
const FERNET_SIGNING_KEY_LEN: usize = 16;

type HmacSha256 = Hmac<Sha256>;
type Aes128CbcDecryptor = Decryptor<Aes128>;

/// Errors returned while authenticating or decrypting a versioned envelope.
///
/// Messages contain only format metadata. No key or decrypted bytes are
/// retained in the error value or exposed through `Display`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncryptionError {
    /// The caller supplied a master key with a length other than 32 bytes.
    InvalidMasterKeyLength { actual: usize },
    /// The envelope does not begin with the exact V1 header.
    UnsupportedEnvelope,
    /// The V2 or V3 envelope does not contain its complete per-file salt.
    TruncatedSalt,
    /// The decoded Fernet frame is shorter than the authenticated minimum.
    TruncatedToken,
    /// The token is not padded URL-safe base64.
    InvalidBase64,
    /// The Fernet version byte is not the standard value.
    UnsupportedFernetVersion(u8),
    /// The token's HMAC does not authenticate its frame.
    AuthenticationFailed,
    /// The authenticated ciphertext is not a non-empty AES block sequence.
    InvalidCiphertextLength,
    /// The authenticated ciphertext does not contain valid PKCS7 padding.
    InvalidPadding,
}

impl fmt::Display for EncryptionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMasterKeyLength { actual } => {
                write!(formatter, "master key must be 32 bytes (got {actual})")
            }
            Self::UnsupportedEnvelope => formatter.write_str("unsupported encryption envelope"),
            Self::TruncatedSalt => formatter.write_str("truncated V2 encryption salt"),
            Self::TruncatedToken => formatter.write_str("truncated Fernet token"),
            Self::InvalidBase64 => formatter.write_str("invalid URL-safe base64 Fernet token"),
            Self::UnsupportedFernetVersion(version) => {
                write!(formatter, "unsupported Fernet version 0x{version:02x}")
            }
            Self::AuthenticationFailed => formatter.write_str("Fernet authentication failed"),
            Self::InvalidCiphertextLength => {
                formatter.write_str("invalid Fernet ciphertext block length")
            }
            Self::InvalidPadding => formatter.write_str("invalid Fernet PKCS7 padding"),
        }
    }
}

impl std::error::Error for EncryptionError {}

/// Decrypts a Python-final standard-Fernet V1 event-store envelope.
///
/// The envelope is `V1_HEADER || token`. The 32-byte caller-provided master
/// key is expanded with PBKDF2-HMAC-SHA256 using [`V1_FIXED_SALT`] and
/// [`PBKDF2_ITERATIONS`]. The derived Fernet key is split as standard Fernet
/// requires: its first 16 bytes authenticate the version/timestamp/IV/
/// ciphertext frame, and its last 16 bytes decrypt AES-128-CBC. Authentication
/// is verified before any CBC decryption or PKCS7 unpadding.
pub fn decrypt_v1(envelope: &[u8], master_key: &[u8]) -> Result<Vec<u8>, EncryptionError> {
    if !envelope.starts_with(V1_HEADER) {
        return Err(EncryptionError::UnsupportedEnvelope);
    }
    if master_key.len() != MASTER_KEY_LEN {
        return Err(EncryptionError::InvalidMasterKeyLength {
            actual: master_key.len(),
        });
    }

    let token = &envelope[V1_HEADER.len()..];
    if token.is_empty() {
        return Err(EncryptionError::TruncatedToken);
    }

    let mut fernet_key = [0_u8; FERNET_KEY_LEN];
    pbkdf2_hmac::<Sha256>(
        master_key,
        V1_FIXED_SALT,
        PBKDF2_ITERATIONS,
        &mut fernet_key,
    );

    decrypt_standard_fernet(token, &fernet_key)
}

/// Decrypts a Python-final standard-Fernet V2 event-store envelope.
///
/// The envelope is `V2_HEADER || salt || token`. The per-file 16-byte salt
/// is used directly for the same PBKDF2-HMAC-SHA256 derivation as V1.
/// Authentication is verified before any CBC decryption or PKCS7 unpadding.
pub fn decrypt_v2(envelope: &[u8], master_key: &[u8]) -> Result<Vec<u8>, EncryptionError> {
    if !envelope.starts_with(V2_HEADER) {
        return Err(EncryptionError::UnsupportedEnvelope);
    }
    if master_key.len() != MASTER_KEY_LEN {
        return Err(EncryptionError::InvalidMasterKeyLength {
            actual: master_key.len(),
        });
    }

    let token_start = V2_HEADER.len() + V2_SALT_LEN;
    if envelope.len() < token_start {
        return Err(EncryptionError::TruncatedSalt);
    }
    let token = &envelope[token_start..];
    if token.is_empty() {
        return Err(EncryptionError::TruncatedToken);
    }

    let salt = &envelope[V2_HEADER.len()..token_start];
    let mut fernet_key = [0_u8; FERNET_KEY_LEN];
    pbkdf2_hmac::<Sha256>(master_key, salt, PBKDF2_ITERATIONS, &mut fernet_key);

    decrypt_standard_fernet(token, &fernet_key)
}

/// Decrypts a Python-final standard-Fernet V3 event-store envelope.
///
/// The envelope is `V3_HEADER || salt || token`. The per-file 16-byte salt
/// is used with HKDF-SHA256 and [`V3_HKDF_INFO`] to derive the standard Fernet
/// key. Authentication is verified before any CBC decryption or PKCS7
/// unpadding.
pub fn decrypt_v3(envelope: &[u8], master_key: &[u8]) -> Result<Vec<u8>, EncryptionError> {
    if !envelope.starts_with(V3_HEADER) {
        return Err(EncryptionError::UnsupportedEnvelope);
    }
    if master_key.len() != MASTER_KEY_LEN {
        return Err(EncryptionError::InvalidMasterKeyLength {
            actual: master_key.len(),
        });
    }

    let token_start = V3_HEADER.len() + V3_SALT_LEN;
    if envelope.len() < token_start {
        return Err(EncryptionError::TruncatedSalt);
    }
    let token = &envelope[token_start..];
    if token.is_empty() {
        return Err(EncryptionError::TruncatedToken);
    }

    let salt = &envelope[V3_HEADER.len()..token_start];
    let fernet_key = derive_v3_key(master_key, salt)?;
    decrypt_standard_fernet(token, &fernet_key)
}

fn derive_v3_key(master_key: &[u8], salt: &[u8]) -> Result<[u8; FERNET_KEY_LEN], EncryptionError> {
    // HKDF-Extract: PRK = HMAC-SHA256(salt, master_key). V3 always supplies
    // its fixed-width per-file salt, so the RFC 5869 absent-salt default is
    // not used here.
    let mut extract = <HmacSha256 as Mac>::new_from_slice(salt)
        .map_err(|_| EncryptionError::AuthenticationFailed)?;
    extract.update(master_key);
    let pseudorandom_key = extract.finalize().into_bytes();

    // HKDF-Expand for one 32-byte block: T(1) = HMAC(PRK, info || 0x01).
    let mut expand = <HmacSha256 as Mac>::new_from_slice(&pseudorandom_key)
        .map_err(|_| EncryptionError::AuthenticationFailed)?;
    expand.update(V3_HKDF_INFO);
    expand.update(&[1]);
    let block = expand.finalize().into_bytes();
    let mut key = [0_u8; FERNET_KEY_LEN];
    key.copy_from_slice(&block);
    Ok(key)
}

fn decrypt_standard_fernet(
    token: &[u8],
    fernet_key: &[u8; FERNET_KEY_LEN],
) -> Result<Vec<u8>, EncryptionError> {
    let frame = URL_SAFE
        .decode(token)
        .map_err(|_| EncryptionError::InvalidBase64)?;
    if frame.len() < FERNET_MIN_FRAME_LEN {
        return Err(EncryptionError::TruncatedToken);
    }
    if frame[0] != FERNET_VERSION {
        return Err(EncryptionError::UnsupportedFernetVersion(frame[0]));
    }

    // The eight-byte big-endian timestamp is part of the authenticated frame.
    // This decrypt-only API intentionally does not apply a Fernet TTL.
    let _timestamp = &frame[1..9];
    let mac_offset = frame.len() - FERNET_MAC_LEN;
    let mut mac = <HmacSha256 as KeyInit>::new_from_slice(&fernet_key[..FERNET_SIGNING_KEY_LEN])
        .map_err(|_| EncryptionError::AuthenticationFailed)?;
    mac.update(&frame[..mac_offset]);
    mac.verify_slice(&frame[mac_offset..])
        .map_err(|_| EncryptionError::AuthenticationFailed)?;

    let iv = &frame[9..9 + FERNET_IV_LEN];
    let ciphertext = &frame[9 + FERNET_IV_LEN..mac_offset];
    if ciphertext.is_empty() || ciphertext.len() % FERNET_BLOCK_LEN != 0 {
        return Err(EncryptionError::InvalidCiphertextLength);
    }

    let decryptor = Aes128CbcDecryptor::new_from_slices(&fernet_key[FERNET_SIGNING_KEY_LEN..], iv)
        .map_err(|_| EncryptionError::InvalidCiphertextLength)?;
    let mut padded = ciphertext.to_vec();
    let plaintext = decryptor
        .decrypt_padded::<Pkcs7>(&mut padded)
        .map_err(|_| EncryptionError::InvalidPadding)?;
    Ok(plaintext.to_vec())
}
