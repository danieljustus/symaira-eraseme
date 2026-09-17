//! Encrypted at-rest event-store lifecycle, safe temp-file handling, and scavenging.
//!
//! Mirrors `internal/eventstore/encrypt.go` and `cleanup.go`.

use super::encryption::{EncryptionError, decrypt_any, encrypt_v3, is_encrypted};
use super::locking::{DbLock, LockError};
use super::store::Store;
use rand::RngCore;
use rusqlite::Connection;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, RwLock};
use std::time::{Duration, SystemTime};

/// Cutoff age for stale decrypted temporary files (300 seconds).
pub const STALE_SCAVENGE_AGE: Duration = Duration::from_secs(300);

/// Master key provider returning the 32-byte identity master key on demand.
pub type MasterKeyProvider = Arc<dyn Fn() -> Result<[u8; 32], EncryptionError> + Send + Sync>;

static MASTER_KEY_PROVIDER: RwLock<Option<MasterKeyProvider>> = RwLock::new(None);
static DIRECT_MASTER_KEY: RwLock<Option<[u8; 32]>> = RwLock::new(None);

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
    let dir = dir.as_ref();
    let file = File::open(dir)?;
    file.sync_all()
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

    let now = SystemTime::now();
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();
        if !is_stale_temp_name(&name_str) {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if metadata.is_dir() {
            continue;
        }

        let mtime = metadata.modified().unwrap_or(now);
        if let Ok(age) = now.duration_since(mtime)
            && age > STALE_SCAVENGE_AGE
        {
            let path = entry.path();
            let _ = fs::remove_file(&path);
            let _ = remove_wal_siblings(&path);
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct RegisteredTemp {
    tmp_path: PathBuf,
    #[allow(dead_code)]
    recovery_path: Option<PathBuf>,
}

static ENCRYPTED_TEMPS: LazyLock<Mutex<HashMap<PathBuf, RegisteredTemp>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn register_temp(enc_path: PathBuf, tmp_path: PathBuf) {
    let mut map = ENCRYPTED_TEMPS.lock().expect("lock encrypted temps");
    map.insert(
        enc_path,
        RegisteredTemp {
            tmp_path,
            recovery_path: None,
        },
    );
}

fn unregister_temp(enc_path: &Path) -> Option<RegisteredTemp> {
    let mut map = ENCRYPTED_TEMPS.lock().expect("lock encrypted temps");
    map.remove(enc_path)
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
        .ok_or_else(|| EncryptedStoreError::InvalidPath("target has no parent".into()))?;
    ensure_private_dir(parent)?;

    let id = rand::rng().next_u64();
    let write_name = format!(".symeraseme_write_{id:016x}.tmp");
    let write_path = parent.join(write_name);

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
    file.sync_all()?;
    drop(file);

    // Atomically replace target
    if let Err(err) = fs::rename(&write_path, target) {
        // Rename failed: write_path remains as recovery
        let rec_name = format!(".symeraseme_recovery_{id:016x}.tmp");
        let rec_path = parent.join(rec_name);
        let _ = fs::rename(&write_path, &rec_path);
        return Err(EncryptedStoreError::ConversionFailed {
            message: format!("rename failed: {err}"),
            recovery_path: Some(rec_path),
        });
    }

    // Target is now replaced with ciphertext. Directory sync must succeed.
    if let Err(err) = sync_dir(parent) {
        // Directory sync failure: leave recovery copy of the new ciphertext
        let rec_name = format!(".symeraseme_recovery_{id:016x}.tmp");
        let rec_path = parent.join(rec_name);
        let _ = fs::write(&rec_path, ciphertext);
        ensure_private_file(&rec_path, 0o600)?;
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
    atomic_transition_with_recovery(path, &plain)
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
    atomic_transition_with_recovery(path, &ciphertext)
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

    let default_tmp = std::env::temp_dir().join("symeraseme-db");
    let tmp = tmp_dir.unwrap_or(&default_tmp);

    if encrypt {
        // Check master key availability before touching anything
        if current_master_key().is_err() {
            return Err(EncryptedStoreError::MasterKeyUnavailable);
        }
        return open_encrypted(path, tmp);
    }

    // Plain production opens hold the sibling lock for their lifetime
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
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

    if let Some(parent) = enc_path.parent() {
        ensure_private_dir(parent)?;
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
            let mut opts = OpenOptions::new();
            opts.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            let f = opts.open(&init_path)?;
            drop(f);
        }

        let init_store = Store::open(&init_path)?;
        init_store.checkpoint_wal()?;
        drop(init_store);

        let init_bytes = fs::read(&init_path)?;
        let ciphertext = encrypt_v3(&init_bytes, &master_key)?;
        atomic_transition_with_recovery(&enc_path, &ciphertext)?;

        let _ = fs::remove_file(&init_path);
        let _ = remove_wal_siblings(&init_path);
    } else if !is_encrypted(&raw) {
        // Convert existing plain database
        let plain_store = Store::open(&enc_path)?;
        plain_store.checkpoint_wal()?;
        drop(plain_store);

        encrypt_existing(&enc_path)?;
    }

    // Read back the confirmed ciphertext
    let ciphertext = fs::read(&enc_path)?;
    let plaintext = decrypt_any(&ciphertext, &master_key)?;

    // Decrypt to private temp file
    let id = rand::rng().next_u64();
    let tmp_path = tmp_dir.join(format!("symeraseme_decrypted_{id:016x}.db"));
    fs::write(&tmp_path, plaintext)?;
    ensure_private_file(&tmp_path, 0o600)?;

    let mut store = Store::open(&tmp_path)?;
    store.encrypted_path = Some(enc_path.clone());
    store.db_lock = Some(lock);

    register_temp(enc_path, tmp_path);
    Ok(store)
}

/// Closes a store, checkpoints the WAL, re-encrypts the temporary database to
/// its canonical path, cleans up temporary files, and releases the lock.
pub fn close_store(mut store: Store) -> Result<(), EncryptedStoreError> {
    store.checkpoint_wal()?;

    if let Some(enc_path) = store.encrypted_path.take() {
        let master_key =
            current_master_key().map_err(|_| EncryptedStoreError::MasterKeyUnavailable)?;
        let tmp_path = store.path().to_path_buf();

        // Close connection by dropping it before reading the plain file
        let db_lock = store.db_lock.take();
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
        let _ = remove_wal_siblings(&enc_path);

        // Cleanup temporary plain DB and WAL siblings
        let _ = fs::remove_file(&tmp_path);
        let _ = remove_wal_siblings(&tmp_path);

        unregister_temp(&enc_path);
        drop(db_lock);
    } else {
        drop(store.db_lock.take());
    }
    Ok(())
}

/// Finalizes every still-registered encrypted store.
pub fn finalise_all() -> Result<(), Vec<EncryptedStoreError>> {
    let mut map = ENCRYPTED_TEMPS.lock().expect("lock encrypted temps");
    let entries: Vec<(PathBuf, RegisteredTemp)> = map.drain().collect();
    let mut errors = Vec::new();

    let master_key = match current_master_key() {
        Ok(k) => k,
        Err(_) => {
            errors.push(EncryptedStoreError::MasterKeyUnavailable);
            return Err(errors);
        }
    };

    for (enc_path, reg) in entries {
        if !reg.tmp_path.exists() {
            continue;
        }
        let plain = match fs::read(&reg.tmp_path) {
            Ok(p) => p,
            Err(err) => {
                errors.push(EncryptedStoreError::Io(err));
                continue;
            }
        };
        let ciphertext = match encrypt_v3(&plain, &master_key) {
            Ok(c) => c,
            Err(err) => {
                errors.push(EncryptedStoreError::Encryption(err));
                continue;
            }
        };
        if let Err(err) = atomic_transition_with_recovery(&enc_path, &ciphertext) {
            errors.push(err);
            continue;
        }
        let _ = remove_wal_siblings(&enc_path);
        let _ = fs::remove_file(&reg.tmp_path);
        let _ = remove_wal_siblings(&reg.tmp_path);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
