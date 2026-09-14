use rusqlite::{Connection, ErrorCode, OpenFlags};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Barrier, Condvar, Mutex, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};
use symeraseme_core::storage::{Store, open};
use tempfile::tempdir;

const GOLDEN_DATABASE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/event-store/golden-campaign.db"
);

const EXPECTED_ORACLE_COMMIT: &str = "bf53346eec234929bedf0314b99e3da85dbb991b";
const EXPECTED_STORE_SOURCE_SHA256: &str =
    "fd1dd416606f29aa4726a62ffe6ad83ef9d7c9eb6c6f42d81e87987913968df3";

const ORACLE_TIMEOUT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// RAII Child Process & Bounded Command Execution
// ---------------------------------------------------------------------------

struct ChildGuard {
    child: Option<Child>,
}

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self { child: Some(child) }
    }

    fn as_mut(&mut self) -> &mut Child {
        self.child.as_mut().expect("child active")
    }

    fn take(&mut self) -> Child {
        self.child.take().expect("child active")
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

struct CommandOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run_command_bounded(
    cmd: &mut Command,
    stdin_data: Option<&[u8]>,
    timeout: Duration,
) -> std::io::Result<CommandOutput> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    if stdin_data.is_some() {
        cmd.stdin(Stdio::piped());
    }
    let mut guard = ChildGuard::new(cmd.spawn()?);
    let child = guard.as_mut();

    if let Some(data) = stdin_data {
        let mut stdin = child.stdin.take().expect("child stdin");
        stdin.write_all(data)?;
        stdin.flush()?;
    }

    let mut stdout_pipe = child.stdout.take().expect("child stdout");
    let mut stderr_pipe = child.stderr.take().expect("child stderr");

    let (tx_out, rx_out) = mpsc::channel();
    let (tx_err, rx_err) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout_pipe.read_to_end(&mut buf);
        let _ = tx_out.send(buf);
    });
    thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr_pipe.read_to_end(&mut buf);
        let _ = tx_err.send(buf);
    });

    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "command exceeded bounded timeout",
            ));
        }
        thread::sleep(Duration::from_millis(5));
    };
    let _ = guard.take().wait();

    let stdout = rx_out
        .recv_timeout(Duration::from_secs(2))
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::TimedOut, err))?;
    let stderr = rx_err
        .recv_timeout(Duration::from_secs(2))
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::TimedOut, err))?;
    Ok(CommandOutput {
        status,
        stdout,
        stderr,
    })
}

// ---------------------------------------------------------------------------
// Pinned Go Oracle Materialization & Execution
// ---------------------------------------------------------------------------

struct OracleHandle {
    _temp_dir: tempfile::TempDir,
    executable: PathBuf,
}

static ORACLE_HANDLE: OnceLock<OracleHandle> = OnceLock::new();

fn git_archive_command(repo_root: &Path, git_ref: &str, paths: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    cmd.args([
        "-c",
        "core.autocrlf=false",
        "-c",
        "core.eol=lf",
        "archive",
        "--format=tar",
        git_ref,
    ]);
    for path in paths {
        cmd.arg(path);
    }
    cmd.current_dir(repo_root);
    cmd
}

fn oracle_executable() -> &'static Path {
    &ORACLE_HANDLE
        .get_or_init(|| {
            let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .canonicalize()
                .expect("canonical repository root");
            let temp_dir = tempdir().expect("create oracle build tempdir");

            // 1. Materialize pinned Go production tree via git archive
            let mut git_cmd = git_archive_command(&repo_root, EXPECTED_ORACLE_COMMIT, &[]);
            let git_out = run_command_bounded(&mut git_cmd, None, ORACLE_TIMEOUT)
                .expect("git archive on pinned commit must succeed");
            assert!(
                git_out.status.success(),
                "git archive failed: {:?}",
                String::from_utf8_lossy(&git_out.stderr)
            );

            let mut archive = tar::Archive::new(&git_out.stdout[..]);
            archive.unpack(temp_dir.path()).expect("unpack pinned tree");

            // 2. Verify exact real source hash of internal/eventstore/store.go
            let store_source = temp_dir.path().join("internal/eventstore/store.go");
            let store_bytes = fs::read(&store_source).expect("read store.go from pinned tree");
            let actual_sha = hex::encode(Sha256::digest(&store_bytes));
            assert_eq!(
                actual_sha, EXPECTED_STORE_SOURCE_SHA256,
                "pinned store.go SHA256 mismatch"
            );

            // 3. Overlay ONLY committed test adapter
            let adapter_source =
                fs::read(repo_root.join("rust-tests/parity/oracle/sqlite_lifecycle/main.go"))
                    .expect("read committed sqlite lifecycle adapter");
            let target_adapter_dir = temp_dir
                .path()
                .join("rust-tests/parity/oracle/sqlite_lifecycle");
            fs::create_dir_all(&target_adapter_dir).expect("create adapter dir");
            fs::write(target_adapter_dir.join("main.go"), adapter_source)
                .expect("write adapter in pinned tree");

            // 4. Compile with bounded timeout
            let executable = temp_dir.path().join(if cfg!(windows) {
                "sqlite-lifecycle-oracle.exe"
            } else {
                "sqlite-lifecycle-oracle"
            });
            let mut build_cmd = Command::new("go");
            build_cmd
                .args(["build", "-o"])
                .arg(&executable)
                .arg("./rust-tests/parity/oracle/sqlite_lifecycle")
                .current_dir(temp_dir.path())
                .env("GOWORK", "off");
            let build_out = run_command_bounded(&mut build_cmd, None, ORACLE_TIMEOUT)
                .expect("Go build must complete within bounded timeout");
            assert!(
                build_out.status.success(),
                "Go oracle build failed: {:?}",
                String::from_utf8_lossy(&build_out.stderr)
            );

            OracleHandle {
                _temp_dir: temp_dir,
                executable,
            }
        })
        .executable
}

fn run_oracle(req: &Value) -> Value {
    let executable = oracle_executable();
    let mut cmd = Command::new(executable);
    let req_bytes = serde_json::to_vec(req).expect("serialize oracle request");
    let out = run_command_bounded(&mut cmd, Some(&req_bytes), ORACLE_TIMEOUT)
        .expect("run Go oracle within bounded timeout");
    assert!(
        out.status.success(),
        "Go oracle exited with failure: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("parse oracle response JSON")
}

// ---------------------------------------------------------------------------
// Synchronized Interactive Oracle Handshakes (Deadlock-Free, Leak-Free)
// ---------------------------------------------------------------------------

struct InteractiveChild {
    guard: ChildGuard,
    stdin: ChildStdin,
    lines: mpsc::Receiver<std::io::Result<String>>,
}

impl InteractiveChild {
    fn spawn(args: &[&str]) -> Self {
        let executable = oracle_executable();
        let mut cmd = Command::new(executable);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut guard = ChildGuard::new(cmd.spawn().expect("spawn interactive oracle"));
        let child = guard.as_mut();
        let stdin = child.stdin.take().expect("child stdin");
        let stdout = child.stdout.take().expect("child stdout");
        let stderr = child.stderr.take().expect("child stderr");

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = BufReader::new(stderr).read_to_end(&mut buf);
        });

        Self {
            guard,
            stdin,
            lines: rx,
        }
    }

    fn read_line(&self, timeout: Duration) -> String {
        match self.lines.recv_timeout(timeout) {
            Ok(Ok(line)) => line,
            Ok(Err(err)) => panic!("error reading oracle stdout: {err}"),
            Err(_) => panic!("timed out waiting for line from oracle stdout within {timeout:?}"),
        }
    }

    fn write_line(&mut self, line: &str) {
        writeln!(self.stdin, "{line}").expect("write line to oracle stdin");
        self.stdin.flush().expect("flush oracle stdin");
    }

    fn wait_for_exit(mut self, timeout: Duration) -> std::process::ExitStatus {
        drop(self.stdin);
        let mut child = self.guard.take();
        let start = Instant::now();
        loop {
            if let Some(status) = child.try_wait().expect("try_wait") {
                return status;
            }
            if start.elapsed() >= timeout {
                let _ = child.kill();
                let _ = child.wait();
                panic!("timed out waiting for oracle to exit within {timeout:?}");
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}

struct InteractiveSnapshotSession(InteractiveChild);

impl InteractiveSnapshotSession {
    fn start(db_path: &Path) -> Self {
        Self(InteractiveChild::spawn(&[
            "--interactive-snapshot",
            "--path",
            db_path.to_str().expect("valid utf8 path"),
        ]))
    }

    fn wait_ready(&self, timeout: Duration) {
        let line = self.0.read_line(timeout);
        assert_eq!(line.trim(), "READY", "expected READY from oracle snapshot");
    }

    fn continue_and_finish(mut self, timeout: Duration) -> Value {
        self.0.write_line("CONTINUE");
        let json_line = self.0.read_line(timeout);
        let status = self.0.wait_for_exit(timeout);
        assert!(
            status.success(),
            "interactive oracle snapshot exited with failure: {status:?}"
        );
        serde_json::from_str(&json_line).expect("parse response JSON from oracle snapshot")
    }
}

struct InteractiveLockSession(InteractiveChild);

impl InteractiveLockSession {
    fn start(db_path: &Path, id: i64, value: &str) -> Self {
        let id_str = id.to_string();
        Self(InteractiveChild::spawn(&[
            "--interactive-lock",
            "--path",
            db_path.to_str().expect("valid utf8 path"),
            "--id",
            &id_str,
            "--value",
            value,
        ]))
    }

    fn wait_held(&self, timeout: Duration) {
        let line = self.0.read_line(timeout);
        assert_eq!(line.trim(), "HELD", "expected HELD from oracle lock");
    }

    fn release(mut self, action: &str, timeout: Duration) {
        self.0.write_line(&format!("RELEASE {action}"));
        let line = self.0.read_line(timeout);
        assert_eq!(
            line.trim(),
            "RELEASED",
            "expected RELEASED from oracle lock"
        );
        let status = self.0.wait_for_exit(timeout);
        assert!(
            status.success(),
            "lock oracle exited with failure: {status:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Native Test Utilities
// ---------------------------------------------------------------------------

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

fn make_read_only(path: &Path) {
    let mut permissions = fs::metadata(path).expect("stat database").permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(permissions.mode() & !0o222);
    }
    #[cfg(not(unix))]
    {
        permissions.set_readonly(true);
    }
    fs::set_permissions(path, permissions).expect("set read-only permissions");
}

// ---------------------------------------------------------------------------
// Native Rust SQLite Lifecycle Tests
// ---------------------------------------------------------------------------

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

#[test]
fn read_only_database_is_readable_but_writes_fail_without_mutation() {
    let tree = tempdir().expect("create isolated database directory");
    let database = tree.path().join("read-only.db");
    let store = Store::open(&database).expect("create read-only fixture");
    drop(store);
    let original = fs::read(&database).expect("read original database bytes");

    make_read_only(&database);

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

// ---------------------------------------------------------------------------
// Runtime Differential Go Oracle Parity Tests (DB-009, DB-010)
// ---------------------------------------------------------------------------

#[test]
fn go_oracle_provenance_and_schema_pragmas_differential() {
    let prov_resp = run_oracle(&json!({ "op": "provenance" }));
    assert_eq!(prov_resp["status"], "ok");
    assert_eq!(
        prov_resp["provenance"]["source_revision"],
        EXPECTED_ORACLE_COMMIT
    );
    assert_eq!(
        prov_resp["provenance"]["source_sha256"],
        EXPECTED_STORE_SOURCE_SHA256
    );

    let tree = tempdir().expect("create isolated database directory");
    let go_db = tree.path().join("go_fresh.db");
    let rust_db = tree.path().join("rust_fresh.db");

    let go_resp = run_oracle(&json!({
        "op": "open",
        "path": go_db.to_str().expect("valid path")
    }));
    assert_eq!(go_resp["status"], "ok");

    let rust_store = Store::open(&rust_db).expect("open fresh rust store");
    assert_eq!(
        rust_store.user_version().expect("read rust user_version"),
        go_resp["user_version"].as_i64().expect("go user_version")
    );

    let rust_busy_timeout: i64 = rust_store
        .connection()
        .pragma_query_value(None, "busy_timeout", |row| row.get(0))
        .expect("read busy_timeout");
    let rust_foreign_keys: i64 = rust_store
        .connection()
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .expect("read foreign_keys");
    let rust_journal_mode: String = rust_store
        .connection()
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .expect("read journal_mode");

    assert_eq!(
        rust_busy_timeout,
        go_resp["pragmas"]["busy_timeout"]
            .as_i64()
            .expect("go busy_timeout")
    );
    assert_eq!(
        rust_foreign_keys,
        go_resp["pragmas"]["foreign_keys"]
            .as_i64()
            .expect("go foreign_keys")
    );
    assert_eq!(
        rust_journal_mode.to_ascii_lowercase(),
        go_resp["pragmas"]["journal_mode"]
            .as_str()
            .expect("go journal_mode")
    );

    let rust_tables = rust_store
        .connection()
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .expect("prepare tables query")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query tables")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect rust tables");
    let go_tables: Vec<String> = go_resp["tables"]
        .as_array()
        .expect("go tables")
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(rust_tables, go_tables);

    let rust_indexes = rust_store
        .connection()
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'index' AND sql IS NOT NULL
             ORDER BY name",
        )
        .expect("prepare indexes query")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query indexes")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect rust indexes");
    let go_indexes: Vec<String> = go_resp["indexes"]
        .as_array()
        .expect("go indexes")
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(rust_indexes, go_indexes);
}

#[test]
fn git_archive_provenance_hostile_autocrlf_regression() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical repository root");

    // 1. Fetch immutable raw blob via git show
    let mut show_cmd = Command::new("git");
    show_cmd
        .args([
            "show",
            &format!("{EXPECTED_ORACLE_COMMIT}:internal/eventstore/store.go"),
        ])
        .current_dir(&repo_root);
    let show_out = run_command_bounded(&mut show_cmd, None, ORACLE_TIMEOUT)
        .expect("git show on pinned store.go must succeed");
    assert!(
        show_out.status.success(),
        "git show failed: {:?}",
        String::from_utf8_lossy(&show_out.stderr)
    );
    let blob_bytes = show_out.stdout;
    let blob_sha = hex::encode(Sha256::digest(&blob_bytes));
    assert_eq!(
        blob_sha, EXPECTED_STORE_SOURCE_SHA256,
        "git show blob sha must match expected source sha"
    );

    // 2. Exercise the SAME git_archive_command helper with hostile autocrlf environment
    let mut archive_cmd = git_archive_command(
        &repo_root,
        EXPECTED_ORACLE_COMMIT,
        &["internal/eventstore/store.go"],
    );
    archive_cmd
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "core.autocrlf")
        .env("GIT_CONFIG_VALUE_0", "true");

    let archive_out = run_command_bounded(&mut archive_cmd, None, ORACLE_TIMEOUT)
        .expect("git archive under hostile config must succeed");
    assert!(
        archive_out.status.success(),
        "git archive failed: {:?}",
        String::from_utf8_lossy(&archive_out.stderr)
    );

    // 3. Read tar bytes for store.go and verify exact blob match & sha256
    let mut archive = tar::Archive::new(&archive_out.stdout[..]);
    let mut extracted_bytes = Vec::new();
    for entry in archive.entries().expect("tar entries") {
        let mut file = entry.expect("valid tar entry");
        let entry_path = file.path().expect("tar entry path");
        if entry_path == Path::new("internal/eventstore/store.go")
            || entry_path.to_string_lossy().replace('\\', "/") == "internal/eventstore/store.go"
        {
            file.read_to_end(&mut extracted_bytes)
                .expect("read store.go from tar");
            break;
        }
    }
    assert!(
        !extracted_bytes.is_empty(),
        "internal/eventstore/store.go must be present in tar archive"
    );
    assert_eq!(
        extracted_bytes, blob_bytes,
        "archived store.go bytes must match immutable git show blob despite hostile autocrlf"
    );
    let extracted_sha = hex::encode(Sha256::digest(&extracted_bytes));
    assert_eq!(
        extracted_sha, EXPECTED_STORE_SOURCE_SHA256,
        "archived store.go sha must match expected sha despite hostile autocrlf"
    );
}

#[test]
fn go_reader_rust_writer_wal_isolation_differential() {
    let tree = tempdir().expect("create isolated database directory");
    let database = tree.path().join("wal_go_reader_rust_writer.db");

    let rust_setup = open(&database).expect("open rust connection to setup table");
    rust_setup
        .execute(
            "CREATE TABLE values_table (id INTEGER PRIMARY KEY, value TEXT NOT NULL)",
            [],
        )
        .expect("create values_table");
    drop(rust_setup);

    let session = InteractiveSnapshotSession::start(&database);
    session.wait_ready(ORACLE_TIMEOUT);

    let rust_writer = open(&database).expect("open rust writer");
    rust_writer
        .execute(
            "INSERT INTO values_table (id, value) VALUES (1, 'written by rust')",
            [],
        )
        .expect("rust writer inserts row");
    drop(rust_writer);

    let resp = session.continue_and_finish(ORACLE_TIMEOUT);
    assert_eq!(resp["status"], "ok");
    assert_eq!(resp["before_count"], 0);
    assert_eq!(resp["during_count"], 0, "Go reader snapshot was breached");
    assert_eq!(
        resp["after_count"], 1,
        "Go reader did not see committed rust row after commit"
    );
}

#[test]
fn rust_reader_go_writer_wal_isolation_differential() {
    let tree = tempdir().expect("create isolated database directory");
    let database = tree.path().join("wal_rust_reader_go_writer.db");

    let setup = open(&database).expect("open connection to setup table");
    setup
        .execute(
            "CREATE TABLE values_table (id INTEGER PRIMARY KEY, value TEXT NOT NULL)",
            [],
        )
        .expect("create values_table");
    drop(setup);

    let reader = open(&database).expect("open rust reader");
    reader
        .execute_batch("BEGIN DEFERRED")
        .expect("begin reader snapshot");
    let before: i64 = reader
        .query_row("SELECT count(*) FROM values_table", [], |row| row.get(0))
        .expect("read initial snapshot");
    assert_eq!(before, 0);

    let write_resp = run_oracle(&json!({
        "op": "write_row",
        "path": database.to_str().expect("valid path"),
        "id": 1,
        "value": "written by go"
    }));
    assert_eq!(write_resp["status"], "ok");

    let during: i64 = reader
        .query_row("SELECT count(*) FROM values_table", [], |row| row.get(0))
        .expect("read stable snapshot");
    assert_eq!(during, 0, "Rust reader snapshot was breached by Go writer");

    reader
        .execute_batch("COMMIT")
        .expect("commit reader snapshot");
    let after: i64 = reader
        .query_row("SELECT count(*) FROM values_table", [], |row| row.get(0))
        .expect("read committed row after snapshot commit");
    assert_eq!(after, 1);
}

#[test]
fn rust_writer_waits_for_go_lock_release_differential() {
    let tree = tempdir().expect("create isolated database directory");
    let database = tree.path().join("lock_go_holder_rust_waiter.db");

    let setup = open(&database).expect("open setup connection");
    setup
        .execute(
            "CREATE TABLE values_table (id INTEGER PRIMARY KEY, value TEXT NOT NULL)",
            [],
        )
        .expect("create values_table");
    drop(setup);

    let session = InteractiveLockSession::start(&database, 1, "written by go holder");
    session.wait_held(ORACLE_TIMEOUT);

    let db_path_clone = database.clone();
    let waiter_thread = thread::spawn(move || -> rusqlite::Result<()> {
        let rust_waiter = open(&db_path_clone)?;
        rust_waiter.execute_batch("BEGIN IMMEDIATE")?;
        rust_waiter.execute(
            "INSERT INTO values_table (id, value) VALUES (2, 'written by rust waiter')",
            [],
        )?;
        rust_waiter.execute_batch("COMMIT")?;
        Ok(())
    });

    session.release("commit", ORACLE_TIMEOUT);

    waiter_thread
        .join()
        .expect("waiter thread join")
        .expect("rust waiter acquires lock and commits after go holder release");

    let check = open(&database).expect("open check connection");
    let count: i64 = check
        .query_row("SELECT count(*) FROM values_table", [], |row| row.get(0))
        .expect("count total committed rows");
    assert_eq!(count, 2);
}

#[test]
fn go_writer_waits_for_rust_lock_release_differential() {
    let tree = tempdir().expect("create isolated database directory");
    let database = tree.path().join("lock_rust_holder_go_waiter.db");

    let setup = open(&database).expect("open setup connection");
    setup
        .execute(
            "CREATE TABLE values_table (id INTEGER PRIMARY KEY, value TEXT NOT NULL)",
            [],
        )
        .expect("create values_table");
    drop(setup);

    let rust_holder = open(&database).expect("open rust holder connection");
    rust_holder
        .execute_batch("BEGIN IMMEDIATE")
        .expect("rust holder acquires write lock");
    rust_holder
        .execute(
            "INSERT INTO values_table (id, value) VALUES (1, 'written by rust holder')",
            [],
        )
        .expect("rust holder inserts row");

    let db_path_clone = database.clone();
    let go_waiter_thread = thread::spawn(move || -> Value {
        run_oracle(&json!({
            "op": "write_row",
            "path": db_path_clone.to_str().expect("valid path"),
            "id": 2,
            "value": "written by go waiter"
        }))
    });

    rust_holder
        .execute_batch("COMMIT")
        .expect("rust holder commits write lock");
    drop(rust_holder);

    let go_result = go_waiter_thread.join().expect("go waiter thread join");
    assert_eq!(go_result["status"], "ok");

    let check = open(&database).expect("open check connection");
    let count: i64 = check
        .query_row("SELECT count(*) FROM values_table", [], |row| row.get(0))
        .expect("count total rows");
    assert_eq!(count, 2);
}

#[test]
fn cross_language_busy_timeout_and_retry_differential() {
    let tree = tempdir().expect("create isolated database directory");
    let database = tree.path().join("cross_busy_timeout.db");

    let setup = open(&database).expect("open setup connection");
    setup
        .execute(
            "CREATE TABLE values_table (id INTEGER PRIMARY KEY, value TEXT NOT NULL)",
            [],
        )
        .expect("create values_table");
    drop(setup);

    let session = InteractiveLockSession::start(&database, 1, "rolled back go value");
    session.wait_held(ORACLE_TIMEOUT);

    let rust_timeout_writer = open(&database).expect("open rust timeout writer");
    rust_timeout_writer
        .busy_timeout(Duration::from_millis(1))
        .expect("set short busy timeout");
    let timeout_err = rust_timeout_writer
        .execute_batch("BEGIN IMMEDIATE")
        .expect_err("rust writer should timeout against held go lock");
    assert!(
        is_contention(&timeout_err),
        "expected SQLITE_BUSY/LOCKED contention error, got {timeout_err:?}"
    );

    session.release("rollback", ORACLE_TIMEOUT);

    rust_timeout_writer
        .busy_timeout(Duration::from_secs(5))
        .expect("restore normal busy timeout");
    rust_timeout_writer
        .execute_batch(
            "BEGIN IMMEDIATE;
             INSERT INTO values_table (id, value) VALUES (2, 'retry by rust after go rollback');
             COMMIT;",
        )
        .expect("rust retry commits after go rollback");

    let count: i64 = rust_timeout_writer
        .query_row("SELECT count(*) FROM values_table", [], |row| row.get(0))
        .expect("read committed row count");
    assert_eq!(count, 1);
}

#[test]
fn interrupted_initialization_cross_language_differential() {
    let tree = tempdir().expect("create isolated database directory");

    // Scenario 1: Interrupted prefix generated by Go, resumed by Rust
    let go_interrupted_db = tree.path().join("go_interrupted.db");
    let create_resp = run_oracle(&json!({
        "op": "init_interrupted_fixture",
        "path": go_interrupted_db.to_str().expect("valid path"),
        "value": "interrupted-go-campaign",
        "notes": "go prefix preserved"
    }));
    assert_eq!(create_resp["status"], "ok");

    let rust_resumed_store =
        Store::open(&go_interrupted_db).expect("Rust resumes Go interrupted initialization");
    assert_eq!(
        rust_resumed_store
            .user_version()
            .expect("read user_version"),
        2
    );
    let notes: String = rust_resumed_store
        .connection()
        .query_row(
            "SELECT notes FROM campaigns WHERE id = 'interrupted-go-campaign'",
            [],
            |row| row.get(0),
        )
        .expect("read preserved campaign notes");
    assert_eq!(notes, "go prefix preserved");
    let imap_table_count: i64 = rust_resumed_store
        .connection()
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'imap_state'",
            [],
            |row| row.get(0),
        )
        .expect("check imap_state table");
    assert_eq!(imap_table_count, 1);
    drop(rust_resumed_store);

    // Scenario 2: Interrupted prefix generated by Rust, resumed by Go
    let rust_interrupted_db = tree.path().join("rust_interrupted.db");
    create_partial_initialization(&rust_interrupted_db);

    let go_resume_resp = run_oracle(&json!({
        "op": "open",
        "path": rust_interrupted_db.to_str().expect("valid path")
    }));
    assert_eq!(go_resume_resp["status"], "ok");
    assert_eq!(go_resume_resp["user_version"], 2);

    let go_notes_resp = run_oracle(&json!({
        "op": "query_campaign_notes",
        "path": rust_interrupted_db.to_str().expect("valid path"),
        "value": "interrupted-campaign"
    }));
    assert_eq!(go_notes_resp["status"], "ok");
    assert_eq!(go_notes_resp["notes"], "preserve me");
}

#[test]
fn future_schema_version_refusal_differential() {
    let tree = tempdir().expect("create isolated database directory");
    let database = tree.path().join("future_version.db");

    let store = Store::open(&database).expect("create v2 database");
    drop(store);

    let conn = Connection::open(&database).expect("open db to set version 3");
    conn.pragma_update(None, "user_version", 3_i64)
        .expect("set user_version = 3");
    drop(conn);

    let original_bytes = fs::read(&database).expect("read original bytes");

    let go_resp = run_oracle(&json!({
        "op": "open",
        "path": database.to_str().expect("valid path")
    }));
    assert_eq!(go_resp["status"], "error");
    let go_error = go_resp["error"].as_str().expect("go error string");
    assert!(
        go_error.contains("user_version=3") && go_error.contains("supports up to 2"),
        "unexpected Go refusal error: {go_error}"
    );

    let rust_err = match Store::open(&database) {
        Ok(_) => panic!("Rust unexpectedly accepted user_version=3"),
        Err(err) => err.to_string(),
    };
    assert!(
        rust_err.contains("user_version=3") && rust_err.contains("supports up to 2"),
        "unexpected Rust refusal error: {rust_err}"
    );

    assert_eq!(
        fs::read(&database).expect("read bytes after refusal"),
        original_bytes,
        "refusal must not mutate database file bytes"
    );
}

#[test]
fn read_only_cross_language_differential() {
    let tree = tempdir().expect("create isolated database directory");
    let database = tree.path().join("read_only_cross.db");

    let store = Store::open(&database).expect("create v2 database");
    drop(store);

    let original_bytes = fs::read(&database).expect("read original bytes");
    make_read_only(&database);

    let go_resp = run_oracle(&json!({
        "op": "write_row",
        "path": database.to_str().expect("valid path"),
        "id": 1,
        "value": "should fail on read only"
    }));
    assert_eq!(
        go_resp["status"], "error",
        "Go write on read-only file should fail"
    );

    let rust_read_only = Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("open read-only SQLite connection");
    let version: i64 = rust_read_only
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read version from read-only connection");
    assert_eq!(version, 2);

    let rust_write_err = rust_read_only
        .execute("CREATE TABLE should_not_exist (id INTEGER)", [])
        .expect_err("Rust write on read-only connection should fail");
    assert_eq!(
        rust_write_err.sqlite_error_code(),
        Some(ErrorCode::ReadOnly)
    );
    drop(rust_read_only);

    assert_eq!(
        fs::read(&database).expect("read bytes after read-only access"),
        original_bytes,
        "read-only access must not modify database file"
    );
}

#[test]
fn corrupt_database_cross_language_differential() {
    let tree = tempdir().expect("create isolated database directory");
    let database = tree.path().join("corrupt_cross.db");
    let corrupt_payload = b"not a SQLite database";
    fs::write(&database, corrupt_payload).expect("write corrupt bytes");

    let go_resp = run_oracle(&json!({
        "op": "open",
        "path": database.to_str().expect("valid path")
    }));
    assert_eq!(
        go_resp["status"], "error",
        "Go should reject corrupt database"
    );

    let rust_err = match Store::open(&database) {
        Ok(_) => panic!("Rust should reject corrupt database"),
        Err(err) => err,
    };
    assert!(
        matches!(
            rust_err.sqlite_error_code(),
            Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt)
        ),
        "expected corrupt error from rust, got {rust_err:?}"
    );

    assert_eq!(
        fs::read(&database).expect("read corrupt bytes after rejection"),
        corrupt_payload,
        "corrupt database rejection must not modify file bytes"
    );
}
