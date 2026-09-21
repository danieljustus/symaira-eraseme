//! `init_profile` key minting and the Go rollback of a freshly minted key.
//!
//! Every case uses an explicit environment map and a `FakeKeyring`, so no test
//! reads the process environment or the OS keychain.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use symeraseme_core::identity::{
    FakeKeyring, KeyringBackend, MasterKeyResolver, Profile, ProfileError, ProfilePaths,
    SERVICE_NAME, USERNAME, decrypt_profile_with_key, init_profile,
};

const EXISTING_KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

fn resolver(keyring: FakeKeyring) -> MasterKeyResolver<FakeKeyring> {
    MasterKeyResolver::with_environment(keyring, BTreeMap::new())
}

fn paths(home: &Path) -> ProfilePaths {
    ProfilePaths::new(Some(home.to_owned()), BTreeMap::new())
}

fn sample() -> Profile {
    Profile {
        full_name: "Oracle User".to_owned(),
        email_addresses: vec!["oracle@example.invalid".to_owned()],
        ..Profile::default()
    }
}

/// A parent that is an existing regular file makes the directory creation fail.
fn unwritable_target(directory: &Path) -> PathBuf {
    let blocked = directory.join("blocked");
    std::fs::write(&blocked, b"not a directory").unwrap();
    blocked.join("identity.encrypted")
}

#[test]
fn init_profile_mints_persists_and_encrypts_with_the_new_key() {
    let home = tempfile::tempdir().unwrap();
    let keyring = FakeKeyring::new();
    let mut keys = resolver(keyring.clone());

    let target = init_profile(&sample(), Path::new(""), &paths(home.path()), &mut keys).unwrap();
    assert_eq!(
        target,
        home.path().join(".config/symeraseme/identity.encrypted")
    );

    let stored = keyring.get(SERVICE_NAME, USERNAME).unwrap().unwrap();
    let key = hex::decode(&stored).unwrap();
    assert_eq!(key.len(), 32);
    let raw = std::fs::read(&target).unwrap();
    let (plaintext, envelope) = decrypt_profile_with_key(&raw, &key).unwrap();
    assert_eq!(envelope.version, 2);
    assert!(
        String::from_utf8(plaintext)
            .unwrap()
            .contains("oracle@example.invalid")
    );
}

#[test]
fn failed_save_rolls_back_a_freshly_minted_key() {
    let home = tempfile::tempdir().unwrap();
    let keyring = FakeKeyring::new();
    let mut keys = resolver(keyring.clone());

    let error = init_profile(
        &sample(),
        &unwritable_target(home.path()),
        &paths(home.path()),
        &mut keys,
    )
    .unwrap_err();

    assert_eq!(error, ProfileError::Mkdir);
    assert_eq!(keyring.get(SERVICE_NAME, USERNAME).unwrap(), None);
}

#[test]
fn failed_save_keeps_a_key_that_already_existed() {
    let home = tempfile::tempdir().unwrap();
    let keyring = FakeKeyring::with_value(EXISTING_KEY);
    let mut keys = resolver(keyring.clone());

    let error = init_profile(
        &sample(),
        &unwritable_target(home.path()),
        &paths(home.path()),
        &mut keys,
    )
    .unwrap_err();

    assert_eq!(error, ProfileError::Mkdir);
    assert_eq!(
        keyring.get(SERVICE_NAME, USERNAME).unwrap().as_deref(),
        Some(EXISTING_KEY)
    );
}
