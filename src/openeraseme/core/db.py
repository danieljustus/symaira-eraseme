"""SQLite connection management for the event store."""

from __future__ import annotations

import os
import sqlite3
import threading
from pathlib import Path

DEFAULT_DB_DIR = "~/.local/share/openeraseme"
DEFAULT_DB_NAME = "openeraseme.db"

_local = threading.local()


def _db_path(path: str | None = None) -> Path:
    if path:
        return Path(path).expanduser().resolve()
    db_dir = Path(os.environ.get("OPENERASEME_DB_DIR", DEFAULT_DB_DIR)).expanduser()
    db_dir.mkdir(parents=True, exist_ok=True)
    return db_dir / DEFAULT_DB_NAME


def get_connection(path: str | None = None) -> sqlite3.Connection:
    """Get a thread-local SQLite connection (singleton per thread)."""
    if not hasattr(_local, "conn") or _local.conn is None:
        db_file = _db_path(path)
        conn = sqlite3.connect(str(db_file))
        conn.row_factory = sqlite3.Row
        conn.execute("PRAGMA journal_mode=WAL")
        conn.execute("PRAGMA foreign_keys=ON")
        _local.conn = conn
    return _local.conn


def close_connection() -> None:
    if hasattr(_local, "conn") and _local.conn is not None:
        _local.conn.close()
        _local.conn = None


def init_db(path: str | None = None) -> Path:
    """Create the database schema if it does not exist.

    Returns the database file path.
    """
    db_file = _db_path(path)
    db_file.parent.mkdir(parents=True, exist_ok=True)

    conn = get_connection(str(db_file))
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
    """)
    conn.commit()
    return db_file
