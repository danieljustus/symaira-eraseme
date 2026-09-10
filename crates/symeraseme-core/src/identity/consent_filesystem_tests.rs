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
        "replacement" | "rename_failure" | "temp_failure" => {
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
            let mut store = fixed_store(&dir);
            if name == "temp_failure" {
                let dir = dir.clone();
                store = store.with_random_source(move |length| {
                    chmod(&dir, 0o500);
                    Ok(vec![7; length])
                });
            }
            let result = store.issue_token("delete", 60);
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
    assert_eq!(cases.len(), 15);
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
