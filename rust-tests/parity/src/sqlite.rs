//! SQLite snapshots use a copied database and deterministic, ordered queries.

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

/// Verify the declared host capability before any snapshot is attempted.
///
/// The harness deliberately uses the platform SQLite CLI instead of linking a
/// database library. Callers therefore get a hard, actionable error rather
/// than a silently skipped or vacuous database comparison.
pub fn require_sqlite_cli() -> std::io::Result<()> {
    let output = Command::new("sqlite3")
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

fn copy_path(path: &Path) -> std::io::Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let destination = std::env::temp_dir().join(format!(
        "symeraseme-parity-db-{}-{stamp}.sqlite",
        std::process::id()
    ));
    fs::copy(path, &destination)?;
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
            "sqlite3 failed with {}",
            output.status
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn snapshot_database(
    database: &Path,
    ordered_queries: &[String],
) -> std::io::Result<SqliteSnapshot> {
    require_sqlite_cli()?;
    let copied_database = copy_path(database)?;
    let schema = sqlite(
        &copied_database,
        "SELECT type || char(9) || name || char(9) || COALESCE(sql, '') FROM sqlite_master WHERE sql IS NOT NULL ORDER BY type, name;",
    )?;
    let mut ordered_results = Vec::with_capacity(ordered_queries.len());
    for query in ordered_queries {
        ordered_results.push(sqlite(&copied_database, query)?);
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
    fn snapshots_schema_and_ordered_query_from_a_copy() {
        let database =
            std::env::temp_dir().join(format!("parity-test-{}.sqlite", std::process::id()));
        let _ = fs::remove_file(&database);
        let status = Command::new("sqlite3").arg(&database).arg("CREATE TABLE items (id INTEGER, name TEXT); INSERT INTO items VALUES (2, 'b'), (1, 'a');").status().unwrap();
        assert!(status.success());
        let snapshot = snapshot_database(
            &database,
            &["SELECT id, name FROM items ORDER BY id;".into()],
        )
        .unwrap();
        assert!(snapshot.schema.contains("items"));
        assert_eq!(snapshot.ordered_results[0], "1\ta\n2\tb\n");
        assert_ne!(snapshot.copied_database, database);
        let _ = fs::remove_file(database);
        let _ = fs::remove_file(snapshot.copied_database);
    }
}
