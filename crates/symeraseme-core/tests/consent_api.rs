use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use symeraseme_core::identity::{
    CONSENT_FILE_MODE, ConsentError, ConsentOptions, ConsentRecord, ConsentStore, ConsentToken,
    read_consent_file,
};
use tempfile::{TempDir, tempdir};

fn fixed_store(directory: &Path, now: i64, fill: u8) -> ConsentStore {
    ConsentStore::new(directory)
        .with_clock(move || now)
        .with_random_source(move |length| Ok(vec![fill; length]))
}

fn write_record(path: &Path, record: &ConsentRecord) {
    fs::write(path, serde_json::to_vec(record).unwrap()).unwrap();
}

fn record(command: &str, issued_at: i64, expires_at: i64, token: Option<&str>) -> ConsentRecord {
    ConsentRecord {
        command: command.to_owned(),
        issued_at,
        expires_at,
        token: token.map(str::to_owned),
    }
}

fn consent_files(directory: &Path) -> Vec<PathBuf> {
    let mut paths = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("consent_") && name.ends_with(".json"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;

    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn consent_store_rejects_invalid_records_and_maps_expiry_and_commands() {
    let directory = tempdir().unwrap();
    let store = fixed_store(directory.path(), 1_000, 7);

    assert_eq!(
        store.verify_token("delete", ""),
        Err(ConsentError::NotFound)
    );
    assert_eq!(
        store.verify_token("delete", "missing"),
        Err(ConsentError::NotFound)
    );

    fs::write(directory.path().join("consent_malformed.json"), b"{").unwrap();
    assert_eq!(
        store.verify_token("delete", "malformed"),
        Err(ConsentError::NotFound)
    );

    write_record(
        &directory.path().join("consent_payload.json"),
        &record("delete", 900, 2_000, Some("another-token")),
    );
    assert_eq!(
        store.verify_token("delete", "payload"),
        Err(ConsentError::NotFound)
    );

    write_record(
        &directory.path().join("consent_command.json"),
        &record("other", 900, 2_000, None),
    );
    assert_eq!(
        store.verify_token("delete", "command"),
        Err(ConsentError::CommandMismatch)
    );

    write_record(
        &directory.path().join("consent_expired.json"),
        &record("delete", 100, 999, None),
    );
    assert_eq!(
        store.verify_token("delete", "expired"),
        Err(ConsentError::Expired)
    );
    assert_eq!(
        store.verify_token("delete", "expired"),
        Err(ConsentError::NotFound)
    );
}

#[test]
fn consent_store_lists_in_order_prunes_expired_and_skips_invalid_entries() {
    let directory = tempdir().unwrap();
    let earlier = fixed_store(directory.path(), 10, 1)
        .issue_token("earlier", 100)
        .unwrap();
    let later = fixed_store(directory.path(), 20, 2)
        .issue_token("later", 100)
        .unwrap();
    let expired = fixed_store(directory.path(), 0, 3)
        .issue_token("expired", 1)
        .unwrap();

    let legacy_path = directory.path().join("consent_legacy.json");
    write_record(&legacy_path, &record("legacy", 15, 100, None));
    fs::write(directory.path().join("consent_bad.json"), b"not-json").unwrap();
    fs::create_dir(directory.path().join("consent_directory.json")).unwrap();
    #[cfg(unix)]
    fs::set_permissions(&legacy_path, fs::Permissions::from_mode(0o644)).unwrap();

    let listed = fixed_store(directory.path(), 30, 9).list_tokens().unwrap();
    assert_eq!(
        listed,
        vec![
            ConsentToken {
                token: earlier,
                command: "earlier".to_owned(),
                issued_at: 10,
                expires_at: 110,
            },
            ConsentToken {
                token: "legacy".to_owned(),
                command: "legacy".to_owned(),
                issued_at: 15,
                expires_at: 100,
            },
            ConsentToken {
                token: later,
                command: "later".to_owned(),
                issued_at: 20,
                expires_at: 120,
            },
        ]
    );
    assert_eq!(
        fixed_store(directory.path(), 30, 9).verify_token("expired", &expired),
        Err(ConsentError::NotFound)
    );
    assert!(directory.path().join("consent_bad.json").exists());
    assert!(directory.path().join("consent_directory.json").is_dir());
    #[cfg(unix)]
    assert_eq!(mode(&legacy_path), CONSENT_FILE_MODE);
}

#[test]
fn consent_store_handles_idempotent_removals_random_sources_and_io_errors() {
    let directory = tempdir().unwrap();
    let store = fixed_store(directory.path(), 1_000, 4);

    assert_eq!(store.consume_token(""), Ok(()));
    assert_eq!(store.revoke_token(""), Ok(false));
    assert_eq!(store.consume_token("missing"), Ok(()));
    assert_eq!(store.revoke_token("missing"), Ok(false));

    let token = store.issue_token("delete", 60).unwrap();
    assert_eq!(store.revoke_token(&token), Ok(true));
    assert_eq!(store.revoke_token(&token), Ok(false));
    assert_eq!(store.consume_token(&token), Ok(()));

    let random_error = ConsentStore::new(directory.path())
        .with_random_source(|_| Err(ConsentError::Denied))
        .issue_token("delete", 60);
    assert_eq!(random_error, Err(ConsentError::Denied));

    let invalid_length = ConsentStore::new(directory.path())
        .with_random_source(|_| Ok(vec![1, 2]))
        .issue_token("delete", 60);
    match invalid_length {
        Err(ConsentError::Io(error)) => assert_eq!(error.kind(), io::ErrorKind::InvalidData),
        other => panic!("invalid random length result = {other:?}"),
    }

    let probe = tempdir().unwrap();
    let probe_store = fixed_store(probe.path(), 1_000, 5);
    probe_store.issue_token("delete", 60).unwrap();
    let filename = consent_files(probe.path())
        .pop()
        .unwrap()
        .file_name()
        .unwrap()
        .to_owned();
    let destination = tempdir().unwrap();
    fs::create_dir(destination.path().join(filename)).unwrap();
    let atomic_error = fixed_store(destination.path(), 1_000, 5).issue_token("delete", 60);
    assert!(matches!(atomic_error, Err(ConsentError::Io(_))));
}

#[test]
fn consent_file_reader_rejects_missing_empty_and_non_regular_inputs() {
    let directory = tempdir().unwrap();
    assert_eq!(read_consent_file("").unwrap(), "");

    let missing = directory.path().join("missing");
    assert_eq!(
        read_consent_file(&missing).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );

    let empty = directory.path().join("empty");
    fs::write(&empty, b" \n\t").unwrap();
    assert_eq!(
        read_consent_file(&empty).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );

    let non_regular = directory.path().join("directory");
    fs::create_dir(&non_regular).unwrap();
    assert!(read_consent_file(&non_regular).is_err());

    let input = directory.path().join("input");
    fs::write(&input, b"  token-value  \nignored\n").unwrap();
    #[cfg(unix)]
    fs::set_permissions(&input, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(read_consent_file(&input).unwrap(), "token-value");
    #[cfg(unix)]
    assert_eq!(mode(&input), CONSENT_FILE_MODE);
}

const CHILD_ENV_KEYS: &[&str] = &[
    "HOME",
    "SYMERASEME_DATA_DIR",
    "SYMERASEME_CONSENT",
    "SYMERASEME_CONSENT_FILE",
    "ID004_CUSTOM_TOKEN",
    "ID004_CUSTOM_FILE",
    "ID004_CONSENT_CHILD_MODE",
    "ID004_CONSENT_DIR",
    "ID004_EXPECTED_DIR",
];

fn run_child(mode: &str, environment: &[(&str, &str)]) -> Output {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--ignored", "--exact", "environment_child", "--nocapture"]);
    for key in CHILD_ENV_KEYS {
        command.env_remove(key);
    }
    command.env("ID004_CONSENT_CHILD_MODE", mode);
    for (key, value) in environment {
        command.env(key, value);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "child mode {mode} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn token_fixture(fill: u8) -> (TempDir, String) {
    let directory = tempdir().unwrap();
    let token = fixed_store(directory.path(), 1_000, fill)
        .issue_token("delete", 60)
        .unwrap();
    (directory, token)
}

fn directory_string(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn consent_options_honor_custom_and_default_environment_precedence() {
    let (custom_token_dir, custom_token) = token_fixture(10);
    run_child(
        "custom-token",
        &[
            (
                "ID004_CONSENT_DIR",
                directory_string(custom_token_dir.path()),
            ),
            ("ID004_CUSTOM_TOKEN", &custom_token),
        ],
    );
    assert_eq!(
        fixed_store(custom_token_dir.path(), 1_000, 10).verify_token("delete", &custom_token),
        Err(ConsentError::NotFound)
    );

    let (custom_file_dir, custom_file_token) = token_fixture(11);
    let custom_file = custom_file_dir.path().join("custom-token");
    fs::write(&custom_file, format!("{custom_file_token}\nignored")).unwrap();
    run_child(
        "custom-file",
        &[
            (
                "ID004_CONSENT_DIR",
                directory_string(custom_file_dir.path()),
            ),
            ("ID004_CUSTOM_FILE", directory_string(&custom_file)),
        ],
    );
    assert_eq!(
        fixed_store(custom_file_dir.path(), 1_000, 11).verify_token("delete", &custom_file_token),
        Err(ConsentError::NotFound)
    );

    let (default_token_dir, default_token) = token_fixture(12);
    run_child(
        "default-token",
        &[
            (
                "ID004_CONSENT_DIR",
                directory_string(default_token_dir.path()),
            ),
            ("SYMERASEME_CONSENT", &default_token),
        ],
    );
    assert_eq!(
        fixed_store(default_token_dir.path(), 1_000, 12).verify_token("delete", &default_token),
        Err(ConsentError::NotFound)
    );

    let (default_file_dir, default_file_token) = token_fixture(13);
    let default_file = default_file_dir.path().join("default-token");
    fs::write(&default_file, format!("{default_file_token}\n")).unwrap();
    run_child(
        "default-file",
        &[
            (
                "ID004_CONSENT_DIR",
                directory_string(default_file_dir.path()),
            ),
            ("SYMERASEME_CONSENT_FILE", directory_string(&default_file)),
        ],
    );
    assert_eq!(
        fixed_store(default_file_dir.path(), 1_000, 13).verify_token("delete", &default_file_token),
        Err(ConsentError::NotFound)
    );

    let (precedence_dir, file_token) = token_fixture(14);
    let token_token = fixed_store(precedence_dir.path(), 1_000, 15)
        .issue_token("delete", 60)
        .unwrap();
    let precedence_file = precedence_dir.path().join("precedence-token");
    fs::write(&precedence_file, format!("{file_token}\n")).unwrap();
    run_child(
        "default-file-over-token",
        &[
            ("ID004_CONSENT_DIR", directory_string(precedence_dir.path())),
            ("SYMERASEME_CONSENT", &token_token),
            (
                "SYMERASEME_CONSENT_FILE",
                directory_string(&precedence_file),
            ),
        ],
    );
    let check = fixed_store(precedence_dir.path(), 1_000, 15);
    assert_eq!(
        check.verify_token("delete", &file_token),
        Err(ConsentError::NotFound)
    );
    assert_eq!(check.verify_token("delete", &token_token), Ok(()));
    check.consume_token(&token_token).unwrap();
}

#[test]
fn default_directory_uses_data_dir_and_home_fallback() {
    let data_home = tempdir().unwrap();
    let data_directory = data_home.path().join("custom-data");
    run_child(
        "default-data-dir",
        &[
            ("HOME", directory_string(data_home.path())),
            ("SYMERASEME_DATA_DIR", "~/custom-data"),
            ("ID004_EXPECTED_DIR", directory_string(&data_directory)),
        ],
    );
    assert_eq!(consent_files(&data_directory).len(), 1);

    let alias_home = tempdir().unwrap();
    run_child(
        "default-data-dir-home-alias",
        &[
            ("HOME", directory_string(alias_home.path())),
            ("SYMERASEME_DATA_DIR", "~"),
            ("ID004_EXPECTED_DIR", directory_string(alias_home.path())),
        ],
    );
    assert_eq!(consent_files(alias_home.path()).len(), 1);

    let home = tempdir().unwrap();
    let home_directory = home.path().join(".local/share/symeraseme");
    run_child(
        "default-home-dir",
        &[
            ("HOME", directory_string(home.path())),
            ("ID004_EXPECTED_DIR", directory_string(&home_directory)),
        ],
    );
    assert_eq!(consent_files(&home_directory).len(), 1);

    run_child("default-no-home", &[]);
}

#[test]
#[ignore]
fn environment_child() {
    let mode = std::env::var("ID004_CONSENT_CHILD_MODE").unwrap();
    match mode.as_str() {
        "custom-token" => {
            let directory = PathBuf::from(std::env::var("ID004_CONSENT_DIR").unwrap());
            let store = fixed_store(&directory, 1_000, 9);
            let options = ConsentOptions {
                consent_env_var: Some("ID004_CUSTOM_TOKEN".to_owned()),
                consent_file_env_var: Some("ID004_CUSTOM_FILE".to_owned()),
                ..Default::default()
            };
            assert_eq!(store.authorize("delete", &options), Ok(()));
        }
        "custom-file" => {
            let directory = PathBuf::from(std::env::var("ID004_CONSENT_DIR").unwrap());
            let store = fixed_store(&directory, 1_000, 9);
            let options = ConsentOptions {
                consent_env_var: Some("ID004_CUSTOM_TOKEN".to_owned()),
                consent_file_env_var: Some("ID004_CUSTOM_FILE".to_owned()),
                ..Default::default()
            };
            assert_eq!(store.authorize("delete", &options), Ok(()));
        }
        "default-token" | "default-file" | "default-file-over-token" => {
            let directory = PathBuf::from(std::env::var("ID004_CONSENT_DIR").unwrap());
            assert_eq!(
                fixed_store(&directory, 1_000, 9).authorize("delete", &Default::default()),
                Ok(())
            );
        }
        "default-data-dir" | "default-data-dir-home-alias" | "default-home-dir" => {
            let expected = PathBuf::from(std::env::var("ID004_EXPECTED_DIR").unwrap());
            let store = ConsentStore::from_default_directory()
                .unwrap()
                .with_clock(|| 1_000)
                .with_random_source(|length| Ok(vec![9; length]));
            let token = store.issue_token("delete", 60).unwrap();
            assert_eq!(store.list_tokens().unwrap()[0].token, token);
            assert!(expected.is_dir());
        }
        "default-no-home" => {
            let error = ConsentStore::from_default_directory().unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::NotFound);
        }
        other => panic!("unknown child mode {other}"),
    }
}
