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

#[test]
fn id005_native_close_handle_failure_rolls_back_like_go() {
    let expected = super::portable_filesystem_tests::fault_fixture_case("close_failure");
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("consent");
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join(".consent-sentinel.tmp"),
        "unrelated sentinel",
    )
    .unwrap();
    let token = fixed_store(&directory).issue_token("before", 60).unwrap();
    let path = directory.join(token_filename(&token));
    let old_bytes = fs::read(&path).unwrap();
    let mut owned_file_closes = 0;
    let error = atomic_write_with(
        &path,
        b"replacement must not be published",
        fs::File::sync_all,
        |file| {
            // Close our valid owner once, then exercise the same production
            // kernel-error boundary with the documented invalid handle value.
            close_windows_file(file)?;
            owned_file_closes += 1;
            checked_windows_close(std::ptr::null_mut())
        },
        |_| panic!("chmod must not follow a failed close"),
    )
    .unwrap_err();
    assert_eq!(owned_file_closes, 1);
    assert_eq!(error.raw_os_error(), Some(6), "real ERROR_INVALID_HANDLE");
    assert_eq!(fs::read(&path).unwrap(), old_bytes);
    assert!(super::portable_filesystem_tests::matches_go_cleanup(
        root.path(),
        true,
        &expected,
    ));
    eprintln!(
        "native CloseHandle error=6 propagated, owner closed once, old token and unrelated sentinel preserved, owned temporary removed"
    );
}
