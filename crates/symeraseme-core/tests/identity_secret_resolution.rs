use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use symeraseme_core::identity::{
    FakeKeyring, KEY_LENGTH, KEYCHAIN_PREFIX, MASTER_KEY_ENV, MasterKey, MasterKeyError,
    SERVICE_NAME, SYMVAULT_PASSPHRASE_ENV, SecretBackend, SecretBackendError, SecretResolver,
    USERNAME,
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
fn master_key_sources_follow_go_priority_and_cache() {
    let keyring =
        FakeKeyring::with_value("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
    let mut resolver = symeraseme_core::identity::MasterKeyResolver::with_environment(
        keyring.clone(),
        BTreeMap::from([
            (
                MASTER_KEY_ENV.to_owned(),
                "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff".to_owned(),
            ),
            (SYMVAULT_PASSPHRASE_ENV.to_owned(), "passphrase".to_owned()),
        ]),
    );

    let cached = MasterKey::from_bytes(&[0x7a; KEY_LENGTH]).unwrap();
    resolver.set_cached(cached.clone());
    assert_eq!(resolver.resolve_existing().unwrap(), cached);
    assert!(keyring.calls().is_empty());

    resolver.clear_cache();
    let direct = resolver.resolve_existing().unwrap();
    assert_eq!(
        hex::encode(direct.as_bytes()),
        "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff"
    );

    resolver.set_environment(MASTER_KEY_ENV, "");
    assert_eq!(resolver.resolve_existing().unwrap(), direct);
    assert!(keyring.calls().is_empty());

    resolver.clear_cache();
    let passphrase = resolver.resolve_existing().unwrap();
    assert_eq!(
        hex::encode(passphrase.as_bytes()),
        [
            "eb9e67f7", "1d018a2b", "b6fe9680", "90a09ec3", "cbeb52fe", "00b9e9fa", "159e6385",
            "1c6384cd",
        ]
        .concat()
    );
    assert!(keyring.calls().is_empty());

    resolver.clear_cache();
    resolver.set_environment(SYMVAULT_PASSPHRASE_ENV, "");
    assert_eq!(resolver.resolve_existing().unwrap().as_bytes()[0], 0xff);
    assert_eq!(
        keyring.calls(),
        vec![(SERVICE_NAME.to_owned(), USERNAME.to_owned())]
    );
}

#[test]
fn malformed_master_sources_fail_closed_at_their_priority() {
    let keyring =
        FakeKeyring::with_value("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
    let mut resolver = symeraseme_core::identity::MasterKeyResolver::with_environment(
        keyring.clone(),
        BTreeMap::from([(MASTER_KEY_ENV.to_owned(), "not-hex".to_owned())]),
    );
    assert_eq!(
        resolver.resolve_existing(),
        Err(MasterKeyError::InvalidHex {
            source: MASTER_KEY_ENV
        })
    );
    assert!(keyring.calls().is_empty());

    resolver.set_environment(MASTER_KEY_ENV, "");
    keyring.set_value(Some("short".to_owned()));
    assert!(matches!(
        resolver.resolve_existing(),
        Err(MasterKeyError::InvalidHex { .. })
    ));

    let mut missing = symeraseme_core::identity::MasterKeyResolver::with_environment(
        FakeKeyring::new(),
        BTreeMap::new(),
    );
    assert_eq!(missing.resolve_existing(), Err(MasterKeyError::Missing));
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
fn supported_uri_forms_delegate_and_legacy_vault_alias_normalizes() {
    let backend = FixtureBackend::default();
    let calls = backend.calls.clone();
    let resolver = SecretResolver::with_environment(
        backend,
        BTreeMap::from([("ID003_ENV_SECRET".to_owned(), SENTINEL.to_owned())]),
    );

    assert_eq!(
        resolver.resolve("env://ID003_ENV_SECRET").unwrap(),
        SENTINEL
    );
    assert_eq!(
        resolver.resolve("keychain://mail/account").unwrap(),
        SENTINEL
    );
    assert_eq!(resolver.resolve("symvault://team/mail").unwrap(), SENTINEL);
    assert_eq!(resolver.resolve("vault://team/mail").unwrap(), SENTINEL);
    assert_eq!(
        calls.lock().expect("fixture calls mutex").as_slice(),
        [
            "keychain:mail/account".to_owned(),
            "symvault:team/mail".to_owned(),
            "symvault:team/mail".to_owned(),
        ]
    );
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
        keyring_value: Some("keyring-secret".to_owned()),
        ..FixtureBackend::default()
    };
    let calls = backend.calls.clone();
    let mut environment = BTreeMap::new();
    environment.insert("ID003_VAULT_FALLBACK".to_owned(), "env://OTHER".to_owned());
    let resolver = SecretResolver::with_environment(backend, environment)
        .with_env_fallback("ID003_VAULT_FALLBACK")
        .with_keyring_service("symeraseme-test");

    assert_eq!(
        resolver.resolve("vault://team/mail").unwrap(),
        "keyring-secret"
    );
    assert_eq!(
        calls.lock().expect("fixture calls mutex").as_slice(),
        [
            "symvault:team/mail".to_owned(),
            "keyring:symeraseme-test/team/mail".to_owned(),
        ]
    );
}

#[test]
fn vault_fallback_prefers_literal_environment_before_keyring() {
    let backend = FixtureBackend {
        symvault_error: Some(SecretBackendError::SymvaultUnavailable),
        keyring_value: Some("keyring-secret".to_owned()),
        ..FixtureBackend::default()
    };
    let calls = backend.calls.clone();
    let resolver = SecretResolver::with_environment(
        backend,
        BTreeMap::from([("ID003_VAULT_FALLBACK".to_owned(), SENTINEL.to_owned())]),
    )
    .with_env_fallback("ID003_VAULT_FALLBACK")
    .with_keyring_service("symeraseme-test");

    assert_eq!(resolver.resolve("symvault://team/mail").unwrap(), SENTINEL);
    assert_eq!(
        calls.lock().expect("fixture calls mutex").as_slice(),
        ["symvault:team/mail".to_owned()]
    );
}

#[test]
fn malformed_uri_references_stop_before_provider_or_fallback() {
    let backend = FixtureBackend::default();
    let calls = backend.calls.clone();
    let resolver = SecretResolver::new(backend)
        .with_env_fallback("ID003_VAULT_FALLBACK")
        .with_keyring_service("symeraseme-test");

    assert!(resolver.resolve("keychain://mail").is_err());
    assert!(resolver.resolve("vault://").is_err());
    assert!(resolver.resolve("symvault://").is_err());
    assert!(calls.lock().expect("fixture calls mutex").is_empty());
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
