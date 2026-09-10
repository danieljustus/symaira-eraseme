//! ID-005 side effects captured from Go; Unix mode/umask assertions run natively.
use super::*;
use serde_json::{Value, json};
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

const CHILD: &str = "identity::consent::filesystem_tests::id005_child";
const FIXTURE: &str = include_str!("../../../../tests/fixtures/consent-contract/id005.json");

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).unwrap()
}

fn chmod(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

fn fixed_store(path: &Path) -> ConsentStore {
    ConsentStore::new(path)
        .with_clock(|| 1000)
        .with_random_source(|length| Ok(vec![7; length]))
}

fn manifest(root: &Path, directory: &Path, out: &mut Vec<Value>) {
    let mut entries = fs::read_dir(directory)
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata = entry.metadata().unwrap();
        let is_dir = metadata.is_dir();
        out.push(json!({
            "path": path.strip_prefix(root).unwrap().to_str().unwrap(),
            "kind": if is_dir { "directory" } else { "file" },
            "mode": metadata.permissions().mode() & 0o777,
            "body": if is_dir { String::new() } else { fs::read_to_string(&path).unwrap() },
        }));
        if is_dir {
            manifest(root, &path, out);
        }
    }
}

fn observe(root: &Path, name: &str) -> Value {
    let mut dir = root.join("consent");
    let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([7; 16]);
    let path = dir.join(token_filename(&token));
    let mut held = None;
    let result = match name {
        "fresh" | "umask_000" | "umask_077" | "umask_777" => {
            fixed_store(&dir).issue_token("delete", 60).map(|_| ())
        }
        "nested" => {
            dir = dir.join("nested");
            fixed_store(&dir).issue_token("delete", 60).map(|_| ())
        }
        "existing_directory" => {
            fs::create_dir(&dir).unwrap();
            chmod(&dir, 0o500);
            fixed_store(&dir).issue_token("delete", 60).map(|_| ())
        }
        "replacement" | "replacement_symlink" | "rename_failure" | "temp_failure" => {
            fs::create_dir(&dir).unwrap();
            chmod(&dir, 0o700);
            let sentinel = dir.join(".consent-sentinel.tmp");
            fs::write(&sentinel, "unrelated sentinel").unwrap();
            chmod(&sentinel, 0o600);
            if name == "rename_failure" {
                fs::create_dir(&path).unwrap();
                chmod(&path, 0o700);
                fs::write(path.join("sentinel"), "destination sentinel").unwrap();
                chmod(&path.join("sentinel"), 0o600);
            } else {
                fixed_store(&dir).issue_token("before", 60).unwrap();
                chmod(&path, 0o400);
                held = Some(fs::File::open(&path).unwrap());
            }
            if name == "replacement_symlink" {
                // Match Go's destination link to an old inode outside consent/
                // while keeping every observed entry inside the isolated root.
                fs::rename(&path, root.join("destination-sentinel")).unwrap();
                std::os::unix::fs::symlink("../destination-sentinel", &path).unwrap();
                assert!(fs::symlink_metadata(&path).unwrap().is_symlink());
            }
            let mut store = fixed_store(&dir);
            if name == "temp_failure" {
                let dir = dir.clone();
                store = store.with_random_source(move |length| {
                    chmod(&dir, 0o500);
                    Ok(vec![7; length])
                });
            }
            let result = store.issue_token("delete", 60);
            if name == "replacement_symlink" && result.is_ok() {
                assert!(fs::symlink_metadata(&path).unwrap().is_file());
            }
            if name == "temp_failure" {
                chmod(&dir, 0o700);
            }
            result.map(|_| ())
        }
        "write_failure" => {
            // The parent prepared the old token before limiting this process.
            held = Some(fs::File::open(&path).unwrap());
            fixed_store(&dir).issue_token("delete", 60).map(|_| ())
        }
        "mkdir_failure" => {
            fs::write(&dir, "parent sentinel").unwrap();
            chmod(&dir, 0o600);
            fixed_store(&dir).issue_token("delete", 60).map(|_| ())
        }
        "verify" | "list" | "wrong_command" | "expired" => {
            let store = fixed_store(&dir);
            store.issue_token("delete", 60).unwrap();
            chmod(&path, 0o644);
            chmod(&dir, 0o755);
            match name {
                "verify" => store.verify_token("delete", &token),
                "list" => store.list_tokens().map(|_| ()),
                "wrong_command" => store.verify_token("other", &token),
                "expired" => store.with_clock(|| 1061).verify_token("delete", &token),
                _ => unreachable!(),
            }
        }
        other => panic!("unknown ID-005 case {other}"),
    };
    let mut old_body = String::new();
    if let Some(mut held) = held {
        held.read_to_string(&mut old_body).unwrap();
    }
    let mut entries = Vec::new();
    manifest(root, root, &mut entries);
    let class = match &result {
        Ok(()) => "ok",
        Err(ConsentError::Expired) => "expired",
        Err(ConsentError::CommandMismatch) => "command_mismatch",
        Err(ConsentError::Io(error)) => match error.kind() {
            io::ErrorKind::PermissionDenied => "permission_denied",
            io::ErrorKind::NotADirectory => "not_a_directory",
            io::ErrorKind::AlreadyExists
            | io::ErrorKind::IsADirectory
            | io::ErrorKind::DirectoryNotEmpty => "destination_conflict",
            io::ErrorKind::FileTooLarge => "file_too_large",
            other => panic!("unclassified filesystem error {other:?}: {error}"),
        },
        other => panic!("unclassified consent result {other:?}"),
    };
    json!({"name": name, "failed": result.is_err(), "error_class": class, "held": old_body, "entries": entries})
}

fn compare(actual: &Value, expected: &Value) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "Go filesystem mismatch: actual={actual}, expected={expected}"
        ))
    }
}

#[test]
fn id005_matches_frozen_go_filesystem() {
    let document = fixture();
    let cases = document["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 16);
    for expected in cases {
        let name = expected["name"].as_str().unwrap();
        let root = tempfile::tempdir().unwrap();
        let mask = name.strip_prefix("umask_").unwrap_or("022");
        let mut command = if name == "write_failure" {
            let dir = root.path().join("consent");
            fixed_store(&dir).issue_token("before", 60).unwrap();
            let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([7; 16]);
            chmod(&dir.join(token_filename(&token)), 0o400);
            let sentinel = dir.join(".consent-sentinel.tmp");
            fs::write(&sentinel, "unrelated sentinel").unwrap();
            chmod(&sentinel, 0o600);
            // Python sets the same one-byte soft RLIMIT_FSIZE as the Go probe.
            // It execs only this test binary, so Rust needs no unsafe pre_exec.
            let mut command = Command::new("python3");
            command.args(["-c", "import os,resource,signal,sys; os.umask(0o022); resource.setrlimit(resource.RLIMIT_FSIZE,(1,resource.getrlimit(resource.RLIMIT_FSIZE)[1])); signal.signal(signal.SIGXFSZ,signal.SIG_IGN); os.execv(sys.argv[1],sys.argv[1:])"]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", "umask \"$1\"; shift; exec \"$@\"", "id005", mask]);
            command
        };
        let output = command
            .arg(std::env::current_exe().unwrap())
            .args([CHILD, "--exact", "--ignored", "--nocapture"])
            .env("ID005_CASE", name)
            .env("ID005_ROOT", root.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "{name}: {output:?}");
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .contains("1 passed;")
        );
    }
}

#[test]
#[ignore = "only launched by the parent with an isolated umask and directory"]
fn id005_child() {
    let name = std::env::var("ID005_CASE").unwrap();
    let root = PathBuf::from(std::env::var_os("ID005_ROOT").unwrap());
    let document = fixture();
    let expected = document["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == name)
        .unwrap();
    compare(&observe(&root, &name), expected).unwrap();
}

#[test]
fn id005_symlink_replacement_matches_go_and_rejects_referent_changes() {
    let document = fixture();
    let expected = document["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == "replacement_symlink")
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let actual = observe(root.path(), "replacement_symlink");
    compare(&actual, expected).unwrap();
    // A writer that follows the destination link could alter either the
    // referent's bytes or permissions. Reject both through the real comparator.
    for (field, value) in [
        ("body", json!("overwritten referent")),
        ("mode", json!(0o600)),
    ] {
        let mut tampered = actual.clone();
        let referent = tampered["entries"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|entry| entry["path"] == "destination-sentinel")
            .unwrap();
        referent[field] = value;
        assert!(compare(&tampered, expected).is_err());
    }
}

#[test]
fn id005_rejects_changed_oracle_modes_and_source() {
    let document = fixture();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (path, expected) in document["source_sha256"].as_object().unwrap() {
        let actual = hex::encode(Sha256::digest(fs::read(root.join(path)).unwrap()));
        assert_eq!(
            actual,
            expected.as_str().unwrap(),
            "Go oracle drift: {path}"
        );
    }
    let root = tempfile::tempdir().unwrap();
    let actual = observe(root.path(), "fresh");
    let expected = &document["cases"][0];
    compare(&actual, expected).unwrap();
    let mut tampered = expected.clone();
    tampered["entries"][0]["mode"] = json!(0o755);
    assert!(compare(&actual, &tampered).is_err());
    tampered = expected.clone();
    tampered["failed"] = json!(true);
    assert!(compare(&actual, &tampered).is_err());
    tampered = expected.clone();
    tampered["error_class"] = json!("expired");
    assert!(compare(&actual, &tampered).is_err());
}

const FAULT_FIXTURE: &str =
    include_str!("../../../../tests/fixtures/consent-contract/id005-faults.json");

fn fault_case(name: &str) -> Value {
    let document: Value = serde_json::from_str(FAULT_FIXTURE).unwrap();
    let cases = document["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 2);
    cases
        .iter()
        .find(|case| case["name"] == name)
        .unwrap()
        .clone()
}

fn fault_setup() -> (tempfile::TempDir, PathBuf, fs::File) {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("consent");
    let token = fixed_store(&dir).issue_token("before", 60).unwrap();
    let path = dir.join(token_filename(&token));
    chmod(&path, 0o400);
    let held = fs::File::open(&path).unwrap();
    let sentinel = dir.join(".consent-sentinel.tmp");
    fs::write(&sentinel, "unrelated sentinel").unwrap();
    chmod(&sentinel, 0o600);
    (root, path, held)
}

fn fault_observation(root: &Path, name: &str, held: &mut fs::File) -> Value {
    let mut entries = Vec::new();
    manifest(root, root, &mut entries);
    let mut body = String::new();
    held.read_to_string(&mut body).unwrap();
    json!({"name": name, "failed": true, "entries": entries, "held": body})
}

#[derive(Debug)]
struct InjectedFailure(&'static str);

impl fmt::Display for InjectedFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for InjectedFailure {}

fn assert_injected(error: &io::Error, stage: &str) {
    assert_eq!(
        error
            .get_ref()
            .unwrap()
            .downcast_ref::<InjectedFailure>()
            .unwrap()
            .0,
        stage
    );
}

#[test]
fn id005_atomic_close_error_preserves_go_rollback() {
    let (root, path, mut held) = fault_setup();
    let error = atomic_write_with(
        &path,
        b"replacement must not be published",
        fs::File::sync_all,
        |file| {
            close_file(file)?;
            // Safe Rust cannot retain a File after consuming its owner. Inject
            // an adapter failure after real close, not a native close fault.
            Err(io::Error::other(InjectedFailure("close")))
        },
        |_| panic!("chmod reached after close failure"),
    )
    .unwrap_err();
    assert_injected(&error, "close");
    let mut expected = fault_case("close_failure");
    assert_eq!(expected["error_class"], "closed_file");
    // Go genuinely returned os.ErrClosed. Only failure + rollback effects
    // are compared with our injected adapter failure, not native error parity.
    expected.as_object_mut().unwrap().remove("error_class");
    let actual = fault_observation(root.path(), "close_failure", &mut held);
    compare(&actual, &expected).unwrap();
    expected["entries"][1]["body"] = json!("tampered sentinel");
    assert!(compare(&actual, &expected).is_err());
}

#[test]
fn id005_atomic_chmod_matches_source_bound_go_fault() {
    let (root, path, mut held) = fault_setup();
    let error = atomic_write_with(
        &path,
        b"replacement must not be published",
        fs::File::sync_all,
        close_file,
        |temporary| {
            fs::remove_file(temporary).unwrap();
            // This calls native chmod, with no preceding metadata lookup.
            chmod_temporary(temporary)
        },
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    let mut actual = fault_observation(root.path(), "chmod_failure", &mut held);
    actual["error_class"] = json!("not_found");
    compare(&actual, &fault_case("chmod_failure")).unwrap();
}

#[test]
fn id005_atomic_injected_close_error_cleans_owned_temporary() {
    let (root, path, mut held) = fault_setup();
    let mut before = Vec::new();
    manifest(root.path(), root.path(), &mut before);
    let old_body = fs::read_to_string(&path).unwrap();
    let sentinel = path.parent().unwrap().join(".consent-sentinel.tmp");
    let mut owned_temporary = None;
    let replacement = b"replacement must not be published";
    let error = atomic_write_with(
        &path,
        replacement,
        fs::File::sync_all,
        |file| {
            assert_eq!(file.metadata().unwrap().len(), replacement.len() as u64);
            let mut temporary_paths = fs::read_dir(path.parent().unwrap())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|entry| entry != &path && entry != &sentinel)
                .collect::<Vec<_>>();
            assert_eq!(temporary_paths.len(), 1);
            let temporary = temporary_paths.pop().unwrap();
            assert_eq!(fs::read(&temporary).unwrap(), replacement);
            owned_temporary = Some(temporary);
            // Fail before a checked close consumes the file. This is a
            // portable adapter injection, not native delayed-close evidence.
            Err(io::Error::other(InjectedFailure("close")))
        },
        |_| panic!("chmod reached after injected close failure"),
    )
    .unwrap_err();
    assert_injected(&error, "close");
    let actual = fault_observation(root.path(), "injected_close_failure", &mut held);
    assert_eq!(actual["entries"], json!(before));
    assert_eq!(actual["held"], old_body);
    assert!(!owned_temporary.unwrap().try_exists().unwrap());
}

#[test]
fn id005_atomic_injected_chmod_error_cleans_owned_temporary() {
    let (root, path, mut held) = fault_setup();
    let mut before = Vec::new();
    manifest(root.path(), root.path(), &mut before);
    let old_body = fs::read_to_string(&path).unwrap();
    let mut owned_temporary = None;
    let replacement = b"replacement must not be published";
    let error = atomic_write_with(
        &path,
        replacement,
        fs::File::sync_all,
        |file| {
            drop(file);
            Ok(())
        },
        |temporary| {
            assert_eq!(fs::read(temporary).unwrap(), replacement);
            owned_temporary = Some(temporary.to_path_buf());
            // Leave the temporary file present for the path guard to remove.
            // This adapter injection is not native chmod failure evidence.
            Err(io::Error::other(InjectedFailure("chmod")))
        },
    )
    .unwrap_err();
    assert_injected(&error, "chmod");
    let actual = fault_observation(root.path(), "injected_chmod_failure", &mut held);
    assert_eq!(actual["entries"], json!(before));
    assert_eq!(actual["held"], old_body);
    assert!(!owned_temporary.unwrap().try_exists().unwrap());
}

#[test]
fn id005_atomic_rust_only_sync_error_cleans_owned_temporary() {
    let (root, path, mut held) = fault_setup();
    let mut before = Vec::new();
    manifest(root.path(), root.path(), &mut before);
    let old_body = fs::read(&path).unwrap();
    let sentinel = path.parent().unwrap().join(".consent-sentinel.tmp");
    let sentinel_body = fs::read(&sentinel).unwrap();
    let mut owned_temporary = None;
    let replacement = b"replacement must not be published";
    let error = atomic_write_with(
        &path,
        replacement,
        |file| {
            assert_eq!(file.metadata().unwrap().len(), replacement.len() as u64);
            let mut temporary_paths = fs::read_dir(path.parent().unwrap())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|entry| entry != &path && entry != &sentinel)
                .collect::<Vec<_>>();
            assert_eq!(temporary_paths.len(), 1);
            let temporary = temporary_paths.pop().unwrap();
            assert_eq!(fs::read(&temporary).unwrap(), replacement);
            owned_temporary = Some(temporary);
            // Rust-only sync behavior: Go has no sync operation. Inject before
            // close/chmod/publication; this does not establish Go parity.
            Err(io::Error::other(InjectedFailure("sync")))
        },
        |_| panic!("checked close reached after injected sync failure"),
        |_| panic!("chmod reached after injected sync failure"),
    )
    .unwrap_err();
    assert_injected(&error, "sync");
    let actual = fault_observation(root.path(), "rust_only_sync_failure", &mut held);
    assert_eq!(actual["entries"], json!(before));
    assert_eq!(actual["held"].as_str().unwrap().as_bytes(), old_body);
    assert_eq!(fs::read(&path).unwrap(), old_body);
    assert_eq!(fs::read(&sentinel).unwrap(), sentinel_body);
    assert!(!owned_temporary.unwrap().try_exists().unwrap());
}

#[test]
fn id005_atomic_sync_error_is_retained_without_go_normalization() {
    let (root, path, mut held) = fault_setup();
    let mut before = Vec::new();
    manifest(root.path(), root.path(), &mut before);
    let old_body = fs::read_to_string(&path).unwrap();
    let error = atomic_write_with(
        &path,
        b"replacement must not be published",
        |_| Err(io::Error::other(InjectedFailure("sync"))),
        |_| panic!("checked close reached after sync failure"),
        |_| panic!("chmod reached after sync failure"),
    )
    .unwrap_err();
    assert_injected(&error, "sync");
    let actual = fault_observation(root.path(), "rust_only_sync_failure", &mut held);
    assert_eq!(actual["entries"], json!(before));
    assert_eq!(actual["held"], old_body);
}

#[test]
fn id005_fault_fixture_is_bound_to_source_and_probe() {
    let document: Value = serde_json::from_str(FAULT_FIXTURE).unwrap();
    assert_eq!(document["schema"], "consent-id005-faults-v1");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (path, expected) in document["source_sha256"].as_object().unwrap() {
        assert_eq!(
            hex::encode(Sha256::digest(fs::read(root.join(path)).unwrap())),
            expected.as_str().unwrap(),
            "Go oracle drift: {path}"
        );
    }
    for (path, expected) in document["helper_sha256"].as_object().unwrap() {
        assert_eq!(
            hex::encode(Sha256::digest(
                fs::read(root.join("scripts/consent-oracle").join(path)).unwrap()
            )),
            expected.as_str().unwrap(),
            "Go helper drift: {path}"
        );
    }
    assert_eq!(
        hex::encode(Sha256::digest(
            fs::read(root.join("scripts/consent-oracle/generate.py")).unwrap()
        )),
        document["generator_sha256"].as_str().unwrap()
    );
}
