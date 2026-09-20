//! Encrypted at-rest event-store lifecycle, safe temp-file handling, and scavenging.
//!
//! Mirrors `internal/eventstore/encrypt.go` and `cleanup.go`.

use super::encryption::{EncryptionError, decrypt_any, encrypt_v3, is_encrypted};
use super::locking::{DbLock, LockError, lock_path_for};
use super::store::Store;
use rand::Rng;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
#[cfg(not(windows))]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, RwLock};
use std::time::{Duration, SystemTime};

#[cfg(windows)]
#[allow(unsafe_code)]
fn replace_file(source: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(source: *const u16, target: *const u16, flags: u32) -> i32;
        fn GetLastError() -> u32;
    }
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    // MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH.
    let ok = unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), 0x1 | 0x8) };
    if ok != 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(
            unsafe { GetLastError() } as i32
        ))
    }
}

/// Cutoff age for stale decrypted temporary files (300 seconds).
pub const STALE_SCAVENGE_AGE: Duration = Duration::from_secs(300);

/// Master key provider returning the 32-byte identity master key on demand.
pub type MasterKeyProvider = Arc<dyn Fn() -> Result<[u8; 32], EncryptionError> + Send + Sync>;

static MASTER_KEY_PROVIDER: RwLock<Option<MasterKeyProvider>> = RwLock::new(None);
static DIRECT_MASTER_KEY: RwLock<Option<[u8; 32]>> = RwLock::new(None);

fn default_encrypted_temp_dir() -> Result<PathBuf, EncryptedStoreError> {
    #[cfg(target_os = "macos")]
    let root = absolute_path(std::env::var_os("HOME").map(PathBuf::from))
        .map(|home| home.join("Library/Caches"))
        .unwrap_or_else(std::env::temp_dir);
    #[cfg(target_os = "windows")]
    let root = absolute_path(std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
        .or_else(|| absolute_path(std::env::var_os("USERPROFILE").map(PathBuf::from)))
        .unwrap_or_else(std::env::temp_dir);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let root = default_unix_cache_root(
        std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    );
    checked_temp_root(root)
}

fn checked_temp_root(root: PathBuf) -> Result<PathBuf, EncryptedStoreError> {
    if root.as_os_str().is_empty() || !root.is_absolute() {
        return Err(EncryptedStoreError::InvalidPath(
            "encrypted temporary root must be absolute".into(),
        ));
    }
    Ok(root.join("symeraseme").join("database"))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn absolute_path(path: Option<PathBuf>) -> Option<PathBuf> {
    path.filter(|path| !path.as_os_str().is_empty() && path.is_absolute())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn default_unix_cache_root(xdg: Option<PathBuf>, home: Option<PathBuf>) -> PathBuf {
    xdg.filter(|path| path.is_absolute())
        .or_else(|| {
            home.filter(|path| path.is_absolute())
                .map(|home| home.join(".cache"))
        })
        .unwrap_or_else(std::env::temp_dir)
}

#[cfg(test)]
mod tests {
    use super::default_encrypted_temp_dir;
    use super::{
        EncryptedStoreError, cleanup_recovery_paths, remember_recovery_path, scavenge_stale_temps,
    };
    use std::fs::{File, FileTimes};
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};
    use tempfile::tempdir;

    #[test]
    fn default_temp_dir_is_user_scoped() {
        #[cfg(target_os = "macos")]
        let root = super::absolute_path(std::env::var_os("HOME").map(PathBuf::from))
            .map(|home| home.join("Library/Caches"))
            .unwrap_or_else(std::env::temp_dir);
        #[cfg(target_os = "windows")]
        let root = super::absolute_path(std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
            .or_else(|| super::absolute_path(std::env::var_os("USERPROFILE").map(PathBuf::from)))
            .unwrap_or_else(std::env::temp_dir);
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let root = super::default_unix_cache_root(
            std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from),
            std::env::var_os("HOME").map(PathBuf::from),
        );

        let expected = root.join("symeraseme").join("database");
        assert_eq!(default_encrypted_temp_dir().unwrap(), expected);
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn cache_environment_candidates_require_nonempty_absolute_paths() {
        assert_eq!(super::absolute_path(None), None);
        assert_eq!(
            super::absolute_path(Some(PathBuf::from("relative/cache"))),
            None
        );
        assert_eq!(super::absolute_path(Some(PathBuf::new())), None);
        let temp = tempdir().unwrap();
        let absolute = temp.path().join("user").join("cache");
        assert_eq!(super::absolute_path(Some(absolute.clone())), Some(absolute));
    }

    #[test]
    fn default_temp_root_rejects_empty_or_relative_fallbacks() {
        assert!(super::checked_temp_root(PathBuf::new()).is_err());
        assert!(super::checked_temp_root(PathBuf::from("relative-cache")).is_err());
        let temp = tempdir().unwrap();
        let absolute = temp.path().join("absolute-cache");
        assert!(super::checked_temp_root(absolute).is_ok());
    }

    #[test]
    fn initializer_collision_keeps_preexisting_file_and_sidecars() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("symeraseme_init_collision.db");
        let wal = PathBuf::from(format!("{}-wal", path.display()));
        let shm = PathBuf::from(format!("{}-shm", path.display()));
        std::fs::write(&path, b"owner").unwrap();
        std::fs::write(&wal, b"wal").unwrap();
        std::fs::write(&shm, b"shm").unwrap();

        let error = super::create_initializer(&path).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&path).unwrap(), b"owner");
        assert_eq!(std::fs::read(wal).unwrap(), b"wal");
        assert_eq!(std::fs::read(shm).unwrap(), b"shm");
    }

    #[cfg(unix)]
    #[test]
    fn existing_parent_dir_permissions_are_not_changed() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let existing = dir.path().join("existing");
        std::fs::create_dir(&existing).unwrap();
        std::fs::set_permissions(&existing, std::fs::Permissions::from_mode(0o755)).unwrap();

        super::ensure_parent_dir(&existing).unwrap();

        assert_eq!(
            existing.metadata().unwrap().permissions().mode() & 0o777,
            0o755
        );
    }

    #[test]
    fn recovery_registration_keeps_all_pending_paths_until_cleanup() {
        let dir = tempdir().unwrap();
        let encrypted = dir.path().join("encrypted.db");
        let first = dir.path().join(".symeraseme_recovery_first.tmp");
        let second = dir.path().join(".symeraseme_recovery_second.tmp");
        File::create(&first).unwrap();
        File::create(&second).unwrap();

        super::ENCRYPTED_TEMPS.lock().unwrap().insert(
            encrypted.clone(),
            super::RegisteredTemp {
                tmp_path: dir.path().join("plain.db"),
                active: false,
                lock: None,
                temp_lock: None,
                recovery_paths: Vec::new(),
            },
        );
        for path in [&first, &second] {
            remember_recovery_path(
                &encrypted,
                &EncryptedStoreError::ConversionFailed {
                    message: "test".into(),
                    recovery_path: Some(path.clone()),
                },
            );
        }
        let paths = super::registered_recovery_paths(&encrypted);
        assert_eq!(paths.len(), 2);
        cleanup_recovery_paths(&paths).unwrap();
        assert!(!first.exists());
        assert!(!second.exists());
        super::ENCRYPTED_TEMPS.lock().unwrap().remove(&encrypted);
    }

    #[test]
    fn scavenge_preserves_registered_stale_temp() {
        let dir = tempdir().unwrap();
        let encrypted = dir.path().join("encrypted.db");
        let temp = dir.path().join("symeraseme_decrypted_registered.db");
        File::create(&temp)
            .unwrap()
            .set_times(FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(301)))
            .unwrap();

        super::ENCRYPTED_TEMPS.lock().unwrap().insert(
            encrypted.clone(),
            super::RegisteredTemp {
                tmp_path: temp.clone(),
                active: true,
                lock: None,
                temp_lock: None,
                recovery_paths: Vec::new(),
            },
        );

        scavenge_stale_temps(dir.path()).unwrap();

        assert!(temp.exists());
        super::unregister_temp(&encrypted);
        std::fs::remove_file(temp).unwrap();
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    #[test]
    fn relative_xdg_cache_path_falls_back_to_absolute_home_cache() {
        assert_eq!(
            super::default_unix_cache_root(
                Some(PathBuf::from("relative/cache")),
                Some(PathBuf::from("/user")),
            ),
            PathBuf::from("/user/.cache")
        );
    }
}

/// Sets the master key provider function.
pub fn set_master_key_provider(provider: Option<MasterKeyProvider>) {
    let mut lock = MASTER_KEY_PROVIDER
        .write()
        .expect("lock master key provider");
    *lock = provider;
}

/// Sets a direct master key for tests or standalone callers.
pub fn set_master_key(key: [u8; 32]) {
    let mut lock = DIRECT_MASTER_KEY.write().expect("lock direct master key");
    *lock = Some(key);
}

/// Clears any configured master key or provider.
pub fn clear_master_key() {
    let mut p_lock = MASTER_KEY_PROVIDER
        .write()
        .expect("lock master key provider");
    *p_lock = None;
    let mut k_lock = DIRECT_MASTER_KEY.write().expect("lock direct master key");
    *k_lock = None;
}

/// Returns the active 32-byte master key from the direct key or provider.
pub fn current_master_key() -> Result<[u8; 32], EncryptionError> {
    if let Some(key) = *DIRECT_MASTER_KEY.read().expect("read direct master key") {
        return Ok(key);
    }
    let p_lock = MASTER_KEY_PROVIDER
        .read()
        .expect("read master key provider");
    if let Some(ref provider) = *p_lock {
        provider()
    } else {
        Err(EncryptionError::AuthenticationFailed)
    }
}

/// Errors occurring during encrypted store lifecycle operations.
#[derive(Debug)]
pub enum EncryptedStoreError {
    Lock(LockError),
    Io(io::Error),
    Sqlite(rusqlite::Error),
    Encryption(EncryptionError),
    Checkpoint(CheckpointError),
    MasterKeyUnavailable,
    InvalidPath(String),
    DirectorySyncFailed {
        path: PathBuf,
        recovery_path: Option<PathBuf>,
        source: io::Error,
    },
    ConversionFailed {
        message: String,
        recovery_path: Option<PathBuf>,
    },
}

impl std::fmt::Display for EncryptedStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lock(err) => write!(f, "{err}"),
            Self::Io(err) => write!(f, "eventstore I/O error: {err}"),
            Self::Sqlite(err) => write!(f, "eventstore SQLite error: {err}"),
            Self::Encryption(err) => write!(f, "eventstore encryption error: {err}"),
            Self::Checkpoint(err) => write!(f, "{err}"),
            Self::MasterKeyUnavailable => {
                f.write_str("eventstore: encryption requested: identity master key unavailable")
            }
            Self::InvalidPath(msg) => write!(f, "eventstore: invalid path: {msg}"),
            Self::DirectorySyncFailed {
                path,
                recovery_path,
                source,
            } => {
                if let Some(rec) = recovery_path {
                    write!(
                        f,
                        "eventstore: directory sync failed for {} (recovery copy at {}): {source}",
                        path.display(),
                        rec.display()
                    )
                } else {
                    write!(
                        f,
                        "eventstore: directory sync failed for {}: {source}",
                        path.display()
                    )
                }
            }
            Self::ConversionFailed {
                message,
                recovery_path,
            } => {
                if let Some(rec) = recovery_path {
                    write!(
                        f,
                        "eventstore: conversion failed (recovery copy at {}): {message}",
                        rec.display()
                    )
                } else {
                    write!(f, "eventstore: conversion failed: {message}")
                }
            }
        }
    }
}

impl std::error::Error for EncryptedStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Lock(err) => Some(err),
            Self::Io(err) => Some(err),
            Self::Sqlite(err) => Some(err),
            Self::Encryption(err) => Some(err),
            Self::Checkpoint(err) => Some(err),
            Self::DirectorySyncFailed { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<LockError> for EncryptedStoreError {
    fn from(err: LockError) -> Self {
        Self::Lock(err)
    }
}

impl From<io::Error> for EncryptedStoreError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<rusqlite::Error> for EncryptedStoreError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Sqlite(err)
    }
}

impl From<EncryptionError> for EncryptedStoreError {
    fn from(err: EncryptionError) -> Self {
        Self::Encryption(err)
    }
}

impl From<CheckpointError> for EncryptedStoreError {
    fn from(err: CheckpointError) -> Self {
        Self::Checkpoint(err)
    }
}

/// WAL checkpoint error details matching the Go eventstore error contract.
#[derive(Debug)]
pub enum CheckpointError {
    Busy {
        busy: i64,
        log_frames: i64,
        checkpointed: i64,
    },
    Query(rusqlite::Error),
}

impl std::fmt::Display for CheckpointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy {
                busy,
                log_frames,
                checkpointed,
            } => write!(
                f,
                "eventstore: WAL checkpoint busy (busy={busy} log_frames={log_frames} checkpointed={checkpointed})"
            ),
            Self::Query(err) => write!(f, "eventstore: WAL checkpoint: {err}"),
        }
    }
}

impl std::error::Error for CheckpointError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Query(err) => Some(err),
            _ => None,
        }
    }
}

/// Runs `PRAGMA wal_checkpoint(TRUNCATE)` on a connection and checks all result columns.
pub fn checkpoint_wal_conn(conn: &Connection) -> Result<(), CheckpointError> {
    let mut stmt = conn
        .prepare("PRAGMA wal_checkpoint(TRUNCATE)")
        .map_err(CheckpointError::Query)?;

    let mut rows = stmt.query([]).map_err(CheckpointError::Query)?;
    if let Some(row) = rows.next().map_err(CheckpointError::Query)? {
        let busy: i64 = row.get(0).map_err(CheckpointError::Query)?;
        let log_frames: i64 = row.get(1).map_err(CheckpointError::Query)?;
        let checkpointed: i64 = row.get(2).map_err(CheckpointError::Query)?;

        if busy != 0 {
            return Err(CheckpointError::Busy {
                busy,
                log_frames,
                checkpointed,
            });
        }
    }
    Ok(())
}

/// Ensures a directory exists and has mode 0700 (POSIX).
pub fn ensure_private_dir(dir: impl AsRef<Path>) -> io::Result<()> {
    let dir = dir.as_ref();
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn ensure_parent_dir(dir: impl AsRef<Path>) -> io::Result<()> {
    fs::create_dir_all(dir)
}

/// Ensures a file has mode 0600 (POSIX).
pub fn ensure_private_file(path: impl AsRef<Path>, mode: u32) -> io::Result<()> {
    let path = path.as_ref();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    let _ = path;
    let _ = mode;
    Ok(())
}

/// Syncs the containing directory to ensure durability of renames and directory metadata.
pub fn sync_dir(dir: impl AsRef<Path>) -> io::Result<()> {
    #[cfg(windows)]
    {
        let _ = dir;
        Ok(())
    }

    #[cfg(not(windows))]
    {
        let dir = dir.as_ref();
        let file = File::open(dir)?;
        file.sync_all()
    }
}

/// Deletes the `-wal` and `-shm` sidecars next to `path`.
pub fn remove_wal_siblings(path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref();
    for suffix in ["-wal", "-shm"] {
        let mut sib = path.as_os_str().to_os_string();
        sib.push(suffix);
        let sib_path = PathBuf::from(sib);
        if let Err(err) = fs::remove_file(&sib_path)
            && err.kind() != io::ErrorKind::NotFound
        {
            return Err(err);
        }
    }
    Ok(())
}

/// Reports whether a filename matches the prefix of a stale temporary transition file.
pub fn is_stale_temp_name(name: &str) -> bool {
    for prefix in [
        "symeraseme_decrypted_",
        "symeraseme_init_",
        ".symeraseme_write_",
        ".symeraseme_recovery_",
        ".symeraseme_previous_",
    ] {
        if name.starts_with(prefix) {
            return true;
        }
    }
    false
}

/// Removes orphaned private transition files older than [`STALE_SCAVENGE_AGE`].
pub fn scavenge_stale_temps(tmp_dir: impl AsRef<Path>) -> io::Result<()> {
    let tmp_dir = tmp_dir.as_ref();
    let entries = match fs::read_dir(tmp_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };

    let registered: HashSet<PathBuf> = ENCRYPTED_TEMPS
        .lock()
        .expect("lock encrypted temps")
        .values()
        .flat_map(|registration| {
            std::iter::once(registration.tmp_path.clone())
                .chain(
                    registration
                        .temp_lock
                        .as_ref()
                        .map(|_| lock_path_for(&registration.tmp_path)),
                )
                .chain(registration.recovery_paths.iter().cloned())
        })
        .collect();
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();
        if !is_stale_temp_name(&name_str) {
            continue;
        }
        // SQLite sidecars and advisory locks are cleaned by their main DB
        // candidate. Treating them as independent stale files could bypass
        // the main temp lock and delete live WAL frames.
        if name_str.ends_with(".lock") || name_str.ends_with("-wal") || name_str.ends_with("-shm") {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if metadata.is_dir() {
            continue;
        }

        let path = entry.path();
        if is_registered_temp_or_sidecar(&path, &registered) {
            continue;
        }

        let mtime = metadata.modified().unwrap_or(now);
        let Ok(age) = now.duration_since(mtime) else {
            continue;
        };
        if age <= STALE_SCAVENGE_AGE {
            continue;
        }

        // A decrypted temp has no canonical-path information in its name, so
        // use a lock next to the temp itself for cross-process ownership.
        let temp_lock = if name_str.starts_with("symeraseme_decrypted_") {
            match DbLock::lock_with_delay(entry.path(), 1, Duration::ZERO) {
                Ok(lock) => Some(lock),
                Err(_) => continue,
            }
        } else {
            None
        };
        let temp_lock_path = temp_lock.as_ref().map(|_| lock_path_for(&path));
        // Keep the main plaintext DB and its lock reachable until every
        // SQLite sidecar has been removed. A failed sidecar cleanup is
        // retryable; deleting the main DB first would strand WAL frames.
        if remove_wal_siblings(&path).is_err() {
            drop(temp_lock);
            continue;
        }
        let removed = remove_if_exists(&path);
        if removed.is_ok()
            && let Some(lock_path) = temp_lock_path
        {
            let _ = remove_if_exists(&lock_path);
        }
        drop(temp_lock);
    }
    Ok(())
}

#[derive(Debug)]
struct RegisteredTemp {
    tmp_path: PathBuf,
    active: bool,
    lock: Option<DbLock>,
    temp_lock: Option<DbLock>,
    recovery_paths: Vec<PathBuf>,
}

static ENCRYPTED_TEMPS: LazyLock<Mutex<HashMap<PathBuf, RegisteredTemp>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn register_temp(enc_path: PathBuf, tmp_path: PathBuf, temp_lock: DbLock) {
    let mut map = ENCRYPTED_TEMPS.lock().expect("lock encrypted temps");
    map.insert(
        enc_path,
        RegisteredTemp {
            tmp_path,
            active: true,
            lock: None,
            temp_lock: Some(temp_lock),
            recovery_paths: Vec::new(),
        },
    );
}

fn unregister_temp(enc_path: &Path) -> Option<RegisteredTemp> {
    let mut map = ENCRYPTED_TEMPS.lock().expect("lock encrypted temps");
    let mut registration = map.remove(enc_path)?;
    drop(map);

    if let Some(mut temp_lock) = registration.temp_lock.take() {
        cleanup_temp_lock_after_main(&registration.tmp_path, Some(&mut temp_lock));
    }
    Some(registration)
}

fn recovery_path(error: &EncryptedStoreError) -> Option<PathBuf> {
    match error {
        EncryptedStoreError::DirectorySyncFailed {
            recovery_path: Some(path),
            ..
        }
        | EncryptedStoreError::ConversionFailed {
            recovery_path: Some(path),
            ..
        } => Some(path.clone()),
        _ => None,
    }
}

fn retain_failed_close(enc_path: &Path, store: &mut Store, error: &EncryptedStoreError) {
    let Some(lock) = store.db_lock.take() else {
        return;
    };
    let mut map = ENCRYPTED_TEMPS.lock().expect("lock encrypted temps");
    let registration = map
        .entry(enc_path.to_path_buf())
        .or_insert_with(|| RegisteredTemp {
            tmp_path: store.sqlite_path.clone(),
            active: false,
            lock: None,
            temp_lock: None,
            recovery_paths: Vec::new(),
        });
    registration.active = false;
    registration.lock = Some(lock);
    if let Some(path) = recovery_path(error)
        && !registration.recovery_paths.contains(&path)
    {
        registration.recovery_paths.push(path);
    }
}

fn remember_recovery_path(enc_path: &Path, error: &EncryptedStoreError) {
    if let Some(path) = recovery_path(error)
        && let Some(registration) = ENCRYPTED_TEMPS
            .lock()
            .expect("lock encrypted temps")
            .get_mut(enc_path)
        && !registration.recovery_paths.contains(&path)
    {
        registration.recovery_paths.push(path);
    }
}

fn registered_recovery_paths(enc_path: &Path) -> Vec<PathBuf> {
    ENCRYPTED_TEMPS
        .lock()
        .expect("lock encrypted temps")
        .get(enc_path)
        .map(|registration| registration.recovery_paths.clone())
        .unwrap_or_default()
}

fn cleanup_unregistered_recovery(error: EncryptedStoreError) -> EncryptedStoreError {
    if let Some(path) = recovery_path(&error) {
        let _ = remove_if_exists(&path);
    }
    error
}

fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn cleanup_recovery_paths(paths: &[PathBuf]) -> io::Result<()> {
    for path in paths {
        remove_if_exists(path)?;
    }
    Ok(())
}

fn is_registered_temp_or_sidecar(path: &Path, registered: &HashSet<PathBuf>) -> bool {
    if registered.contains(path) {
        return true;
    }
    registered.iter().any(|registered_path| {
        let mut wal = registered_path.as_os_str().to_os_string();
        wal.push("-wal");
        let mut shm = registered_path.as_os_str().to_os_string();
        shm.push("-shm");
        path.as_os_str() == wal || path.as_os_str() == shm
    })
}

/// Removes an initializer and every SQLite sidecar, reporting the first cleanup error.
fn cleanup_initializer(path: &Path) -> io::Result<()> {
    let mut first = None;
    let mut candidates = Vec::with_capacity(3);
    for suffix in ["-shm", "-wal"] {
        let mut sibling = path.as_os_str().to_os_string();
        sibling.push(suffix);
        candidates.push(PathBuf::from(sibling));
    }
    candidates.push(path.to_path_buf());
    for candidate in candidates {
        if let Err(error) = remove_if_exists(&candidate)
            && first.is_none()
        {
            first = Some(io::Error::new(
                error.kind(),
                format!("{}: {error}", candidate.display()),
            ));
        }
    }
    first.map_or(Ok(()), Err)
}

fn cleanup_temp_lock_after_main(path: &Path, temp_lock: Option<&mut DbLock>) {
    if let Some(temp_lock) = temp_lock
        && temp_lock.close().is_ok()
    {
        let _ = remove_if_exists(&lock_path_for(path));
    }
}

fn cleanup_plain_temp(path: &Path) -> io::Result<()> {
    cleanup_plain_temp_with_lock(path, None)
}

fn cleanup_plain_temp_with_lock(path: &Path, mut temp_lock: Option<DbLock>) -> io::Result<()> {
    let siblings_error = remove_wal_siblings(path).err();
    let main_error = remove_if_exists(path).err();
    if main_error.is_none() {
        cleanup_temp_lock_after_main(path, temp_lock.as_mut());
    }
    main_error.or(siblings_error).map_or(Ok(()), Err)
}

/// Atomically transitions `target` with `ciphertext`.
/// Writes to `.symeraseme_write_<id>` in the target directory, syncs, renames, and syncs parent.
/// If rename or directory sync fails, leaves a recovery file at `.symeraseme_recovery_<id>`
/// and returns `(Err, Some(recovery_path))`.
pub fn atomic_transition_with_recovery(
    target: &Path,
    ciphertext: &[u8],
) -> Result<(), EncryptedStoreError> {
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    ensure_parent_dir(parent)?;

    let id = rand::rng().next_u64();
    let write_name = format!(".symeraseme_write_{id:016x}.tmp");
    let write_path = parent.join(write_name);

    let write_result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&write_path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(ciphertext)?;
        file.sync_all()
    })();
    if let Err(error) = write_result {
        let cleanup_error = remove_if_exists(&write_path).err();
        return Err(EncryptedStoreError::ConversionFailed {
            message: cleanup_error.as_ref().map_or_else(
                || format!("write replacement failed: {error}"),
                |cleanup| format!("write replacement failed: {error}; cleanup failed: {cleanup}"),
            ),
            recovery_path: cleanup_error.map(|_| write_path.clone()),
        });
    }

    // Atomically replace target
    let replace = || {
        #[cfg(windows)]
        {
            replace_file(&write_path, target)
        }
        #[cfg(not(windows))]
        {
            fs::rename(&write_path, target)
        }
    };
    if let Err(err) = replace() {
        // Rename failed: write_path remains as recovery
        let rec_name = format!(".symeraseme_recovery_{id:016x}.tmp");
        let rec_path = parent.join(rec_name);
        let recovery_path = match fs::rename(&write_path, &rec_path) {
            Ok(()) => rec_path,
            Err(recovery_err) => {
                return Err(EncryptedStoreError::ConversionFailed {
                    message: format!(
                        "rename failed: {err}; recovery rename failed: {recovery_err}"
                    ),
                    recovery_path: Some(write_path),
                });
            }
        };
        return Err(EncryptedStoreError::ConversionFailed {
            message: format!("rename failed: {err}"),
            recovery_path: Some(recovery_path),
        });
    }

    // Target is now replaced with ciphertext. Directory sync must succeed.
    if let Err(err) = sync_dir(parent) {
        // Directory sync failure: leave recovery copy of the new ciphertext
        let rec_name = format!(".symeraseme_recovery_{id:016x}.tmp");
        let rec_path = parent.join(rec_name);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let recovery_result = (|| -> io::Result<()> {
            let mut recovery = options.open(&rec_path)?;
            recovery.write_all(ciphertext)?;
            recovery.sync_all()
        })();
        if let Err(recovery_error) = recovery_result {
            let cleanup_error = remove_if_exists(&rec_path).err();
            let message = match cleanup_error.as_ref() {
                Some(cleanup) => format!(
                    "directory sync failed: {err}; recovery write failed: {recovery_error}; cleanup failed: {cleanup}"
                ),
                None => format!(
                    "directory sync failed: {err}; recovery write failed: {recovery_error}; recovery removed"
                ),
            };
            return Err(EncryptedStoreError::DirectorySyncFailed {
                path: target.to_path_buf(),
                recovery_path: cleanup_error.map(|_| rec_path),
                source: io::Error::other(message),
            });
        }
        return Err(EncryptedStoreError::DirectorySyncFailed {
            path: target.to_path_buf(),
            recovery_path: Some(rec_path),
            source: err,
        });
    }

    Ok(())
}

/// Atomically replaces an encrypted database with its plaintext contents.
pub fn decrypt_existing(path: &Path) -> Result<(), EncryptedStoreError> {
    let raw = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(EncryptedStoreError::Io(err)),
    };
    if raw.is_empty() || !is_encrypted(&raw) {
        return Ok(());
    }

    let master_key = current_master_key().map_err(|_| EncryptedStoreError::MasterKeyUnavailable)?;
    let plain = decrypt_any(&raw, &master_key)?;
    atomic_transition_with_recovery(path, &plain).map_err(cleanup_unregistered_recovery)
}

/// Encrypts an existing plaintext database in-place to standard Fernet V3.
pub fn encrypt_existing(path: &Path) -> Result<(), EncryptedStoreError> {
    let raw = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(EncryptedStoreError::Io(err)),
    };
    if raw.is_empty() || is_encrypted(&raw) {
        return Ok(());
    }

    let master_key = current_master_key().map_err(|_| EncryptedStoreError::MasterKeyUnavailable)?;
    let ciphertext = encrypt_v3(&raw, &master_key)?;
    atomic_transition_with_recovery(path, &ciphertext).map_err(cleanup_unregistered_recovery)
}

fn validate_temp_dir(path: &Path) -> Result<(), EncryptedStoreError> {
    if path.to_string_lossy().trim().is_empty() {
        return Err(EncryptedStoreError::InvalidPath(
            "temporary directory must not be empty".into(),
        ));
    }
    if !path.is_absolute() {
        return Err(EncryptedStoreError::InvalidPath(
            "temporary directory must be absolute".into(),
        ));
    }
    Ok(())
}

/// Opens the database using the requested at-rest mode.
/// Single entry point for CLI and engine callers.
pub fn open_configured(
    path: impl AsRef<Path>,
    tmp_dir: Option<&Path>,
    encrypt: bool,
) -> Result<Store, EncryptedStoreError> {
    let path = path.as_ref();
    if path.as_os_str().is_empty() {
        return Err(EncryptedStoreError::InvalidPath(
            "database path must not be empty".into(),
        ));
    }
    if let Some(tmp_dir) = tmp_dir {
        validate_temp_dir(tmp_dir)?;
    }

    if encrypt {
        let default_tmp = default_encrypted_temp_dir()?;
        let tmp = tmp_dir.unwrap_or(&default_tmp);
        // Check master key availability before touching anything
        if current_master_key().is_err() {
            return Err(EncryptedStoreError::MasterKeyUnavailable);
        }
        return open_encrypted(path, tmp);
    }

    // Plain production opens hold the sibling lock for their lifetime
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        ensure_parent_dir(parent)?;
    }
    let lock = DbLock::lock(path, 1)?;

    let raw = match fs::read(path) {
        Ok(b) => b,
        Err(err) if err.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(err) => return Err(EncryptedStoreError::Io(err)),
    };

    if !raw.is_empty() && is_encrypted(&raw) {
        decrypt_existing(path)?;
    }

    let mut store = Store::open(path)?;
    store.db_lock = Some(lock);
    Ok(store)
}

/// Opens an encrypted database using a private decrypted temporary file.
pub fn open_encrypted(
    enc_path: impl AsRef<Path>,
    tmp_dir: impl AsRef<Path>,
) -> Result<Store, EncryptedStoreError> {
    let enc_path = enc_path.as_ref().to_path_buf();
    let tmp_dir = tmp_dir.as_ref().to_path_buf();
    validate_temp_dir(&tmp_dir)?;

    if let Some(parent) = enc_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        ensure_parent_dir(parent)?;
    }
    ensure_private_dir(&tmp_dir)?;

    let lock = DbLock::lock(&enc_path, 1)?;
    let master_key = match current_master_key() {
        Ok(k) => k,
        Err(_) => return Err(EncryptedStoreError::MasterKeyUnavailable),
    };

    let raw = match fs::read(&enc_path) {
        Ok(b) => b,
        Err(err) if err.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(err) => return Err(EncryptedStoreError::Io(err)),
    };

    if raw.is_empty() {
        // Initialize SQLite away from canonical path, checkpoint, and publish encrypted
        let id = rand::rng().next_u64();
        let init_path = tmp_dir.join(format!("symeraseme_init_{id:016x}.db"));
        {
            let f = create_initializer(&init_path).map_err(EncryptedStoreError::Io)?;
            drop(f);
        }

        let init_store = match Store::open(&init_path) {
            Ok(store) => store,
            Err(error) => {
                let cleanup = cleanup_initializer(&init_path);
                return Err(match cleanup {
                    Ok(()) => EncryptedStoreError::Sqlite(error),
                    Err(cleanup) => EncryptedStoreError::Io(io::Error::other(format!(
                        "initialize: {error}; cleanup: {cleanup}"
                    ))),
                });
            }
        };
        if let Err(error) = init_store.checkpoint_wal() {
            drop(init_store);
            let cleanup = cleanup_initializer(&init_path);
            return Err(match cleanup {
                Ok(()) => EncryptedStoreError::Checkpoint(error),
                Err(cleanup) => EncryptedStoreError::Io(io::Error::other(format!(
                    "initialize checkpoint: {error}; cleanup: {cleanup}"
                ))),
            });
        }
        drop(init_store);

        let init_bytes = match fs::read(&init_path) {
            Ok(bytes) => bytes,
            Err(error) => {
                let cleanup = cleanup_initializer(&init_path);
                return Err(match cleanup {
                    Ok(()) => EncryptedStoreError::Io(error),
                    Err(cleanup) => EncryptedStoreError::Io(io::Error::other(format!(
                        "read initializer: {error}; cleanup: {cleanup}"
                    ))),
                });
            }
        };
        let ciphertext = match encrypt_v3(&init_bytes, &master_key) {
            Ok(ciphertext) => ciphertext,
            Err(error) => {
                let cleanup = cleanup_initializer(&init_path);
                return Err(match cleanup {
                    Ok(()) => EncryptedStoreError::Encryption(error),
                    Err(cleanup) => EncryptedStoreError::Io(io::Error::other(format!(
                        "encrypt initializer: {error}; cleanup: {cleanup}"
                    ))),
                });
            }
        };
        if let Err(error) = atomic_transition_with_recovery(&enc_path, &ciphertext) {
            let cleanup = cleanup_initializer(&init_path);
            let transition_error = cleanup_unregistered_recovery(error);
            return Err(match cleanup {
                Ok(()) => transition_error,
                Err(cleanup) => EncryptedStoreError::Io(io::Error::other(format!(
                    "publish initializer: {transition_error}; cleanup: {cleanup}"
                ))),
            });
        }
        cleanup_initializer(&init_path)?;
    } else if !is_encrypted(&raw) {
        // Convert existing plain database
        let plain_store = Store::open(&enc_path)?;
        plain_store.checkpoint_wal()?;
        drop(plain_store);

        encrypt_existing(&enc_path)?;
    }

    // Migrate every legacy envelope while the canonical lock is held. This
    // makes the on-disk format V3 before a decrypted temp is exposed.
    let mut ciphertext = fs::read(&enc_path)?;
    if is_encrypted(&ciphertext)
        && (ciphertext.starts_with(super::encryption::V1_HEADER)
            || ciphertext.starts_with(super::encryption::V2_HEADER)
            || super::encryption::is_legacy_go_envelope(&ciphertext))
    {
        let plaintext = if ciphertext.starts_with(super::encryption::V1_HEADER) {
            super::encryption::decrypt_v1(&ciphertext, &master_key)?
        } else if ciphertext.starts_with(super::encryption::V2_HEADER) {
            super::encryption::decrypt_v2(&ciphertext, &master_key)?
        } else {
            super::encryption::decrypt_v3(&ciphertext, &master_key)?
        };
        let migrated = encrypt_v3(&plaintext, &master_key)?;
        atomic_transition_with_recovery(&enc_path, &migrated)
            .map_err(cleanup_unregistered_recovery)?;
        ciphertext = migrated;
    }

    // Read back the confirmed ciphertext and authenticate before touching temp storage.
    let plaintext = decrypt_any(&ciphertext, &master_key)?;

    // Decrypt to private temp file
    let id = rand::rng().next_u64();
    let tmp_path = tmp_dir.join(format!("symeraseme_decrypted_{id:016x}.db"));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut temp = options.open(&tmp_path)?;
    if let Err(error) = temp.write_all(&plaintext).and_then(|_| temp.sync_all()) {
        drop(temp);
        let cleanup = cleanup_plain_temp(&tmp_path);
        return Err(match cleanup {
            Ok(()) => EncryptedStoreError::Io(error),
            Err(cleanup) => EncryptedStoreError::Io(io::Error::other(format!(
                "write decrypted temp: {error}; cleanup: {cleanup}"
            ))),
        });
    }
    drop(temp);

    let temp_lock = match DbLock::lock(&tmp_path, 1) {
        Ok(lock) => lock,
        Err(error) => {
            let cleanup = cleanup_plain_temp(&tmp_path);
            return Err(match cleanup {
                Ok(()) => EncryptedStoreError::Io(io::Error::other(format!(
                    "lock decrypted temp: {error}"
                ))),
                Err(cleanup) => EncryptedStoreError::Io(io::Error::other(format!(
                    "lock decrypted temp: {error}; cleanup: {cleanup}"
                ))),
            });
        }
    };

    let mut store = match Store::open(&tmp_path) {
        Ok(store) => store,
        Err(error) => {
            let cleanup = cleanup_plain_temp_with_lock(&tmp_path, Some(temp_lock));
            return Err(match cleanup {
                Ok(()) => EncryptedStoreError::Sqlite(error),
                Err(cleanup) => EncryptedStoreError::Io(io::Error::other(format!(
                    "open decrypted temp: {error}; cleanup: {cleanup}"
                ))),
            });
        }
    };
    store.set_encrypted_paths(enc_path.clone(), tmp_path.clone());
    store.db_lock = Some(lock);

    register_temp(enc_path, tmp_path, temp_lock);
    Ok(store)
}

fn create_initializer(path: &Path) -> io::Result<std::fs::File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// Closes a store, checkpoints the WAL, re-encrypts the temporary database to
/// its canonical path, cleans up temporary files, and releases the lock.
pub fn close_store(mut store: Store) -> Result<(), EncryptedStoreError> {
    let encrypted_path = store.encrypted_path.clone();
    let result = close_store_inner(&mut store);
    if let Err(ref error) = result
        && let Some(enc_path) = encrypted_path
    {
        retain_failed_close(&enc_path, &mut store, error);
    }
    result
}

fn close_store_inner(store: &mut Store) -> Result<(), EncryptedStoreError> {
    store.checkpoint_wal()?;

    if let Some(enc_path) = store.encrypted_path.clone() {
        let master_key =
            current_master_key().map_err(|_| EncryptedStoreError::MasterKeyUnavailable)?;
        let tmp_path = store.sqlite_path.clone();

        // Close connection by dropping it before reading the plain file
        // Extract connection by swapping or dropping store
        // Connection is closed when dropped
        let conn = std::mem::replace(
            &mut store.connection,
            rusqlite::Connection::open_in_memory()?,
        );
        drop(conn);

        let plain = fs::read(&tmp_path)?;
        let ciphertext = encrypt_v3(&plain, &master_key)?;

        atomic_transition_with_recovery(&enc_path, &ciphertext)?;
        cleanup_recovery_paths(&registered_recovery_paths(&enc_path))?;
        remove_wal_siblings(&enc_path)?;

        // Keep the registration until every cleanup operation succeeds. A
        // failed cleanup is retryable from the still-private plaintext temp.
        cleanup_plain_temp(&tmp_path)?;

        unregister_temp(&enc_path);
    }
    Ok(())
}

/// Finalizes failed-close registrations whose SQLite handle is already closed.
///
/// Active stores cannot be safely closed through this global registry because
/// ownership of their [`Store`] handle is not available here. They therefore
/// remain registered and produce `WouldBlock`; callers must close that `Store`
/// explicitly before retrying this recovery helper.
pub fn finalise_all() -> Result<(), Vec<EncryptedStoreError>> {
    let entries: Vec<PathBuf> = ENCRYPTED_TEMPS
        .lock()
        .expect("lock encrypted temps")
        .keys()
        .cloned()
        .collect();
    let mut errors = Vec::new();

    let mut master_key = None;

    for enc_path in entries {
        let Some((tmp_path, active, recovery_paths)) = ENCRYPTED_TEMPS
            .lock()
            .expect("lock encrypted temps")
            .get(&enc_path)
            .map(|registration| {
                (
                    registration.tmp_path.clone(),
                    registration.active,
                    registration.recovery_paths.clone(),
                )
            })
        else {
            continue;
        };
        if active {
            errors.push(EncryptedStoreError::Io(io::Error::new(
                io::ErrorKind::WouldBlock,
                format!(
                    "active encrypted store not finalized: {}",
                    enc_path.display()
                ),
            )));
            continue;
        }
        let key = match master_key {
            Some(key) => key,
            None => match current_master_key() {
                Ok(key) => {
                    master_key = Some(key);
                    key
                }
                Err(_) => {
                    errors.push(EncryptedStoreError::MasterKeyUnavailable);
                    continue;
                }
            },
        };
        if !tmp_path.exists() {
            errors.push(EncryptedStoreError::Io(io::Error::new(
                io::ErrorKind::NotFound,
                format!("registered encrypted temp missing: {}", tmp_path.display()),
            )));
            continue;
        }
        let checkpoint = Store::open(&tmp_path).and_then(|store| {
            store
                .checkpoint_wal()
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
        });
        if let Err(error) = checkpoint {
            errors.push(EncryptedStoreError::Sqlite(error));
            continue;
        }
        let plain = match fs::read(&tmp_path) {
            Ok(p) => p,
            Err(err) => {
                errors.push(EncryptedStoreError::Io(err));
                continue;
            }
        };
        let ciphertext = match encrypt_v3(&plain, &key) {
            Ok(c) => c,
            Err(err) => {
                errors.push(EncryptedStoreError::Encryption(err));
                continue;
            }
        };
        if let Err(err) = atomic_transition_with_recovery(&enc_path, &ciphertext) {
            remember_recovery_path(&enc_path, &err);
            errors.push(err);
            continue;
        }
        if let Err(error) = cleanup_recovery_paths(&recovery_paths) {
            errors.push(EncryptedStoreError::Io(error));
            continue;
        }
        if let Err(error) = remove_wal_siblings(&enc_path) {
            errors.push(EncryptedStoreError::Io(error));
            continue;
        }
        if let Err(error) = cleanup_plain_temp(&tmp_path) {
            errors.push(EncryptedStoreError::Io(error));
            continue;
        }
        unregister_temp(&enc_path);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
