"""SQLite connection management for the event store.

When ``SYMERASEME_ENCRYPT_DB=1`` is set (or the database file is already
encrypted), the file is transparently encrypted at rest using AES-256-GCM
(Fernet) with a key derived from the identity master key in the system keyring.

The decrypted temp file is placed in a secure temporary directory with
restrictive permissions. On Linux ``/dev/shm`` (tmpfs, memory-backed) is used
when available. On macOS and other platforms the OS temp directory is used
(which may be disk-backed). A startup scavenger cleans up stale temp files
from previous aborted runs after a short grace period.

A startup scavenger cleans up any stale temp files from previous aborted runs.
"""

from __future__ import annotations

import atexit
import hashlib
import logging
import os
import platform
import secrets
import signal
import sqlite3
import sys
import tempfile
import threading
import time
from base64 import urlsafe_b64encode
from contextlib import contextmanager, suppress
from pathlib import Path

from cryptography.fernet import Fernet

from symeraseme.core.config import get_config

logger = logging.getLogger(__name__)

DEFAULT_DB_DIR = "~/.local/share/symeraseme"
DEFAULT_DB_NAME = "symeraseme.db"

_ENC_HEADER_V1 = b"SYMERASEME_ENCv1\n"
_ENC_MAGIC_V2 = b"SYMERASEME_ENCv2\n"
_ENC_SALT_LEN = 16
_PBKDF2_ITERATIONS = 600_000
_PBKDF2_FIXED_SALT = b"symeraseme-db-encryption-v1"

_DB_TEMP: dict[Path, Path] = {}
_FERNET_KEY_CACHE: dict[bytes | None, bytes] = {}
_STALE_SCAVENGE_AGE = 300

_local = threading.local()


def _get_secure_temp_dir() -> Path:
    try:
        uid = os.getuid()
    except AttributeError:
        uid = os.getpid()
    system = platform.system()
    if system == "Linux" and Path("/dev/shm").exists():
        secure_dir = Path("/dev/shm") / f"symeraseme-db-{uid}"
    elif system == "Darwin":
        # On macOS /tmp is not a RAM disk — it is persistent storage.
        # Use the standard temp directory (respects TMPDIR) and rely on
        # the short stale-scavenger window (300 s) plus atexit/SIGTERM
        # cleanup to minimise exposure.
        secure_dir = Path(tempfile.gettempdir()) / f"symeraseme-db-{uid}"
    else:
        # Windows and other platforms: fall back to the OS temp directory.
        # On Windows this is disk-backed; see the README security section
        # for mitigation strategies.
        secure_dir = Path(tempfile.gettempdir()) / f"symeraseme-db-{uid}"
    secure_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
    return secure_dir


def _scavenge_stale_temp_dbs() -> None:
    secure_dir = _get_secure_temp_dir()
    if not secure_dir.exists():
        return
    now = time.time()
    for entry in secure_dir.iterdir():
        if entry.name.startswith("symeraseme_decrypted_") and entry.is_file():
            age = now - entry.stat().st_mtime
            if age > _STALE_SCAVENGE_AGE:
                with suppress(OSError):
                    entry.unlink(missing_ok=True)


def _db_encryption_enabled() -> bool:
    val = os.environ.get("SYMERASEME_ENCRYPT_DB", "").strip().lower()
    return val in ("1", "true", "yes")


def _get_db_fernet_key(*, salt: bytes | None = None) -> bytes | None:
    cache_key = salt if salt else b""
    if cache_key in _FERNET_KEY_CACHE:
        return _FERNET_KEY_CACHE[cache_key]

    try:
        from symeraseme.core.identity import _get_existing_master_key

        master_key = _get_existing_master_key()
    except (ImportError, RuntimeError, OSError, ValueError) as exc:
        logger.debug("DB encryption key unavailable: %s", exc)
        return None

    derived = hashlib.pbkdf2_hmac(
        "sha256",
        master_key,
        salt if salt else _PBKDF2_FIXED_SALT,
        _PBKDF2_ITERATIONS,
    )
    key = urlsafe_b64encode(derived)
    _FERNET_KEY_CACHE[cache_key] = key
    return key


def _is_encrypted(path: Path) -> bool:
    if not path.exists() or path.stat().st_size == 0:
        return False
    head = path.read_bytes()[: max(len(_ENC_HEADER_V1), len(_ENC_MAGIC_V2) + _ENC_SALT_LEN)]
    return head.startswith(_ENC_HEADER_V1) or head.startswith(_ENC_MAGIC_V2)


def _migrate_v1_to_v2(path: Path) -> None:
    """Transparently re-encrypt a V1-format DB to V2 on open."""
    logger.info("Migrating V1-encrypted DB to V2 format: %s", path)
    tmp = _decrypt_to_temp(path)
    try:
        _encrypt_file(tmp, path)
        logger.info("V1→V2 migration complete: %s", path)
    finally:
        # Clean up the temporary decrypted file
        try:
            if tmp.exists() and tmp != path:
                tmp.unlink(missing_ok=True)
        except OSError as exc:
            logger.warning("Failed to remove temp file %s: %s", tmp, exc)


def _decrypt_to_temp(path: Path) -> Path:
    raw = path.read_bytes()
    if raw.startswith(_ENC_HEADER_V1):
        header_len = len(_ENC_HEADER_V1)
        salt = None
    elif raw.startswith(_ENC_MAGIC_V2):
        header_len = len(_ENC_MAGIC_V2) + _ENC_SALT_LEN
        salt = raw[len(_ENC_MAGIC_V2) : header_len]
    else:
        logger.debug("Unrecognized encryption header in %s", path)
        msg = "Unrecognized encryption header in database file."
        raise RuntimeError(msg)
    encrypted_data = raw[header_len:]
    fernet_key = _get_db_fernet_key(salt=salt)
    if fernet_key is None:
        logger.debug("Cannot decrypt DB at %s — master key unavailable", path)
        msg = (
            "Cannot decrypt database — identity master key is not available. "
            "Run `symeraseme init-profile` first."
        )
        raise RuntimeError(msg)
    f = Fernet(fernet_key)
    decrypted = f.decrypt(encrypted_data)

    secure_dir = _get_secure_temp_dir()
    with tempfile.NamedTemporaryFile(
        delete=False, suffix=".db", prefix="symeraseme_decrypted_", dir=secure_dir
    ) as tmp:
        tmp.write(decrypted)
        tmp_path = Path(tmp.name)
    os.chmod(tmp_path, 0o600)
    _DB_TEMP[path.resolve()] = tmp_path
    return tmp_path


def _encrypt_file(source: Path, target: Path) -> None:
    salt = secrets.token_bytes(_ENC_SALT_LEN)
    fernet_key = _get_db_fernet_key(salt=salt)
    if fernet_key is None:
        logger.warning("Cannot encrypt DB \u2014 identity master key is not available.")
        return
    f = Fernet(fernet_key)
    encrypted = f.encrypt(source.read_bytes())
    target.write_bytes(_ENC_MAGIC_V2 + salt + encrypted)


@atexit.register
def _cleanup_temp_files() -> None:
    for orig, tmp in list(_DB_TEMP.items()):
        try:
            if tmp.exists():
                _encrypt_file(tmp, orig)
        except (OSError, RuntimeError, ValueError) as exc:
            logger.warning("Failed to re-encrypt DB %s: %s", orig, exc)
        finally:
            try:
                if tmp.exists():
                    tmp.unlink(missing_ok=True)
                for suffix in ("-wal", "-shm"):
                    sibling = tmp.with_suffix(tmp.suffix + suffix)
                    if sibling.exists():
                        sibling.unlink(missing_ok=True)
            except OSError as exc:
                logger.warning("Failed to remove temp file %s: %s", tmp, exc)
    _DB_TEMP.clear()


def _handle_sigterm(signum: int, frame: object) -> None:
    """SIGTERM handler that cleans up temp files before exit."""
    logger.info("Received signal %d, cleaning up encrypted DB temp files ...", signum)
    _cleanup_temp_files()
    sys.exit(128 + signum)


# Register SIGTERM handler (SIGKILL cannot be caught, but SIGTERM can)
signal.signal(signal.SIGTERM, _handle_sigterm)


def _db_path(path: str | None = None) -> Path:
    if path:
        return Path(path).expanduser().resolve()
    config = get_config()
    db_dir = config.db_dir
    db_dir.mkdir(parents=True, exist_ok=True)
    return config.db_path


def get_connection(path: str | None = None) -> sqlite3.Connection:
    requested_path = str(_db_path(path))
    if (
        not hasattr(_local, "conn")
        or _local.conn is None
        or not hasattr(_local, "db_path")
        or _local.db_path != requested_path
    ):
        _scavenge_stale_temp_dbs()
        db_file = _db_path(path)

        should_encrypt = _db_encryption_enabled() or _is_encrypted(db_file)

        if should_encrypt and db_file.exists() and _is_encrypted(db_file):
            raw = db_file.read_bytes()
            if raw.startswith(_ENC_HEADER_V1):
                _migrate_v1_to_v2(db_file)
            db_file = _decrypt_to_temp(db_file)
        elif should_encrypt and not db_file.exists():
            _DB_TEMP[db_file.resolve()] = db_file

        conn = sqlite3.connect(str(db_file))
        conn.row_factory = sqlite3.Row
        if should_encrypt:
            conn.execute("PRAGMA journal_mode=DELETE")
        else:
            conn.execute("PRAGMA journal_mode=WAL")
        conn.execute("PRAGMA foreign_keys=ON")
        _local.conn = conn
        _local.db_path = requested_path
    return _local.conn


def close_connection() -> None:
    if hasattr(_local, "conn") and _local.conn is not None:
        _local.conn.close()
        _local.conn = None

    for orig, tmp in list(_DB_TEMP.items()):
        try:
            if tmp.exists():
                _encrypt_file(tmp, orig)
        except (OSError, RuntimeError, ValueError) as exc:
            logger.warning("Failed to re-encrypt DB %s: %s", orig, exc)
        finally:
            try:
                if tmp.exists() and tmp != orig:
                    tmp.unlink(missing_ok=True)
                for suffix in ("-wal", "-shm"):
                    sibling = tmp.with_suffix(tmp.suffix + suffix)
                    if sibling.exists():
                        sibling.unlink(missing_ok=True)
            except OSError as exc:
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
    """)
    conn.commit()
    return db_file
