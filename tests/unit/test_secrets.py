"""Tests for vault:// secret resolution."""

from __future__ import annotations

import logging
import os
import textwrap
from pathlib import Path
from unittest.mock import patch

import pytest

from symeraseme.core.secrets import (
    SecretResolutionError,
    _resolve_via_symvault,
    _symvault_available,
    resolve_secret,
)

# ---------------------------------------------------------------------------
# Helpers: fake symvault scripts
# ---------------------------------------------------------------------------


def _write_fake_symvault(tmp_path: Path, *, stdout: str = "my-secret\n", exit_code: int = 0) -> str:
    """Write a fake symvault script and return its directory."""
    script_dir = tmp_path / "bin"
    script_dir.mkdir(parents=True, exist_ok=True)
    script = script_dir / "symvault"
    script.write_text(
        textwrap.dedent(f"""\
            #!/bin/sh
            echo "{stdout.strip()}"
            exit {exit_code}
        """)
    )
    script.chmod(0o755)
    return str(script_dir)


def _write_fake_symvault_timeout(tmp_path: Path, delay: float = 10.0) -> str:
    """Write a fake symvault that sleeps forever (for timeout tests)."""
    script_dir = tmp_path / "bin"
    script_dir.mkdir(parents=True, exist_ok=True)
    script = script_dir / "symvault"
    script.write_text(
        textwrap.dedent(f"""\
            #!/bin/sh
            sleep {delay}
        """)
    )
    script.chmod(0o755)
    return str(script_dir)


# ---------------------------------------------------------------------------
# Basic resolution tests
# ---------------------------------------------------------------------------


class TestResolveSecretBasic:
    def test_literal_value_returned_unchanged(self):
        assert resolve_secret("sk-ant-12345") == "sk-ant-12345"

    def test_empty_vault_prefix_raises(self):
        with pytest.raises(SecretResolutionError, match="vault:// URI is empty"):
            resolve_secret("vault://")

    def test_non_vault_value_ignores_fallbacks(self):
        """A literal value never touches symvault, env, or keyring."""
        assert resolve_secret("direct-value", env_fallback="NONEXISTENT_VAR") == "direct-value"


# ---------------------------------------------------------------------------
# symvault resolution tests
# ---------------------------------------------------------------------------


class TestResolveViaSymvault:
    def test_success(self):
        """Successful symvault call returns stripped stdout."""
        import subprocess

        fake_result = subprocess.CompletedProcess(
            args=["symvault", "get", "github/token"],
            returncode=0,
            stdout=b"vault-secret-42\n",
            stderr=b"",
        )
        with (
            patch("symeraseme.core.secrets._symvault_available", return_value=True),
            patch("subprocess.run", return_value=fake_result),
        ):
            result = _resolve_via_symvault("github/token")
        assert result == "vault-secret-42"

    def test_exit_nonzero_returns_none(self):
        """Non-zero exit code is treated as failure (returns None)."""
        import subprocess

        fake_result = subprocess.CompletedProcess(
            args=["symvault", "get", "bad/path"],
            returncode=1,
            stdout=b"",
            stderr=b"error: not found",
        )
        with (
            patch("symeraseme.core.secrets._symvault_available", return_value=True),
            patch("subprocess.run", return_value=fake_result),
        ):
            result = _resolve_via_symvault("bad/path")
        assert result is None

    def test_symvault_not_installed(self):
        with patch("symeraseme.core.secrets._symvault_available", return_value=False):
            result = _resolve_via_symvault("any/path")
        assert result is None

    def test_timeout_returns_none(self):
        import subprocess as _subprocess

        with (
            patch("symeraseme.core.secrets._symvault_available", return_value=True),
            patch(
                "subprocess.run",
                side_effect=_subprocess.TimeoutExpired(cmd="symvault", timeout=5),
            ),
        ):
            result = _resolve_via_symvault("slow/path")
        assert result is None

    def test_empty_output_returns_none(self):
        """Empty stdout is treated as failure."""
        import subprocess

        fake_result = subprocess.CompletedProcess(
            args=["symvault", "get", "empty/path"],
            returncode=0,
            stdout=b"\n",
            stderr=b"",
        )
        with (
            patch("symeraseme.core.secrets._symvault_available", return_value=True),
            patch("subprocess.run", return_value=fake_result),
        ):
            result = _resolve_via_symvault("empty/path")
        assert result is None


# ---------------------------------------------------------------------------
# Full fallback chain tests
# ---------------------------------------------------------------------------


class TestResolveSecretFallbackChain:
    def test_vault_to_env(self, tmp_path, monkeypatch):
        """vault:// fails → env var succeeds."""
        monkeypatch.delenv("TEST_VAULT_FALLBACK", raising=False)
        monkeypatch.setenv("TEST_VAULT_FALLBACK", "env-fallback-value")

        with patch("symeraseme.core.secrets._resolve_via_symvault", return_value=None):
            result = resolve_secret(
                "vault://secret/path",
                env_fallback="TEST_VAULT_FALLBACK",
            )
        assert result == "env-fallback-value"

    def test_vault_to_keyring(self, tmp_path):
        """vault:// fails → env fails → keyring succeeds."""
        with (
            patch("symeraseme.core.secrets._resolve_via_symvault", return_value=None),
            patch("symeraseme.core.secrets._resolve_via_env", return_value=None),
            patch(
                "symeraseme.core.secrets._resolve_via_keyring",
                return_value="keyring-value",
            ),
        ):
            result = resolve_secret(
                "vault://secret/path",
                env_fallback="NONEXISTENT",
                keyring_service="symeraseme",
            )
        assert result == "keyring-value"

    def test_all_layers_fail_raises(self):
        """All layers exhausted → SecretResolutionError."""
        with (
            patch("symeraseme.core.secrets._resolve_via_symvault", return_value=None),
            patch("symeraseme.core.secrets._resolve_via_env", return_value=None),
            patch("symeraseme.core.secrets._resolve_via_keyring", return_value=None),
            pytest.raises(SecretResolutionError, match="Cannot resolve secret"),
        ):
            resolve_secret(
                "vault://secret/path",
                env_fallback="NONEXISTENT",
                keyring_service="symeraseme",
            )

    def test_vault_success_skips_env_and_keyring(self, tmp_path):
        """When vault:// succeeds, env and keyring are never touched."""
        with (
            patch("symeraseme.core.secrets._resolve_via_symvault", return_value="vault-ok"),
            patch("symeraseme.core.secrets._resolve_via_env") as mock_env,
            patch("symeraseme.core.secrets._resolve_via_keyring") as mock_kr,
        ):
            result = resolve_secret(
                "vault://secret/path",
                env_fallback="TEST",
                keyring_service="test",
            )
        assert result == "vault-ok"
        mock_env.assert_not_called()
        mock_kr.assert_not_called()


# ---------------------------------------------------------------------------
# No-leak test: secret must never appear in log output
# ---------------------------------------------------------------------------


class TestNoLeak:
    def test_secret_not_in_log_output(self, caplog):
        """The resolved secret value must not appear in any log record."""
        secret_value = "super-secret-api-key-12345"

        with (
            caplog.at_level(logging.DEBUG, logger="symeraseme.core.secrets"),
            patch(
                "symeraseme.core.secrets._resolve_via_symvault",
                return_value=secret_value,
            ),
        ):
            result = resolve_secret("vault://secret/path")

        assert result == secret_value

        # Check that the actual secret value does not appear in any log message.
        for record in caplog.records:
            assert secret_value not in record.getMessage(), (
                f"Secret leaked into log record: {record.getMessage()}"
            )

    def test_symvault_error_context_logged_but_not_secret(self, caplog):
        """symvault errors produce diagnostic log entries, but never the secret."""
        with (
            caplog.at_level(logging.DEBUG, logger="symeraseme.core.secrets"),
            patch("symeraseme.core.secrets._symvault_available", return_value=True),
            patch(
                "subprocess.run",
                return_value=__import__("subprocess").CompletedProcess(
                    args=["symvault", "get", "path"],
                    returncode=1,
                    stdout=b"",
                    stderr=b"error: vault corrupted",
                ),
            ),
        ):
            result = _resolve_via_symvault("path")

        assert result is None
        # Error context IS logged (diagnostic), but the actual secret is not.
        error_logged = any("exited with code 1" in r.getMessage() for r in caplog.records)
        assert error_logged, "Expected a warning about symvault failure"


# ---------------------------------------------------------------------------
# Integration: real subprocess with fake symvault on PATH
# ---------------------------------------------------------------------------


class TestSymvaultIntegration:
    def test_real_subprocess_with_fake_symvault(self, tmp_path):
        """End-to-end: a real subprocess call against a fake symvault script."""
        bin_dir = _write_fake_symvault(tmp_path, stdout="real-secret\n")
        original_path = os.environ.get("PATH", "")

        try:
            os.environ["PATH"] = f"{bin_dir}:{original_path}"
            result = _resolve_via_symvault("test/path")
        finally:
            os.environ["PATH"] = original_path

        assert result == "real-secret"

    def test_symvault_available_when_installed(self, tmp_path):
        bin_dir = _write_fake_symvault(tmp_path)
        original_path = os.environ.get("PATH", "")

        try:
            os.environ["PATH"] = f"{bin_dir}:{original_path}"
            assert _symvault_available() is True
        finally:
            os.environ["PATH"] = original_path

    def test_symvault_not_available_when_missing(self):
        original_path = os.environ.get("PATH", "")
        try:
            os.environ["PATH"] = ""
            assert _symvault_available() is False
        finally:
            os.environ["PATH"] = original_path
