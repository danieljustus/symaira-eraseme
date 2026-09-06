//! SQLite snapshots use rusqlite's bundled SQLite and online backup API.

use rusqlite::backup::Backup;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use std::fs::{self, OpenOptions};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SqliteSnapshot {
    pub schema: String,
    pub ordered_results: Vec<String>,
    pub copied_database: PathBuf,
}

impl Drop for SqliteSnapshot {
    fn drop(&mut self) {
        remove_database_artifacts(&self.copied_database);
    }
}

fn remove_database_artifacts(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = fs::remove_file(path.with_extension("sqlite-shm"));
}

fn database_error(context: &str, _error: rusqlite::Error) -> std::io::Error {
    std::io::Error::other(context)
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

/// Reject symlink components in a case-relative path before any SQLite open.
pub fn reject_symlink_components(root: &Path, relative: &Path) -> std::io::Result<()> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        if matches!(
            component,
            Component::CurDir | Component::ParentDir | Component::Prefix(_)
        ) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "SQLite case path is not sandbox-local",
            ));
        }
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "SQLite case path contains a symlink component",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn reject_symlink_database_leaf(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "SQLite database path is a symlink",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Copy the live database, including WAL state, without relying on a host CLI.
fn backup_database(path: &Path) -> std::io::Result<PathBuf> {
    reject_symlink_database_leaf(path)?;
    let destination = unique_copy_path();
    let mut target_file = OpenOptions::new();
    target_file.write(true).create_new(true);
    #[cfg(unix)]
    target_file.mode(0o600);
    {
        let _target = target_file
            .open(&destination)
            .map_err(|_| std::io::Error::other("creating SQLite backup target"))?;
    }
    let result = (|| {
        let source = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| database_error("opening SQLite source", error))?;
        let mut target =
            Connection::open_with_flags(&destination, OpenFlags::SQLITE_OPEN_READ_WRITE)
                .map_err(|error| database_error("opening SQLite backup target", error))?;
        let backup = Backup::new(&source, &mut target)
            .map_err(|error| database_error("starting SQLite online backup", error))?;
        backup
            .run_to_completion(128, Duration::from_millis(10), None)
            .map_err(|error| database_error("running SQLite online backup", error))?;
        Ok::<_, std::io::Error>(())
    })();
    if let Err(error) = result {
        remove_database_artifacts(&destination);
        return Err(error);
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
    let mut statement = statement_or_error(&connection, sql)?;
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

fn statement_or_error<'a>(
    connection: &'a Connection,
    sql: &str,
) -> std::io::Result<rusqlite::Statement<'a>> {
    connection
        .prepare(sql)
        .map_err(|error| database_error("preparing SQLite snapshot query", error))
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
            remove_database_artifacts(&copied_database);
            return Err(error);
        }
    };
    let mut ordered_results = Vec::with_capacity(ordered_queries.len());
    for query in ordered_queries {
        match query_text(&copied_database, query) {
            Ok(result) => ordered_results.push(result),
            Err(error) => {
                remove_database_artifacts(&copied_database);
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
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
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
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(&snapshot.copied_database)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        drop(snapshot);
        drop(connection);
        let _ = fs::remove_file(&database);
        let _ = fs::remove_file(database.with_extension("sqlite-wal"));
        let _ = fs::remove_file(database.with_extension("sqlite-shm"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_database_components() {
        let root = std::env::temp_dir().join(format!("parity-symlink-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("real")).unwrap();
        fs::write(root.join("real/db.sqlite"), b"not sqlite").unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("link")).unwrap();
        let error = reject_symlink_components(&root, Path::new("link/db.sqlite")).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        let _ = fs::remove_dir_all(root);
    }
}
