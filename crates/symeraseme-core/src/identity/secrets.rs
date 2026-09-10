//! Master-key resolution compatible with the Go identity package.
//!
//! Resolution order is deliberately explicit: in-process cache, direct
//! hexadecimal environment value, scrypt-derived passphrase, then OS keyring.
//! Read paths never create or replace a key.

use super::keyring::{KeyringBackend, OsKeyring, SERVICE_NAME, USERNAME};
use rand::RngCore;
use scrypt::{Params, scrypt};
use std::collections::BTreeMap;
use std::fmt;
use zeroize::Zeroize;

/// Direct hexadecimal master-key environment variable.
pub const MASTER_KEY_ENV: &str = "SYMERASEME_IDENTITY_MASTER_KEY";
/// Ecosystem passphrase environment variable.
pub const SYMVAULT_PASSPHRASE_ENV: &str = "SYMVAULT_PASSPHRASE";
/// AES-256 key size in bytes.
pub const KEY_LENGTH: usize = 32;
/// Domain-separated salt used by the Go resolver.
pub const PASSPHRASE_SALT: &[u8] = b"symeraseme-identity-master-key-v1";

/// A zeroized 32-byte identity master key.
#[derive(Clone, Eq, PartialEq)]
pub struct MasterKey([u8; KEY_LENGTH]);

impl MasterKey {
    /// Construct a key, rejecting every length other than 32 bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, MasterKeyError> {
        if bytes.len() != KEY_LENGTH {
            return Err(MasterKeyError::InvalidLength {
                source: "master key",
                actual: bytes.len(),
            });
        }
        let mut key = [0_u8; KEY_LENGTH];
        key.copy_from_slice(bytes);
        Ok(Self(key))
    }

    /// Borrow the key for a cryptographic operation.
    pub fn as_bytes(&self) -> &[u8; KEY_LENGTH] {
        &self.0
    }
}

impl fmt::Debug for MasterKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MasterKey([REDACTED])")
    }
}

impl Drop for MasterKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// Errors from master-key lookup. None of the variants contains secret input.
#[derive(Debug, Eq, PartialEq)]
pub enum MasterKeyError {
    /// No configured source supplied a key.
    Missing,
    /// An environment or keyring value was not hexadecimal.
    InvalidHex { source: &'static str },
    /// An encoded key did not decode to exactly 32 bytes.
    InvalidLength { source: &'static str, actual: usize },
    /// The memory-hard derivation parameters could not be constructed.
    DerivationFailed,
}

impl fmt::Display for MasterKeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => formatter.write_str("identity: master key missing"),
            Self::InvalidHex { source } => {
                write!(formatter, "identity: {source}: invalid hex")
            }
            Self::InvalidLength { source, actual } => write!(
                formatter,
                "identity: {source} must be {KEY_LENGTH} bytes, got {actual}"
            ),
            Self::DerivationFailed => formatter.write_str("identity: derive master key failed"),
        }
    }
}

impl std::error::Error for MasterKeyError {}

/// Master-key resolver with explicit environment and keyring dependencies.
///
/// `from_process` is the thin process adapter. Tests should use `new` and
/// provide an explicit environment map so ambient credentials cannot affect
/// the result.
pub struct MasterKeyResolver<K = OsKeyring> {
    keyring: K,
    environment: BTreeMap<String, String>,
    cached: Option<MasterKey>,
}

impl MasterKeyResolver<OsKeyring> {
    /// Build a resolver from the two relevant process environment variables.
    pub fn from_process() -> Self {
        let mut environment = BTreeMap::new();
        for name in [MASTER_KEY_ENV, SYMVAULT_PASSPHRASE_ENV] {
            if let Ok(value) = std::env::var(name) {
                environment.insert(name.to_owned(), value);
            }
        }
        Self::with_environment(OsKeyring, environment)
    }
}

impl<K: KeyringBackend> MasterKeyResolver<K> {
    /// Build a resolver with an injected keyring and no ambient environment.
    pub fn new(keyring: K) -> Self {
        Self {
            keyring,
            environment: BTreeMap::new(),
            cached: None,
        }
    }

    /// Build a resolver with a fully explicit environment.
    pub fn with_environment(keyring: K, environment: BTreeMap<String, String>) -> Self {
        Self {
            keyring,
            environment,
            cached: None,
        }
    }

    /// Set or replace one environment value for this resolver.
    pub fn set_environment(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.environment.insert(name.into(), value.into());
    }

    /// Store a validated key in the in-process cache.
    pub fn set_cached(&mut self, key: MasterKey) {
        self.cached = Some(key);
    }

    /// Clear the in-process cache.
    pub fn clear_cache(&mut self) {
        self.cached = None;
    }

    /// Resolve an existing key without creating one.
    pub fn resolve_existing(&mut self) -> Result<MasterKey, MasterKeyError> {
        if let Some(key) = &self.cached {
            return Ok(key.clone());
        }

        if let Some(value) = self.environment.get(MASTER_KEY_ENV)
            && !value.is_empty()
        {
            let key = decode_key(MASTER_KEY_ENV, value)?;
            self.cached = Some(key.clone());
            return Ok(key);
        }

        if let Some(passphrase) = self.environment.get(SYMVAULT_PASSPHRASE_ENV)
            && !passphrase.is_empty()
        {
            let key = derive_passphrase_key(passphrase.as_bytes())?;
            self.cached = Some(key.clone());
            return Ok(key);
        }

        if let Ok(Some(value)) = self.keyring.get(SERVICE_NAME, USERNAME)
            && !value.is_empty()
        {
            let key = decode_key("stored keychain master key", &value)?;
            self.cached = Some(key.clone());
            return Ok(key);
        }

        Err(MasterKeyError::Missing)
    }
}

/// Generate a fresh random AES-256 key for explicit initialization callers.
pub fn generate_master_key() -> MasterKey {
    let mut bytes = [0_u8; KEY_LENGTH];
    rand::rng().fill_bytes(&mut bytes);
    MasterKey(bytes)
}

fn decode_key(source: &'static str, value: &str) -> Result<MasterKey, MasterKeyError> {
    let bytes = hex::decode(value).map_err(|_| MasterKeyError::InvalidHex { source })?;
    MasterKey::from_bytes(&bytes).map_err(|error| match error {
        MasterKeyError::InvalidLength { actual, .. } => {
            MasterKeyError::InvalidLength { source, actual }
        }
        other => other,
    })
}

fn derive_passphrase_key(passphrase: &[u8]) -> Result<MasterKey, MasterKeyError> {
    let params = Params::new(15, 8, 1).map_err(|_| MasterKeyError::DerivationFailed)?;
    let mut bytes = [0_u8; KEY_LENGTH];
    scrypt(passphrase, PASSPHRASE_SALT, &params, &mut bytes)
        .map_err(|_| MasterKeyError::DerivationFailed)?;
    Ok(MasterKey(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::keyring::FakeKeyring;
    use std::collections::BTreeMap;

    fn resolver(keyring: FakeKeyring, values: &[(&str, &str)]) -> MasterKeyResolver<FakeKeyring> {
        let environment = values
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect::<BTreeMap<_, _>>();
        MasterKeyResolver::with_environment(keyring, environment)
    }

    #[test]
    fn cache_has_highest_priority() {
        let keyring = FakeKeyring::with_value("not-a-key");
        let mut resolver = resolver(keyring.clone(), &[(MASTER_KEY_ENV, "also-not-a-key")]);
        let expected = MasterKey::from_bytes(&[7; KEY_LENGTH]).unwrap();
        resolver.set_cached(expected.clone());

        assert_eq!(resolver.resolve_existing().unwrap(), expected);
        assert!(keyring.calls().is_empty());
    }

    #[test]
    fn direct_hex_beats_passphrase_and_keyring() {
        let key_bytes: [u8; KEY_LENGTH] = std::array::from_fn(|index| index as u8);
        let encoded_key = hex::encode(key_bytes);
        let keyring = FakeKeyring::with_value(
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        );
        let mut resolver = resolver(
            keyring.clone(),
            &[
                (MASTER_KEY_ENV, encoded_key.as_str()),
                (SYMVAULT_PASSPHRASE_ENV, "ignored-passphrase"),
            ],
        );

        assert_eq!(resolver.resolve_existing().unwrap().as_bytes()[0], 0x00);
        assert!(keyring.calls().is_empty());
    }

    #[test]
    fn invalid_direct_hex_fails_closed_before_lower_sources() {
        let keyring = FakeKeyring::with_value(
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        );
        let mut resolver = resolver(keyring.clone(), &[(MASTER_KEY_ENV, "not-hex")]);

        assert_eq!(
            resolver.resolve_existing(),
            Err(MasterKeyError::InvalidHex {
                source: MASTER_KEY_ENV
            })
        );
        assert!(keyring.calls().is_empty());
    }

    #[test]
    fn passphrase_derivation_matches_go_parameters() {
        let keyring = FakeKeyring::new();
        let mut resolver = resolver(keyring, &[(SYMVAULT_PASSPHRASE_ENV, "passphrase")]);
        let key = resolver.resolve_existing().unwrap();
        assert_eq!(
            hex::encode(key.as_bytes()),
            "eb9e67f71d018a2bb6fe968090a09ec3cbeb52fe00b9e9fa159e63851c6384cd"
        );
    }

    #[test]
    fn keyring_is_last_and_uses_stable_coordinates() {
        let keyring = FakeKeyring::with_value(
            "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
        );
        let mut resolver = resolver(keyring.clone(), &[]);
        let key = resolver.resolve_existing().unwrap();

        assert_eq!(key.as_bytes()[0], 0x00);
        assert_eq!(
            keyring.calls(),
            vec![(SERVICE_NAME.into(), USERNAME.into())]
        );
    }

    #[test]
    fn missing_key_is_distinct_and_debug_is_redacted() {
        let mut resolver = resolver(FakeKeyring::new(), &[]);
        assert_eq!(resolver.resolve_existing(), Err(MasterKeyError::Missing));
        let secret = MasterKey::from_bytes(&[0x5a; KEY_LENGTH]).unwrap();
        assert!(!format!("{secret:?}").contains("5a5a"));
    }

    #[test]
    fn generated_key_has_the_contract_length() {
        assert_eq!(generate_master_key().as_bytes().len(), KEY_LENGTH);
    }
}
