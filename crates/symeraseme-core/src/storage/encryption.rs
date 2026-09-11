//! Encryption and decryption for the shipped Python-final V1/V2/V3 event-store envelopes.
//!
//! V1 and V2 use PBKDF2-HMAC-SHA256 followed by a standard Fernet token.
//! V3 uses HKDF-SHA256 followed by the same standard Fernet token.

use aes::Aes128;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE;
use cbc::Decryptor;
use cbc::cipher::block_padding::Pkcs7;
use cbc::cipher::{BlockModeDecrypt, KeyIvInit};
use hmac::{Hmac, KeyInit, Mac};
use pbkdf2::pbkdf2_hmac;
use rand::TryRngCore;
use sha2::Sha256;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::Zeroize;

pub const V1_HEADER: &[u8] = b"SYMERASEME_ENCv1\n";
pub const V2_HEADER: &[u8] = b"SYMERASEME_ENCv2\n";
pub const V3_HEADER: &[u8] = b"SYMERASEME_ENCv3\n";
pub const V2_SALT_LEN: usize = 16;
pub const V3_SALT_LEN: usize = 16;
pub const V1_FIXED_SALT: &[u8] = b"symeraseme-db-encryption-v1";
pub const PBKDF2_ITERATIONS: u32 = 600_000;
pub const V3_HKDF_INFO: &[u8] = b"symeraseme-db-encryption-v3";

const MASTER_KEY_LEN: usize = 32;
const FERNET_KEY_LEN: usize = 32;
const FERNET_VERSION: u8 = 0x80;
const FERNET_IV_LEN: usize = 16;
const FERNET_MAC_LEN: usize = 32;
const FERNET_BLOCK_LEN: usize = 16;
const FERNET_MIN_FRAME_LEN: usize = 1 + 8 + FERNET_IV_LEN + FERNET_BLOCK_LEN + FERNET_MAC_LEN;
const FERNET_SIGNING_KEY_LEN: usize = 16;

type HmacSha256 = Hmac<Sha256>;
type Aes128CbcDecryptor = Decryptor<Aes128>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncryptionError {
    InvalidMasterKeyLength { actual: usize },
    UnsupportedEnvelope,
    TruncatedSalt,
    TruncatedToken,
    InvalidBase64,
    UnsupportedFernetVersion(u8),
    AuthenticationFailed,
    InvalidCiphertextLength,
    InvalidPadding,
    RandomnessUnavailable,
    ClockUnavailable,
}

impl fmt::Display for EncryptionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMasterKeyLength { actual } => {
                write!(f, "master key must be 32 bytes (got {actual})")
            }
            Self::UnsupportedEnvelope => f.write_str("unsupported encryption envelope"),
            Self::TruncatedSalt => f.write_str("truncated V2/V3 encryption salt"),
            Self::TruncatedToken => f.write_str("truncated Fernet token"),
            Self::InvalidBase64 => f.write_str("invalid URL-safe base64 Fernet token"),
            Self::UnsupportedFernetVersion(version) => {
                write!(f, "unsupported Fernet version 0x{version:02x}")
            }
            Self::AuthenticationFailed => f.write_str("Fernet authentication failed"),
            Self::InvalidCiphertextLength => f.write_str("invalid Fernet ciphertext block length"),
            Self::InvalidPadding => f.write_str("invalid Fernet PKCS7 padding"),
            Self::RandomnessUnavailable => f.write_str("OS randomness unavailable"),
            Self::ClockUnavailable => f.write_str("system clock unavailable"),
        }
    }
}
impl std::error::Error for EncryptionError {}

pub fn decrypt_v1(envelope: &[u8], master_key: &[u8]) -> Result<Vec<u8>, EncryptionError> {
    if !envelope.starts_with(V1_HEADER) {
        return Err(EncryptionError::UnsupportedEnvelope);
    }
    validate_master_key(master_key)?;
    let token = &envelope[V1_HEADER.len()..];
    if token.is_empty() {
        return Err(EncryptionError::TruncatedToken);
    }
    let mut key = [0_u8; FERNET_KEY_LEN];
    pbkdf2_hmac::<Sha256>(master_key, V1_FIXED_SALT, PBKDF2_ITERATIONS, &mut key);
    let result = decrypt_standard_fernet(token, &key);
    key.zeroize();
    result
}

pub fn decrypt_v2(envelope: &[u8], master_key: &[u8]) -> Result<Vec<u8>, EncryptionError> {
    decrypt_pbkdf2_envelope(envelope, master_key, V2_HEADER)
}

fn decrypt_pbkdf2_envelope(
    envelope: &[u8],
    master_key: &[u8],
    header: &[u8],
) -> Result<Vec<u8>, EncryptionError> {
    if !envelope.starts_with(header) {
        return Err(EncryptionError::UnsupportedEnvelope);
    }
    validate_master_key(master_key)?;
    let token_start = header.len() + V2_SALT_LEN;
    if envelope.len() < token_start {
        return Err(EncryptionError::TruncatedSalt);
    }
    let token = &envelope[token_start..];
    if token.is_empty() {
        return Err(EncryptionError::TruncatedToken);
    }
    let mut key = [0_u8; FERNET_KEY_LEN];
    pbkdf2_hmac::<Sha256>(
        master_key,
        &envelope[header.len()..token_start],
        PBKDF2_ITERATIONS,
        &mut key,
    );
    let result = decrypt_standard_fernet(token, &key);
    key.zeroize();
    result
}

/// Encrypts plaintext as a V3 envelope using OS CSPRNG salt and IV.
/// The master key is borrowed and is never included in errors or output.
pub fn encrypt_v3(plaintext: &[u8], master_key: &[u8]) -> Result<Vec<u8>, EncryptionError> {
    validate_master_key(master_key)?;
    let mut salt = [0_u8; V3_SALT_LEN];
    let mut iv = [0_u8; FERNET_IV_LEN];
    if rand::rngs::OsRng.try_fill_bytes(&mut salt).is_err() {
        salt.zeroize();
        iv.zeroize();
        return Err(EncryptionError::RandomnessUnavailable);
    }
    if rand::rngs::OsRng.try_fill_bytes(&mut iv).is_err() {
        salt.zeroize();
        iv.zeroize();
        return Err(EncryptionError::RandomnessUnavailable);
    }
    let timestamp = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(_) => {
            salt.zeroize();
            iv.zeroize();
            return Err(EncryptionError::ClockUnavailable);
        }
    };
    let result = encrypt_v3_with_material(plaintext, master_key, &salt, &iv, timestamp);
    salt.zeroize();
    iv.zeroize();
    result
}

fn encrypt_v3_with_material(
    plaintext: &[u8],
    master_key: &[u8],
    salt: &[u8],
    iv: &[u8],
    timestamp: u64,
) -> Result<Vec<u8>, EncryptionError> {
    validate_master_key(master_key)?;
    if salt.len() != V3_SALT_LEN || iv.len() != FERNET_IV_LEN {
        return Err(EncryptionError::InvalidCiphertextLength);
    }
    let mut key = derive_v3_key(master_key, salt)?;
    let result = encrypt_standard_fernet(plaintext, &key, iv, timestamp);
    key.zeroize();
    result.map(|token| {
        let mut envelope = Vec::with_capacity(V3_HEADER.len() + salt.len() + token.len());
        envelope.extend_from_slice(V3_HEADER);
        envelope.extend_from_slice(salt);
        envelope.extend_from_slice(&token);
        envelope
    })
}

fn validate_master_key(key: &[u8]) -> Result<(), EncryptionError> {
    if key.len() == MASTER_KEY_LEN {
        Ok(())
    } else {
        Err(EncryptionError::InvalidMasterKeyLength { actual: key.len() })
    }
}

fn derive_v3_key(master_key: &[u8], salt: &[u8]) -> Result<[u8; FERNET_KEY_LEN], EncryptionError> {
    let mut extract = <HmacSha256 as KeyInit>::new_from_slice(salt)
        .map_err(|_| EncryptionError::AuthenticationFailed)?;
    extract.update(master_key);
    let pseudorandom_key = extract.finalize().into_bytes();
    let mut expand = <HmacSha256 as KeyInit>::new_from_slice(&pseudorandom_key)
        .map_err(|_| EncryptionError::AuthenticationFailed)?;
    expand.update(V3_HKDF_INFO);
    expand.update(&[1]);
    let block = expand.finalize().into_bytes();
    let mut key = [0_u8; FERNET_KEY_LEN];
    key.copy_from_slice(&block);
    Ok(key)
}

fn encrypt_standard_fernet(
    plaintext: &[u8],
    key: &[u8; FERNET_KEY_LEN],
    iv: &[u8],
    timestamp: u64,
) -> Result<Vec<u8>, EncryptionError> {
    use cbc::Encryptor;
    use cbc::cipher::{BlockModeEncrypt, KeyIvInit};
    let mut padded = vec![0_u8; plaintext.len() + FERNET_BLOCK_LEN];
    padded[..plaintext.len()].copy_from_slice(plaintext);
    let ciphertext = Encryptor::<Aes128>::new_from_slices(&key[FERNET_SIGNING_KEY_LEN..], iv)
        .map_err(|_| EncryptionError::InvalidCiphertextLength)?
        .encrypt_padded::<Pkcs7>(&mut padded, plaintext.len())
        .map_err(|_| EncryptionError::InvalidPadding)?;
    let mut frame = Vec::with_capacity(1 + 8 + FERNET_IV_LEN + ciphertext.len() + FERNET_MAC_LEN);
    frame.push(FERNET_VERSION);
    frame.extend_from_slice(&timestamp.to_be_bytes());
    frame.extend_from_slice(iv);
    frame.extend_from_slice(ciphertext);
    let mut mac = <HmacSha256 as KeyInit>::new_from_slice(&key[..FERNET_SIGNING_KEY_LEN])
        .map_err(|_| EncryptionError::AuthenticationFailed)?;
    mac.update(&frame);
    frame.extend_from_slice(&mac.finalize().into_bytes());
    Ok(URL_SAFE.encode(frame).into_bytes())
}

fn decrypt_standard_fernet(
    token: &[u8],
    key: &[u8; FERNET_KEY_LEN],
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
    let mac_offset = frame.len() - FERNET_MAC_LEN;
    let mut mac = <HmacSha256 as KeyInit>::new_from_slice(&key[..FERNET_SIGNING_KEY_LEN])
        .map_err(|_| EncryptionError::AuthenticationFailed)?;
    mac.update(&frame[..mac_offset]);
    mac.verify_slice(&frame[mac_offset..])
        .map_err(|_| EncryptionError::AuthenticationFailed)?;
    let ciphertext = &frame[9 + FERNET_IV_LEN..mac_offset];
    if ciphertext.is_empty() || ciphertext.len() % FERNET_BLOCK_LEN != 0 {
        return Err(EncryptionError::InvalidCiphertextLength);
    }
    let mut padded = ciphertext.to_vec();
    let plaintext =
        Aes128CbcDecryptor::new_from_slices(&key[FERNET_SIGNING_KEY_LEN..], &frame[9..25])
            .map_err(|_| EncryptionError::InvalidCiphertextLength)?
            .decrypt_padded::<Pkcs7>(&mut padded)
            .map_err(|_| EncryptionError::InvalidPadding)?;
    Ok(plaintext.to_vec())
}

pub fn decrypt_v3(envelope: &[u8], master_key: &[u8]) -> Result<Vec<u8>, EncryptionError> {
    if !envelope.starts_with(V3_HEADER) {
        return Err(EncryptionError::UnsupportedEnvelope);
    }
    validate_master_key(master_key)?;
    let token_start = V3_HEADER.len() + V3_SALT_LEN;
    if envelope.len() < token_start {
        return Err(EncryptionError::TruncatedSalt);
    }
    let token = &envelope[token_start..];
    if token.is_empty() {
        return Err(EncryptionError::TruncatedToken);
    }
    let mut key = derive_v3_key(master_key, &envelope[V3_HEADER.len()..token_start])?;
    let result = decrypt_standard_fernet(token, &key);
    key.zeroize();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    const KEY: [u8; 32] = [0x11; 32];
    const SALT: [u8; 16] = [0x22; 16];
    const IV: [u8; 16] = [0x33; 16];

    #[test]
    fn deterministic_vector_is_stable_and_round_trips() {
        let envelope = encrypt_v3_with_material(
            b"SQLite format 3\0\xff boundary",
            &KEY,
            &SALT,
            &IV,
            1_700_000_000,
        )
        .unwrap();
        assert_eq!(
            envelope,
            encrypt_v3_with_material(
                b"SQLite format 3\0\xff boundary",
                &KEY,
                &SALT,
                &IV,
                1_700_000_000,
            )
            .unwrap()
        );
        assert_eq!(
            decrypt_v3(&envelope, &KEY).unwrap(),
            b"SQLite format 3\0\xff boundary"
        );
    }

    #[test]
    fn writer_handles_empty_and_block_boundaries_with_fresh_randomness() {
        for plaintext in [b"".as_slice(), &[0_u8; 16][..], &[0_u8; 17][..]] {
            let first = encrypt_v3(plaintext, &KEY).unwrap();
            let second = encrypt_v3(plaintext, &KEY).unwrap();
            assert_ne!(
                first, second,
                "production V3 writes must not be deterministic"
            );
            assert_eq!(decrypt_v3(&first, &KEY).unwrap(), plaintext);
            assert_eq!(decrypt_v3(&second, &KEY).unwrap(), plaintext);
        }
    }

    #[test]
    fn writer_rejects_short_keys_and_authentication_failures() {
        assert_eq!(
            encrypt_v3(b"x", &[0; 31]),
            Err(EncryptionError::InvalidMasterKeyLength { actual: 31 })
        );
        let envelope = encrypt_v3(b"x", &KEY).unwrap();
        assert_eq!(
            decrypt_v3(&envelope, &[0; 32]),
            Err(EncryptionError::AuthenticationFailed)
        );
        let mut tampered = envelope;
        *tampered.last_mut().unwrap() = b'!';
        assert_eq!(
            decrypt_v3(&tampered, &KEY),
            Err(EncryptionError::InvalidBase64)
        );
    }
}
