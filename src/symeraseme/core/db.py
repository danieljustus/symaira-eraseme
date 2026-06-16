"""SQLite connection management for the event store.

When ``SYMERASEME_ENCRYPT_DB=1`` is set (or the database file is already
encrypted), the file is transparently encrypted at rest using AES-256-GCM
(Fernet) with a key derived from the identity master key in the system keyring.

.. note::

    Implementations live in :mod:`db_encryption`, :mod:`db_connection`,
    and :mod:`db_cleanup`.  This module re-exports every public name so
    that existing ``from symeraseme.core.db import ...`` statements keep
    working.
"""

from __future__ import annotations

from symeraseme.core.db_cleanup import (  # noqa: F401
    _DB_LOCK_RETRY_ATTEMPTS,
    _DB_LOCK_RETRY_DELAY,
    STALE_SCAVENGE_AGE,
    _acquire_db_lock,
    _cleanup_temp_files,
    _db_lock_file,
    _handle_sigterm,
    _reencrypt_and_remove_temp,
    _release_db_lock,
    _scavenge_stale_temp_dbs,
)
from symeraseme.core.db_connection import (  # noqa: F401
    _SCHEMA_VERSION_CACHE,
    SCHEMA_VERSION,
    _db_path,
    close_connection,
    connection_context,
    get_connection,
    init_db,
)
from symeraseme.core.db_encryption import (  # noqa: F401
    _FERNET_KEY_CACHE,
    DB_INITIAL_DATA_HASH,
    DB_TEMP,
    ENC_HEADER_V1,
    ENC_MAGIC_V2,
    ENC_MAGIC_V3,
    ENC_SALT_LEN,
    PBKDF2_FIXED_SALT,
    PBKDF2_ITERATIONS,
    _db_encryption_enabled,
    _decrypt_to_temp,
    _encrypt_file,
    _get_db_fernet_key,
    _get_secure_temp_dir,
    _is_encrypted,
    _migrate_v1_to_v2,
    _migrate_v2_to_v3,
)

DEFAULT_DB_DIR = "~/.local/share/symeraseme"
DEFAULT_DB_NAME = "symeraseme.db"

# Backward-compat aliases — tests and CLI commands import these by old names.
_DB_TEMP = DB_TEMP
_STALE_SCAVENGE_AGE = STALE_SCAVENGE_AGE
_SCHEMA_VERSION = SCHEMA_VERSION
_ENC_HEADER_V1 = ENC_HEADER_V1
_ENC_MAGIC_V2 = ENC_MAGIC_V2
_ENC_MAGIC_V3 = ENC_MAGIC_V3
_ENC_SALT_LEN = ENC_SALT_LEN
_PBKDF2_FIXED_SALT = PBKDF2_FIXED_SALT
_PBKDF2_ITERATIONS = PBKDF2_ITERATIONS
