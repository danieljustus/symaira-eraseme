//! The production SQLite connection and event-store seams.

pub mod encryption;
pub mod repository;
pub mod store;
pub mod types;

pub use repository::{ListRemovalRequestsOptions, Repository};
pub use store::{SCHEMA_VERSION, Store};
pub use types::{
    Campaign, EventRecord, EventType, RemovalRequest, RemovalRequestRow, Source, TickCandidate,
};

use rusqlite::{Connection, Result};
use std::{fs, path::Path};

/// Opens a file-backed SQLite database with the Go store's connection pragmas.
///
/// SQLite is bundled through the workspace's `rusqlite` dependency so the
/// connection behavior does not depend on a system SQLite installation.
pub fn open(path: impl AsRef<Path>) -> Result<Connection> {
    let path = path.as_ref();
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    }

    symaira_core_sqlite::open_with_existing_parent(path).map_err(|error| match error {
        symaira_core_sqlite::Error::Open(source) => source,
        other => rusqlite::Error::ToSqlConversionFailure(Box::new(other)),
    })
}
