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
from typing import IO

from cryptography.fernet import Fernet

from symeraseme.core.config import get_config

logger = logging.getLogger(__name__)

try:
    import fcntl

    _HAVE_FCNTL = True
except ImportError:
    _HAVE_FCNTL = False

DEFAULT_DB_DIR = "~/.local/share/symeraseme"
DEFAULT_DB_NAME = "symeraseme.db"

_ENC_HEADER_V1 = b"SYMERASEME_ENCv1\n"
_ENC_MAGIC_V2 = b"SYMERASEME_ENCv2\n"
_ENC_MAGIC_V3 = b"SYMERASEME_ENCv3\n"
_ENC_SALT_LEN = 16
_PBKDF2_ITERATIONS = 600_000
_PBKDF2_FIXED_SALT = b"symeraseme-db-encryption-v1"

_DB_TEMP: dict[Path, Path] = {}
_DB_INITIAL_DATA_HASH: dict[Path, str] = {}
_FERNET_KEY_CACHE: dict[bytes | None, bytes] = {}
_STALE_SCAVENGE_AGE = 300
_DB_LOCK_RETRY_ATTEMPTS = 3
_DB_LOCK_RETRY_DELAY = 1.0

_local = threading.local()
_db_lock_file: IO | None = None


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
    for entry in sorted(secure_dir.iterdir()):
        if not entry.name.startswith("symeraseme_decrypted_"):
            continue
        if not entry.is_file():
            continue
        age = now - entry.stat().st_mtime
        if age > _STALE_SCAVENGE_AGE:
            with suppress(OSError):
                entry.unlink(missing_ok=True)
            for suffix in ("-wal", "-shm"):
                sibling = entry.with_suffix(entry.suffix + suffix)
                with suppress(OSError):
                    sibling.unlink(missing_ok=True)


def _db_encryption_enabled() -> bool:
    val = os.environ.get("SYMERASEME_ENCRYPT_DB", "").strip().lower()
    return val in ("1", "true", "yes")


def _acquire_db_lock(db_path: Path, *, retry: bool = True) -> None:
    global _db_lock_file
    if not _HAVE_FCNTL or not _db_encryption_enabled():
        return
    lock_path = db_path.parent / f"{db_path.name}.lock"
    attempts = _DB_LOCK_RETRY_ATTEMPTS if retry else 1
    for attempt in range(attempts):
        lock_file = None
        try:
            lock_file = open(lock_path, "w")  # noqa: SIM115
            fcntl.flock(lock_file, fcntl.LOCK_EX | fcntl.LOCK_NB)
            _db_lock_file = lock_file
            return
        except OSError:
            if lock_file is not None:
                lock_file.close()
            if attempt < attempts - 1:
                time.sleep(_DB_LOCK_RETRY_DELAY)
            else:
                msg = (
                    "Another symaira-eraseme process is using the encrypted database. "
                    "Wait for it to finish or remove the stale lock file: "
                    f"{lock_path}"
                )
                raise RuntimeError(msg) from None
        finally:
            if lock_file is not None and lock_file is not _db_lock_file:
                lock_file.close()


def _release_db_lock() -> None:
    global _db_lock_file
    if _db_lock_file is None:
        return
    try:
        fcntl.flock(_db_lock_file, fcntl.LOCK_UN)
        _db_lock_file.close()
    except OSError:
        pass
    finally:
        _db_lock_file = None


def _get_db_fernet_key(*, salt: bytes | None = None, version: int = 2) -> bytes | None:
    cache_key = (salt, version)
    if cache_key in _FERNET_KEY_CACHE:
        return _FERNET_KEY_CACHE[cache_key]

    try:
        from symeraseme.core.identity import _get_existing_master_key

        master_key = _get_existing_master_key()
    except (ImportError, RuntimeError, OSError, ValueError) as exc:
        logger.debug("DB encryption key unavailable: %s", exc)
        return None

    if version >= 3 and salt:
        from cryptography.hazmat.primitives import hashes
        from cryptography.hazmat.primitives.kdf.hkdf import HKDF

        hkdf = HKDF(
            algorithm=hashes.SHA256(),
            length=32,
            salt=salt,
            info=b"symeraseme-db-encryption-v3",
        )
        derived = hkdf.derive(master_key)
    else:
        derived = hashlib.pbkdf2_hmac(
            "sha256",
            master_key,
            salt if salt else _PBKDF2_FIXED_SALT,
            _PBKDF2_ITERATIONS,
        )
    key = urlsafe_b64encode(derived)
    _FERNET_KEY_CACHE[(salt, version)] = key
    return key


def _is_encrypted(path: Path) -> bool:
    if not path.exists() or path.stat().st_size == 0:
        return False
    max_header = max(
        len(_ENC_HEADER_V1),
        len(_ENC_MAGIC_V2) + _ENC_SALT_LEN,
        len(_ENC_MAGIC_V3) + _ENC_SALT_LEN,
    )
    head = path.read_bytes()[:max_header]
    return (
        head.startswith(_ENC_HEADER_V1)
        or head.startswith(_ENC_MAGIC_V2)
        or head.startswith(_ENC_MAGIC_V3)
    )


def _migrate_v1_to_v2(path: Path) -> None:
    """Transparently re-encrypt a V1-format DB to V2 on open."""
    logger.info("Migrating V1-encrypted DB to V2 format: %s", path)
    tmp = _decrypt_to_temp(path)
    try:
        _encrypt_file(tmp, path)
        logger.info("V1→V2 migration complete: %s", path)
    finally:
        try:
            if tmp.exists() and tmp != path:
                tmp.unlink(missing_ok=True)
        except OSError as exc:
            logger.warning("Failed to remove temp file %s: %s", tmp, exc)


def _migrate_v2_to_v3(path: Path) -> None:
    """Transparently re-encrypt a V2-format DB to V3 on open."""
    logger.info("Migrating V2-encrypted DB to V3 format: %s", path)
    tmp = _decrypt_to_temp(path)
    try:
        _encrypt_file(tmp, path, version=3)
        logger.info("V2→V3 migration complete: %s", path)
    finally:
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
        version = 1
    elif raw.startswith(_ENC_MAGIC_V3):
        header_len = len(_ENC_MAGIC_V3) + _ENC_SALT_LEN
        salt = raw[len(_ENC_MAGIC_V3) : header_len]
        version = 3
    elif raw.startswith(_ENC_MAGIC_V2):
        header_len = len(_ENC_MAGIC_V2) + _ENC_SALT_LEN
        salt = raw[len(_ENC_MAGIC_V2) : header_len]
        version = 2
    else:
        logger.debug("Unrecognized encryption header in %s", path)
        msg = "Unrecognized encryption header in database file."
        raise RuntimeError(msg)
    encrypted_data = raw[header_len:]
    fernet_key = _get_db_fernet_key(salt=salt, version=version)
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
    _DB_INITIAL_DATA_HASH[tmp_path] = hashlib.sha256(tmp_path.read_bytes()).hexdigest()
    return tmp_path


def _encrypt_file(source: Path, target: Path, *, version: int = 3) -> None:
    salt = secrets.token_bytes(_ENC_SALT_LEN)
    fernet_key = _get_db_fernet_key(salt=salt, version=version)
    if fernet_key is None:
        logger.warning("Cannot encrypt DB \u2014 identity master key is not available.")
        return
    f = Fernet(fernet_key)
    encrypted = f.encrypt(source.read_bytes())
    if version >= 3:
        target.write_bytes(_ENC_MAGIC_V3 + salt + encrypted)
    else:
        target.write_bytes(_ENC_MAGIC_V2 + salt + encrypted)


def _checkpoint_and_cleanup_wal(db_path: Path) -> None:
    """Checkpoint WAL files so they can be safely removed."""
    for suffix in ("-wal", "-shm"):
        sibling = db_path.with_suffix(db_path.suffix + suffix)
        if sibling.exists():
            try:
                sibling.unlink(missing_ok=True)
            except OSError as exc:
                logger.warning("Failed to remove WAL sibling %s: %s", sibling, exc)


def _reencrypt_and_remove_temp(orig: Path, tmp: Path) -> None:
    try:
        if tmp.exists():
            try:
                conn = sqlite3.connect(str(tmp))
                conn.execute("PRAGMA wal_checkpoint(TRUNCATE)")
                conn.close()
            except OSError:
                pass
            current_hash = hashlib.sha256(tmp.read_bytes()).hexdigest()
            initial_hash = _DB_INITIAL_DATA_HASH.get(tmp)
            db_changed = initial_hash is None or current_hash != initial_hash
            if not db_changed:
                logger.debug("DB unchanged, skipping re-encryption: %s", orig)
            else:
                _encrypt_file(tmp, orig)
    except (OSError, RuntimeError, ValueError) as exc:
        logger.warning("Failed to re-encrypt DB %s: %s", orig, exc)
    finally:
        _DB_INITIAL_DATA_HASH.pop(tmp, None)
        try:
            if tmp.exists() and tmp != orig:
                tmp.unlink(missing_ok=True)
            _checkpoint_and_cleanup_wal(tmp)
        except OSError as exc:
            logger.warning("Failed to remove temp file %s: %s", tmp, exc)


@atexit.register
def _cleanup_temp_files() -> None:
    for orig, tmp in list(_DB_TEMP.items()):
        _reencrypt_and_remove_temp(orig, tmp)
    _DB_TEMP.clear()
    _release_db_lock()


def _handle_sigterm(signum: int, frame: object) -> None:
    """SIGTERM handler that exits via sys.exit, letting atexit clean up temp files."""
    logger.info("Received signal %d, exiting ...", signum)
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

        should_encrypt = _db_encryption_enabled()

        if should_encrypt:
            _acquire_db_lock(db_file, retry=True)

        if should_encrypt and db_file.exists():
            raw = db_file.read_bytes()
            is_enc = bool(raw) and (
                raw.startswith(_ENC_HEADER_V1)
                or raw.startswith(_ENC_MAGIC_V2)
                or raw.startswith(_ENC_MAGIC_V3)
            )
            if is_enc:
                if raw.startswith(_ENC_HEADER_V1):
                    _migrate_v1_to_v2(db_file)
                    _migrate_v2_to_v3(db_file)
                elif raw.startswith(_ENC_MAGIC_V2):
                    _migrate_v2_to_v3(db_file)
                db_file = _decrypt_to_temp(db_file)
            else:
                # Existing plaintext DB with encryption enabled — register for encrypt-on-close
                _DB_TEMP[db_file.resolve()] = db_file
        elif should_encrypt and not db_file.exists():
            _DB_TEMP[db_file.resolve()] = db_file

        conn = sqlite3.connect(str(db_file))
        conn.row_factory = sqlite3.Row
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
        _reencrypt_and_remove_temp(orig, tmp)
    _DB_TEMP.clear()

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


_SCHEMA_VERSION = 1


def init_db(path: str | None = None) -> Path:
    """Create the database schema if it does not exist.

    Returns the database file path.
    """
    db_file = _db_path(path)
    db_file.parent.mkdir(parents=True, exist_ok=True)

    conn = get_connection(str(db_file))

    current_version = conn.execute("PRAGMA user_version").fetchone()[0]
    if current_version >= _SCHEMA_VERSION:
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
    return db_file
