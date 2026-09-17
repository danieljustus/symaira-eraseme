//! Database file locking for mutual exclusion across processes.
//!
//! Mirrors `internal/eventstore/cleanup.go` and `flock_unix.go` / `flock_windows.go`.
//! Uses an exclusive non-blocking lock on `<path>.lock`.

use fs2::FileExt;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

#[derive(Debug)]
pub enum LockError {
    Acquire { path: PathBuf, source: io::Error },
    Release { path: PathBuf, source: io::Error },
}

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Acquire { path, source } => {
                write!(
                    f,
                    "eventstore: cannot acquire DB lock {}: {source}",
                    path.display()
                )
            }
            Self::Release { path, source } => {
                write!(
                    f,
                    "eventstore: lock release failed for {}: {source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for LockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Acquire { source, .. } | Self::Release { source, .. } => Some(source),
        }
    }
}

/// An exclusive interprocess lock on a sibling `.lock` file.
///
/// Uses `flock` on Unix and `LockFileEx` on Windows (via `fs2`).
/// Released explicitly by [`DbLock::close`] or automatically on [`Drop`].
#[derive(Debug)]
pub struct DbLock {
    path: PathBuf,
    file: Option<File>,
}

impl DbLock {
    /// Acquires an exclusive non-blocking lock at `<db_path>.lock`.
    ///
    /// Acquisition retries up to `retry_max` times with 1-second sleeps.
    /// Callers never proceed with a best-effort or file-existence-only lock.
    pub fn lock(db_path: impl AsRef<Path>, retry_max: usize) -> Result<Self, LockError> {
        Self::lock_with_delay(db_path, retry_max, Duration::from_secs(1))
    }

    /// Acquires an exclusive non-blocking lock at `<db_path>.lock` with a custom retry delay.
    pub fn lock_with_delay(
        db_path: impl AsRef<Path>,
        retry_max: usize,
        delay: Duration,
    ) -> Result<Self, LockError> {
        let db_path = db_path.as_ref();
        let lock_path = lock_path_for(db_path);
        let attempts = retry_max.max(1);
        let mut last_err = None;

        for _ in 0..attempts {
            let mut options = OpenOptions::new();
            options.read(true).write(true).create(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }

            match options.open(&lock_path) {
                Ok(file) => match file.try_lock_exclusive() {
                    Ok(()) => {
                        return Ok(Self {
                            path: lock_path,
                            file: Some(file),
                        });
                    }
                    Err(err) => {
                        last_err = Some(err);
                        thread::sleep(delay);
                    }
                },
                Err(err) => {
                    last_err = Some(err);
                    thread::sleep(delay);
                }
            }
        }

        Err(LockError::Acquire {
            path: lock_path,
            source: last_err.unwrap_or_else(|| io::Error::other("lock attempts exhausted")),
        })
    }

    /// Returns the path to the `.lock` file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Explicitly releases the lock. Safe to call multiple times.
    pub fn close(&mut self) -> Result<(), LockError> {
        if let Some(file) = self.file.take() {
            let _ = file.unlock();
        }
        Ok(())
    }
}

impl Drop for DbLock {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// Derives the lock path for a database: `<db_path>.lock`.
pub fn lock_path_for(db_path: &Path) -> PathBuf {
    let mut lock_name = db_path
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    lock_name.push(".lock");
    db_path.with_file_name(lock_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn lock_acquire_and_release_roundtrip() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let mut lock = DbLock::lock(&db_path, 1).expect("acquire lock");
        assert_eq!(lock.path(), &dir.path().join("test.db.lock"));

        // Second lock attempt must fail immediately
        let attempt = DbLock::lock_with_delay(&db_path, 1, Duration::from_millis(10));
        assert!(attempt.is_err());

        // Release first lock
        lock.close().expect("release lock");

        // Second lock attempt should now succeed
        let lock2 = DbLock::lock(&db_path, 1).expect("acquire second lock");
        drop(lock2);
    }
}
