//! Native Windows ID-005 replacement with an open old handle.
use super::*;
use std::io::Read;

fn fixed_store(directory: &Path) -> ConsentStore {
    ConsentStore::new(directory)
        .with_clock(|| 1_000)
        .with_random_source(|length| Ok(vec![7; length]))
}

#[test]
fn id005_windows_failed_replacement_preserves_open_old_file() {
    let root = tempfile::tempdir().unwrap();
    let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([7; 16]);
    let path = root.path().join(token_filename(&token));
    let store = fixed_store(root.path());

    assert_eq!(store.issue_token("before", 60).unwrap(), token);
    let old_bytes = fs::read(&path).unwrap();

    let mut old_file = fs::File::open(&path).unwrap();
    assert!(matches!(
        store.issue_token("after", 60),
        Err(ConsentError::Io(error)) if error.kind() == io::ErrorKind::PermissionDenied
    ));
    assert_eq!(fs::read(&path).unwrap(), old_bytes);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);

    let mut held_bytes = Vec::new();
    old_file.read_to_end(&mut held_bytes).unwrap();
    assert_eq!(
        held_bytes, old_bytes,
        "failed replacement changed an open old handle"
    );
    let record: ConsentRecord = serde_json::from_slice(&old_bytes).unwrap();
    assert_eq!(record.command, "before");
    assert_eq!(record.token.as_deref(), Some(token.as_str()));
}
