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
use std::{fs, path::Path};

/// Opens a file-backed SQLite database with the Go store's connection pragmas.
///
/// SQLite is bundled through the workspace's `rusqlite` dependency so the
/// connection behavior does not depend on a system SQLite installation.
/// Parent creation stays here (not CoreKit's `open`) because CoreKit creates
/// the parent at mode `0o700`, a behavior change out of scope for this pin.
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
        symaira_core_sqlite::Error::Open(err) => err,
        other => rusqlite::Error::ToSqlConversionFailure(Box::new(other)),
    })
}
