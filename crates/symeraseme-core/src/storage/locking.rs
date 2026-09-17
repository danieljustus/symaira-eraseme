//! Exclusive locks for canonical event-store paths.

use std::fmt;
use std::fs::{File, OpenOptions, TryLockError};
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
            Self::Acquire { path, source } => write!(
                f,
                "eventstore: cannot acquire DB lock {}: {source}",
                path.display()
            ),
            Self::Release { path, source } => write!(
                f,
                "eventstore: lock release failed for {}: {source}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for LockError {}

#[derive(Debug)]
pub struct DbLock {
    path: PathBuf,
    file: Option<File>,
}

impl DbLock {
    pub fn lock(path: impl AsRef<Path>, attempts: usize) -> Result<Self, LockError> {
        Self::lock_with_delay(path, attempts, Duration::from_secs(1))
    }

    pub fn lock_with_delay(
        path: impl AsRef<Path>,
        attempts: usize,
        delay: Duration,
    ) -> Result<Self, LockError> {
        let lock_path = lock_path_for(path.as_ref());
        let mut last_error = None;
        for _ in 0..attempts.max(1) {
            let mut options = OpenOptions::new();
            options.read(true).write(true).create(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&lock_path) {
                Ok(file) => match file.try_lock() {
                    Ok(()) => {
                        return Ok(Self {
                            path: lock_path,
                            file: Some(file),
                        });
                    }
                    Err(TryLockError::WouldBlock) => {
                        last_error = Some(io::Error::from(io::ErrorKind::WouldBlock));
                        thread::sleep(delay);
                    }
                    Err(TryLockError::Error(error)) => {
                        last_error = Some(error);
                        thread::sleep(delay);
                    }
                },
                Err(error) => {
                    last_error = Some(error);
                    thread::sleep(delay);
                }
            }
        }
        Err(LockError::Acquire {
            path: lock_path,
            source: last_error.unwrap_or_else(|| io::Error::other("lock attempts exhausted")),
        })
    }

    pub fn close(&mut self) -> Result<(), LockError> {
        let Some(file) = self.file.take() else {
            return Ok(());
        };
        if let Err(source) = file.unlock() {
            self.file = Some(file);
            return Err(LockError::Release {
                path: self.path.clone(),
                source,
            });
        }
        Ok(())
    }
}

impl Drop for DbLock {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

pub fn lock_path_for(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(std::ffi::OsStr::to_os_string)
        .unwrap_or_default();
    name.push(".lock");
    path.with_file_name(name)
}
