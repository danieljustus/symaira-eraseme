//! High-water-mark stores. The staging store is what makes reply persistence
//! happen before a durable HWM advance: a failed insert leaves the persisted
//! mark untouched, so a retry can ingest the same batch again.

use std::collections::HashMap;
use std::sync::Mutex;

/// Persists the last processed UID per host/folder together with the
/// UIDVALIDITY it was observed under. A UIDVALIDITY change forces a cold start.
pub trait HwmStore: Send + Sync {
    fn get(&self, host: &str, folder: &str) -> Result<(Option<u32>, Option<u32>), String>;
    fn set(&self, host: &str, folder: &str, uid_validity: u32, last_uid: u32)
    -> Result<(), String>;
}

fn key(host: &str, folder: &str) -> String {
    format!("{host}\u{0}{folder}")
}

#[derive(Default)]
pub struct MemoryHwmStore {
    records: Mutex<HashMap<String, (u32, u32)>>,
}

impl MemoryHwmStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl HwmStore for MemoryHwmStore {
    fn get(&self, host: &str, folder: &str) -> Result<(Option<u32>, Option<u32>), String> {
        let records = self.records.lock().expect("hwm store lock");
        Ok(match records.get(&key(host, folder)) {
            Some((validity, last)) => (Some(*validity), Some(*last)),
            None => (None, None),
        })
    }

    fn set(
        &self,
        host: &str,
        folder: &str,
        uid_validity: u32,
        last_uid: u32,
    ) -> Result<(), String> {
        let mut records = self.records.lock().expect("hwm store lock");
        records.insert(key(host, folder), (uid_validity, last_uid));
        Ok(())
    }
}

/// Keeps HWM updates in memory until [`StagingHwmStore::commit`].
pub struct StagingHwmStore<'a> {
    underlying: Option<&'a dyn HwmStore>,
    staged: Mutex<Vec<(String, String, u32, u32)>>,
}

impl<'a> StagingHwmStore<'a> {
    pub fn new(underlying: Option<&'a dyn HwmStore>) -> Self {
        Self {
            underlying,
            staged: Mutex::new(Vec::new()),
        }
    }

    /// Writes every staged record through, in the order it was staged. The
    /// first failure stops the commit and keeps the remaining records staged.
    pub fn commit(&self) -> Result<(), String> {
        let mut staged = self.staged.lock().expect("hwm store lock");
        let Some(underlying) = self.underlying else {
            staged.clear();
            return Ok(());
        };
        for (host, folder, uid_validity, last_uid) in staged.iter() {
            underlying
                .set(host, folder, *uid_validity, *last_uid)
                .map_err(|error| {
                    format!("email: commit high-water mark ({host}/{folder}): {error}")
                })?;
        }
        staged.clear();
        Ok(())
    }
}

impl HwmStore for StagingHwmStore<'_> {
    fn get(&self, host: &str, folder: &str) -> Result<(Option<u32>, Option<u32>), String> {
        {
            let staged = self.staged.lock().expect("hwm store lock");
            for (staged_host, staged_folder, uid_validity, last_uid) in staged.iter().rev() {
                if staged_host == host && staged_folder == folder {
                    return Ok((Some(*uid_validity), Some(*last_uid)));
                }
            }
        }
        match self.underlying {
            Some(underlying) => underlying.get(host, folder),
            None => Ok((None, None)),
        }
    }

    fn set(
        &self,
        host: &str,
        folder: &str,
        uid_validity: u32,
        last_uid: u32,
    ) -> Result<(), String> {
        self.staged.lock().expect("hwm store lock").push((
            host.to_string(),
            folder.to_string(),
            uid_validity,
            last_uid,
        ));
        Ok(())
    }
}
