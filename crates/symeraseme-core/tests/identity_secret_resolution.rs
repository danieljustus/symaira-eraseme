use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use symeraseme_core::identity::{
    FakeKeyring, KEY_LENGTH, KEYCHAIN_PREFIX, MASTER_KEY_ENV, MasterKeyError,
    SYMVAULT_PASSPHRASE_ENV, SecretBackend, SecretBackendError, SecretResolver,
};

const SENTINEL: &str = "ID003-RESOLUTION-SENTINEL";

#[derive(Clone, Default)]
struct FixtureBackend {
    calls: Arc<Mutex<Vec<String>>>,
    keychain_error: Option<SecretBackendError>,
    symvault_error: Option<SecretBackendError>,
    keyring_error: Option<SecretBackendError>,
    keyring_value: Option<String>,
}

impl SecretBackend for FixtureBackend {
    fn resolve_keychain(&self, service: &str, account: &str) -> Result<String, SecretBackendError> {
        self.calls
            .lock()
            .expect("fixture calls mutex")
            .push(format!("keychain:{service}/{account}"));
        match self.keychain_error {
            Some(error) => Err(error),
            None => Ok(SENTINEL.to_owned()),
        }
    }

    fn resolve_symvault(&self, path: &str) -> Result<String, SecretBackendError> {
        self.calls
            .lock()
            .expect("fixture calls mutex")
            .push(format!("symvault:{path}"));
        match self.symvault_error {
            Some(error) => Err(error),
            None => Ok(SENTINEL.to_owned()),
        }
    }

    fn get_keyring(
        &self,
        service: &str,
        username: &str,
    ) -> Result<Option<String>, SecretBackendError> {
        self.calls
            .lock()
            .expect("fixture calls mutex")
            .push(format!("keyring:{service}/{username}"));
        if let Some(error) = self.keyring_error {
            return Err(error);
        }
        Ok(self.keyring_value.clone())
    }
}

fn assert_error_is_redacted<T: std::fmt::Display + std::fmt::Debug>(error: &T) {
    let display = error.to_string();
    let debug = format!("{error:?}");
    assert!(
        !display.contains(SENTINEL),
        "display leaked sentinel: {display}"
    );
    assert!(!debug.contains(SENTINEL), "debug leaked sentinel: {debug}");
}

#[test]
fn literals_pass_through_without_invoking_a_provider() {
    let backend = FixtureBackend::default();
    let calls = backend.calls.clone();
    let resolver = SecretResolver::new(backend);

    assert_eq!(resolver.resolve(SENTINEL).unwrap(), SENTINEL);
    assert!(calls.lock().expect("fixture calls mutex").is_empty());
}

#[test]
fn env_references_use_the_explicit_environment_and_missing_errors_are_safe() {
    let mut environment = BTreeMap::new();
    environment.insert("ID003_ENV_SECRET".to_owned(), SENTINEL.to_owned());
    let resolver = SecretResolver::with_environment(FixtureBackend::default(), environment);

    assert_eq!(
        resolver.resolve("env://ID003_ENV_SECRET").unwrap(),
        SENTINEL
    );

    let error = resolver.resolve("env://ID003_MISSING").unwrap_err();
    assert_error_is_redacted(&error);
}

#[test]
fn keychain_failures_are_redacted_on_the_public_resolver_path() {
    let backend = FixtureBackend {
        keychain_error: Some(SecretBackendError::KeychainUnavailable),
        ..FixtureBackend::default()
    };
    let calls = backend.calls.clone();
    let resolver = SecretResolver::new(backend);

    let error = resolver
        .resolve("keychain://mail/account")
        .expect_err("keychain failure");
    assert_error_is_redacted(&error);
    assert_eq!(
        calls.lock().expect("fixture calls mutex").as_slice(),
        ["keychain:mail/account".to_string()]
    );
}

#[test]
fn symvault_failures_discard_provider_output_and_keep_fallback_errors_safe() {
    let backend = FixtureBackend {
        symvault_error: Some(SecretBackendError::SymvaultUnavailable),
        keyring_error: Some(SecretBackendError::KeyringUnavailable),
        ..FixtureBackend::default()
    };
    let mut environment = BTreeMap::new();
    environment.insert(
        "ID003_VAULT_FALLBACK".to_owned(),
        format!("{KEYCHAIN_PREFIX}other/account"),
    );
    let resolver = SecretResolver::with_environment(backend, environment)
        .with_env_fallback("ID003_VAULT_FALLBACK")
        .with_keyring_service("symeraseme-test");

    let error = resolver
        .resolve("symvault://team/mail")
        .expect_err("all secret sources fail");
    assert_error_is_redacted(&error);
}

#[test]
fn vault_resolution_preserves_go_fallback_order_and_literal_secret_values() {
    let backend = FixtureBackend {
        symvault_error: Some(SecretBackendError::SymvaultUnavailable),
        keyring_value: Some(SENTINEL.to_owned()),
        ..FixtureBackend::default()
    };
    let mut environment = BTreeMap::new();
    environment.insert("ID003_VAULT_FALLBACK".to_owned(), "env://OTHER".to_owned());
    let resolver = SecretResolver::with_environment(backend, environment)
        .with_env_fallback("ID003_VAULT_FALLBACK")
        .with_keyring_service("symeraseme-test");

    assert_eq!(resolver.resolve("vault://team/mail").unwrap(), SENTINEL);
}

#[test]
fn resolver_debug_output_does_not_include_environment_values() {
    let mut environment = BTreeMap::new();
    environment.insert("ID003_ENV_SECRET".to_owned(), SENTINEL.to_owned());
    let resolver = SecretResolver::with_environment(FixtureBackend::default(), environment)
        .with_env_fallback("ID003_VAULT_FALLBACK")
        .with_keyring_service("symeraseme-test")
        .with_keyring_username("account");

    let debug = format!("{resolver:?}");
    assert!(
        !debug.contains(SENTINEL),
        "resolver debug leaked sentinel: {debug}"
    );
}

#[test]
fn master_key_resolution_errors_do_not_include_env_or_keyring_values() {
    let mut environment = BTreeMap::new();
    environment.insert(MASTER_KEY_ENV.to_owned(), SENTINEL.to_owned());
    let mut resolver = symeraseme_core::identity::MasterKeyResolver::with_environment(
        FakeKeyring::new(),
        environment,
    );
    let error = resolver.resolve_existing().unwrap_err();
    assert!(matches!(error, MasterKeyError::InvalidHex { .. }));
    assert_error_is_redacted(&error);

    let mut resolver = symeraseme_core::identity::MasterKeyResolver::with_environment(
        FakeKeyring::with_value(SENTINEL),
        BTreeMap::from([(SYMVAULT_PASSPHRASE_ENV.to_owned(), String::new())]),
    );
    let error = resolver.resolve_existing().unwrap_err();
    assert!(matches!(error, MasterKeyError::InvalidHex { .. }));
    assert_error_is_redacted(&error);
    assert_eq!(KEY_LENGTH, 32);
}
