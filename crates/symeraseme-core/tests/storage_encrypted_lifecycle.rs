use std::fs::{self, File, FileTimes, OpenOptions};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};
use symeraseme_core::storage::encrypted_store::{
    CheckpointError, EncryptedStoreError, clear_master_key, decrypt_existing, encrypt_existing,
    finalise_all, is_stale_temp_name, open_configured, open_encrypted, scavenge_stale_temps,
    set_master_key,
};
use symeraseme_core::storage::encryption::{
    EncryptionError, EnvelopeVersion, decrypt_v3, detect_version, encrypt_v3,
};
use symeraseme_core::storage::{DbLock, LockError, lock_path_for};
use tempfile::tempdir;

static TEST_LOCK: Mutex<()> = Mutex::new(());
const KEY: [u8; 32] = [0x42; 32];

#[test]
fn atomic_transition_rejects_nul_paths_before_mutation() {
    let dir = tempdir().unwrap();
    let prefix = dir.path().join("prefix.db");
    fs::write(&prefix, b"original sentinel").unwrap();
    for target in [prefix.clone(), dir.path().join("new-parent").join("new.db")] {
        let mut invalid = target.into_os_string();
        invalid.push("\0ignored-suffix");
        let error = symeraseme_core::storage::atomic_transition_with_recovery(
            Path::new(&invalid),
            b"replacement",
        )
        .expect_err("NUL must not retarget the replacement");
        assert!(matches!(error, EncryptedStoreError::Io(ref error)
            if error.kind() == std::io::ErrorKind::InvalidInput));
        assert_eq!(fs::read(&prefix).unwrap(), b"original sentinel");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    symeraseme_core::storage::atomic_transition_with_recovery(&prefix, b"replacement").unwrap();
    assert_eq!(fs::read(prefix).unwrap(), b"replacement");
}

#[test]
fn encrypted_open_uses_canonical_path_and_private_sqlite_temp() {
    let _guard = TEST_LOCK.lock().unwrap();
    set_master_key(KEY);
    let dir = tempdir().unwrap();
    let canonical = dir.path().join("canonical.db");
    let tmp_dir = dir.path().join("tmp");
    // Pre-create the temp root with a permissive mode: `open_encrypted` has to
    // tighten it to 0700 itself, so the assertion below cannot pass merely
    // because of the process umask.
    fs::create_dir_all(&tmp_dir).unwrap();
    #[cfg(unix)]
    fs::set_permissions(&tmp_dir, fs::Permissions::from_mode(0o755)).unwrap();

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
    // CRY-007 covers the directory as well as the file: the decrypted SQLite
    // temp must live in a private 0700 directory, otherwise a local user could
    // read the plaintext database through the directory entry.
    #[cfg(unix)]
    assert_eq!(
        fs::metadata(&tmp_dir).unwrap().permissions().mode() & 0o777,
        0o700,
        "the decrypted temp directory must be private"
    );
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
fn failed_store_open_removes_temp_lock_after_temp_deletion() {
    let _guard = TEST_LOCK.lock().unwrap();
    set_master_key(KEY);
    let dir = tempdir().unwrap();
    let canonical = dir.path().join("invalid-plaintext.db");
    let tmp_dir = dir.path().join("tmp");
    fs::write(&canonical, encrypt_v3(b"not sqlite", &KEY).unwrap()).unwrap();

    assert!(open_encrypted(&canonical, &tmp_dir).is_err());
    let leftovers: Vec<_> = fs::read_dir(&tmp_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert!(
        leftovers.is_empty(),
        "failed open left temp artifacts: {leftovers:?}"
    );
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
    // The active registration is the liveness guard being tested, even after
    // the temp crosses the stale-age threshold.
    {
        let old = SystemTime::now() - Duration::from_secs(301);
        File::open(&temp)
            .unwrap()
            .set_times(FileTimes::new().set_modified(old))
            .unwrap();
    }
    scavenge_stale_temps(&tmp_dir).unwrap();
    assert!(temp.exists());
    store.close().unwrap();
    clear_master_key();
}

#[test]
fn scavenge_preserves_stale_sidecars_when_main_temp_is_locked() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dir = tempdir().unwrap();
    let tmp_dir = dir.path().join("tmp");
    fs::create_dir_all(&tmp_dir).unwrap();
    let temp = tmp_dir.join("symeraseme_decrypted_locked.db");
    let wal = PathBuf::from(format!("{}-wal", temp.display()));
    let shm = PathBuf::from(format!("{}-shm", temp.display()));
    for path in [&temp, &wal, &shm] {
        File::create(path).unwrap();
    }
    let old = SystemTime::now() - Duration::from_secs(301);
    OpenOptions::new()
        .write(true)
        .open(&temp)
        .unwrap()
        .set_times(FileTimes::new().set_modified(old))
        .unwrap();
    let _owner = DbLock::lock(&temp, 1).unwrap();

    scavenge_stale_temps(&tmp_dir).unwrap();

    assert!(temp.exists());
    assert!(wal.exists());
    assert!(shm.exists());
}

#[test]
fn scavenge_retries_main_after_sidecar_cleanup_failure() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dir = tempdir().unwrap();
    let tmp_dir = dir.path().join("tmp");
    fs::create_dir_all(&tmp_dir).unwrap();
    let temp = tmp_dir.join("symeraseme_decrypted_retry.db");
    let wal = PathBuf::from(format!("{}-wal", temp.display()));
    let shm = PathBuf::from(format!("{}-shm", temp.display()));
    File::create(&temp).unwrap();
    fs::create_dir(&wal).unwrap();
    File::create(&shm).unwrap();
    let old = SystemTime::now() - Duration::from_secs(301);
    OpenOptions::new()
        .write(true)
        .open(&temp)
        .unwrap()
        .set_times(FileTimes::new().set_modified(old))
        .unwrap();

    scavenge_stale_temps(&tmp_dir).unwrap();

    assert!(temp.exists());
    assert!(wal.exists());
    assert!(shm.exists());
    assert!(lock_path_for(&temp).exists());

    fs::remove_dir(&wal).unwrap();
    scavenge_stale_temps(&tmp_dir).unwrap();

    assert!(!temp.exists());
    assert!(!shm.exists());
    assert!(!lock_path_for(&temp).exists());
}

#[test]
fn scavenge_does_not_create_lock_for_recent_unregistered_temp() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dir = tempdir().unwrap();
    let tmp_dir = dir.path().join("tmp");
    fs::create_dir_all(&tmp_dir).unwrap();
    let temp = tmp_dir.join("symeraseme_decrypted_recent.db");
    File::create(&temp).unwrap();

    scavenge_stale_temps(&tmp_dir).unwrap();

    assert!(temp.exists());
    assert!(!lock_path_for(&temp).exists());
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
    OpenOptions::new()
        .write(true)
        .open(&temp)
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

// ---- failure and recovery paths ---------------------------------------------
//
// Every test below runs in its own temporary directory, forces a real
// filesystem failure (read-only directory, occupied target) and asserts the
// returned error together with the files left on disk. Read-only-directory
// tests skip when the platform does not enforce directory modes (root).

const PLAINTEXT_MARKER: &[u8] = b"SQLite format 3\0 plaintext-marker-1304";

fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Asserts no regular file below `dir` contains `needle`.
fn assert_no_file_contains(dir: &Path, needle: &[u8]) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            assert_no_file_contains(&path, needle);
        } else if let Ok(bytes) = fs::read(&path) {
            assert!(
                !bytes.windows(needle.len()).any(|window| window == needle),
                "plaintext residue in {}",
                path.display()
            );
        }
    }
}

fn make_non_empty_dir(path: &Path) {
    fs::create_dir(path).unwrap();
    fs::write(path.join("child"), b"keep").unwrap();
}

/// Restores owner write access so `tempdir` can always clean up.
#[cfg(unix)]
struct RestoreMode(PathBuf);

#[cfg(unix)]
impl Drop for RestoreMode {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
    }
}

#[cfg(unix)]
fn make_read_only(dir: &Path) -> RestoreMode {
    fs::set_permissions(dir, fs::Permissions::from_mode(0o500)).unwrap();
    RestoreMode(dir.to_path_buf())
}

#[cfg(unix)]
fn read_only_dirs_are_enforced() -> bool {
    let probe = tempdir().unwrap();
    let _restore = make_read_only(probe.path());
    let enforced = File::create(probe.path().join("probe")).is_err();
    if !enforced {
        eprintln!("skipping: read-only directories are not enforced (running as root?)");
    }
    enforced
}

#[cfg(unix)]
#[test]
fn atomic_transition_write_failure_leaves_target_and_directory_untouched() {
    if !read_only_dirs_are_enforced() {
        return;
    }
    let dir = tempdir().unwrap();
    let store_dir = dir.path().join("store");
    fs::create_dir(&store_dir).unwrap();
    let target = store_dir.join("enc.db");
    fs::write(&target, b"old ciphertext").unwrap();

    let restore = make_read_only(&store_dir);
    let error = symeraseme_core::storage::atomic_transition_with_recovery(&target, b"new")
        .expect_err("a read-only parent must fail the write");
    drop(restore);

    let EncryptedStoreError::ConversionFailed {
        ref message,
        ref recovery_path,
    } = error
    else {
        panic!("unexpected variant: {error:?}");
    };
    assert!(message.starts_with("write replacement failed"), "{message}");
    assert_eq!(*recovery_path, None, "nothing was left behind to recover");
    assert!(
        error
            .to_string()
            .starts_with("eventstore: conversion failed:")
    );
    assert_eq!(fs::read(&target).unwrap(), b"old ciphertext");
    assert_eq!(listing(&store_dir), ["enc.db"]);
}

#[test]
fn atomic_transition_rename_failure_keeps_recovery_copy_at_reported_path() {
    let dir = tempdir().unwrap();
    let target = dir.path().join("enc.db");
    make_non_empty_dir(&target);

    let error = symeraseme_core::storage::atomic_transition_with_recovery(&target, b"new-cipher")
        .expect_err("renaming a file over a non-empty directory must fail");

    let EncryptedStoreError::ConversionFailed {
        ref message,
        ref recovery_path,
    } = error
    else {
        panic!("unexpected variant: {error:?}");
    };
    assert!(message.starts_with("rename failed"), "{message}");
    assert!(!message.contains("recovery rename failed"), "{message}");
    let recovery = recovery_path.clone().expect("recovery path is reported");
    assert_eq!(recovery.parent().unwrap(), dir.path());
    let name = recovery.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with(".symeraseme_recovery_") && is_stale_temp_name(&name));
    assert_eq!(fs::read(&recovery).unwrap(), b"new-cipher");
    #[cfg(unix)]
    assert_eq!(
        recovery.metadata().unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(error.to_string().contains(&recovery.display().to_string()));
    // The occupied target is untouched and the write temp was renamed away.
    assert_eq!(fs::read(target.join("child")).unwrap(), b"keep");
    assert_eq!(listing(dir.path()), [name.as_str(), "enc.db"]);
}

#[test]
fn atomic_transition_rejects_a_parent_that_is_a_file() {
    let dir = tempdir().unwrap();
    let parent = dir.path().join("not-a-dir");
    fs::write(&parent, b"owner").unwrap();

    let error =
        symeraseme_core::storage::atomic_transition_with_recovery(&parent.join("enc.db"), b"x")
            .expect_err("a file cannot be a parent directory");

    assert!(matches!(error, EncryptedStoreError::Io(_)), "{error:?}");
    assert!(std::error::Error::source(&error).is_some());
    assert_eq!(fs::read(&parent).unwrap(), b"owner");
    assert_eq!(listing(dir.path()), ["not-a-dir"]);
}

#[test]
fn decrypt_existing_fails_closed_without_the_right_key() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("decrypt.db");
    let ciphertext = encrypt_v3(PLAINTEXT_MARKER, &KEY).unwrap();
    fs::write(&path, &ciphertext).unwrap();

    set_master_key([0x43; 32]);
    let error = decrypt_existing(&path).expect_err("wrong key must not decrypt");
    assert!(
        matches!(error, EncryptedStoreError::Encryption(_)),
        "{error:?}"
    );

    clear_master_key();
    let error = decrypt_existing(&path).expect_err("missing key must not decrypt");
    assert!(matches!(error, EncryptedStoreError::MasterKeyUnavailable));

    assert_eq!(fs::read(&path).unwrap(), ciphertext);
    assert_eq!(listing(dir.path()), ["decrypt.db"]);
    assert_no_file_contains(dir.path(), PLAINTEXT_MARKER);
}

#[test]
fn decrypt_existing_leaves_missing_empty_and_plain_files_alone() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dir = tempdir().unwrap();
    decrypt_existing(&dir.path().join("missing.db")).unwrap();
    let empty = dir.path().join("empty.db");
    fs::write(&empty, b"").unwrap();
    decrypt_existing(&empty).unwrap();
    let plain = dir.path().join("plain.db");
    fs::write(&plain, PLAINTEXT_MARKER).unwrap();
    decrypt_existing(&plain).unwrap();
    assert_eq!(fs::read(&empty).unwrap(), b"");
    assert_eq!(fs::read(&plain).unwrap(), PLAINTEXT_MARKER);

    // An unreadable path is an I/O error, not a silent success.
    let directory = dir.path().join("a-directory");
    fs::create_dir(&directory).unwrap();
    let error = decrypt_existing(&directory).expect_err("directories cannot be decrypted");
    assert!(matches!(error, EncryptedStoreError::Io(_)), "{error:?}");
    let error = encrypt_existing(&directory).expect_err("directories cannot be encrypted");
    assert!(matches!(error, EncryptedStoreError::Io(_)), "{error:?}");
}

#[cfg(unix)]
#[test]
fn decrypt_existing_write_failure_keeps_ciphertext_and_writes_no_plaintext() {
    let _guard = TEST_LOCK.lock().unwrap();
    if !read_only_dirs_are_enforced() {
        return;
    }
    set_master_key(KEY);
    let dir = tempdir().unwrap();
    let path = dir.path().join("decrypt-ro.db");
    let ciphertext = encrypt_v3(PLAINTEXT_MARKER, &KEY).unwrap();
    fs::write(&path, &ciphertext).unwrap();

    let restore = make_read_only(dir.path());
    let error = decrypt_existing(&path).expect_err("read-only parent must fail the rewrite");
    drop(restore);
    clear_master_key();

    assert!(matches!(
        error,
        EncryptedStoreError::ConversionFailed {
            recovery_path: None,
            ..
        }
    ));
    assert_eq!(fs::read(&path).unwrap(), ciphertext);
    assert_eq!(listing(dir.path()), ["decrypt-ro.db"]);
    assert_no_file_contains(dir.path(), PLAINTEXT_MARKER);
}

#[test]
fn encrypt_existing_fails_closed_without_a_key_and_round_trips_with_one() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("encrypt.db");
    fs::write(&path, PLAINTEXT_MARKER).unwrap();

    clear_master_key();
    let error = encrypt_existing(&path).expect_err("missing key must not encrypt");
    assert!(matches!(error, EncryptedStoreError::MasterKeyUnavailable));
    assert_eq!(fs::read(&path).unwrap(), PLAINTEXT_MARKER);
    assert_eq!(listing(dir.path()), ["encrypt.db"]);

    set_master_key(KEY);
    encrypt_existing(&path).unwrap();
    let ciphertext = fs::read(&path).unwrap();
    assert_eq!(decrypt_v3(&ciphertext, &KEY).unwrap(), PLAINTEXT_MARKER);
    // Already-encrypted, empty and missing files are no-ops.
    encrypt_existing(&path).unwrap();
    assert_eq!(fs::read(&path).unwrap(), ciphertext);
    encrypt_existing(&dir.path().join("missing.db")).unwrap();
    assert_eq!(listing(dir.path()), ["encrypt.db"]);
    clear_master_key();
}

#[cfg(unix)]
#[test]
fn encrypt_existing_write_failure_keeps_original_and_leaves_no_copy() {
    let _guard = TEST_LOCK.lock().unwrap();
    if !read_only_dirs_are_enforced() {
        return;
    }
    set_master_key(KEY);
    let dir = tempdir().unwrap();
    let path = dir.path().join("encrypt-ro.db");
    fs::write(&path, PLAINTEXT_MARKER).unwrap();

    let restore = make_read_only(dir.path());
    let error = encrypt_existing(&path).expect_err("read-only parent must fail the rewrite");
    drop(restore);
    clear_master_key();

    assert!(matches!(
        error,
        EncryptedStoreError::ConversionFailed {
            recovery_path: None,
            ..
        }
    ));
    assert_eq!(fs::read(&path).unwrap(), PLAINTEXT_MARKER);
    assert_eq!(listing(dir.path()), ["encrypt-ro.db"]);
}

#[cfg(unix)]
#[test]
fn close_write_failure_keeps_canonical_ciphertext_and_retry_leaves_no_plaintext() {
    let _guard = TEST_LOCK.lock().unwrap();
    if !read_only_dirs_are_enforced() {
        return;
    }
    set_master_key(KEY);
    let dir = tempdir().unwrap();
    let canonical_dir = dir.path().join("canonical");
    fs::create_dir(&canonical_dir).unwrap();
    let canonical = canonical_dir.join("close-failure.db");
    let tmp_dir = dir.path().join("tmp");
    let store = open_encrypted(&canonical, &tmp_dir).expect("open encrypted store");
    store
        .connection()
        .execute(
            "INSERT INTO campaigns (id, kind, notes) VALUES ('failed-close', 'initial', 'survives')",
            [],
        )
        .unwrap();
    let before = fs::read(&canonical).unwrap();

    let restore = make_read_only(&canonical_dir);
    let error = store.close().expect_err("re-encryption must fail");
    let EncryptedStoreError::ConversionFailed {
        ref message,
        ref recovery_path,
    } = error
    else {
        panic!("unexpected variant: {error:?}");
    };
    assert!(message.starts_with("write replacement failed"), "{message}");
    assert_eq!(*recovery_path, None);

    // The canonical file is still the previous ciphertext and nothing but the
    // lock sits next to it: no plaintext, no transition temp.
    assert_eq!(fs::read(&canonical).unwrap(), before);
    assert!(decrypt_v3(&before, &KEY).is_ok());
    let lock_name = lock_path_for(&canonical)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        listing(&canonical_dir),
        ["close-failure.db", lock_name.as_str()]
    );
    assert_no_file_contains(&canonical_dir, b"SQLite format 3\0");
    // Plaintext stays only in the private temp area until the retry.
    let temps = listing(&tmp_dir);
    assert!(
        temps
            .iter()
            .any(|name| name.starts_with("symeraseme_decrypted_")),
        "{temps:?}"
    );
    for name in &temps {
        let mode = fs::metadata(tmp_dir.join(name))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode & 0o077, 0, "{name} must not be group/world accessible");
    }

    drop(restore);
    finalise_all().expect("retry after restoring write access");
    assert_eq!(listing(&tmp_dir), Vec::<String>::new());
    assert_no_file_contains(&tmp_dir, b"SQLite format 3\0");
    let reopened = open_encrypted(&canonical, dir.path().join("reopen-tmp")).unwrap();
    let notes: String = reopened
        .connection()
        .query_row(
            "SELECT notes FROM campaigns WHERE id = 'failed-close'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(notes, "survives");
    reopened.close().unwrap();
    clear_master_key();
}

#[cfg(unix)]
#[test]
fn open_encrypted_publish_failure_removes_the_plaintext_initializer() {
    let _guard = TEST_LOCK.lock().unwrap();
    if !read_only_dirs_are_enforced() {
        return;
    }
    set_master_key(KEY);
    let dir = tempdir().unwrap();
    let canonical_dir = dir.path().join("canonical");
    fs::create_dir(&canonical_dir).unwrap();
    let canonical = canonical_dir.join("publish-failure.db");
    let tmp_dir = dir.path().join("tmp");
    // Pre-create the lock file: opening an existing file needs no directory
    // write access, so only publishing the new ciphertext can fail.
    File::create(lock_path_for(&canonical)).unwrap();

    let restore = make_read_only(&canonical_dir);
    let error = open_encrypted(&canonical, &tmp_dir).expect_err("publish must fail");
    drop(restore);
    clear_master_key();

    assert!(
        matches!(
            error,
            EncryptedStoreError::ConversionFailed {
                recovery_path: None,
                ..
            }
        ),
        "{error:?}"
    );
    assert!(!canonical.exists());
    assert_eq!(listing(&tmp_dir), Vec::<String>::new());
    assert_no_file_contains(dir.path(), b"SQLite format 3\0");
}

#[test]
fn open_encrypted_with_wrong_key_creates_no_plaintext_temp() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dir = tempdir().unwrap();
    let canonical = dir.path().join("wrong-key.db");
    let tmp_dir = dir.path().join("tmp");
    let ciphertext = encrypt_v3(PLAINTEXT_MARKER, &KEY).unwrap();
    fs::write(&canonical, &ciphertext).unwrap();

    set_master_key([0x43; 32]);
    let error = open_encrypted(&canonical, &tmp_dir).expect_err("wrong key must not open");
    clear_master_key();

    assert!(
        matches!(error, EncryptedStoreError::Encryption(_)),
        "{error:?}"
    );
    assert_eq!(fs::read(&canonical).unwrap(), ciphertext);
    assert_eq!(listing(&tmp_dir), Vec::<String>::new());
    assert_no_file_contains(dir.path(), PLAINTEXT_MARKER);
}

#[test]
fn error_display_source_and_from_conversions_cover_every_variant() {
    use std::error::Error;
    let io = || std::io::Error::other("disk");
    let lock = LockError::Acquire {
        path: PathBuf::from("/db.lock"),
        source: io(),
    };
    assert!(lock.to_string().contains("cannot acquire DB lock /db.lock"));
    let release = LockError::Release {
        path: PathBuf::from("/db.lock"),
        source: io(),
    };
    assert!(
        release
            .to_string()
            .contains("lock release failed for /db.lock")
    );

    let cases: Vec<(EncryptedStoreError, &str, bool)> = vec![
        (lock.into(), "cannot acquire DB lock", true),
        (io().into(), "eventstore I/O error: disk", true),
        (
            rusqlite::Error::QueryReturnedNoRows.into(),
            "eventstore SQLite error",
            true,
        ),
        (
            EncryptionError::AuthenticationFailed.into(),
            "eventstore encryption error",
            true,
        ),
        (
            CheckpointError::Query(rusqlite::Error::QueryReturnedNoRows).into(),
            "eventstore: WAL checkpoint:",
            true,
        ),
        (
            EncryptedStoreError::MasterKeyUnavailable,
            "identity master key unavailable",
            false,
        ),
        (
            EncryptedStoreError::InvalidPath("bad".into()),
            "eventstore: invalid path: bad",
            false,
        ),
        (
            EncryptedStoreError::DirectorySyncFailed {
                path: PathBuf::from("/enc.db"),
                recovery_path: None,
                source: io(),
            },
            "eventstore: directory sync failed for /enc.db: disk",
            true,
        ),
        (
            EncryptedStoreError::ConversionFailed {
                message: "boom".into(),
                recovery_path: None,
            },
            "eventstore: conversion failed: boom",
            false,
        ),
    ];
    for (error, expected, has_source) in cases {
        assert!(
            error.to_string().contains(expected),
            "{error} !~ {expected}"
        );
        assert_eq!(error.source().is_some(), has_source, "{error:?}");
    }

    let busy = CheckpointError::Busy {
        busy: 1,
        log_frames: 2,
        checkpointed: 3,
    };
    assert_eq!(
        busy.to_string(),
        "eventstore: WAL checkpoint busy (busy=1 log_frames=2 checkpointed=3)"
    );
    assert!(busy.source().is_none());
    let query = CheckpointError::Query(rusqlite::Error::QueryReturnedNoRows);
    assert!(query.source().is_some());
}

#[test]
fn scavenge_removes_only_unregistered_stale_transition_files() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dir = tempdir().unwrap();
    let tmp_dir = dir.path().join("tmp");
    fs::create_dir_all(&tmp_dir).unwrap();
    let old = SystemTime::now() - Duration::from_secs(301);
    let age = |path: &Path| {
        File::open(path)
            .unwrap()
            .set_times(FileTimes::new().set_modified(old))
            .unwrap();
    };

    let stale = [
        ".symeraseme_write_old.tmp",
        ".symeraseme_recovery_old.tmp",
        ".symeraseme_previous_old.tmp",
        "symeraseme_decrypted_old.db",
        "symeraseme_init_old.db",
    ];
    for name in stale {
        fs::write(tmp_dir.join(name), b"stale").unwrap();
        age(&tmp_dir.join(name));
    }
    // Sidecars of a stale main file go with it.
    for suffix in ["-wal", "-shm"] {
        fs::write(
            tmp_dir.join(format!("symeraseme_init_old.db{suffix}")),
            b"x",
        )
        .unwrap();
    }

    // Survivors: recent transition files, unrelated names, directories that
    // merely look like transition files, and orphan locks.
    let keep_recent = tmp_dir.join(".symeraseme_write_recent.tmp");
    let keep_unrelated = tmp_dir.join("unrelated.db");
    let keep_dir = tmp_dir.join(".symeraseme_recovery_dir.tmp");
    let keep_lock = tmp_dir.join("symeraseme_init_orphan.db.lock");
    fs::write(&keep_recent, b"recent").unwrap();
    fs::write(&keep_unrelated, b"unrelated").unwrap();
    make_non_empty_dir(&keep_dir);
    fs::write(&keep_lock, b"lock").unwrap();
    for path in [&keep_unrelated, &keep_dir, &keep_lock] {
        age(path);
    }

    scavenge_stale_temps(&tmp_dir).unwrap();

    let mut expected = vec![
        keep_recent
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        keep_unrelated
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        keep_dir.file_name().unwrap().to_string_lossy().into_owned(),
        keep_lock
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
    ];
    expected.sort();
    assert_eq!(listing(&tmp_dir), expected);
}

#[test]
fn scavenge_reports_unreadable_directory_but_ignores_a_missing_one() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dir = tempdir().unwrap();
    scavenge_stale_temps(dir.path().join("missing")).unwrap();
    let file = dir.path().join("not-a-dir");
    fs::write(&file, b"x").unwrap();
    assert!(scavenge_stale_temps(&file).is_err());
}
