//! The production SQLite connection and event-store seams.

pub mod encryption;
pub mod projection;
pub mod repository;
pub mod store;
pub mod types;

pub use projection::{
    ProjectionError, ProjectionResult, ProjectionState, append_and_project_tx, fold_events,
};
pub use repository::{ListRemovalRequestsOptions, Repository};
pub use store::{SCHEMA_VERSION, Store};
pub use types::{
    Campaign, EventRecord, EventType, RemovalRequest, RemovalRequestRow, Source, TickCandidate,
};

use rusqlite::{Connection, Result};
use std::{fs, path::Path, time::Duration};

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

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

    let connection = Connection::open(path)?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    Ok(connection)
}
