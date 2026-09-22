//! SQLite database lifecycle and schema migration.

use super::encrypted_store::{
    CheckpointError, EncryptedStoreError, checkpoint_wal_conn, close_store,
};
use super::locking::DbLock;
use super::open;
use rusqlite::{Connection, Result};
use std::path::{Path, PathBuf};

/// Current schema version written by the Go event-store oracle.
pub const SCHEMA_VERSION: i64 = 2;

/// A schema-owning SQLite store.
pub struct Store {
    pub(crate) connection: Connection,
    path: PathBuf,
    pub(crate) sqlite_path: PathBuf,
    pub(crate) encrypted_path: Option<PathBuf>,
    pub(crate) db_lock: Option<DbLock>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("path", &self.path)
            .field("sqlite_path", &self.sqlite_path)
            .field("encrypted_path", &self.encrypted_path)
            .finish()
    }
}

impl Store {
    /// Opens a database, applies the Go connection pragmas, and runs the
    /// idempotent schema initialization/migration sequence.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let connection = open(&path)?;
        let store = Self {
            connection,
            sqlite_path: path.clone(),
            path,
            encrypted_path: None,
            db_lock: None,
        };
        store.init_schema()?;
        Ok(store)
    }

    /// Returns the underlying connection for the next storage slices and
    /// focused contract tests.
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Alias matching the Go store's raw database accessor.
    pub fn db(&self) -> &Connection {
        self.connection()
    }

    /// Returns the path supplied to [`Store::open`].
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn set_encrypted_paths(&mut self, canonical: PathBuf, sqlite: PathBuf) {
        self.path = canonical.clone();
        self.sqlite_path = sqlite;
        self.encrypted_path = Some(canonical);
    }

    pub fn encrypted_path(&self) -> Option<&Path> {
        self.encrypted_path.as_deref()
    }

    pub fn checkpoint_wal(&self) -> std::result::Result<(), CheckpointError> {
        checkpoint_wal_conn(&self.connection)
    }

    pub fn close(self) -> std::result::Result<(), EncryptedStoreError> {
        close_store(self)
    }

    /// Reads SQLite's user_version pragma.
    pub fn user_version(&self) -> Result<i64> {
        self.connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
    }

    /// Go's `replies.Repository.Latest` reduced to existence: whether any
    /// inbox reply is stored for the request.
    ///
    /// ponytail: only the nil vs not-nil branch is ported — `auto_confirm`
    /// stops before `confirmation.AutoConfirm`, whose browser subsystem has no
    /// Rust counterpart. Upgrade path: return the row when that port lands.
    pub fn has_inbox_reply(&self, request_id: i64) -> Result<bool> {
        let found = match self.connection.query_row(
            "SELECT id FROM inbox_replies WHERE request_id = ?1 \
             ORDER BY received_at DESC, id DESC LIMIT 1",
            [request_id],
            |row| row.get::<usize, i64>(0),
        ) {
            Ok(id) => Some(id),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(error),
        };
        Ok(found.is_some())
    }

    pub fn init_schema(&self) -> Result<()> {
        let current = self.user_version()?;
        if current > SCHEMA_VERSION {
            return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
                UnsupportedSchema {
                    path: self.path.clone(),
                    version: current,
                },
            )));
        }

        if current == 0 {
            for statement in V1_SCHEMA {
                self.connection.execute_batch(statement)?;
            }
        }
        if current < SCHEMA_VERSION {
            for statement in V2_MIGRATION {
                self.connection.execute_batch(statement)?;
            }
        }
        for statement in AUXILIARY_SCHEMA {
            self.connection.execute_batch(statement)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct UnsupportedSchema {
    path: PathBuf,
    version: i64,
}

impl std::fmt::Display for UnsupportedSchema {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "eventstore: DB at {} has user_version={}, Go port supports up to {} — refusing to operate",
            self.path.display(),
            self.version,
            SCHEMA_VERSION
        )
    }
}

impl std::error::Error for UnsupportedSchema {}

const V1_SCHEMA: &[&str] = &[
    r#"CREATE TABLE IF NOT EXISTS campaigns (
                id              TEXT PRIMARY KEY,
                created_at      TIMESTAMP NOT NULL DEFAULT (datetime('now')),
                kind            TEXT NOT NULL DEFAULT 'initial',
                notes           TEXT
            )"#,
    r#"CREATE TABLE IF NOT EXISTS removal_requests (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                broker_id       TEXT NOT NULL,
                channel         TEXT NOT NULL DEFAULT 'email',
                campaign_id     TEXT NOT NULL,
                created_at      TIMESTAMP NOT NULL DEFAULT (datetime('now')),
                jurisdiction    TEXT NOT NULL,
                template_id     TEXT NOT NULL DEFAULT '',
                identity_snapshot_hash TEXT NOT NULL DEFAULT ''
            )"#,
    r#"CREATE TABLE IF NOT EXISTS request_events (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                request_id      INTEGER NOT NULL REFERENCES removal_requests(id),
                occurred_at     TIMESTAMP NOT NULL DEFAULT (datetime('now')),
                recorded_at     TIMESTAMP NOT NULL DEFAULT (datetime('now')),
                event_type      TEXT NOT NULL,
                payload_json    TEXT NOT NULL DEFAULT '{}',
                source          TEXT NOT NULL DEFAULT 'system'
            )"#,
    r#"CREATE INDEX IF NOT EXISTS idx_events_request
                ON request_events(request_id, occurred_at)"#,
    r#"CREATE INDEX IF NOT EXISTS idx_events_occurred_at
                ON request_events(occurred_at DESC)"#,
    r#"CREATE TABLE IF NOT EXISTS request_state (
                request_id      INTEGER PRIMARY KEY REFERENCES removal_requests(id),
                current_status  TEXT NOT NULL DEFAULT 'PLANNED',
                last_event_id   INTEGER NOT NULL DEFAULT 0,
                last_event_at   TIMESTAMP NOT NULL DEFAULT (datetime('now')),
                sent_at         TIMESTAMP,
                acknowledged_at TIMESTAMP,
                resolved_at     TIMESTAMP,
                deadline_at     TIMESTAMP,
                next_action_at  TIMESTAMP,
                reminders_sent  INTEGER NOT NULL DEFAULT 0,
                escalation_level INTEGER NOT NULL DEFAULT 0
            )"#,
    r#"CREATE INDEX IF NOT EXISTS idx_request_state_next_action
                ON request_state(next_action_at, current_status)"#,
    r#"CREATE INDEX IF NOT EXISTS idx_removal_requests_campaign
                ON removal_requests(campaign_id)"#,
    r#"CREATE INDEX IF NOT EXISTS idx_removal_requests_broker
                ON removal_requests(broker_id)"#,
    r#"CREATE INDEX IF NOT EXISTS idx_removal_requests_jurisdiction
                ON removal_requests(jurisdiction)"#,
    "PRAGMA user_version = 1",
];

const V2_MIGRATION: &[&str] = &[
    r#"CREATE TABLE IF NOT EXISTS imap_state (
            host           TEXT NOT NULL,
            folder         TEXT NOT NULL,
            uid_validity   INTEGER NOT NULL,
            last_uid       INTEGER NOT NULL DEFAULT 0,
            updated_at     TIMESTAMP NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (host, folder)
        )"#,
    "PRAGMA user_version = 2",
];

const AUXILIARY_SCHEMA: &[&str] = &[
    r#"CREATE TABLE IF NOT EXISTS manual_tasks (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            request_id          INTEGER REFERENCES removal_requests(id),
            broker_id           TEXT NOT NULL DEFAULT '',
            broker_name         TEXT NOT NULL DEFAULT '',
            form_url            TEXT NOT NULL DEFAULT '',
            reason              TEXT NOT NULL DEFAULT 'generic_error',
            instructions        TEXT NOT NULL DEFAULT '',
            screenshot_path     TEXT NOT NULL DEFAULT '',
            html_snapshot_path  TEXT NOT NULL DEFAULT '',
            form_fields_json    TEXT NOT NULL DEFAULT '{}',
            status              TEXT NOT NULL DEFAULT 'pending',
            created_at          TIMESTAMP NOT NULL DEFAULT (datetime('now')),
            completed_at        TIMESTAMP,
            notes               TEXT NOT NULL DEFAULT ''
        )"#,
    "CREATE INDEX IF NOT EXISTS idx_manual_tasks_status ON manual_tasks(status)",
    "CREATE INDEX IF NOT EXISTS idx_manual_tasks_request ON manual_tasks(request_id)",
    r#"CREATE TABLE IF NOT EXISTS inbox_replies (
            id INTEGER PRIMARY KEY AUTOINCREMENT, request_id INTEGER REFERENCES removal_requests(id),
            message_id TEXT UNIQUE NOT NULL, thread_id TEXT, received_at TIMESTAMP NOT NULL DEFAULT (datetime('now')),
            from_addr TEXT, subject TEXT, snippet TEXT, classified_as TEXT,
            classifier_confidence REAL, llm_summary TEXT
        )"#,
    "CREATE INDEX IF NOT EXISTS idx_inbox_replies_request ON inbox_replies(request_id)",
    "CREATE INDEX IF NOT EXISTS idx_inbox_replies_classified ON inbox_replies(classified_as)",
    r#"CREATE TABLE IF NOT EXISTS reply_drafts (
            id INTEGER PRIMARY KEY AUTOINCREMENT, reply_id INTEGER NOT NULL REFERENCES inbox_replies(id),
            request_id INTEGER REFERENCES removal_requests(id), draft_body TEXT NOT NULL, subject TEXT NOT NULL DEFAULT '',
            created_at TIMESTAMP NOT NULL DEFAULT (datetime('now')), sent_at TIMESTAMP, account TEXT
        )"#,
];
