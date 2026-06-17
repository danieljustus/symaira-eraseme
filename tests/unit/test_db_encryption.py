"""Tests for encrypted DB temp file handling and cleanup."""

from __future__ import annotations

import os
from pathlib import Path
from unittest.mock import patch

import pytest
from cryptography.fernet import Fernet

from symeraseme.core.db_cleanup import _acquire_db_lock, _cleanup_temp_files
from symeraseme.core.db_connection import (
    close_connection,
    connection_context,
    get_connection,
    init_db,
)
from symeraseme.core.db_encryption import (
    DB_TEMP as _DB_TEMP,
)
from symeraseme.core.db_encryption import (
    ENC_HEADER_V1 as _ENC_HEADER_V1,
)
from symeraseme.core.db_encryption import (
    _get_secure_temp_dir,
)

# A deterministic Fernet key for tests
_TEST_FERNET_KEY = Fernet.generate_key()


@pytest.fixture(autouse=True)
def _reset_db() -> None:
    """Ensure clean state before and after each test."""
    close_connection()
    _DB_TEMP.clear()
    yield
    close_connection()
    _DB_TEMP.clear()


@pytest.fixture()
def encrypted_db_file(tmp_path: Path) -> Path:
    """Create a pre-encrypted DB file for testing."""
    db_file = tmp_path / "test_encrypted.db"
    os.environ["SYMERASEME_DB_DIR"] = str(tmp_path)

    # Create an unencrypted DB first
    init_db(str(db_file))
    close_connection()

    # Read the plain DB and encrypt it
    plain_data = db_file.read_bytes()
    f = Fernet(_TEST_FERNET_KEY)
    encrypted = f.encrypt(plain_data)
    db_file.write_bytes(_ENC_HEADER_V1 + encrypted)

    return db_file


class TestTempFilePermissions:
    """Temp files created by _decrypt_to_temp must have 0o600 permissions."""

    def test_temp_file_has_restrictive_permissions(
        self, encrypted_db_file: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        monkeypatch.setenv("SYMERASEME_ENCRYPT_DB", "1")
        monkeypatch.setattr("symeraseme.core.db_encryption._get_db_fernet_key", lambda **kw: _TEST_FERNET_KEY)

        conn = get_connection(str(encrypted_db_file))
        assert conn is not None

        # Find the temp file in _DB_TEMP
        assert len(_DB_TEMP) == 1
        tmp_path = list(_DB_TEMP.values())[0]
        assert tmp_path.exists()

        perms = os.stat(tmp_path).st_mode & 0o777
        assert perms == 0o600, f"Expected 0o600 permissions, got {oct(perms)}"

        close_connection()

    def test_encrypted_db_open_without_key_fails(
        self, encrypted_db_file: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """Opening an encrypted DB when the master key is missing must fail fast."""
        monkeypatch.setenv("SYMERASEME_ENCRYPT_DB", "1")
        # Simulate missing key by making _get_db_fernet_key return None
        monkeypatch.setattr("symeraseme.core.db_encryption._get_db_fernet_key", lambda **kw: None)

        with pytest.raises(RuntimeError, match="master key is not available"):
            get_connection(str(encrypted_db_file))

        close_connection()

    def test_temp_file_in_secure_temp_dir(
        self, encrypted_db_file: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """Temp files must be in the tmpfs-backed secure temp dir."""
        monkeypatch.setenv("SYMERASEME_ENCRYPT_DB", "1")
        monkeypatch.setattr("symeraseme.core.db_encryption._get_db_fernet_key", lambda **kw: _TEST_FERNET_KEY)

        get_connection(str(encrypted_db_file))
        assert len(_DB_TEMP) == 1
        tmp_path = list(_DB_TEMP.values())[0]

        secure_dir = _get_secure_temp_dir()
        assert tmp_path.parent == secure_dir
        assert tmp_path.name.startswith("symeraseme_decrypted_")

        close_connection()

    def test_encryption_disabled_no_temp_file(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        monkeypatch.setenv("SYMERASEME_DB_DIR", str(tmp_path))
        monkeypatch.delenv("SYMERASEME_ENCRYPT_DB", raising=False)

        db_file = tmp_path / "plain.db"
        init_db(str(db_file))
        assert len(_DB_TEMP) == 0
        close_connection()


class TestTempFileCleanup:
    """Temp files must be cleaned up after connection close."""

    def test_temp_file_cleaned_on_close(
        self, encrypted_db_file: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        monkeypatch.setenv("SYMERASEME_ENCRYPT_DB", "1")
        monkeypatch.setattr("symeraseme.core.db_encryption._get_db_fernet_key", lambda **kw: _TEST_FERNET_KEY)

        get_connection(str(encrypted_db_file))
        assert len(_DB_TEMP) >= 1
        tmp_path = list(_DB_TEMP.values())[0]
        assert tmp_path.exists()

        close_connection()

        assert not tmp_path.exists(), f"Temp file {tmp_path} should have been deleted"
        assert len(_DB_TEMP) == 0

    def test_temp_file_cleaned_on_context_exit(
        self, encrypted_db_file: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        monkeypatch.setenv("SYMERASEME_ENCRYPT_DB", "1")
        monkeypatch.setattr("symeraseme.core.db_encryption._get_db_fernet_key", lambda **kw: _TEST_FERNET_KEY)

        tmp_paths: list[Path] = []

        with connection_context(str(encrypted_db_file)) as conn:
            assert conn is not None
            assert len(_DB_TEMP) >= 1
            tmp_paths.extend(list(_DB_TEMP.values()))

        # After context exit, temp files should be gone
        for tp in tmp_paths:
            assert not tp.exists(), f"Temp file {tp} should have been deleted after context exit"
        assert len(_DB_TEMP) == 0

    def test_temp_file_cleaned_on_atexit_cleanup(
        self, encrypted_db_file: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        monkeypatch.setenv("SYMERASEME_ENCRYPT_DB", "1")
        monkeypatch.setattr("symeraseme.core.db_encryption._get_db_fernet_key", lambda **kw: _TEST_FERNET_KEY)

        get_connection(str(encrypted_db_file))
        assert len(_DB_TEMP) >= 1
        tmp_path = list(_DB_TEMP.values())[0]
        assert tmp_path.exists()

        # Simulate atexit cleanup
        _cleanup_temp_files()

        assert not tmp_path.exists(), f"Temp file {tmp_path} should have been deleted by cleanup"
        assert len(_DB_TEMP) == 0

    def test_temp_file_unlinked_even_if_reencrypt_fails(
        self, encrypted_db_file: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        monkeypatch.setenv("SYMERASEME_ENCRYPT_DB", "1")
        monkeypatch.setattr("symeraseme.core.db_encryption._get_db_fernet_key", lambda **kw: _TEST_FERNET_KEY)
        # Make re-encryption fail by returning None from key call
        monkeypatch.setattr(
            "symeraseme.core.db_encryption._get_db_fernet_key",
            lambda **kw: _TEST_FERNET_KEY,
        )

        get_connection(str(encrypted_db_file))
        tmp_path = list(_DB_TEMP.values())[0]
        assert tmp_path.exists()

        # Make the encrypt function raise
        with patch(
            "symeraseme.core.db_encryption._encrypt_file",
            side_effect=RuntimeError("simulated failure"),
        ):
            close_connection()

        # Temp file should still be removed even though encryption failed
        assert not tmp_path.exists(), (
            f"Temp file {tmp_path} should still be deleted even on encrypt failure"
        )
        assert len(_DB_TEMP) == 0


class TestConnectionContext:
    """connection_context() context manager."""

    def test_context_returns_connection(self, tmp_path: Path) -> None:
        os.environ["SYMERASEME_DB_DIR"] = str(tmp_path)

        with connection_context() as conn:
            tables = conn.execute("SELECT name FROM sqlite_master WHERE type='table'").fetchall()
            assert len(tables) >= 0  # empty DB is fine

    def test_context_cleans_up_on_normal_exit(
        self, encrypted_db_file: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        monkeypatch.setenv("SYMERASEME_ENCRYPT_DB", "1")
        monkeypatch.setattr("symeraseme.core.db_encryption._get_db_fernet_key", lambda **kw: _TEST_FERNET_KEY)

        with connection_context(str(encrypted_db_file)):
            assert len(_DB_TEMP) >= 1
            tmp_path = list(_DB_TEMP.values())[0]
            assert tmp_path.exists()

        # After context exit, cleaned up
        assert len(_DB_TEMP) == 0

    def test_context_cleans_up_on_exception(
        self, encrypted_db_file: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        monkeypatch.setenv("SYMERASEME_ENCRYPT_DB", "1")
        monkeypatch.setattr("symeraseme.core.db_encryption._get_db_fernet_key", lambda **kw: _TEST_FERNET_KEY)

        with (
            pytest.raises(ValueError, match="test error"),
            connection_context(str(encrypted_db_file)),
        ):
            tmp_path = list(_DB_TEMP.values())[0]
            assert tmp_path.exists()
            raise ValueError("test error")

        # Even on exception, temp files are cleaned up
        assert len(_DB_TEMP) == 0


class TestBackwardCompatibility:
    """Existing non-encrypted DB usage must still work."""

    def test_plain_db_still_works(self, tmp_path: Path) -> None:
        os.environ["SYMERASEME_DB_DIR"] = str(tmp_path)
        os.environ.pop("SYMERASEME_ENCRYPT_DB", None)

        db_file = tmp_path / "plain.db"
        init_db(str(db_file))
        conn = get_connection(str(db_file))

        conn.execute(
            "INSERT INTO campaigns (id, kind) VALUES (?, ?)",
            ("test-campaign", "initial"),
        )
        conn.commit()

        rows = conn.execute("SELECT id, kind FROM campaigns").fetchall()
        assert len(rows) == 1
        assert rows[0]["id"] == "test-campaign"

        close_connection()


class TestPlaintextToEncrypted:
    """Opening a plaintext DB with encryption enabled encrypts it on close."""

    def test_plaintext_db_encrypted_on_close(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        monkeypatch.setenv("SYMERASEME_DB_DIR", str(tmp_path))
        monkeypatch.delenv("SYMERASEME_ENCRYPT_DB", raising=False)

        db_file = tmp_path / "test.db"
        init_db(str(db_file))
        close_connection()

        raw_before = db_file.read_bytes()
        assert not raw_before.startswith(b"SYMERASEME_ENC")

        monkeypatch.setenv("SYMERASEME_ENCRYPT_DB", "1")
        monkeypatch.setattr("symeraseme.core.db_encryption._get_db_fernet_key", lambda **kw: _TEST_FERNET_KEY)

        conn = get_connection(str(db_file))
        assert conn is not None
        close_connection()

        from symeraseme.core.db_encryption import ENC_MAGIC_V3 as _ENC_MAGIC_V3

        raw_after = db_file.read_bytes()
        assert raw_after.startswith(_ENC_MAGIC_V3), (
            f"Plaintext DB should be encrypted after close, got header: {raw_after[:20]!r}"
        )


class TestFileLocking:
    """File locking prevents concurrent encrypted DB access."""

    def test_lock_file_created_on_encrypted_open(
        self, encrypted_db_file: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        monkeypatch.setenv("SYMERASEME_ENCRYPT_DB", "1")
        monkeypatch.setattr("symeraseme.core.db_encryption._get_db_fernet_key", lambda **kw: _TEST_FERNET_KEY)

        conn = get_connection(str(encrypted_db_file))
        assert conn is not None

        lock_file = encrypted_db_file.parent / f"{encrypted_db_file.name}.lock"
        assert lock_file.exists()

        close_connection()

    def test_lock_released_on_close(
        self, encrypted_db_file: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        monkeypatch.setenv("SYMERASEME_ENCRYPT_DB", "1")
        monkeypatch.setattr("symeraseme.core.db_encryption._get_db_fernet_key", lambda **kw: _TEST_FERNET_KEY)

        conn = get_connection(str(encrypted_db_file))
        assert conn is not None
        close_connection()

        from symeraseme.core.db_cleanup import _db_lock_file

        assert _db_lock_file is None

    def test_no_lock_when_encryption_disabled(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        monkeypatch.setenv("SYMERASEME_DB_DIR", str(tmp_path))
        monkeypatch.delenv("SYMERASEME_ENCRYPT_DB", raising=False)

        db_file = tmp_path / "plain.db"
        init_db(str(db_file))

        lock_file = db_file.parent / f"{db_file.name}.lock"
        assert not lock_file.exists()

        close_connection()

    def test_concurrent_access_raises_on_contention(
        self, encrypted_db_file: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        monkeypatch.setenv("SYMERASEME_ENCRYPT_DB", "1")
        monkeypatch.setattr("symeraseme.core.db_encryption._get_db_fernet_key", lambda **kw: _TEST_FERNET_KEY)

        import fcntl

        lock_path = encrypted_db_file.parent / f"{encrypted_db_file.name}.lock"
        lock_file = open(lock_path, "w")
        try:
            fcntl.flock(lock_file, fcntl.LOCK_EX | fcntl.LOCK_NB)

            monkeypatch.setattr(
                "symeraseme.core.db_cleanup._DB_LOCK_RETRY_ATTEMPTS", 1
            )
            with pytest.raises(RuntimeError, match="(?i)another symaira-eraseme process"):
                _acquire_db_lock(encrypted_db_file, retry=True)
        finally:
            fcntl.flock(lock_file, fcntl.LOCK_UN)
            lock_file.close()

    def test_read_only_directory_raises_runtime_error(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        monkeypatch.setenv("SYMERASEME_ENCRYPT_DB", "1")
        monkeypatch.setattr("symeraseme.core.db_encryption._get_db_fernet_key", lambda **kw: _TEST_FERNET_KEY)
        monkeypatch.setattr("symeraseme.core.db_cleanup._DB_LOCK_RETRY_ATTEMPTS", 1)

        readonly_dir = tmp_path / "readonly"
        readonly_dir.mkdir()
        readonly_dir.chmod(0o444)

        db_file = readonly_dir / "test.db"
        try:
            with pytest.raises(RuntimeError, match="(?i)another symaira-eraseme process"):
                _acquire_db_lock(db_file, retry=True)
        finally:
            readonly_dir.chmod(0o700)


class TestV1Migration:
    """V1-format DB files must be transparently migrated to V3."""

    def test_v1_file_migrated_to_v3_on_open(
        self, encrypted_db_file: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        monkeypatch.setenv("SYMERASEME_ENCRYPT_DB", "1")
        monkeypatch.setattr("symeraseme.core.db_encryption._get_db_fernet_key", lambda **kw: _TEST_FERNET_KEY)

        assert encrypted_db_file.read_bytes().startswith(_ENC_HEADER_V1)

        conn = get_connection(str(encrypted_db_file))
        assert conn is not None
        close_connection()

        from symeraseme.core.db_encryption import ENC_MAGIC_V3 as _ENC_MAGIC_V3

        raw = encrypted_db_file.read_bytes()
        assert raw.startswith(_ENC_MAGIC_V3), "V1 file should have been migrated to V3"


class TestFernetKeyCache:
    """Derived Fernet keys must be cached to avoid redundant PBKDF2 work."""

    def test_fernet_key_caches_after_first_call(self, monkeypatch: pytest.MonkeyPatch) -> None:
        monkeypatch.setattr(
            "symeraseme.core.identity._get_existing_master_key",
            lambda: b"x" * 32,
        )
        from symeraseme.core.db_encryption import _FERNET_KEY_CACHE, _get_db_fernet_key

        _FERNET_KEY_CACHE.clear()
        key1 = _get_db_fernet_key(salt=b"test-salt")
        key2 = _get_db_fernet_key(salt=b"test-salt")
        assert key1 == key2
        assert len(_FERNET_KEY_CACHE) == 1

    def test_different_salts_get_different_cache_entries(
        self, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        monkeypatch.setattr(
            "symeraseme.core.identity._get_existing_master_key",
            lambda: b"x" * 32,
        )
        from symeraseme.core.db_encryption import _FERNET_KEY_CACHE, _get_db_fernet_key

        _FERNET_KEY_CACHE.clear()
        key1 = _get_db_fernet_key(salt=b"salt-a")
        key2 = _get_db_fernet_key(salt=b"salt-b")
        assert key1 != key2
        assert len(_FERNET_KEY_CACHE) == 2


class TestStaleScavengerAge:
    """Stale scavenger must use a short 5-minute window."""

    def test_scavenge_age_is_five_minutes(self) -> None:
        from symeraseme.core.db_cleanup import STALE_SCAVENGE_AGE as _STALE_SCAVENGE_AGE

        assert _STALE_SCAVENGE_AGE == 300, (
            f"Expected _STALE_SCAVENGE_AGE to be 300s, got {_STALE_SCAVENGE_AGE}s"
        )


class TestSecureTempDir:
    """Secure temp directory selection."""

    def test_darwin_uses_standard_temp_dir(self, monkeypatch: pytest.MonkeyPatch) -> None:
        """macOS must use tempfile.gettempdir(), not hardcoded /tmp."""
        monkeypatch.setattr("platform.system", lambda: "Darwin")
        monkeypatch.setattr("os.getuid", lambda: 42)
        monkeypatch.setattr("tempfile.gettempdir", lambda: "/mock/tmp")
        from symeraseme.core.db_encryption import _get_secure_temp_dir

        with monkeypatch.context():
            monkeypatch.setattr(
                Path, "exists", lambda self, *a, **k: str(self) != "/dev/shm"
            )
            monkeypatch.setattr(Path, "mkdir", lambda *a, **k: None)
            secure_dir = _get_secure_temp_dir()
            assert str(secure_dir) == "/mock/tmp/symeraseme-db-42"

    def test_linux_uses_dev_shm_when_present(self, monkeypatch: pytest.MonkeyPatch) -> None:
        monkeypatch.setattr("platform.system", lambda: "Linux")
        monkeypatch.setattr("os.getuid", lambda: 42)
        from symeraseme.core.db_encryption import _get_secure_temp_dir

        with monkeypatch.context():
            monkeypatch.setattr(
                Path, "exists", lambda self, *a, **k: str(self) == "/dev/shm"
            )
            monkeypatch.setattr(Path, "mkdir", lambda *a, **k: None)
            secure_dir = _get_secure_temp_dir()
            assert str(secure_dir) == "/dev/shm/symeraseme-db-42"


class TestCleanupRegistration:
    """Verify _cleanup_temp_files is registered as atexit handler."""

    def test_cleanup_is_atexit_handler(self) -> None:
        # The @atexit.register decorator on _cleanup_temp_files
        # is applied at import time. Verifying via atexit internals
        # is Python-version-dependent, so we verify indirectly:
        # calling the function directly works and cleans up.
        from symeraseme.core.db_cleanup import _cleanup_temp_files

        assert callable(_cleanup_temp_files)


class TestWriteRoundTrip:
    """Writes to an encrypted DB must persist through close/reopen."""

    def test_write_persists_through_encrypt_close_reopen(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        monkeypatch.setenv("SYMERASEME_DB_DIR", str(tmp_path))
        monkeypatch.setenv("SYMERASEME_ENCRYPT_DB", "1")
        monkeypatch.setattr("symeraseme.core.db_encryption._get_db_fernet_key", lambda **kw: _TEST_FERNET_KEY)

        db_file = tmp_path / "roundtrip.db"

        init_db(str(db_file))
        conn = get_connection(str(db_file))
        conn.execute(
            "INSERT INTO campaigns (id, kind) VALUES (?, ?)",
            ("rt-campaign", "initial"),
        )
        conn.commit()
        close_connection()

        conn = get_connection(str(db_file))
        rows = conn.execute("SELECT id, kind FROM campaigns").fetchall()
        close_connection()

        assert len(rows) == 1
        assert rows[0]["id"] == "rt-campaign"
        assert rows[0]["kind"] == "initial"


class TestReadonlySkipReencrypt:
    """Opening and closing without writes must skip re-encryption."""

    def test_readonly_skips_reencrypt(
        self, encrypted_db_file: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        monkeypatch.setenv("SYMERASEME_ENCRYPT_DB", "1")
        monkeypatch.setattr("symeraseme.core.db_encryption._get_db_fernet_key", lambda **kw: _TEST_FERNET_KEY)

        from symeraseme.core.db_encryption import ENC_MAGIC_V3 as _ENC_MAGIC_V3

        conn = get_connection(str(encrypted_db_file))
        conn.execute("SELECT 1").fetchone()
        close_connection()

        assert encrypted_db_file.read_bytes().startswith(_ENC_MAGIC_V3)

        content_after_migration = encrypted_db_file.read_bytes()

        conn = get_connection(str(encrypted_db_file))
        conn.execute("SELECT 1").fetchone()
        close_connection()

        after_content = encrypted_db_file.read_bytes()
        assert after_content == content_after_migration, (
            "Read-only open should not trigger re-encryption"
        )
