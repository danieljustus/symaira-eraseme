use symeraseme_core::storage::open;
use tempfile::tempdir;

fn pragma_value<T>(connection: &rusqlite::Connection, name: &str) -> T
where
    T: rusqlite::types::FromSql,
{
    connection
        .pragma_query_value(None, name, |row| row.get(0))
        .expect("read SQLite pragma")
}

fn row_count(connection: &rusqlite::Connection) -> i64 {
    connection
        .query_row("SELECT count(*) FROM items", [], |row| row.get(0))
        .expect("count committed rows")
}

#[test]
fn sqlite_portability_smoke_uses_isolated_file_wal_and_transactions() {
    let tree = tempdir().expect("create isolated SQLite state");
    let database = tree.path().join("nested").join("smoke.sqlite");

    let mut connection = open(&database).expect("open or create SQLite database");
    assert!(database.is_file(), "SQLite file was not created");
    assert_eq!(pragma_value::<i64>(&connection, "busy_timeout"), 5_000);
    assert_eq!(pragma_value::<i64>(&connection, "foreign_keys"), 1);
    assert_eq!(
        pragma_value::<String>(&connection, "journal_mode").to_ascii_lowercase(),
        "wal"
    );

    connection
        .execute(
            "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
            [],
        )
        .expect("create smoke table");

    {
        let transaction = connection
            .transaction()
            .expect("begin rollback transaction");
        transaction
            .execute("INSERT INTO items (id, name) VALUES (1, 'rolled back')", [])
            .expect("write inside rollback transaction");
        transaction.rollback().expect("roll back transaction");
    }
    assert_eq!(row_count(&connection), 0);

    {
        let transaction = connection.transaction().expect("begin commit transaction");
        transaction
            .execute("INSERT INTO items (id, name) VALUES (2, 'committed')", [])
            .expect("write inside commit transaction");
        transaction.commit().expect("commit transaction");
    }
    assert_eq!(row_count(&connection), 1);
}
