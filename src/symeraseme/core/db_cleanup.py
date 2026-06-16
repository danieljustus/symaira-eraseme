"""Database cleanup: scavenging, temp file management, and atexit handlers.

Handles stale temp file cleanup, WAL checkpoint removal, re-encryption
on close, file locking for encrypted DBs, and atexit/signal registration.
"""

from __future__ import annotations

import atexit
import hashlib
import logging
import signal
import sqlite3
import sys
import time
from contextlib import suppress
from pathlib import Path
from typing import IO

from symeraseme.core.db_encryption import (
    DB_INITIAL_DATA_HASH,
    DB_TEMP,
    _db_encryption_enabled,
    _get_secure_temp_dir,
)

logger = logging.getLogger(__name__)

try:
    import fcntl

    _HAVE_FCNTL = True
except ImportError:
    _HAVE_FCNTL = False

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

STALE_SCAVENGE_AGE = 300  # 5 minutes
_DB_LOCK_RETRY_ATTEMPTS = 3
_DB_LOCK_RETRY_DELAY = 1.0

# ---------------------------------------------------------------------------
# Mutable state
# ---------------------------------------------------------------------------

_db_lock_file: IO | None = None
_db_temp_locks: dict[Path, object] = {}
_db_temp_locks_lock = object()


# ---------------------------------------------------------------------------
# Stale temp file scavenger
# ---------------------------------------------------------------------------


def _scavenge_stale_temp_dbs() -> None:
    """Remove stale decrypted temp files from previous aborted runs."""
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
        if age > STALE_SCAVENGE_AGE:
            with suppress(OSError):
                entry.unlink(missing_ok=True)
            for suffix in ("-wal", "-shm"):
                sibling = entry.with_suffix(entry.suffix + suffix)
                with suppress(OSError):
                    sibling.unlink(missing_ok=True)


# ---------------------------------------------------------------------------
# WAL cleanup
# ---------------------------------------------------------------------------


def _checkpoint_and_cleanup_wal(db_path: Path) -> None:
    """Remove WAL siblings so they can be safely cleaned up."""
    for suffix in ("-wal", "-shm"):
        sibling = db_path.with_suffix(db_path.suffix + suffix)
        if sibling.exists():
            try:
                sibling.unlink(missing_ok=True)
            except OSError as exc:
                logger.warning("Failed to remove WAL sibling %s: %s", sibling, exc)


# ---------------------------------------------------------------------------
# Re-encrypt & remove
# ---------------------------------------------------------------------------


def _reencrypt_and_remove_temp(orig: Path, tmp: Path) -> None:
    """Re-encrypt *tmp* back to *orig* (if changed) and remove *tmp*."""
    from symeraseme.core import db as _db_mod

    try:
        if tmp.exists():
            try:
                conn = sqlite3.connect(str(tmp))
                conn.execute("PRAGMA wal_checkpoint(TRUNCATE)")
                conn.close()
            except OSError:
                pass
            current_hash = hashlib.sha256(tmp.read_bytes()).hexdigest()
            initial_hash = DB_INITIAL_DATA_HASH.get(tmp)
            db_changed = initial_hash is None or current_hash != initial_hash
            if not db_changed:
                logger.debug("DB unchanged, skipping re-encryption: %s", orig)
            else:
                _db_mod._encrypt_file(tmp, orig)
    except (OSError, RuntimeError, ValueError) as exc:
        logger.warning("Failed to re-encrypt DB %s: %s", orig, exc)
    finally:
        DB_INITIAL_DATA_HASH.pop(tmp, None)
        try:
            if tmp.exists() and tmp != orig:
                tmp.unlink(missing_ok=True)
            _checkpoint_and_cleanup_wal(tmp)
        except OSError as exc:
            logger.warning("Failed to remove temp file %s: %s", tmp, exc)


# ---------------------------------------------------------------------------
# File locking
# ---------------------------------------------------------------------------


def _acquire_db_lock(db_path: Path, *, retry: bool = True) -> None:
    """Acquire an exclusive file lock for encrypted DB access."""
    from symeraseme.core import db as _db_mod

    global _db_lock_file
    if not _HAVE_FCNTL or not _db_encryption_enabled():
        return
    lock_path = db_path.parent / f"{db_path.name}.lock"
    attempts = _db_mod._DB_LOCK_RETRY_ATTEMPTS if retry else 1
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
                    "Wait for it to finish, or remove the stale lock file:\n\n"
                    f"  rm {lock_path}\n\n"
                    "If no other process is running, the lock file may be stale from a "
                    "previous crash. You can safely remove it and retry."
                )
                raise RuntimeError(msg) from None
        finally:
            if lock_file is not None and lock_file is not _db_lock_file:
                lock_file.close()


def _release_db_lock() -> None:
    """Release the exclusive file lock."""
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


# ---------------------------------------------------------------------------
# atexit / signal cleanup
# ---------------------------------------------------------------------------


@atexit.register
def _cleanup_temp_files() -> None:
    """atexit handler: re-encrypt and remove all temp files, release locks."""
    for orig, tmp in list(DB_TEMP.items()):
        _reencrypt_and_remove_temp(orig, tmp)
    DB_TEMP.clear()
    _release_db_lock()


def _handle_sigterm(signum: int, frame: object) -> None:
    """SIGTERM handler that exits via sys.exit, letting atexit clean up."""
    logger.info("Received signal %d, exiting ...", signum)
    sys.exit(128 + signum)


# Register SIGTERM handler (SIGKILL cannot be caught, but SIGTERM can)
signal.signal(signal.SIGTERM, _handle_sigterm)
