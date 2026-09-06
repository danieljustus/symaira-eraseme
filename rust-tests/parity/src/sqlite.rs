//! SQLite snapshots use rusqlite's bundled SQLite and online backup API.

use rusqlite::backup::Backup;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SqliteSnapshot {
    pub schema: String,
    pub ordered_results: Vec<String>,
    pub copied_database: PathBuf,
}

impl Drop for SqliteSnapshot {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.copied_database);
        let _ = fs::remove_file(self.copied_database.with_extension("sqlite-wal"));
        let _ = fs::remove_file(self.copied_database.with_extension("sqlite-shm"));
    }
}

fn database_error(context: &str, error: rusqlite::Error) -> std::io::Error {
    std::io::Error::other(format!("{context}: {error}"))
}

fn unique_copy_path() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "symeraseme-parity-db-{}-{stamp}.sqlite",
        std::process::id()
    ))
}

/// Copy the live database, including WAL state, without relying on a host CLI.
fn backup_database(path: &Path) -> std::io::Result<PathBuf> {
    let destination = unique_copy_path();
    let source = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| database_error("opening SQLite source", error))?;
    let mut target = Connection::open(&destination)
        .map_err(|error| database_error("opening SQLite backup target", error))?;
    let backup = Backup::new(&source, &mut target)
        .map_err(|error| database_error("starting SQLite online backup", error))?;
    if let Err(error) = backup.run_to_completion(128, Duration::from_millis(10), None) {
        let _ = fs::remove_file(&destination);
        return Err(database_error("running SQLite online backup", error));
    }
    Ok(destination)
}

fn render_value(value: ValueRef<'_>) -> String {
    match value {
        ValueRef::Null => String::new(),
        ValueRef::Integer(value) => value.to_string(),
        ValueRef::Real(value) => value.to_string(),
        ValueRef::Text(value) => String::from_utf8_lossy(value).into_owned(),
        ValueRef::Blob(value) => value.iter().map(|byte| format!("{byte:02x}")).collect(),
    }
}

fn query_text(database: &Path, sql: &str) -> std::io::Result<String> {
    let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| database_error("opening SQLite snapshot", error))?;
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| database_error("preparing SQLite snapshot query", error))?;
    if statement.column_count() == 0 {
        statement
            .execute([])
            .map_err(|error| database_error("executing SQLite snapshot statement", error))?;
        return Ok(String::new());
    }
    let column_count = statement.column_count();
    let mut rows = statement
        .query([])
        .map_err(|error| database_error("running SQLite snapshot query", error))?;
    let mut output = String::new();
    while let Some(row) = rows
        .next()
        .map_err(|error| database_error("reading SQLite snapshot row", error))?
    {
        for column in 0..column_count {
            if column > 0 {
                output.push('\t');
            }
            let value = row
                .get_ref(column)
                .map_err(|error| database_error("reading SQLite snapshot value", error))?;
            output.push_str(&render_value(value));
        }
        output.push('\n');
    }
    Ok(output)
}

pub fn snapshot_database(
    database: &Path,
    ordered_queries: &[String],
) -> std::io::Result<SqliteSnapshot> {
    let copied_database = backup_database(database)?;
    let schema = match query_text(
        &copied_database,
        "SELECT type, name, COALESCE(sql, '') FROM sqlite_master WHERE sql IS NOT NULL ORDER BY type, name;",
    ) {
        Ok(schema) => schema,
        Err(error) => {
            let _ = fs::remove_file(&copied_database);
            return Err(error);
        }
    };
    let mut ordered_results = Vec::with_capacity(ordered_queries.len());
    for query in ordered_queries {
        match query_text(&copied_database, query) {
            Ok(result) => ordered_results.push(result),
            Err(error) => {
                let _ = fs::remove_file(&copied_database);
                return Err(error);
            }
        }
    }
    Ok(SqliteSnapshot {
        schema,
        ordered_results,
        copied_database,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_schema_and_ordered_query_from_a_wal_aware_backup() {
        let database =
            std::env::temp_dir().join(format!("parity-test-{}.sqlite", std::process::id()));
        let _ = fs::remove_file(&database);
        let _ = fs::remove_file(database.with_extension("sqlite-wal"));
        let _ = fs::remove_file(database.with_extension("sqlite-shm"));
        let connection = Connection::open(&database).unwrap();
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .unwrap();
        connection
            .execute_batch(
                "CREATE TABLE items (id INTEGER, name TEXT); INSERT INTO items VALUES (2, 'b'), (1, 'a');",
            )
            .unwrap();
        let snapshot = snapshot_database(
            &database,
            &["SELECT id, name FROM items ORDER BY id;".into()],
        )
        .unwrap();
        assert!(snapshot.schema.contains("items"));
        assert_eq!(snapshot.ordered_results[0], "1\ta\n2\tb\n");
        assert_ne!(snapshot.copied_database, database);
        drop(snapshot);
        drop(connection);
        let _ = fs::remove_file(&database);
        let _ = fs::remove_file(database.with_extension("sqlite-wal"));
        let _ = fs::remove_file(database.with_extension("sqlite-shm"));
    }
}
