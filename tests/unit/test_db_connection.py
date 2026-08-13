"""Unit tests for SQLite connection lifecycle and path normalization.

Regression coverage for #670: config-derived and explicit-path DB lookups
must normalize to the same canonical path, so the thread-local connection is
reused instead of being silently replaced (and leaked) without a close.

The reuse tests force a symlink into the DB directory (``link -> real``)
because macOS system temp dirs (/var -> /private/var) are already canonical
inside pytest's tmp_path, which would otherwise mask the path mismatch.
"""

from __future__ import annotations

import gc
import os
import sqlite3
import sys
from collections.abc import Iterator
from pathlib import Path

import pytest

from symeraseme.core.db_connection import (
    _db_path,
    _local,
    close_connection,
    get_connection,
    init_db,
)


@pytest.fixture
def isolated_db(tmp_path: Path) -> Iterator[Path]:
    """Point the DB layer at an isolated temp dir and reset connection state."""
    old_dir = os.environ.get("SYMERASEME_DB_DIR")
    old_encrypt = os.environ.get("SYMERASEME_ENCRYPT_DB")
    os.environ["SYMERASEME_DB_DIR"] = str(tmp_path)
    os.environ["SYMERASEME_ENCRYPT_DB"] = ""
    close_connection()
    yield tmp_path
    close_connection()
    if old_dir is not None:
        os.environ["SYMERASEME_DB_DIR"] = old_dir
    else:
        os.environ.pop("SYMERASEME_DB_DIR", None)
    if old_encrypt is not None:
        os.environ["SYMERASEME_ENCRYPT_DB"] = old_encrypt
    else:
        os.environ.pop("SYMERASEME_ENCRYPT_DB", None)


@pytest.fixture
def symlinked_db_dir(tmp_path: Path) -> Path:
    """Create ``<tmp>/link`` -> ``<tmp>/real`` and point the DB layer at it.

    The config-derived path (``.../link/...``) and the resolved path
    (``.../real/...``) differ as strings unless ``_db_path`` normalizes them.
    """
    real = tmp_path / "real"
    real.mkdir()
    link = tmp_path / "link"
    link.symlink_to(real, target_is_directory=True)
    os.environ["SYMERASEME_DB_DIR"] = str(link)
    os.environ["SYMERASEME_ENCRYPT_DB"] = ""
    close_connection()
    yield link
    close_connection()


def test_db_path_normalization_is_consistent(isolated_db):
    """Config-derived and explicit-path lookups resolve to the same file.

    ``_db_path(None)`` used to return the unresolved ``config.db_path`` while
    ``_db_path(path)`` resolved symlinks (macOS /var -> /private/var). The
    mismatch made ``get_connection`` replace the thread-local connection
    without closing it, leaking sqlite3 connections (issue #670).
    """
    config_path = _db_path(None)
    assert config_path == config_path.resolve()
    assert _db_path(str(config_path)) == config_path


def test_get_connection_reuses_thread_local_connection(symlinked_db_dir):
    """A symlinked DB dir must not spawn a second connection."""
    first = get_connection(None)
    second = get_connection(str(_db_path(None)))
    assert first is second


def test_init_db_and_get_connection_share_the_same_connection(symlinked_db_dir):
    """init_db() and plain get_connection() must not churn connections."""
    init_db()
    assert get_connection() is get_connection(str(_db_path(None)))


def test_path_switch_closes_previous_connection(isolated_db, tmp_path: Path):
    """A genuine DB-path switch must close the old connection, not drop it.

    The symlink fix for #670 canonicalized alias paths; this covers the
    remaining case where the path really changes (e.g. SYMERASEME_DB_DIR is
    re-pointed mid-process). The previous thread-local connection must be
    closed via close_connection() instead of being replaced unclosed.
    """
    first = get_connection()
    assert first.execute("SELECT 1").fetchone() is not None

    other_dir = tmp_path / "other"
    other_dir.mkdir()
    os.environ["SYMERASEME_DB_DIR"] = str(other_dir)

    second = get_connection()
    assert second is not first
    with pytest.raises(sqlite3.ProgrammingError):
        first.execute("SELECT 1")


def test_close_connection_leaves_no_unclosed_connection(isolated_db):
    """close_connection() must fully release the thread-local connection.

    sqlite3.Connection emits ResourceWarning from ``__del__``, which bypasses
    the warnings pipeline (it surfaces via the unraisable hook instead), so
    intercept that hook while forcing collection.
    """
    init_db()
    close_connection()

    unraisable: list[BaseException] = []
    old_hook = sys.unraisablehook

    def _hook(unraisable_exc) -> None:
        if isinstance(unraisable_exc.exc_value, ResourceWarning):
            unraisable.append(unraisable_exc.exc_value)

    sys.unraisablehook = _hook
    try:
        gc.collect()
    finally:
        sys.unraisablehook = old_hook
    assert not unraisable, "unclosed SQLite connection detected after close"
    assert _local.conn is None
