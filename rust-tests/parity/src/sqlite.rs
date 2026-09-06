//! SQLite snapshots use the SQLite online backup API through the CLI.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

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

fn require_sqlite_binary(binary: &str) -> std::io::Result<()> {
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("parity harness requires the sqlite3 CLI: {error}"),
            )
        })?;
    if !output.status.success() || output.stdout.is_empty() {
        return Err(std::io::Error::other(
            "parity harness requires a working sqlite3 CLI",
        ));
    }
    Ok(())
}

/// Verify the declared host capability before any snapshot is attempted.
pub fn require_sqlite_cli() -> std::io::Result<()> {
    require_sqlite_binary("sqlite3")
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

fn sqlite_literal(path: &Path) -> String {
    path.to_string_lossy().replace('\'', "''")
}

fn backup_database(path: &Path) -> std::io::Result<PathBuf> {
    let destination = unique_copy_path();
    let command = format!(".backup '{}'", sqlite_literal(&destination));
    let output = Command::new("sqlite3")
        .args(["-batch", "-bail"])
        .arg(path)
        .arg(command)
        .output()?;
    if !output.status.success() {
        let _ = fs::remove_file(&destination);
        return Err(std::io::Error::other(format!(
            "sqlite3 online backup failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(destination)
}

fn sqlite(database: &Path, sql: &str) -> std::io::Result<String> {
    let output = Command::new("sqlite3")
        .args(["-batch", "-noheader", "-separator", "\t"])
        .arg(database)
        .arg(sql)
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "sqlite3 failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn snapshot_database(
    database: &Path,
    ordered_queries: &[String],
) -> std::io::Result<SqliteSnapshot> {
    require_sqlite_cli()?;
    let copied_database = backup_database(database)?;
    let schema = match sqlite(
        &copied_database,
        "SELECT type || char(9) || name || char(9) || COALESCE(sql, '') FROM sqlite_master WHERE sql IS NOT NULL ORDER BY type, name;",
    ) {
        Ok(schema) => schema,
        Err(error) => {
            let _ = fs::remove_file(&copied_database);
            return Err(error);
        }
    };
    let mut ordered_results = Vec::with_capacity(ordered_queries.len());
    for query in ordered_queries {
        match sqlite(&copied_database, query) {
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
    fn missing_sqlite_capability_is_an_explicit_error() {
        let error =
            require_sqlite_binary("parity-test-missing-sqlite3").expect_err("binary is absent");
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert!(error.to_string().contains("requires the sqlite3 CLI"));
    }

    #[test]
    fn snapshots_schema_and_ordered_query_from_a_wal_aware_backup() {
        require_sqlite_cli().expect("sqlite3 is a declared test capability");
        let database =
            std::env::temp_dir().join(format!("parity-test-{}.sqlite", std::process::id()));
        let _ = fs::remove_file(&database);
        let _ = fs::remove_file(database.with_extension("sqlite-wal"));
        let _ = fs::remove_file(database.with_extension("sqlite-shm"));
        let status = Command::new("sqlite3")
            .arg(&database)
            .arg("PRAGMA journal_mode=WAL; CREATE TABLE items (id INTEGER, name TEXT); INSERT INTO items VALUES (2, 'b'), (1, 'a');")
            .status()
            .unwrap();
        assert!(status.success());
        let snapshot = snapshot_database(
            &database,
            &["SELECT id, name FROM items ORDER BY id;".into()],
        )
        .unwrap();
        assert!(snapshot.schema.contains("items"));
        assert_eq!(snapshot.ordered_results[0], "1\ta\n2\tb\n");
        assert_ne!(snapshot.copied_database, database);
        drop(snapshot);
        let _ = fs::remove_file(&database);
        let _ = fs::remove_file(database.with_extension("sqlite-wal"));
        let _ = fs::remove_file(database.with_extension("sqlite-shm"));
    }
}
