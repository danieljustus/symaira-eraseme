use rusqlite::{Connection, ErrorCode, OpenFlags};
use std::fs;
use std::sync::{Arc, Barrier, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::Duration;
use symeraseme_core::storage::{Store, open};
use tempfile::tempdir;

const GOLDEN_DATABASE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/event-store/golden-campaign.db"
);

#[derive(Default)]
struct BusyState {
    calls: usize,
    release: bool,
}

struct BusyGate {
    state: Mutex<BusyState>,
    wake: Condvar,
}

static BUSY_GATE: OnceLock<BusyGate> = OnceLock::new();

fn busy_gate() -> &'static BusyGate {
    BUSY_GATE.get_or_init(|| BusyGate {
        state: Mutex::new(BusyState::default()),
        wake: Condvar::new(),
    })
}

fn reset_busy_gate() {
    let mut state = busy_gate().state.lock().expect("lock busy-handler state");
    *state = BusyState::default();
}

fn blocking_busy_handler(_count: i32) -> bool {
    let gate = busy_gate();
    let mut state = gate.state.lock().expect("lock busy-handler state");
    state.calls += 1;
    gate.wake.notify_all();
    while !state.release {
        state = gate
            .wake
            .wait(state)
            .expect("wait for busy-handler release");
    }
    true
}

fn busy_callback_seen() -> bool {
    let gate = busy_gate();
    let state = gate.state.lock().expect("lock busy-handler state");
    let (state, _) = gate
        .wake
        .wait_timeout_while(state, Duration::from_secs(2), |state| state.calls == 0)
        .expect("wait for busy callback");
    state.calls > 0
}

fn release_busy_callback() {
    let gate = busy_gate();
    let mut state = gate.state.lock().expect("lock busy-handler state");
    state.release = true;
    gate.wake.notify_all();
}

fn is_contention(error: &rusqlite::Error) -> bool {
    matches!(
        error.sqlite_error_code(),
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    )
}

fn copy_golden_database(path: &std::path::Path) {
    fs::copy(GOLDEN_DATABASE, path).expect("copy Go-produced golden database");
}

fn create_partial_initialization(path: &std::path::Path) {
    let connection = Connection::open(path).expect("create interrupted-init fixture");
    connection
        .execute_batch(
            "CREATE TABLE campaigns (
                id TEXT PRIMARY KEY,
                created_at TIMESTAMP NOT NULL DEFAULT (datetime('now')),
                kind TEXT NOT NULL DEFAULT 'initial',
                notes TEXT
            );
            INSERT INTO campaigns (id, kind, notes)
                VALUES ('interrupted-campaign', 'initial', 'preserve me');",
        )
        .expect("write the prefix of Go initialization");
}

#[test]
fn wal_reader_and_writer_progress_on_the_same_file() {
    let tree = tempdir().expect("create isolated database directory");
    let database = tree.path().join("concurrent.db");
    let reader = open(&database).expect("open reader connection");
    let writer = open(&database).expect("open writer connection");

    writer
        .execute(
            "CREATE TABLE values_table (id INTEGER PRIMARY KEY, value TEXT NOT NULL)",
            [],
        )
        .expect("create contention table");

    reader
        .execute_batch("BEGIN DEFERRED")
        .expect("begin reader snapshot");
    let before: i64 = reader
        .query_row("SELECT count(*) FROM values_table", [], |row| row.get(0))
        .expect("read initial snapshot");
    assert_eq!(before, 0);

    let ready = Arc::new(Barrier::new(2));
    let writer_ready = Arc::clone(&ready);
    let writer_thread = thread::spawn(move || -> rusqlite::Result<()> {
        writer_ready.wait();
        writer.execute(
            "INSERT INTO values_table (id, value) VALUES (1, 'written while read snapshot is open')",
            [],
        )?;
        Ok(())
    });

    ready.wait();
    writer_thread
        .join()
        .expect("writer thread did not panic")
        .expect("WAL writer committed while reader held a snapshot");

    let during: i64 = reader
        .query_row("SELECT count(*) FROM values_table", [], |row| row.get(0))
        .expect("read stable snapshot");
    assert_eq!(during, 0, "reader snapshot changed before commit");

    reader
        .execute_batch("COMMIT")
        .expect("commit reader snapshot");
    let after: i64 = reader
        .query_row("SELECT count(*) FROM values_table", [], |row| row.get(0))
        .expect("read committed writer row");
    assert_eq!(after, 1);
}

#[test]
fn busy_writer_waits_for_a_real_lock_release_then_commits() {
    reset_busy_gate();
    let tree = tempdir().expect("create isolated database directory");
    let database = tree.path().join("busy-success.db");
    let holder = open(&database).expect("open lock holder");
    holder
        .execute(
            "CREATE TABLE values_table (id INTEGER PRIMARY KEY, value TEXT NOT NULL)",
            [],
        )
        .expect("create contention table");
    let waiter = open(&database).expect("open waiting writer");

    holder
        .execute_batch("BEGIN IMMEDIATE")
        .expect("acquire writer lock");

    let waiter_thread = thread::spawn(move || -> Result<(), String> {
        waiter
            .busy_handler(Some(blocking_busy_handler))
            .map_err(|error| error.to_string())?;
        waiter
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|error| error.to_string())?;
        waiter
            .execute(
                "INSERT INTO values_table (id, value) VALUES (1, 'released')",
                [],
            )
            .map_err(|error| error.to_string())?;
        waiter
            .execute_batch("COMMIT")
            .map_err(|error| error.to_string())
    });

    let callback_seen = busy_callback_seen();
    holder
        .execute_batch("ROLLBACK")
        .expect("rollback lock holder");
    release_busy_callback();

    assert!(callback_seen, "waiter never observed the held SQLite lock");
    waiter_thread
        .join()
        .expect("waiting writer thread did not panic")
        .expect("waiting writer failed after lock release");

    let count: i64 = holder
        .query_row("SELECT count(*) FROM values_table", [], |row| row.get(0))
        .expect("read committed waiter row");
    assert_eq!(count, 1);
}

#[test]
fn busy_timeout_is_distinct_from_rollback_and_retry_succeeds() {
    let tree = tempdir().expect("create isolated database directory");
    let database = tree.path().join("busy-timeout.db");
    let holder = open(&database).expect("open lock holder");
    holder
        .execute(
            "CREATE TABLE values_table (id INTEGER PRIMARY KEY, value TEXT NOT NULL)",
            [],
        )
        .expect("create contention table");
    let waiter = open(&database).expect("open timeout writer");

    holder
        .execute_batch("BEGIN IMMEDIATE")
        .expect("acquire writer lock");
    holder
        .execute(
            "INSERT INTO values_table (id, value) VALUES (1, 'rolled back')",
            [],
        )
        .expect("write uncommitted holder row");

    waiter
        .busy_timeout(Duration::from_millis(1))
        .expect("set deterministic short busy timeout");
    let timeout = waiter
        .execute_batch("BEGIN IMMEDIATE")
        .expect_err("locked writer unexpectedly succeeded");
    assert!(
        is_contention(&timeout),
        "expected SQLITE_BUSY/LOCKED timeout, got {timeout:?}"
    );

    holder
        .execute_batch("ROLLBACK")
        .expect("rollback holder transaction");
    let after_rollback: i64 = waiter
        .query_row("SELECT count(*) FROM values_table", [], |row| row.get(0))
        .expect("read after holder rollback");
    assert_eq!(after_rollback, 0);

    waiter
        .execute_batch(
            "BEGIN IMMEDIATE;
             INSERT INTO values_table (id, value) VALUES (2, 'retry committed');
             COMMIT;",
        )
        .expect("retry writer after rollback");
    let committed: i64 = waiter
        .query_row("SELECT count(*) FROM values_table", [], |row| row.get(0))
        .expect("read retry row");
    assert_eq!(committed, 1);
}

#[test]
fn interrupted_initialization_resumes_from_a_real_go_prefix() {
    let tree = tempdir().expect("create isolated database directory");
    let database = tree.path().join("interrupted-init.db");
    create_partial_initialization(&database);

    let store = Store::open(&database).expect("resume interrupted initialization");
    assert_eq!(
        store.user_version().expect("read resumed schema version"),
        2
    );
    let campaign: String = store
        .connection()
        .query_row(
            "SELECT notes FROM campaigns WHERE id = 'interrupted-campaign'",
            [],
            |row| row.get(0),
        )
        .expect("read data from initialization prefix");
    assert_eq!(campaign, "preserve me");
    let imap_state: i64 = store
        .connection()
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'imap_state'",
            [],
            |row| row.get(0),
        )
        .expect("check resumed v2 table");
    assert_eq!(imap_state, 1);
}

#[test]
fn interrupted_migration_with_a_created_v2_table_is_retryable() {
    let tree = tempdir().expect("create isolated database directory");
    let database = tree.path().join("interrupted-migration.db");
    copy_golden_database(&database);

    let connection = Connection::open(&database).expect("open Go-produced migration fixture");
    connection
        .pragma_update(None, "user_version", 1_i64)
        .expect("rewind fixture version before simulated interruption");
    drop(connection);

    let store = Store::open(&database).expect("resume interrupted v2 version update");
    assert_eq!(store.user_version().expect("read migrated version"), 2);
    let imap_rows: i64 = store
        .connection()
        .query_row("SELECT count(*) FROM imap_state", [], |row| row.get(0))
        .expect("read pre-existing v2 table");
    assert_eq!(imap_rows, 0);
}

#[test]
fn locked_migration_times_out_without_advancing_version_then_retries_after_rollback() {
    let tree = tempdir().expect("create isolated database directory");
    let database = tree.path().join("locked-migration.db");
    let store = Store::open(&database).expect("create migration database");
    store
        .connection()
        .execute_batch("DROP TABLE imap_state; PRAGMA user_version = 1")
        .expect("prepare v1 migration state");

    let holder = open(&database).expect("open migration lock holder");
    holder
        .execute_batch("BEGIN IMMEDIATE")
        .expect("hold migration writer lock");
    store
        .connection()
        .busy_timeout(Duration::from_millis(1))
        .expect("set deterministic migration timeout");

    let timeout = store
        .init_schema()
        .expect_err("locked migration unexpectedly succeeded");
    assert!(
        is_contention(&timeout),
        "expected migration SQLITE_BUSY/LOCKED timeout, got {timeout:?}"
    );
    assert_eq!(store.user_version().expect("read version after timeout"), 1);
    let table_count: i64 = store
        .connection()
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'imap_state'",
            [],
            |row| row.get(0),
        )
        .expect("check migration rollback state");
    assert_eq!(table_count, 0);

    holder
        .execute_batch("ROLLBACK")
        .expect("rollback migration lock holder");
    store.init_schema().expect("retry migration after rollback");
    assert_eq!(store.user_version().expect("read retried version"), 2);
}

#[test]
fn corrupt_database_is_rejected_without_rewriting_the_file() {
    let tree = tempdir().expect("create isolated database directory");
    let database = tree.path().join("corrupt.db");
    let original = b"not a SQLite database";
    fs::write(&database, original).expect("write corrupt fixture");

    let error = match Store::open(&database) {
        Ok(_) => panic!("corrupt database was accepted"),
        Err(error) => error,
    };
    assert!(
        matches!(
            error.sqlite_error_code(),
            Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt)
        ),
        "expected corrupt-database error, got {error:?}"
    );
    assert_eq!(fs::read(&database).expect("read corrupt fixture"), original);
}

#[test]
fn newer_schema_is_rejected_without_running_migrations() {
    let tree = tempdir().expect("create isolated database directory");
    let database = tree.path().join("newer-schema.db");
    let store = Store::open(&database).expect("create schema fixture");
    drop(store);

    let connection = Connection::open(&database).expect("open schema fixture");
    connection
        .pragma_update(None, "user_version", 3_i64)
        .expect("set future schema version");
    drop(connection);
    let original = fs::read(&database).expect("read future-schema fixture");

    let error = match Store::open(&database) {
        Ok(_) => panic!("newer schema was accepted"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("user_version=3")
            && error.to_string().contains("supports up to 2"),
        "unexpected newer-schema error: {error:?}"
    );
    assert_eq!(
        fs::read(&database).expect("read future-schema fixture after rejection"),
        original,
        "newer schema rejection must not mutate the database"
    );
}

#[cfg(unix)]
#[test]
fn read_only_database_is_readable_but_writes_fail_without_mutation() {
    use std::os::unix::fs::PermissionsExt;

    let tree = tempdir().expect("create isolated database directory");
    let database = tree.path().join("read-only.db");
    let store = Store::open(&database).expect("create read-only fixture");
    drop(store);
    let original = fs::read(&database).expect("read original database bytes");

    let mut permissions = fs::metadata(&database)
        .expect("stat database")
        .permissions();
    permissions.set_mode(permissions.mode() & !0o222);
    fs::set_permissions(&database, permissions).expect("make database read-only");

    let read_only = Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("open read-only SQLite connection");
    let version: i64 = read_only
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read schema version from read-only connection");
    assert_eq!(version, 2);
    let write_error = read_only
        .execute("CREATE TABLE should_not_exist (id INTEGER)", [])
        .expect_err("read-only SQLite connection accepted a write");
    assert_eq!(write_error.sqlite_error_code(), Some(ErrorCode::ReadOnly));
    drop(read_only);

    let store = Store::open(&database).expect("open complete database read-only");
    assert_eq!(
        store.user_version().expect("read read-only schema version"),
        2
    );
    let write_error = store
        .connection()
        .execute("CREATE TABLE should_not_exist (id INTEGER)", [])
        .expect_err("read-only Store connection accepted a write");
    assert_eq!(write_error.sqlite_error_code(), Some(ErrorCode::ReadOnly));
    drop(store);
    assert_eq!(
        fs::read(&database).expect("read database after read-only access"),
        original
    );
}
