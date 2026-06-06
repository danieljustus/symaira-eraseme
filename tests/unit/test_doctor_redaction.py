"""Tests for doctor command JSON output redaction."""

from __future__ import annotations

import json

import pytest
from typer.testing import CliRunner

from symeraseme.cli import app

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
