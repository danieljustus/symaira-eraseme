use std::fs::{self, File, FileTimes};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};
use symeraseme_core::storage::encrypted_store::{
    EncryptedStoreError, clear_master_key, finalise_all, open_configured, open_encrypted,
    scavenge_stale_temps, set_master_key,
};
use symeraseme_core::storage::encryption::{EnvelopeVersion, decrypt_v3, detect_version};
use symeraseme_core::storage::{DbLock, lock_path_for};
use tempfile::tempdir;

static TEST_LOCK: Mutex<()> = Mutex::new(());
const KEY: [u8; 32] = [0x42; 32];

#[test]
fn encrypted_open_uses_canonical_path_and_private_sqlite_temp() {
    let _guard = TEST_LOCK.lock().unwrap();
    set_master_key(KEY);
    let dir = tempdir().unwrap();
    let canonical = dir.path().join("canonical.db");
    let tmp_dir = dir.path().join("tmp");

    let store = open_encrypted(&canonical, &tmp_dir).expect("open encrypted store");
    assert_eq!(store.path(), canonical);
    let temps: Vec<_> = fs::read_dir(&tmp_dir)
        .unwrap()
        .map(|entry| entry.unwrap())
        .collect();
    let temp = temps
        .iter()
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("symeraseme_decrypted_")
                && entry.file_name().to_string_lossy().ends_with(".db")
        })
        .expect("private sqlite temp");
    #[cfg(unix)]
    assert_eq!(temp.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    assert!(
        decrypt_v3(&fs::read(&canonical).unwrap(), &KEY)
            .unwrap()
            .starts_with(b"SQLite format 3\0")
    );
    store.close().expect("close encrypted store");
    assert!(!temp.path().exists());
    assert!(!lock_path_for(&temp.path()).exists());
    clear_master_key();
}

#[test]
fn encrypted_open_migrates_legacy_versions_before_temp_open() {
    let _guard = TEST_LOCK.lock().unwrap();
    set_master_key(*b"symaira-eraseme-golden-master-32");
    let dir = tempdir().unwrap();
    let tmp_dir = dir.path().join("tmp");
    let fixture_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/event-store/crypto");

    for (name, expected) in [
        ("golden-campaign-v1-python.db", EnvelopeVersion::V3),
        ("golden-campaign-v2-python.db", EnvelopeVersion::V3),
        ("golden-campaign-v3-legacy-go.db", EnvelopeVersion::V3),
    ] {
        let canonical = dir.path().join(name);
        fs::copy(fixture_dir.join(name), &canonical).expect("copy crypto fixture");
        let store = open_encrypted(&canonical, &tmp_dir).expect("open and migrate fixture");
        assert_eq!(
            detect_version(&fs::read(&canonical).unwrap()),
            Some(expected)
        );
        store.close().expect("close migrated fixture");
    }
    clear_master_key();
}

#[test]
fn finalise_all_keeps_failed_registration_for_retry() {
    let _guard = TEST_LOCK.lock().unwrap();
    set_master_key(KEY);
    let dir = tempdir().unwrap();
    let canonical = dir.path().join("retry.db");
    let tmp_dir = dir.path().join("tmp");
    let store = open_encrypted(&canonical, &tmp_dir).expect("open encrypted store");
    store
        .connection()
        .execute(
            "INSERT INTO campaigns (id, kind, notes) VALUES ('retry', 'initial', 'retained')",
            [],
        )
        .unwrap();

    clear_master_key();
    assert!(store.close().is_err());
    assert!(open_encrypted(&canonical, dir.path().join("other-tmp")).is_err());
    set_master_key(KEY);
    finalise_all().expect("failed registration remains retryable");
    let reopened = open_encrypted(&canonical, dir.path().join("other-tmp")).unwrap();
    let notes: String = reopened
        .connection()
        .query_row(
            "SELECT notes FROM campaigns WHERE id = 'retry'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(notes, "retained");
    reopened.close().unwrap();
    clear_master_key();
}

#[test]
fn finalise_all_reports_an_active_store_and_close_keeps_wal_data() {
    let _guard = TEST_LOCK.lock().unwrap();
    set_master_key(KEY);
    let dir = tempdir().unwrap();
    let canonical = dir.path().join("active.db");
    let tmp_dir = dir.path().join("tmp");
    let store = open_encrypted(&canonical, &tmp_dir).unwrap();
    store
        .connection()
        .execute(
            "INSERT INTO campaigns (id, kind, notes) VALUES ('active', 'initial', 'wal-safe')",
            [],
        )
        .unwrap();
    clear_master_key();
    assert!(
        finalise_all().is_err(),
        "active stores must remain unfinished"
    );
    set_master_key(KEY);
    store.close().expect("close active store");
    let reopened = open_encrypted(&canonical, dir.path().join("reopen-tmp")).unwrap();
    let notes: String = reopened
        .connection()
        .query_row(
            "SELECT notes FROM campaigns WHERE id = 'active'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(notes, "wal-safe");
    reopened.close().unwrap();
    clear_master_key();
}

#[test]
fn open_configured_rejects_whitespace_only_temp_dir() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dir = tempdir().unwrap();
    for tmp_dir in [Path::new(" \t\n"), Path::new("relative-tmp")] {
        let error = open_configured(dir.path().join("configured.db"), Some(tmp_dir), false)
            .expect_err("invalid temp directory must be rejected");
        assert!(matches!(error, EncryptedStoreError::InvalidPath(_)));

        let error = open_encrypted(dir.path().join("encrypted.db"), tmp_dir)
            .expect_err("invalid encrypted temp directory must be rejected");
        assert!(matches!(error, EncryptedStoreError::InvalidPath(_)));
    }
}

#[test]
fn plain_open_without_temp_dir_does_not_resolve_encrypted_temp_root() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("plain.db");
    let store = open_configured(&path, None, false).expect("plain open without temp root");
    assert_eq!(store.path(), path);
    store.close().expect("close plain store");
}

#[cfg(unix)]
#[test]
fn plain_open_does_not_chmod_an_existing_canonical_parent() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dir = tempdir().unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let path = dir.path().join("plain-parent.db");

    let store = open_configured(&path, None, false).expect("plain open");
    store.close().expect("close plain store");

    assert_eq!(
        dir.path().metadata().unwrap().permissions().mode() & 0o777,
        0o755
    );
}

#[test]
fn scavenge_preserves_registered_active_temp_even_when_old() {
    let _guard = TEST_LOCK.lock().unwrap();
    set_master_key(KEY);
    let dir = tempdir().unwrap();
    let canonical = dir.path().join("scavenge.db");
    let tmp_dir = dir.path().join("tmp");
    let store = open_encrypted(&canonical, &tmp_dir).unwrap();
    let temp = fs::read_dir(&tmp_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("symeraseme_decrypted_")
                && path.extension().and_then(|ext| ext.to_str()) == Some("db")
        })
        .unwrap();
    let old = SystemTime::now() - Duration::from_secs(301);
    File::open(&temp)
        .unwrap()
        .set_times(FileTimes::new().set_modified(old))
        .unwrap();
    scavenge_stale_temps(&tmp_dir).unwrap();
    assert!(temp.exists());
    store.close().unwrap();
    clear_master_key();
}

#[test]
fn scavenge_preserves_unregistered_decrypted_temp_for_other_process() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dir = tempdir().unwrap();
    let tmp_dir = dir.path().join("tmp");
    fs::create_dir_all(&tmp_dir).unwrap();
    let temp = tmp_dir.join("symeraseme_decrypted_other_process.db");
    File::create(&temp).unwrap();
    let old = SystemTime::now() - Duration::from_secs(301);
    File::open(&temp)
        .unwrap()
        .set_times(FileTimes::new().set_modified(old))
        .unwrap();
    let _other_process_lock = DbLock::lock(&temp, 1).unwrap();

    scavenge_stale_temps(&tmp_dir).unwrap();

    assert!(
        temp.exists(),
        "unknown private SQLite temps are never scavenged"
    );
    drop(_other_process_lock);
    scavenge_stale_temps(&tmp_dir).unwrap();
    assert!(!temp.exists());
    assert!(!lock_path_for(&temp).exists());
}
