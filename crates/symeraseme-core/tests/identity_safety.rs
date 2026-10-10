//! Release-safety regressions using only disposable stores and a fake keyring.

use std::collections::BTreeMap;
use std::sync::{Arc, Barrier};
use symeraseme_core::identity::{
    ConsentError, ConsentOptions, ConsentStore, FakeKeyring, KeyringBackend, MASTER_KEY_ENV,
    MasterKeyResolver, Profile, ProfileError, ProfilePaths, SERVICE_NAME, USERNAME, init_profile,
};

#[test]
fn invalid_key_sources_never_replace_an_existing_key() {
    for invalid in ["not-hex", "01"] {
        let existing = hex::encode([0x4a; 32]);
        let keyring = FakeKeyring::with_value(&existing);
        let mut keys = MasterKeyResolver::with_environment(
            keyring.clone(),
            BTreeMap::from([(MASTER_KEY_ENV.to_owned(), invalid.to_owned())]),
        );
        let expected = keys.resolve_existing().unwrap_err();
        assert_eq!(keys.init(), Err(expected));
        assert_eq!(keyring.get(SERVICE_NAME, USERNAME).unwrap(), Some(existing));
    }
}

#[test]
fn invalid_profile_key_input_preserves_existing_key_and_profile() {
    for blocked_target in [false, true] {
        let home = tempfile::tempdir().unwrap();
        let paths = ProfilePaths::new(Some(home.path().to_owned()), BTreeMap::new());
        let target = home.path().join("identity.encrypted");
        let profile = Profile {
            full_name: "Regression User".to_owned(),
            email_addresses: vec!["regression@example.invalid".to_owned()],
            ..Profile::default()
        };
        let existing = hex::encode([0x4a; 32]);
        let keyring = FakeKeyring::with_value(&existing);
        let mut keys = MasterKeyResolver::new(keyring.clone());
        init_profile(&profile, &target, &paths, &mut keys).unwrap();
        let before = std::fs::read(&target).unwrap();
        let mut invalid = MasterKeyResolver::with_environment(
            keyring.clone(),
            BTreeMap::from([(MASTER_KEY_ENV.to_owned(), "not-hex".to_owned())]),
        );
        let requested_target = if blocked_target {
            let blocked = home.path().join("blocked");
            std::fs::write(&blocked, b"not a directory").unwrap();
            blocked.join("identity.encrypted")
        } else {
            target.clone()
        };
        assert!(matches!(
            init_profile(&profile, &requested_target, &paths, &mut invalid),
            Err(ProfileError::Key(_))
        ));
        assert_eq!(keyring.get(SERVICE_NAME, USERNAME).unwrap(), Some(existing));
        assert_eq!(std::fs::read(&target).unwrap(), before);
    }
}

#[test]
fn overlapping_authorizations_accept_a_single_token_only_once() {
    let directory = tempfile::tempdir().unwrap();
    let issuer = ConsentStore::new(directory.path()).with_clock(|| 1000);
    let token = issuer.issue_token("delete", 60).unwrap();
    let rendezvous = Arc::new(Barrier::new(2));
    let store = ConsentStore::new(directory.path()).with_clock(move || {
        // Both callers have read and validated the record before either consumes it.
        rendezvous.wait();
        1000
    });
    let options = ConsentOptions {
        consent_token: Some(token.clone()),
        ..Default::default()
    };
    let outcomes = std::thread::scope(|scope| {
        let first = scope.spawn(|| store.authorize("delete", &options));
        let second = scope.spawn(|| store.authorize("delete", &options));
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, Err(ConsentError::NotFound)))
            .count(),
        1
    );
    assert_eq!(
        issuer.verify_token("delete", &token),
        Err(ConsentError::NotFound)
    );
    // Standalone cleanup remains idempotent; authorization is stricter.
    issuer.consume_token(&token).unwrap();
}
