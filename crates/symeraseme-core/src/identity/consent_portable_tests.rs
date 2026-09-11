//! ID-005 destination-conflict cleanup, without Unix-only APIs or subprocesses.
//! Native modes and error codes remain covered by the separate platform gates.
use super::*;
use serde_json::{Value, json};

fn fixture_case(name: &str) -> Value {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/consent-contract/id005.json"
    ))
    .unwrap();
    assert_eq!(fixture["schema"], "consent-id005-v1");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (path, expected) in fixture["source_sha256"].as_object().unwrap() {
        assert_eq!(
            hex::encode(Sha256::digest(fs::read(root.join(path)).unwrap())),
            expected.as_str().unwrap(),
            "Go oracle drift: {path}"
        );
    }
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 16);
    cases
        .iter()
        .find(|case| case["name"] == name)
        .unwrap()
        .clone()
}

fn manifest(root: &Path, directory: &Path, entries: &mut Vec<Value>) {
    let mut children = fs::read_dir(directory)
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        let path = child.path();
        let kind = child.file_type().unwrap();
        assert!(
            kind.is_dir() || kind.is_file(),
            "unexpected entry: {path:?}"
        );
        // Match the oracle helper's filepath.ToSlash on every host.
        let relative = path
            .strip_prefix(root)
            .unwrap()
            .iter()
            .map(|component| component.to_str().unwrap())
            .collect::<Vec<_>>()
            .join("/");
        entries.push(json!({
            "path": relative,
            "kind": if kind.is_dir() { "directory" } else { "file" },
            "body": if kind.is_dir() { String::new() } else { fs::read_to_string(&path).unwrap() },
        }));
        if kind.is_dir() {
            manifest(root, &path, entries);
        }
    }
}

fn matches_go_cleanup(root: &Path, failed: bool, expected: &Value) -> bool {
    let mut entries = Vec::new();
    manifest(root, root, &mut entries);
    // This additional portable subset compares failure and the complete tree's
    // paths/types/bytes. It does not reinterpret Unix modes or native errors.
    let expected_entries = expected["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| json!({"path": entry["path"], "kind": entry["kind"], "body": entry["body"]}))
        .collect::<Vec<_>>();
    expected["failed"] == json!(failed) && entries == expected_entries
}

#[test]
fn id005_destination_conflict_cleans_owned_temp_and_preserves_go_tree() {
    let expected = fixture_case("rename_failure");
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("consent");
    let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([7; 16]);
    let destination = dir.join(token_filename(&token));
    fs::create_dir_all(&destination).unwrap();
    fs::write(destination.join("sentinel"), "destination sentinel").unwrap();
    fs::write(dir.join(".consent-sentinel.tmp"), "unrelated sentinel").unwrap();
    let fresh = fixture_case("fresh");
    let body = fresh["entries"][1]["body"].as_str().unwrap().as_bytes();
    let mut owned_temp = None;
    let result = atomic_write_with(
        &destination,
        body,
        fs::File::sync_all,
        close_file,
        |temporary| {
            assert_eq!(temporary.parent(), Some(dir.as_path()));
            assert_eq!(fs::read(temporary).unwrap(), body);
            chmod_temporary(temporary)?;
            owned_temp = Some(temporary.to_path_buf());
            Ok(())
        },
    );
    assert!(result.is_err(), "publication over a directory must fail");
    let owned_temp = owned_temp.expect("real write, sync, close and chmod must succeed");
    assert_eq!(
        fs::symlink_metadata(&owned_temp).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
    assert!(matches_go_cleanup(root.path(), result.is_err(), &expected));

    // Reject false success, leaked owned files and changed destination bytes
    // through the same comparison that accepts the real production outcome.
    assert!(!matches_go_cleanup(root.path(), false, &expected));
    fs::write(&owned_temp, body).unwrap();
    assert!(!matches_go_cleanup(root.path(), true, &expected));
    fs::remove_file(&owned_temp).unwrap();
    fs::write(destination.join("sentinel"), "changed destination").unwrap();
    assert!(!matches_go_cleanup(root.path(), true, &expected));
}
