"""Tests for doctor command JSON output redaction."""

from __future__ import annotations

import json

import pytest
from typer.testing import CliRunner

from symeraseme.cli import app
from symeraseme.cli.commands import inspection_commands

runner = CliRunner()


class TestDoctorJsonRedaction:
    @pytest.fixture(autouse=True)
    def _clean_env(self, monkeypatch: pytest.MonkeyPatch):
        # Remove sensitive env vars before each test
        for var in ["IMAP_PASSWORD", "CAPSOLVER_API_KEY"]:
            monkeypatch.delenv(var, raising=False)
        yield

    def test_env_labels_present_in_json(self, monkeypatch: pytest.MonkeyPatch):
        """Allowed env labels appear in the JSON output."""
        monkeypatch.setenv("SYMERASEME_LLM_PROVIDER", "openai")

        result = runner.invoke(app, ["--output", "json", "doctor"])
        assert result.exit_code == 0

        data = json.loads(result.output)
        env_detail = data["checks"]["Environment"]["detail"]
        assert "LLM provider" in env_detail

    def test_sensitive_vars_not_exposed_in_json(self, monkeypatch: pytest.MonkeyPatch):
        """Secret env var names never appear in the JSON output."""
        monkeypatch.setenv("IMAP_PASSWORD", "super_secret_123")
        monkeypatch.setenv("CAPSOLVER_API_KEY", "cap_secret_456")
        monkeypatch.setenv("SYMERASEME_LLM_PROVIDER", "openai")

        result = runner.invoke(app, ["--output", "json", "doctor"])
        assert result.exit_code == 0

        output = result.output
        assert "IMAP_PASSWORD" not in output
        assert "CAPSOLVER_API_KEY" not in output
        assert "super_secret_123" not in output
        assert "cap_secret_456" not in output

    def test_credentials_shows_configured_not_values(self, monkeypatch: pytest.MonkeyPatch):
        """Environment check returns generic 'credentials: configured' message."""
        monkeypatch.setenv("IMAP_PASSWORD", "secret")
        monkeypatch.setenv("SYMERASEME_LLM_PROVIDER", "openai")

        result = runner.invoke(app, ["--output", "json", "doctor"])
        assert result.exit_code == 0

        data = json.loads(result.output)
        env_detail = data["checks"]["Environment"]["detail"]
        assert "credentials: configured" in env_detail
        assert "secret" not in env_detail

    def test_pattern_based_sensitive_vars_redacted(self, monkeypatch: pytest.MonkeyPatch):
        monkeypatch.setenv("SMTP_PASSWORD", "smtp_secret")
        monkeypatch.setenv("OAUTH_CLIENT_SECRET", "oauth_secret")
        monkeypatch.setenv("GITHUB_TOKEN", "gh_token")
        monkeypatch.setenv("ANTHROPIC_API_KEY", "anthropic_key")
        monkeypatch.setenv("SYMERASEME_LLM_PROVIDER", "openai")

        result = runner.invoke(app, ["--output", "json", "doctor"])
        assert result.exit_code == 0

        output = result.output
        assert "SMTP_PASSWORD" not in output
        assert "OAUTH_CLIENT_SECRET" not in output
        assert "GITHUB_TOKEN" not in output
        assert "ANTHROPIC_API_KEY" not in output
        assert "smtp_secret" not in output
        assert "oauth_secret" not in output
        assert "gh_token" not in output
        assert "anthropic_key" not in output

    def test_non_sensitive_vars_not_redacted(self, monkeypatch: pytest.MonkeyPatch):
        monkeypatch.setenv("SYMERASEME_LLM_PROVIDER", "openai")
        monkeypatch.setenv("HOME", "/Users/test")

        result = runner.invoke(app, ["--output", "json", "doctor"])
        assert result.exit_code == 0

        output = result.output
        assert "LLM provider" in output


class TestDoctorVerdict:
    """The overall environment verdict must ignore optional, not-configured features."""

    @staticmethod
    def _patch_checks(
        monkeypatch: pytest.MonkeyPatch,
        core_ok: bool = True,
    ) -> None:
        """Force every doctor check to a deterministic outcome."""
        for name in (
            "_check_python_version",
            "_check_deps",
            "_check_config",
            "_check_database",
            "_check_db_encryption",
            "_check_registry",
            "_check_env",
        ):
            monkeypatch.setattr(
                inspection_commands,
                name,
                lambda name=name: (core_ok, f"{name} (stubbed)"),
            )
        monkeypatch.setattr(
            inspection_commands,
            "_check_keyring",
            lambda: (False, "no secure keyring available"),
        )
        monkeypatch.setattr(
            inspection_commands,
            "_check_llm",
            lambda: (False, "provider=anthropic, ANTHROPIC_API_KEY=✗ (not set)"),
        )

    def test_healthy_install_without_optional_features_passes(
        self, monkeypatch: pytest.MonkeyPatch
    ):
        """Unset keyring/LLM must not flip a healthy install to 'failed'."""
        self._patch_checks(monkeypatch)

        result = runner.invoke(app, ["doctor"])
        assert result.exit_code == 0
        assert "Environment check passed" in result.stdout
        assert "○ optional Keyring" in result.stdout
        assert "○ optional LLM config" in result.stdout
        assert "✓ Python version" in result.stdout
        assert "  ✗ " not in result.stdout

        data = json.loads(runner.invoke(app, ["--output", "json", "doctor"]).output)
        assert data["ok"] is True
        assert data["checks"]["Keyring"]["ok"] is False
        assert data["checks"]["Keyring"]["optional"] is True
        assert data["checks"]["LLM config"]["optional"] is True
        assert "optional" not in data["checks"]["Python version"]

    def test_core_check_failure_still_fails(self, monkeypatch: pytest.MonkeyPatch):
        """A genuinely broken core check must keep the 'failed' verdict."""
        self._patch_checks(monkeypatch, core_ok=False)

        result = runner.invoke(app, ["doctor"])
        assert result.exit_code == 0
        assert "Environment check failed" in result.stdout
        assert "✗ Python version" in result.stdout
        assert "○ optional Keyring" in result.stdout

        data = json.loads(runner.invoke(app, ["--output", "json", "doctor"]).output)
        assert data["ok"] is False

    def test_configured_optional_features_show_checkmarks(self, monkeypatch: pytest.MonkeyPatch):
        """Configured optional features render as ✓ rows and never flip the verdict."""
        self._patch_checks(monkeypatch)
        monkeypatch.setattr(
            inspection_commands,
            "_check_keyring",
            lambda: (True, "KeyringBackend (available)"),
        )
        monkeypatch.setattr(
            inspection_commands,
            "_check_llm",
            lambda: (True, "provider=anthropic, ANTHROPIC_API_KEY=✓"),
        )

        result = runner.invoke(app, ["doctor"])
        assert result.exit_code == 0
        assert "Environment check passed" in result.stdout
        assert "✓ Keyring" in result.stdout
        assert "✓ LLM config" in result.stdout
        assert "○ optional" not in result.stdout
