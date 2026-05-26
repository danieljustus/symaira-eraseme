"""SQLite connection management for the event store.

When ``SYMERASEME_ENCRYPT_DB=1`` is set (or the database file is already
encrypted), the file is transparently encrypted at rest using AES-256-GCM
(Fernet) with a key derived from the identity master key in the system keyring.
"""

from __future__ import annotations

import atexit
import hashlib
import logging
import os
import signal
import sqlite3
import tempfile
import threading
from base64 import urlsafe_b64encode
from contextlib import contextmanager
from pathlib import Path

from cryptography.fernet import Fernet

logger = logging.getLogger(__name__)

DEFAULT_DB_DIR = "~/.local/share/symeraseme"
DEFAULT_DB_NAME = "symeraseme.db"

_ENC_HEADER = b"SYMERASEME_ENCv1\n"

_DB_TEMP: dict[Path, Path] = {}

_local = threading.local()


def _db_encryption_enabled() -> bool:
    val = os.environ.get("SYMERASEME_ENCRYPT_DB", "").strip().lower()
    return val in ("1", "true", "yes")


def _get_db_fernet_key() -> bytes | None:
    try:
        from symeraseme.core.identity import _get_or_create_master_key

        master_key = _get_or_create_master_key()
    except Exception as exc:
        logger.debug("DB encryption key unavailable: %s", exc)
        return None

    derived = hashlib.pbkdf2_hmac(
        "sha256",
        master_key,
        b"symeraseme-db-encryption-v1",
        100_000,
    )
    return urlsafe_b64encode(derived)


def _is_encrypted(path: Path) -> bool:
    if not path.exists() or path.stat().st_size == 0:
        return False
    return path.read_bytes()[: len(_ENC_HEADER)] == _ENC_HEADER


def _decrypt_to_temp(path: Path) -> Path:
    raw = path.read_bytes()
    encrypted_data = raw[len(_ENC_HEADER) :]
    fernet_key = _get_db_fernet_key()
    if fernet_key is None:
        msg = (
            f"Cannot decrypt DB {path} \u2014 identity master key is not available. "
            "Run `symeraseme init-profile` first."
        )
        raise RuntimeError(msg)
    f = Fernet(fernet_key)
    decrypted = f.decrypt(encrypted_data)

    # Use the user's data directory instead of /tmp to reduce exposure
    # on shared systems if SIGKILL leaves the temp file behind.
    temp_dir = _db_path(str(path)).parent
    temp_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(delete=False, suffix=".db", dir=temp_dir) as tmp:
        tmp.write(decrypted)
        tmp_path = Path(tmp.name)
    os.chmod(tmp_path, 0o600)
    _DB_TEMP[path.resolve()] = tmp_path
    return tmp_path


def _encrypt_file(source: Path, target: Path) -> None:
    fernet_key = _get_db_fernet_key()
    if fernet_key is None:
        logger.warning("Cannot encrypt DB \u2014 identity master key is not available.")
        return
    f = Fernet(fernet_key)
    encrypted = f.encrypt(source.read_bytes())
    target.write_bytes(_ENC_HEADER + encrypted)


@atexit.register
def _cleanup_temp_files() -> None:
    for orig, tmp in list(_DB_TEMP.items()):
        try:
            if tmp.exists():
                _encrypt_file(tmp, orig)
        except Exception as exc:
            logger.warning("Failed to re-encrypt DB %s: %s", orig, exc)
        finally:
            try:
                if tmp.exists():
                    tmp.unlink(missing_ok=True)
            except Exception as exc:
                logger.warning("Failed to remove temp file %s: %s", tmp, exc)
    _DB_TEMP.clear()


def _handle_sigterm(signum: int, frame: object) -> None:
    """SIGTERM handler that cleans up temp files before exit."""
    logger.info("Received signal %d, cleaning up encrypted DB temp files ...", signum)
    _cleanup_temp_files()
    signal.signal(signum, signal.SIG_DFL)
    os.kill(os.getpid(), signum)


# Register SIGTERM handler (SIGKILL cannot be caught, but SIGTERM can)
signal.signal(signal.SIGTERM, _handle_sigterm)


def _db_path(path: str | None = None) -> Path:
    if path:
        return Path(path).expanduser().resolve()
    db_dir = Path(os.environ.get("SYMERASEME_DB_DIR", DEFAULT_DB_DIR)).expanduser()
    db_dir.mkdir(parents=True, exist_ok=True)
    return db_dir / DEFAULT_DB_NAME


def get_connection(path: str | None = None) -> sqlite3.Connection:
    if not hasattr(_local, "conn") or _local.conn is None:
        db_file = _db_path(path)

        should_encrypt = _db_encryption_enabled() or _is_encrypted(db_file)

        if should_encrypt and db_file.exists() and _is_encrypted(db_file):
            db_file = _decrypt_to_temp(db_file)
        elif should_encrypt and not db_file.exists():
            _DB_TEMP[db_file.resolve()] = db_file

        conn = sqlite3.connect(str(db_file))
        conn.row_factory = sqlite3.Row
        conn.execute("PRAGMA journal_mode=WAL")
        conn.execute("PRAGMA foreign_keys=ON")
        _local.conn = conn
        _local.db_path = str(_db_path(path)) if not should_encrypt else str(db_file)
    return _local.conn


def close_connection() -> None:
    if hasattr(_local, "conn") and _local.conn is not None:
        _local.conn.close()
        _local.conn = None

    for orig, tmp in list(_DB_TEMP.items()):
        try:
            if tmp.exists():
                _encrypt_file(tmp, orig)
        except Exception as exc:
            logger.warning("Failed to re-encrypt DB %s: %s", orig, exc)
        finally:
            try:
                if tmp.exists() and tmp != orig:
                    tmp.unlink(missing_ok=True)
            except Exception as exc:
                logger.warning("Failed to remove temp file %s: %s", tmp, exc)
    _DB_TEMP.clear()

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
    """)
    conn.commit()
    return db_file
