use base64::Engine;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use symeraseme_core::identity::{
    FakeKeyring, MasterKey, MasterKeyError, MasterKeyResolver, ProfileError, ProfilePaths,
    load_profile, profile_exists,
};

const ORACLE: &str = "bf53346eec234929bedf0314b99e3da85dbb991b";

#[derive(Deserialize)]
struct Capture {
    oracle: String,
    helper_sha256: String,
    validator_sha256: String,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    name: String,
    path: String,
    environment: BTreeMap<String, String>,
    files: BTreeMap<String, Option<String>>,
    directories: Vec<String>,
    key: String,
    expected: Expected,
}
#[derive(Deserialize)]
struct Expected {
    exists: bool,
    profile: Value,
    class: String,
    error: String,
    key_reads: usize,
}

fn classify(error: &ProfileError) -> &'static str {
    match error {
        ProfileError::NotFound => "not_found",
        ProfileError::Stat => "stat",
        ProfileError::Read => "read",
        ProfileError::NoSeparator => "separator",
        ProfileError::Header => "header",
        ProfileError::LegacyV0 => "legacy_v0",
        ProfileError::Key(MasterKeyError::Missing) => "key_missing",
        ProfileError::Key(_) => panic!("unexpected key error"),
        ProfileError::Nonce => "nonce",
        ProfileError::Authentication => "authentication",
        ProfileError::Json => "json",
    }
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, (bool, bool, Vec<u8>)> {
    fn walk(root: &Path, path: &Path, result: &mut BTreeMap<PathBuf, (bool, bool, Vec<u8>)>) {
        let metadata = fs::symlink_metadata(path).unwrap();
        assert!(!metadata.is_symlink());
        result.insert(
            path.strip_prefix(root).unwrap().to_owned(),
            (
                metadata.is_dir(),
                metadata.permissions().readonly(),
                if metadata.is_file() {
                    fs::read(path).unwrap()
                } else {
                    Vec::new()
                },
            ),
        );
        if metadata.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                walk(root, &entry.unwrap().path(), result);
            }
        }
    }
    let mut result = BTreeMap::new();
    walk(root, root, &mut result);
    result
}

#[test]
fn profile_corpus_matches_go() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let capture: Capture = serde_json::from_slice(
        &fs::read(root.join("rust-tests/parity/oracle/profile_read/cases.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(capture.oracle, ORACLE);
    for (path, expected) in [
        (
            "rust-tests/parity/oracle/profile_read/main.go",
            capture.helper_sha256,
        ),
        (
            "rust-tests/parity/profile_read.py",
            capture.validator_sha256,
        ),
    ] {
        let bytes = fs::read(root.join(path)).unwrap();
        // Tracked helper text is LF on every native checkout (see attributes).
        assert_eq!(hex::encode(Sha256::digest(&bytes)), expected, "{path}");
    }
    assert_eq!(capture.cases.len(), 46);
    let mut executed = BTreeSet::new();
    for case in capture.cases {
        assert!(executed.insert(case.name.clone()));
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path();
        for name in &case.directories {
            fs::create_dir_all(home.join(name)).unwrap();
        }
        for (name, value) in &case.files {
            let path = home.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(value.as_deref().unwrap_or(""))
                .unwrap();
            fs::write(path, bytes).unwrap();
        }
        let before = snapshot(home);
        let expand = |value: &str| value.replace("$ROOT", home.to_str().unwrap());
        let environment = case
            .environment
            .iter()
            .map(|(k, v)| (k.clone(), expand(v)))
            .collect();
        let paths = ProfilePaths::new(Some(home.to_owned()), environment);
        let keyring = FakeKeyring::new();
        let mut keys = MasterKeyResolver::new(keyring.clone());
        if case.key != "missing" {
            let byte = if case.key == "wrong" { 0x43 } else { 0x42 };
            keys.set_cached(MasterKey::from_bytes(&[byte; 32]).unwrap());
        }
        let path = PathBuf::from(expand(&case.path));
        assert_eq!(
            profile_exists(&path, &paths),
            case.expected.exists,
            "{} existence",
            case.name
        );
        match load_profile(&path, &paths, &mut keys) {
            Ok(profile) => {
                assert_eq!(case.expected.class, "ok", "{}", case.name);
                assert_eq!(
                    serde_json::to_value(profile).unwrap(),
                    case.expected.profile,
                    "{}",
                    case.name
                );
            }
            Err(error) => {
                let expected_class = if case.expected.class == "nonce_panic" {
                    // Go's malformed-nonce panic is captured, not reproduced.
                    // The read-only API fails closed with a typed error.
                    assert_eq!(case.name, "short-nonce");
                    "nonce"
                } else {
                    case.expected.class.as_str()
                };
                assert_eq!(classify(&error), expected_class, "{}: {error}", case.name);
                assert_eq!(case.expected.profile, Value::Null);
                // Exact Go diagnostics where no untrusted parser/OS excerpts
                // need redaction. Other branches compare the typed outcome.
                if matches!(
                    error,
                    ProfileError::NotFound
                        | ProfileError::NoSeparator
                        | ProfileError::LegacyV0
                        | ProfileError::Authentication
                        | ProfileError::Key(_)
                ) {
                    assert_eq!(error.to_string(), case.expected.error, "{}", case.name);
                }
                for sentinel in ["TEST", "example.invalid", home.to_str().unwrap()] {
                    assert!(!error.to_string().contains(sentinel));
                    assert!(!format!("{error:?}").contains(sentinel));
                }
            }
        }
        assert_eq!(
            keyring.calls().len(),
            case.expected.key_reads,
            "{} key lookup",
            case.name
        );
        assert_eq!(snapshot(home), before, "{} wrote filesystem", case.name);
        println!("profile case PASS: {}", case.name);
    }
    assert_eq!(executed.len(), 46);
}

#[test]
fn malformed_nonce_is_error_not_go_panic() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("profile");
    fs::write(&path, b"{\"version\":2,\"nonce\":\"00\"}\nTEST").unwrap();
    let mut keys = MasterKeyResolver::new(FakeKeyring::new());
    keys.set_cached(MasterKey::from_bytes(&[0x42; 32]).unwrap());
    assert!(matches!(
        load_profile(&path, &ProfilePaths::default(), &mut keys),
        Err(ProfileError::Nonce)
    ));
}

#[cfg(unix)]
#[test]
fn special_file_is_rejected_before_key_lookup() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("profile");
    let _socket = std::os::unix::net::UnixListener::bind(&path).unwrap();
    let keyring = FakeKeyring::new();
    let mut keys = MasterKeyResolver::new(keyring.clone());
    assert!(profile_exists(&path, &ProfilePaths::default()));
    assert!(matches!(
        load_profile(&path, &ProfilePaths::default(), &mut keys),
        Err(ProfileError::Read)
    ));
    assert!(keyring.calls().is_empty());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(32))]
    #[test]
    fn arbitrary_envelopes_never_panic_or_write(raw in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..4096)) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("profile");
        fs::write(&path, &raw).unwrap();
        let mut keys = MasterKeyResolver::new(FakeKeyring::new());
        keys.set_cached(MasterKey::from_bytes(&[0x42;32]).unwrap());
        let _result = load_profile(&path, &ProfilePaths::default(), &mut keys);
        proptest::prop_assert_eq!(fs::read(&path).unwrap(), raw);
        proptest::prop_assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
