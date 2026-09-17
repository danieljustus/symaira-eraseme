use std::fs::{self, File, FileTimes};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};
use symeraseme_core::storage::encrypted_store::{
    STALE_SCAVENGE_AGE, clear_master_key, open_configured, open_encrypted, remove_wal_siblings,
    scavenge_stale_temps, set_master_key,
};
use symeraseme_core::storage::encryption::{
    EnvelopeVersion, decrypt_v3, detect_version, is_encrypted,
};
use symeraseme_core::storage::{EncryptedStoreError, Store};
use tempfile::tempdir;

static TEST_MUTEX: Mutex<()> = Mutex::new(());
const TEST_KEY: [u8; 32] = [0x42; 32];

fn setup_test_key() -> std::sync::MutexGuard<'static, ()> {
    let guard = TEST_MUTEX.lock().unwrap();
    set_master_key(TEST_KEY);
    guard
}

#[test]
fn open_configured_rejects_encryption_without_master_key() {
    let _guard = TEST_MUTEX.lock().unwrap();
    clear_master_key();
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let tmp_dir = dir.path().join("tmp");

    let err = open_configured(&db_path, Some(&tmp_dir), true).unwrap_err();
    assert!(matches!(err, EncryptedStoreError::MasterKeyUnavailable));
    assert!(
        !db_path.exists(),
        "must not create DB when key is unavailable"
    );
}

#[test]
fn open_configured_rejects_empty_path() {
    let _guard = setup_test_key();
    let err = open_configured("", None, true).unwrap_err();
    assert!(matches!(err, EncryptedStoreError::InvalidPath(_)));
}

#[test]
fn open_encrypted_publishes_valid_v3_ciphertext_and_private_modes() {
    let _guard = setup_test_key();
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("campaign.db");
    let tmp_dir = dir.path().join("tmp");

    // Open new encrypted store
    let store = open_encrypted(&db_path, &tmp_dir).expect("open encrypted");
    assert!(db_path.exists(), "database file must exist on disk");

    // Check directory and file permissions on Unix
    #[cfg(unix)]
    {
        let tmp_meta = fs::metadata(&tmp_dir).unwrap();
        assert_eq!(
            tmp_meta.permissions().mode() & 0o777,
            0o700,
            "tmp_dir must have mode 0700"
        );

        let db_meta = fs::metadata(&db_path).unwrap();
        assert_eq!(
            db_meta.permissions().mode() & 0o777,
            0o600,
            "db file must have mode 0600"
        );
    }

    // Verify disk contents is valid encrypted Fernet V3 envelope
    let raw_on_disk = fs::read(&db_path).unwrap();
    assert!(is_encrypted(&raw_on_disk));
    assert_eq!(detect_version(&raw_on_disk), Some(EnvelopeVersion::V3));

    // Decrypting the ciphertext on disk must produce a valid SQLite header
    let plaintext = decrypt_v3(&raw_on_disk, &TEST_KEY).expect("decrypt V3");
    assert!(plaintext.starts_with(b"SQLite format 3\0"));

    // Query through store works
    let version = store.user_version().expect("read user version");
    assert_eq!(version, 2);

    // Close store
    store.close().expect("close store");

    // Target remains encrypted on disk after close
    let closed_raw = fs::read(&db_path).unwrap();
    assert!(is_encrypted(&closed_raw));
    assert_eq!(detect_version(&closed_raw), Some(EnvelopeVersion::V3));
}

#[test]
fn open_encrypted_excludes_concurrent_open_until_close() {
    let _guard = setup_test_key();
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("contended.db");
    let tmp_dir = dir.path().join("tmp");

    let store1 = open_encrypted(&db_path, &tmp_dir).expect("first open");

    // Second open on same path must fail with Lock error
    let result = open_encrypted(&db_path, &tmp_dir);
    assert!(result.is_err(), "second open on locked database must fail");
    match result.unwrap_err() {
        EncryptedStoreError::Lock(_) => {}
        other => panic!("expected EncryptedStoreError::Lock, got {other:?}"),
    }

    // Close store1
    store1.close().expect("close first store");

    // Now second open must succeed
    let store2 = open_encrypted(&db_path, &tmp_dir).expect("second open after close");
    store2.close().expect("close second store");
}

#[test]
fn open_configured_plain_encrypted_transitions_persist_data() {
    let _guard = setup_test_key();
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("transition.db");
    let tmp_dir = dir.path().join("tmp");

    // 1. Create plain database and insert data
    {
        let store = open_configured(&db_path, Some(&tmp_dir), false).expect("open plain");
        store
            .connection()
            .execute(
                "INSERT INTO campaigns (id, kind, notes) VALUES ('c1', 'initial', 'plain notes')",
                [],
            )
            .expect("insert campaign");
        store.close().expect("close plain");
    }

    // Verify it is plaintext on disk
    let plain_bytes = fs::read(&db_path).unwrap();
    assert!(!is_encrypted(&plain_bytes));
    assert!(plain_bytes.starts_with(b"SQLite format 3\0"));

    // 2. Open configured with encrypt = true (converts plain to encrypted)
    {
        let store =
            open_configured(&db_path, Some(&tmp_dir), true).expect("open converted to encrypted");
        let notes: String = store
            .connection()
            .query_row("SELECT notes FROM campaigns WHERE id = 'c1'", [], |row| {
                row.get(0)
            })
            .expect("query notes in encrypted store");
        assert_eq!(notes, "plain notes");

        // Insert additional record
        store
            .connection()
            .execute(
                "INSERT INTO campaigns (id, kind, notes) VALUES ('c2', 'quarterly', 'encrypted notes')",
                [],
            )
            .expect("insert second campaign");
        store.close().expect("close encrypted");
    }

    // Verify it is now encrypted V3 on disk
    let enc_bytes = fs::read(&db_path).unwrap();
    assert!(is_encrypted(&enc_bytes));
    assert_eq!(detect_version(&enc_bytes), Some(EnvelopeVersion::V3));

    // 3. Open configured with encrypt = false (converts encrypted back to plain)
    {
        let store =
            open_configured(&db_path, Some(&tmp_dir), false).expect("open converted back to plain");
        let count: i64 = store
            .connection()
            .query_row("SELECT count(*) FROM campaigns", [], |row| row.get(0))
            .expect("count campaigns");
        assert_eq!(count, 2);
        store.close().expect("close plain");
    }

    // Verify it is plaintext on disk again
    let final_bytes = fs::read(&db_path).unwrap();
    assert!(!is_encrypted(&final_bytes));
    assert!(final_bytes.starts_with(b"SQLite format 3\0"));
}

#[test]
fn open_configured_rejects_malformed_encrypted_file_without_mutation() {
    let _guard = setup_test_key();
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("corrupt.db");
    let tmp_dir = dir.path().join("tmp");

    let malformed = b"SYMERASEME_ENCv3\n0123456789abcdefTHIS_IS_NOT_VALID_BASE64";
    fs::write(&db_path, malformed).unwrap();

    let err = open_configured(&db_path, Some(&tmp_dir), true).unwrap_err();
    assert!(matches!(err, EncryptedStoreError::Encryption(_)));

    // File on disk must remain unmutated
    let after = fs::read(&db_path).unwrap();
    assert_eq!(after, malformed);
}

#[test]
fn scavenge_stale_temps_removes_only_old_artifacts() {
    let dir = tempdir().unwrap();
    let stale_decrypted = dir.path().join("symeraseme_decrypted_1234.db");
    let stale_init = dir.path().join("symeraseme_init_5678.db");
    let stale_write = dir.path().join(".symeraseme_write_9999.tmp");
    let recent_decrypted = dir.path().join("symeraseme_decrypted_recent.db");
    let unrelated_file = dir.path().join("unrelated.txt");

    for file in [
        &stale_decrypted,
        &stale_init,
        &stale_write,
        &recent_decrypted,
        &unrelated_file,
    ] {
        fs::write(file, b"content").unwrap();
    }
    // Also create -wal and -shm siblings for stale_decrypted
    let stale_wal = dir.path().join("symeraseme_decrypted_1234.db-wal");
    let stale_shm = dir.path().join("symeraseme_decrypted_1234.db-shm");
    fs::write(&stale_wal, b"wal").unwrap();
    fs::write(&stale_shm, b"shm").unwrap();

    // Age the stale files by setting mtime > 300s ago
    let old_time = SystemTime::now() - STALE_SCAVENGE_AGE - Duration::from_secs(10);
    let times = FileTimes::new().set_modified(old_time);
    File::open(&stale_decrypted)
        .unwrap()
        .set_times(times)
        .unwrap();
    File::open(&stale_init).unwrap().set_times(times).unwrap();
    File::open(&stale_write).unwrap().set_times(times).unwrap();

    scavenge_stale_temps(dir.path()).expect("scavenge stale temps");

    // Stale artifacts and their WAL siblings must be deleted
    assert!(!stale_decrypted.exists(), "stale decrypted must be deleted");
    assert!(!stale_wal.exists(), "stale wal must be deleted");
    assert!(!stale_shm.exists(), "stale shm must be deleted");
    assert!(!stale_init.exists(), "stale init must be deleted");
    assert!(!stale_write.exists(), "stale write must be deleted");

    // Recent and unrelated files must be kept
    assert!(recent_decrypted.exists(), "recent decrypted must remain");
    assert!(unrelated_file.exists(), "unrelated file must remain");
}

#[test]
fn remove_wal_siblings_removes_both_files() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("test.db");
    let wal = dir.path().join("test.db-wal");
    let shm = dir.path().join("test.db-shm");

    fs::write(&db, b"db").unwrap();
    fs::write(&wal, b"wal").unwrap();
    fs::write(&shm, b"shm").unwrap();

    remove_wal_siblings(&db).expect("remove wal siblings");
    assert!(!wal.exists());
    assert!(!shm.exists());
    assert!(db.exists());

    // Idempotent: calling again on missing files must succeed
    remove_wal_siblings(&db).expect("remove wal siblings idempotent");
}

#[test]
fn checkpoint_wal_truncates_successfully() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("checkpoint_test.db");
    let store = Store::open(&db_path).unwrap();

    // Insert records to create WAL activity
    store
        .connection()
        .execute(
            "INSERT INTO campaigns (id, kind, notes) VALUES ('c1', 'initial', 'test')",
            [],
        )
        .unwrap();

    store.checkpoint_wal().expect("checkpoint WAL TRUNCATE");
}

#[test]
fn finalise_all_persists_unclosed_stores() {
    let _guard = setup_test_key();
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("unclosed.db");
    let tmp_dir = dir.path().join("tmp");

    let store = open_encrypted(&db_path, &tmp_dir).expect("open encrypted");
    store
        .connection()
        .execute(
            "INSERT INTO campaigns (id, kind, notes) VALUES ('finalise_test', 'initial', 'saved by finalise')",
            [],
        )
        .expect("insert");
    // Simulate abrupt shutdown where store is not closed, but finalise_all is invoked
    store.checkpoint_wal().expect("checkpoint");
    // Explicitly drop store connection so file can be safely read on all OSes
    drop(store);

    symeraseme_core::storage::encrypted_store::finalise_all().expect("finalise all");

    // File on disk must now contain the data encrypted with V3
    let enc_bytes = fs::read(&db_path).unwrap();
    assert!(is_encrypted(&enc_bytes));
    let plain = decrypt_v3(&enc_bytes, &TEST_KEY).expect("decrypt finalised db");
    assert!(
        plain
            .windows(b"saved by finalise".len())
            .any(|w| w == b"saved by finalise")
    );
}

#[test]
fn open_encrypted_migrates_v1_v2_v3_golden_fixtures() {
    let _guard = TEST_MUTEX.lock().unwrap();
    let dir = tempdir().unwrap();
    let tmp_dir = dir.path().join("tmp");

    let fixtures_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/event-store/crypto");

    // Pinned test master key for golden fixtures
    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(b"symaira-eraseme-golden-master-32");
    set_master_key(key_bytes);

    for fixture_name in [
        "golden-campaign-v1-python.db",
        "golden-campaign-v2-python.db",
        "golden-campaign-v3-python.db",
    ] {
        let fixture_path = fixtures_dir.join(fixture_name);
        if !fixture_path.exists() {
            continue;
        }
        let test_db = dir.path().join(fixture_name);
        fs::copy(&fixture_path, &test_db).unwrap();

        // Must open successfully and query schema
        let store = open_encrypted(&test_db, &tmp_dir).expect("open golden fixture");
        let count: i64 = store
            .connection()
            .query_row("SELECT count(*) FROM campaigns", [], |row| row.get(0))
            .expect("query campaigns count");
        assert!(count >= 1, "fixture must contain at least 1 campaign");

        // Close store, which migrates to standard V3
        store.close().expect("close golden fixture");

        let migrated_raw = fs::read(&test_db).unwrap();
        assert_eq!(
            detect_version(&migrated_raw),
            Some(EnvelopeVersion::V3),
            "migrated fixture must be V3"
        );
    }
}
