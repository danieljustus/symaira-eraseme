//! Keyring boundary for identity secrets.
//!
//! The production adapter deliberately collapses backend errors to an absent
//! value during read-only resolution. This mirrors the Go resolver's best
//! effort keyring lookup and prevents backend diagnostics from exposing secret
//! material. Reads also accept values written by the Go era's `go-keyring`
//! macOS backend. Tests use [`FakeKeyring`] and never touch the user's
//! keychain, except for one explicitly ignored macOS probe.

use base64::Engine as _;
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

    /// Durably store a value. Explicit initialization only.
    fn set(&self, service: &str, username: &str, value: &str) -> Result<(), KeyringError>;

    /// Remove the entry. An absent entry is not an error.
    fn delete(&self, service: &str, username: &str) -> Result<(), KeyringError>;
}

/// Prefixes used by the Go era's `go-keyring` macOS backend.
const GO_HEX_PREFIX: &str = "go-keyring-encoded:";
const GO_BASE64_PREFIX: &str = "go-keyring-base64:";

/// Decode a value written by the Go era's `go-keyring` macOS backend.
///
/// That backend stored secrets as `go-keyring-base64:<standard base64>` and
/// its reader also accepted the older `go-keyring-encoded:<hex>` form, both
/// after trimming whitespace. Unprefixed values are returned trimmed.
/// Decode failures are opaque and carry none of the stored input.
fn decode_go_keyring_value(value: &str) -> Result<String, KeyringError> {
    let trimmed = value.trim();
    if let Some(encoded) = trimmed.strip_prefix(GO_HEX_PREFIX) {
        let bytes = hex::decode(encoded).map_err(|_| KeyringError)?;
        String::from_utf8(bytes).map_err(|_| KeyringError)
    } else if let Some(encoded) = trimmed.strip_prefix(GO_BASE64_PREFIX) {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| KeyringError)?;
        String::from_utf8(bytes).map_err(|_| KeyringError)
    } else {
        Ok(trimmed.to_owned())
    }
}

/// Native OS keyring adapter.
#[derive(Clone, Copy, Debug, Default)]
pub struct OsKeyring;

impl KeyringBackend for OsKeyring {
    fn get(&self, service: &str, username: &str) -> Result<Option<String>, KeyringError> {
        let entry = keyring::Entry::new(service, username).map_err(|_| KeyringError)?;
        let value = match entry.get_password() {
            Ok(value) if !value.is_empty() => value,
            // Absent entries and backend failures are indistinguishable to
            // read-only callers by design.
            Ok(_) | Err(_) => return Ok(None),
        };
        decode_go_keyring_value(&value).map(Some)
    }

    fn set(&self, service: &str, username: &str, value: &str) -> Result<(), KeyringError> {
        let entry = keyring::Entry::new(service, username).map_err(|_| KeyringError)?;
        // Rust is the writer now: master keys are already plain hex, so do not
        // re-apply the Go-era base64 prefix. Reads still decode both historical
        // prefixes so existing keychain items remain usable.
        entry.set_password(value).map_err(|_| KeyringError)
    }

    fn delete(&self, service: &str, username: &str) -> Result<(), KeyringError> {
        let entry = keyring::Entry::new(service, username).map_err(|_| KeyringError)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(KeyringError),
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

    fn set(&self, _service: &str, _username: &str, value: &str) -> Result<(), KeyringError> {
        self.set_value(Some(value.to_owned()));
        Ok(())
    }

    fn delete(&self, _service: &str, _username: &str) -> Result<(), KeyringError> {
        self.set_value(None);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Dummy value only: never use a real master key in tests.
    const HEX_VALUE: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    #[test]
    fn go_keyring_plain_value_is_trimmed() {
        let stored = format!("  {HEX_VALUE}\n");
        assert_eq!(decode_go_keyring_value(&stored).unwrap(), HEX_VALUE);
    }

    #[test]
    fn go_keyring_hex_prefixed_value_is_decoded() {
        let stored = format!("{GO_HEX_PREFIX}{}", hex::encode(HEX_VALUE.as_bytes()));
        assert_eq!(decode_go_keyring_value(&stored).unwrap(), HEX_VALUE);
    }

    #[test]
    fn go_keyring_base64_prefixed_value_is_decoded() {
        let encoded = base64::engine::general_purpose::STANDARD.encode(HEX_VALUE);
        let stored = format!("{GO_BASE64_PREFIX}{encoded}");
        assert_eq!(decode_go_keyring_value(&stored).unwrap(), HEX_VALUE);
    }

    #[test]
    fn go_keyring_invalid_encoded_values_are_opaque_errors() {
        for stored in [
            format!("{GO_HEX_PREFIX}not-hex"),
            format!("{GO_BASE64_PREFIX}not-base64!!!"),
            format!("{GO_BASE64_PREFIX}ff/8="),
        ] {
            let error = decode_go_keyring_value(&stored).unwrap_err();
            assert_eq!(error, KeyringError);
            assert_eq!(error.to_string(), "keyring unavailable");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "writes a dedicated probe item to the real macOS Keychain; run explicitly with --ignored"]
    fn os_keyring_mac_keychain_round_trip() {
        const PROBE_SERVICE: &str = "symeraseme-keyring-probe";
        const PROBE_ACCOUNT: &str = "round-trip";
        let backend = OsKeyring;

        // Remove any item left behind by an interrupted earlier probe run.
        let _ = backend.delete(PROBE_SERVICE, PROBE_ACCOUNT);

        let outcome = (|| -> Result<(bool, bool), KeyringError> {
            backend.set(PROBE_SERVICE, PROBE_ACCOUNT, HEX_VALUE)?;
            let read_back =
                backend.get(PROBE_SERVICE, PROBE_ACCOUNT)? == Some(HEX_VALUE.to_owned());
            // Existence only: never request or print the stored value.
            let visible = std::process::Command::new("security")
                .args(["find-generic-password", "-s", PROBE_SERVICE])
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false);
            Ok((read_back, visible))
        })();

        let cleanup = backend.delete(PROBE_SERVICE, PROBE_ACCOUNT);
        let (read_back, visible) = outcome.expect("keychain probe failed");
        assert!(cleanup.is_ok(), "keychain probe cleanup failed");
        assert!(read_back, "probe value did not read back");
        assert!(visible, "probe item not visible to security");
        assert_eq!(
            backend
                .get(PROBE_SERVICE, PROBE_ACCOUNT)
                .expect("post-delete lookup"),
            None
        );
    }
}
