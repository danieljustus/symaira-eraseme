"""SQLite connection management, schema initialisation, and connection pooling.

Provides thread-local connection caching, WAL journal mode, foreign keys,
and automatic encryption/decryption orchestration for the event store.
"""

from __future__ import annotations

import logging
import sqlite3
import threading
from contextlib import contextmanager
from pathlib import Path

from symeraseme.core.config import get_config
from symeraseme.core.db_cleanup import (
    _acquire_db_lock,
    _reencrypt_and_remove_temp,
    _release_db_lock,
    _scavenge_stale_temp_dbs,
)
from symeraseme.core.db_encryption import (
    DB_TEMP,
    _db_encryption_enabled,
    _decrypt_to_temp,
    _migrate_v1_to_v2,
    _migrate_v2_to_v3,
)

logger = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

SCHEMA_VERSION = 1

# ---------------------------------------------------------------------------
# Mutable state
# ---------------------------------------------------------------------------

_SCHEMA_VERSION_CACHE: dict[str, int] = {}
_local = threading.local()


# ---------------------------------------------------------------------------
# Path resolution
# ---------------------------------------------------------------------------


def _db_path(path: str | None = None) -> Path:
    """Resolve the database file path."""
    if path:
        return Path(path).expanduser().resolve()
    config = get_config()
    db_dir = config.db_dir
    db_dir.mkdir(parents=True, exist_ok=True)
    return config.db_path


# ---------------------------------------------------------------------------
# Connection management
# ---------------------------------------------------------------------------


def get_connection(path: str | None = None) -> sqlite3.Connection:
    """Return a thread-local connection, creating one if necessary.

    When encryption is enabled, the encrypted DB is transparently decrypted
    to a secure temp file and re-encrypted on close.
    """
    requested_path = str(_db_path(path))
    if (
        not hasattr(_local, "conn")
        or _local.conn is None
        or not hasattr(_local, "db_path")
        or _local.db_path != requested_path
    ):
        _scavenge_stale_temp_dbs()
        db_file = _db_path(path)

        should_encrypt = _db_encryption_enabled()

        if should_encrypt:
            _acquire_db_lock(db_file, retry=True)

        if should_encrypt and db_file.exists():
            raw = db_file.read_bytes()
            is_enc = bool(raw) and (
                raw.startswith(b"SYMERASEME_ENCv1\n")
                or raw.startswith(b"SYMERASEME_ENCv2\n")
                or raw.startswith(b"SYMERASEME_ENCv3\n")
            )
            if is_enc:
                if raw.startswith(b"SYMERASEME_ENCv1\n"):
                    logger.warning(
                        "V1-encrypted database detected at %s. "
                        "Migrating to V3 format automatically. "
                        "Consider running 'symeraseme db migrate' explicitly.",
                        db_file,
                    )
                    _migrate_v1_to_v2(db_file)
                    _migrate_v2_to_v3(db_file)
                elif raw.startswith(b"SYMERASEME_ENCv2\n"):
                    _migrate_v2_to_v3(db_file)
                db_file = _decrypt_to_temp(db_file)
            else:
                # Existing plaintext DB with encryption enabled — register for encrypt-on-close
                DB_TEMP[db_file.resolve()] = db_file
        elif should_encrypt and not db_file.exists():
            DB_TEMP[db_file.resolve()] = db_file

        conn = sqlite3.connect(str(db_file))
        conn.row_factory = sqlite3.Row
        conn.execute("PRAGMA journal_mode=WAL")
        conn.execute("PRAGMA foreign_keys=ON")
        _local.conn = conn
        _local.db_path = requested_path
    return _local.conn


def close_connection() -> None:
    """Close the thread-local connection and re-encrypt temp files."""
    if hasattr(_local, "conn") and _local.conn is not None:
        _local.conn.close()
        _local.conn = None

    for orig, tmp in list(DB_TEMP.items()):
        _reencrypt_and_remove_temp(orig, tmp)
    DB_TEMP.clear()

    _release_db_lock()

    if hasattr(_local, "db_path"):
        _local.db_path = None


@contextmanager
def connection_context(path: str | None = None):
    """Context manager for DB connections with automatic cleanup.

    When encryption is enabled, the decrypted temp file is
    re-encrypted and removed when the context exits.

    Usage::

        with connection_context() as conn:
            conn.execute(...)
    """
    conn = get_connection(path)
    try:
        yield conn
    finally:
        close_connection()


# ---------------------------------------------------------------------------
# Schema initialisation
# ---------------------------------------------------------------------------


def init_db(path: str | None = None) -> Path:
    """Create the database schema if it does not exist.

    Returns the database file path.
    """
    db_file = _db_path(path)
    db_key = str(db_file)

    # Skip PRAGMA check when cache is already current for this DB.
    if _SCHEMA_VERSION_CACHE.get(db_key, -1) >= SCHEMA_VERSION:
        return db_file

    db_file.parent.mkdir(parents=True, exist_ok=True)

    conn = get_connection(db_key)

    current_version = conn.execute("PRAGMA user_version").fetchone()[0]
    _SCHEMA_VERSION_CACHE[db_key] = current_version
    if current_version >= SCHEMA_VERSION:
        return db_file

    conn.executescript("""
        CREATE TABLE IF NOT EXISTS campaigns (
            id              TEXT PRIMARY KEY,
            created_at      TIMESTAMP NOT NULL DEFAULT (datetime('now')),
            kind            TEXT NOT NULL DEFAULT 'initial',
            notes           TEXT
        );

        CREATE TABLE IF NOT EXISTS removal_requests (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            broker_id       TEXT NOT NULL,
            channel         TEXT NOT NULL DEFAULT 'email',
            campaign_id     TEXT NOT NULL,
            created_at      TIMESTAMP NOT NULL DEFAULT (datetime('now')),
            jurisdiction    TEXT NOT NULL,
            template_id     TEXT NOT NULL DEFAULT '',
            identity_snapshot_hash TEXT NOT NULL DEFAULT ''
        );

        CREATE TABLE IF NOT EXISTS request_events (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            request_id      INTEGER NOT NULL REFERENCES removal_requests(id),
            occurred_at     TIMESTAMP NOT NULL DEFAULT (datetime('now')),
            recorded_at     TIMESTAMP NOT NULL DEFAULT (datetime('now')),
            event_type      TEXT NOT NULL,
            payload_json    TEXT NOT NULL DEFAULT '{}',
            source          TEXT NOT NULL DEFAULT 'system'
        );
        CREATE INDEX IF NOT EXISTS idx_events_request
            ON request_events(request_id, occurred_at);
        CREATE INDEX IF NOT EXISTS idx_events_occurred_at
            ON request_events(occurred_at DESC);

        CREATE TABLE IF NOT EXISTS request_state (
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
        );
        CREATE INDEX IF NOT EXISTS idx_request_state_next_action
            ON request_state(next_action_at, current_status);

        CREATE TABLE IF NOT EXISTS inbox_replies (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            request_id      INTEGER REFERENCES removal_requests(id),
            message_id      TEXT UNIQUE NOT NULL,
            thread_id       TEXT,
            received_at     TIMESTAMP NOT NULL DEFAULT (datetime('now')),
            from_addr       TEXT,
            subject         TEXT,
            snippet         TEXT,
            classified_as   TEXT,
            classifier_confidence REAL,
            llm_summary     TEXT
        );

        CREATE TABLE IF NOT EXISTS reply_drafts (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            reply_id        INTEGER NOT NULL REFERENCES inbox_replies(id),
            request_id      INTEGER REFERENCES removal_requests(id),
            draft_body      TEXT NOT NULL,
            subject         TEXT NOT NULL DEFAULT '',
            created_at      TIMESTAMP NOT NULL DEFAULT (datetime('now')),
            sent_at         TIMESTAMP,
            account         TEXT
        );

        CREATE TABLE IF NOT EXISTS manual_tasks (
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
        );
        CREATE INDEX IF NOT EXISTS idx_manual_tasks_status
            ON manual_tasks(status);
        CREATE INDEX IF NOT EXISTS idx_manual_tasks_request
            ON manual_tasks(request_id);

        CREATE INDEX IF NOT EXISTS idx_removal_requests_campaign
            ON removal_requests(campaign_id);
        CREATE INDEX IF NOT EXISTS idx_removal_requests_broker
            ON removal_requests(broker_id);
        CREATE INDEX IF NOT EXISTS idx_removal_requests_jurisdiction
            ON removal_requests(jurisdiction);

        CREATE INDEX IF NOT EXISTS idx_inbox_replies_request
            ON inbox_replies(request_id);
        CREATE INDEX IF NOT EXISTS idx_inbox_replies_classified
            ON inbox_replies(classified_as);

        PRAGMA user_version = 1;
    """)
    conn.commit()
    _SCHEMA_VERSION_CACHE[db_key] = SCHEMA_VERSION
    return db_file


def with_db(func):
    """Decorator that ensures init_db() is called before the wrapped function.

    Eliminates redundant init_db() calls across service handlers.
    """

    def wrapper(*args, **kwargs):
        init_db()
        return func(*args, **kwargs)

    return wrapper
