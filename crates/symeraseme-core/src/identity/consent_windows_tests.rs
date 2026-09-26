//! Native Windows ID-005 path, replacement, permission and checked-close proof.
use super::*;
use std::io::Read;

fn fixed_store(directory: &Path) -> ConsentStore {
    ConsentStore::new(directory)
        .with_clock(|| 1_000)
        .with_random_source(|length| Ok(vec![7; length]))
}

#[test]
fn id005_windows_token_path_permissions_and_atomic_replacement() {
    let root = tempfile::tempdir().unwrap();
    let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([7; 16]);
    let path = root.path().join(token_filename(&token));
    let store = fixed_store(root.path());

    assert_eq!(store.issue_token("before", 60).unwrap(), token);
    let old_bytes = fs::read(&path).unwrap();
    assert!(!fs::metadata(&path).unwrap().permissions().readonly());

    let mut old_file = fs::File::open(&path).unwrap();
    store.issue_token("after", 60).unwrap();
    let new_bytes = fs::read(&path).unwrap();
    assert_ne!(new_bytes, old_bytes);
    assert!(!fs::metadata(&path).unwrap().permissions().readonly());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);

    let mut held_bytes = Vec::new();
    old_file.read_to_end(&mut held_bytes).unwrap();
    assert_eq!(
        held_bytes, old_bytes,
        "replacement changed an open old handle"
    );
    let record: ConsentRecord = serde_json::from_slice(&new_bytes).unwrap();
    assert_eq!(record.command, "after");
    assert_eq!(record.token.as_deref(), Some(token.as_str()));
}

#[test]
fn id005_windows_checked_close_returns_native_invalid_handle_error() {
    assert!(close_raw_handle(std::ptr::null_mut()).is_err());
}
