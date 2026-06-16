"""Database encryption: Fernet key derivation, encrypt/decrypt, and temp files.

Handles AES-256-GCM (Fernet) encryption at rest with PBKDF2/HKDF key
derivation from the identity master key. Manages encrypted file format
headers (V1/V2/V3), version migration, and the secure temporary directory
for decrypted copies.
"""

from __future__ import annotations

import hashlib
import logging
import os
import platform
import secrets
import tempfile
from base64 import urlsafe_b64encode
from pathlib import Path

from cryptography.fernet import Fernet

logger = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# Encryption constants
# ---------------------------------------------------------------------------

ENC_HEADER_V1 = b"SYMERASEME_ENCv1\n"
ENC_MAGIC_V2 = b"SYMERASEME_ENCv2\n"
ENC_MAGIC_V3 = b"SYMERASEME_ENCv3\n"
ENC_SALT_LEN = 16
PBKDF2_ITERATIONS = 600_000
PBKDF2_FIXED_SALT = b"symeraseme-db-encryption-v1"

# ---------------------------------------------------------------------------
# Shared mutable state (also used by db_cleanup / db_connection)
# ---------------------------------------------------------------------------

DB_TEMP: dict[Path, Path] = {}
DB_INITIAL_DATA_HASH: dict[Path, str] = {}
_FERNET_KEY_CACHE: dict[tuple[bytes | None, int], bytes] = {}


# ---------------------------------------------------------------------------
# Encryption helpers
# ---------------------------------------------------------------------------


def _db_encryption_enabled() -> bool:
    """Return ``True`` when ``SYMERASEME_ENCRYPT_DB`` is truthy."""
    val = os.environ.get("SYMERASEME_ENCRYPT_DB", "").strip().lower()
    return val in ("1", "true", "yes")


def _get_secure_temp_dir() -> Path:
    """Return a platform-appropriate secure temp directory.

    On Linux, ``/dev/shm`` (tmpfs, memory-backed) is preferred.  On macOS
    and other platforms the OS temp directory is used (which may be
    disk-backed).
    """
    try:
        uid = os.getuid()
    except AttributeError:
        uid = os.getpid()
    system = platform.system()
    if system == "Linux" and Path("/dev/shm").exists():
        secure_dir = Path("/dev/shm") / f"symeraseme-db-{uid}"
    elif system == "Darwin":
        if _db_encryption_enabled():
            logger.warning(
                "macOS: decrypted database files are stored in /tmp (disk-backed). "
                "Consider setting TMPDIR to a RAM disk (e.g., /dev/shm) for better security."
            )
        secure_dir = Path(tempfile.gettempdir()) / f"symeraseme-db-{uid}"
    else:
        # Windows and other platforms: fall back to the OS temp directory.
        secure_dir = Path(tempfile.gettempdir()) / f"symeraseme-db-{uid}"
    secure_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
    return secure_dir


def _get_db_fernet_key(*, salt: bytes | None = None, version: int = 2) -> bytes | None:
    """Derive a Fernet key from the identity master key.

    Uses PBKDF2 for V1/V2 and HKDF for V3.  Results are cached to avoid
    redundant derivation work.
    """
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
            salt if salt else PBKDF2_FIXED_SALT,
            PBKDF2_ITERATIONS,
        )
    key = urlsafe_b64encode(derived)
    _FERNET_KEY_CACHE[(salt, version)] = key
    return key


def _is_encrypted(path: Path) -> bool:
    """Check whether *path* has a recognised encryption header."""
    if not path.exists() or path.stat().st_size == 0:
        return False
    max_header = max(
        len(ENC_HEADER_V1),
        len(ENC_MAGIC_V2) + ENC_SALT_LEN,
        len(ENC_MAGIC_V3) + ENC_SALT_LEN,
    )
    head = path.read_bytes()[:max_header]
    return (
        head.startswith(ENC_HEADER_V1)
        or head.startswith(ENC_MAGIC_V2)
        or head.startswith(ENC_MAGIC_V3)
    )


def _decrypt_to_temp(path: Path) -> Path:
    """Decrypt an encrypted database file to a secure temp file.

    Returns the path to the decrypted temp file and registers the mapping
    in ``DB_TEMP`` / ``DB_INITIAL_DATA_HASH``.
    """
    # Lazy import: _get_db_fernet_key may be monkeypatched via
    # ``symeraseme.core.db._get_db_fernet_key`` in tests.
    from symeraseme.core import db as _db_mod

    raw = path.read_bytes()
    if raw.startswith(ENC_HEADER_V1):
        header_len = len(ENC_HEADER_V1)
        salt = None
        version = 1
    elif raw.startswith(ENC_MAGIC_V3):
        header_len = len(ENC_MAGIC_V3) + ENC_SALT_LEN
        salt = raw[len(ENC_MAGIC_V3) : header_len]
        version = 3
    elif raw.startswith(ENC_MAGIC_V2):
        header_len = len(ENC_MAGIC_V2) + ENC_SALT_LEN
        salt = raw[len(ENC_MAGIC_V2) : header_len]
        version = 2
    else:
        logger.debug("Unrecognized encryption header in %s", path)
        msg = "Unrecognized encryption header in database file."
        raise RuntimeError(msg)
    encrypted_data = raw[header_len:]
    fernet_key = _db_mod._get_db_fernet_key(salt=salt, version=version)
    if fernet_key is None:
        logger.debug("Cannot decrypt DB at %s — master key unavailable", path)
        msg = (
            "Cannot decrypt database — identity master key is not available. "
            "Run `symeraseme init-profile` or ensure your master key is in the system keyring."
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
    DB_TEMP[path.resolve()] = tmp_path
    DB_INITIAL_DATA_HASH[tmp_path] = hashlib.sha256(tmp_path.read_bytes()).hexdigest()
    return tmp_path


def _encrypt_file(source: Path, target: Path, *, version: int = 3) -> None:
    """Encrypt *source* into *target* using Fernet with a random salt."""
    from symeraseme.core import db as _db_mod

    salt = secrets.token_bytes(ENC_SALT_LEN)
    fernet_key = _db_mod._get_db_fernet_key(salt=salt, version=version)
    if fernet_key is None:
        logger.warning("Cannot encrypt DB \u2014 identity master key is not available.")
        return
    f = Fernet(fernet_key)
    encrypted = f.encrypt(source.read_bytes())
    if version >= 3:
        target.write_bytes(ENC_MAGIC_V3 + salt + encrypted)
    else:
        target.write_bytes(ENC_MAGIC_V2 + salt + encrypted)


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
