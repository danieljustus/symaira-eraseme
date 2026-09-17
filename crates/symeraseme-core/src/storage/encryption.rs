//! Encryption and decryption for the shipped Python-final V1/V2/V3 event-store envelopes.
//!
//! V1 and V2 use PBKDF2-HMAC-SHA256 followed by a standard Fernet token.
//! V3 uses HKDF-SHA256 followed by the same standard Fernet token.

use aes::Aes128;
use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit as AesGcmKeyInit, Nonce};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE;
use cbc::Decryptor;
use cbc::cipher::block_padding::Pkcs7;
use cbc::cipher::{BlockModeDecrypt, KeyIvInit};
use crypto_common::Output as CryptoOutput;
use hmac::{Hmac, KeyInit, Mac};
use pbkdf2::pbkdf2_hmac;
use rand::TryRngCore;
use sha2::Sha256;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::{Zeroize, Zeroizing};

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
    TruncatedEnvelope { version: u8 },
    TruncatedToken,
    InvalidBase64,
    UnsupportedFernetVersion(u8),
    AuthenticationFailed,
    InvalidCiphertextLength,
    InvalidPadding,
    LegacyTokenInvalid,
    RandomnessUnavailable,
    ClockUnavailable,
}

impl fmt::Display for EncryptionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMasterKeyLength { actual } => {
                write!(f, "master key must be 32 bytes (got {actual})")
            }
            Self::UnsupportedEnvelope => f.write_str("eventstore: unrecognized encryption header"),
            Self::TruncatedSalt => f.write_str("truncated V2/V3 encryption salt"),
            Self::TruncatedEnvelope { version } => {
                write!(f, "eventstore: encrypted V{version} envelope is truncated")
            }
            Self::TruncatedToken => {
                f.write_str("eventstore: fernet token invalid, tampered, or unsupported format")
            }
            Self::InvalidBase64 => f.write_str("invalid URL-safe base64 Fernet token"),
            Self::UnsupportedFernetVersion(version) => {
                write!(f, "unsupported Fernet version 0x{version:02x}")
            }
            Self::AuthenticationFailed => f.write_str("Fernet authentication failed"),
            Self::InvalidCiphertextLength => f.write_str("invalid Fernet ciphertext block length"),
            Self::InvalidPadding => f.write_str("invalid Fernet PKCS7 padding"),
            Self::LegacyTokenInvalid => {
                f.write_str("eventstore: fernet token invalid, tampered, or unsupported format")
            }
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
    let result = decrypt_fernet_compatible(token, &key);
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
        return Err(EncryptionError::TruncatedEnvelope {
            version: if header == V2_HEADER { 2 } else { 3 },
        });
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
    let result = decrypt_fernet_compatible(token, &key);
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
    // `CtOutput::into_bytes` clones the digest output, so own that clone directly.
    let pseudorandom_key: Zeroizing<CryptoOutput<HmacSha256>> =
        Zeroizing::new(extract.finalize().into_bytes());
    let mut expand = <HmacSha256 as KeyInit>::new_from_slice(pseudorandom_key.as_slice())
        .map_err(|_| EncryptionError::AuthenticationFailed)?;
    expand.update(V3_HKDF_INFO);
    expand.update(&[1]);
    let block: Zeroizing<CryptoOutput<HmacSha256>> = Zeroizing::new(expand.finalize().into_bytes());
    let mut key = [0_u8; FERNET_KEY_LEN];
    key.copy_from_slice(block.as_slice());
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
    let frame = match URL_SAFE.decode(token) {
        Ok(frame) => frame,
        Err(_) if token.len() >= FERNET_MIN_FRAME_LEN && token[0] == FERNET_VERSION => {
            token.to_vec()
        }
        Err(_) => return Err(EncryptionError::InvalidBase64),
    };
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

const LEGACY_GO_NONCE_LEN: usize = 12;
const LEGACY_GO_GCM_TAG_LEN: usize = 16;
const LEGACY_GO_MIN_FRAME_LEN: usize =
    1 + 8 + LEGACY_GO_NONCE_LEN + LEGACY_GO_GCM_TAG_LEN + FERNET_MAC_LEN;

fn decrypt_fernet_compatible(
    token: &[u8],
    key: &[u8; FERNET_KEY_LEN],
) -> Result<Vec<u8>, EncryptionError> {
    // Match Go's compatibility order: standard Fernet first, including a raw
    // binary standard frame, then the accidental raw Go AES-GCM format. This
    // ordering is required because both raw formats begin with 0x80.
    match decrypt_standard_fernet(token, key) {
        Ok(plaintext) => Ok(plaintext),
        Err(_)
            if token.len() >= LEGACY_GO_MIN_FRAME_LEN && token.first() == Some(&FERNET_VERSION) =>
        {
            match decrypt_legacy_go_token(token, key) {
                Ok(plaintext) => Ok(plaintext),
                Err(_) => Err(EncryptionError::LegacyTokenInvalid),
            }
        }
        Err(error) => Err(error),
    }
}

/// Decrypts the accidental Go AES-256-GCM token format.
///
/// The Go implementation emitted a raw binary frame rather than a base64
/// Fernet token: version (0x80), timestamp, 12-byte nonce, GCM ciphertext
/// including its 16-byte tag, and an outer HMAC-SHA256 over the preceding
/// bytes. Authentication is checked before attempting GCM decryption.
fn decrypt_legacy_go_token(
    token: &[u8],
    key: &[u8; FERNET_KEY_LEN],
) -> Result<Vec<u8>, EncryptionError> {
    if token.len() < LEGACY_GO_MIN_FRAME_LEN {
        return Err(EncryptionError::TruncatedToken);
    }
    if token[0] != FERNET_VERSION {
        return Err(EncryptionError::UnsupportedFernetVersion(token[0]));
    }

    let mac_offset = token.len() - FERNET_MAC_LEN;
    let mut mac = <HmacSha256 as KeyInit>::new_from_slice(key)
        .map_err(|_| EncryptionError::AuthenticationFailed)?;
    mac.update(&token[..mac_offset]);
    mac.verify_slice(&token[mac_offset..])
        .map_err(|_| EncryptionError::AuthenticationFailed)?;

    let nonce = Nonce::try_from(&token[9..9 + LEGACY_GO_NONCE_LEN])
        .map_err(|_| EncryptionError::InvalidCiphertextLength)?;
    let ciphertext = &token[9 + LEGACY_GO_NONCE_LEN..mac_offset];
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|_| EncryptionError::AuthenticationFailed)?;
    cipher
        .decrypt(&nonce, ciphertext)
        .map_err(|_| EncryptionError::AuthenticationFailed)
}

/// Reports whether raw is a known envelope containing the accidental raw Go
/// AES-GCM frame. This is structural detection only; decryption still verifies
/// the outer HMAC and GCM tag.
pub fn is_legacy_go_envelope(raw: &[u8]) -> bool {
    let token_offset = if raw.starts_with(V1_HEADER) {
        V1_HEADER.len()
    } else if raw.starts_with(V2_HEADER) || raw.starts_with(V3_HEADER) {
        let header_len = if raw.starts_with(V2_HEADER) {
            V2_HEADER.len()
        } else {
            V3_HEADER.len()
        };
        header_len + V2_SALT_LEN
    } else {
        return false;
    };
    raw.len() >= token_offset + LEGACY_GO_MIN_FRAME_LEN && raw[token_offset] == FERNET_VERSION
}

pub fn decrypt_v3(envelope: &[u8], master_key: &[u8]) -> Result<Vec<u8>, EncryptionError> {
    if !envelope.starts_with(V3_HEADER) {
        return Err(EncryptionError::UnsupportedEnvelope);
    }
    validate_master_key(master_key)?;
    let token_start = V3_HEADER.len() + V3_SALT_LEN;
    if envelope.len() < token_start {
        return Err(EncryptionError::TruncatedEnvelope { version: 3 });
    }
    let token = &envelope[token_start..];
    if token.is_empty() {
        return Err(EncryptionError::TruncatedToken);
    }
    let mut key = derive_v3_key(master_key, &envelope[V3_HEADER.len()..token_start])?;
    let result = decrypt_fernet_compatible(token, &key);
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

    #[test]
    fn errors_describe_only_format_metadata() {
        let errors = [
            EncryptionError::InvalidMasterKeyLength { actual: 31 },
            EncryptionError::UnsupportedEnvelope,
            EncryptionError::TruncatedSalt,
            EncryptionError::TruncatedToken,
            EncryptionError::InvalidBase64,
            EncryptionError::UnsupportedFernetVersion(0x81),
            EncryptionError::AuthenticationFailed,
            EncryptionError::InvalidCiphertextLength,
            EncryptionError::InvalidPadding,
            EncryptionError::RandomnessUnavailable,
            EncryptionError::ClockUnavailable,
        ];

        for error in errors {
            let rendered = error.to_string();
            assert!(!rendered.contains("master-key-sentinel"));
            assert!(!rendered.contains("plaintext-sentinel"));
        }
    }

    #[test]
    fn writer_rejects_invalid_test_material_lengths() {
        assert_eq!(
            encrypt_v3_with_material(b"x", &KEY, &[0; V3_SALT_LEN - 1], &IV, 0),
            Err(EncryptionError::InvalidCiphertextLength)
        );
        assert_eq!(
            encrypt_v3_with_material(b"x", &KEY, &SALT, &[0; FERNET_IV_LEN - 1], 0),
            Err(EncryptionError::InvalidCiphertextLength)
        );
    }

    #[test]
    fn v3_rejects_short_or_misaligned_authenticated_frames() {
        let mut short_envelope = Vec::from(V3_HEADER);
        short_envelope.extend_from_slice(&SALT);
        short_envelope.extend_from_slice(URL_SAFE.encode([FERNET_VERSION]).as_bytes());
        assert_eq!(
            decrypt_v3(&short_envelope, &KEY),
            Err(EncryptionError::TruncatedToken)
        );

        let mut key = derive_v3_key(&KEY, &SALT).unwrap();
        let mut frame = Vec::from([FERNET_VERSION]);
        frame.extend_from_slice(&0_u64.to_be_bytes());
        frame.extend_from_slice(&IV);
        frame.extend_from_slice(&[0; FERNET_BLOCK_LEN + 1]);
        let mut mac =
            <HmacSha256 as KeyInit>::new_from_slice(&key[..FERNET_SIGNING_KEY_LEN]).unwrap();
        mac.update(&frame);
        frame.extend_from_slice(&mac.finalize().into_bytes());
        key.zeroize();

        let mut misaligned_envelope = Vec::from(V3_HEADER);
        misaligned_envelope.extend_from_slice(&SALT);
        misaligned_envelope.extend_from_slice(URL_SAFE.encode(frame).as_bytes());
        assert_eq!(
            decrypt_v3(&misaligned_envelope, &KEY),
            Err(EncryptionError::InvalidCiphertextLength)
        );
    }

    #[test]
    fn hkdf_finalization_owners_are_zeroizing_and_derive_correctly() {
        let source = include_str!("encryption.rs").replace("\r\n", "\n");
        let derivation = source
            .split_once("fn derive_v3_key")
            .and_then(|(_, body)| body.split_once("\nfn encrypt_standard_fernet"))
            .map(|(body, _)| body)
            .expect("V3 derivation source");

        for stage in ["extract", "expand"] {
            let finalization = format!("Zeroizing::new({stage}.finalize().into_bytes())");
            assert!(
                derivation.contains(&finalization),
                "{stage} finalization output must be directly owned by Zeroizing"
            );
        }
        assert!(
            derivation.contains("let pseudorandom_key: Zeroizing<CryptoOutput<HmacSha256>>"),
            "the HKDF extract output must have an explicit zeroizing owner"
        );
        assert!(
            derivation.contains("let block: Zeroizing<CryptoOutput<HmacSha256>>"),
            "the HKDF expand output must have an explicit zeroizing owner"
        );
        assert!(
            !derivation.contains("copy_from_slice(&extract.finalize().into_bytes())"),
            "destination-only wrapping must not reintroduce a plain extract output"
        );
        assert!(
            !derivation.contains("copy_from_slice(&expand.finalize().into_bytes())"),
            "destination-only wrapping must not reintroduce a plain expand output"
        );

        let derived_key = derive_v3_key(&KEY, &SALT).unwrap();
        let plaintext = b"HKDF finalization ownership regression";
        let token = encrypt_standard_fernet(plaintext, &derived_key, &IV, 1_700_000_000).unwrap();
        let mut envelope = Vec::from(V3_HEADER);
        envelope.extend_from_slice(&SALT);
        envelope.extend_from_slice(&token);
        assert_eq!(decrypt_v3(&envelope, &KEY).unwrap(), plaintext);
    }
}
