//! Keyring boundary for identity secrets.
//!
//! The production adapter deliberately collapses backend errors to an absent
//! value during read-only resolution. This mirrors the Go resolver's best
//! effort keyring lookup and prevents backend diagnostics from exposing secret
//! material. Tests use [`FakeKeyring`] and never touch the user's keychain.

use std::sync::{Arc, Mutex};

/// Stable keyring coordinates shared with the Go identity implementation.
pub const SERVICE_NAME: &str = "symeraseme";
pub const USERNAME: &str = "identity-master-key";

/// Opaque keyring failure. It intentionally carries no provider text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyringError;

impl std::fmt::Display for KeyringError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("keyring unavailable")
    }
}

impl std::error::Error for KeyringError {}

/// Read-only keyring operations required by master-key resolution.
pub trait KeyringBackend: Send + Sync {
    /// Return the stored value, or `None` when the entry is absent.
    fn get(&self, service: &str, username: &str) -> Result<Option<String>, KeyringError>;
}

/// Native OS keyring adapter.
#[derive(Clone, Copy, Debug, Default)]
pub struct OsKeyring;

impl KeyringBackend for OsKeyring {
    fn get(&self, service: &str, username: &str) -> Result<Option<String>, KeyringError> {
        let entry = keyring::Entry::new(service, username).map_err(|_| KeyringError)?;
        match entry.get_password() {
            Ok(value) if !value.is_empty() => Ok(Some(value)),
            Ok(_) => Ok(None),
            Err(_) => Ok(None),
        }
    }
}

/// In-memory keyring for deterministic tests and adapter-level differential cases.
#[derive(Clone, Default)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct FakeKeyring {
    value: Arc<Mutex<Option<String>>>,
    calls: Arc<Mutex<Vec<(String, String)>>>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl FakeKeyring {
    /// Create an empty fake keyring.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the encoded key returned by subsequent reads.
    pub fn with_value(value: impl Into<String>) -> Self {
        Self {
            value: Arc::new(Mutex::new(Some(value.into()))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Change the returned value. Intended for ordered-resolution tests.
    pub fn set_value(&self, value: Option<String>) {
        *self.value.lock().expect("fake keyring mutex") = value;
    }

    /// Return the recorded coordinates without returning the secret value.
    pub fn calls(&self) -> Vec<(String, String)> {
        self.calls.lock().expect("fake keyring mutex").clone()
    }
}

impl KeyringBackend for FakeKeyring {
    fn get(&self, service: &str, username: &str) -> Result<Option<String>, KeyringError> {
        self.calls
            .lock()
            .expect("fake keyring mutex")
            .push((service.to_owned(), username.to_owned()));
        Ok(self.value.lock().expect("fake keyring mutex").clone())
    }
}
