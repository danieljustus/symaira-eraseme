"""Verify the JSON error envelope produced by service handlers.

The CliResult(success=False) envelope contract: any failed handler must
return a JSON envelope with ``"success": false`` and an ``"error"`` field,
never a bare ``typer.Exit(1)`` before the envelope is rendered.
"""

from __future__ import annotations

import json
from pathlib import Path

from typer.testing import CliRunner

from symeraseme.cli import app
from symeraseme.core.result_types import CliResult

runner = CliRunner()


def _setup(monkeypatch, tmp_path) -> Path:
    consent_dir = tmp_path / "consent"
    consent_dir.mkdir()
    monkeypatch.setenv("SYMERASEME_DATA_DIR", str(consent_dir))
    return consent_dir


def test_envelope_shape_on_success() -> None:
    r = CliResult(success=True, data={"message": "ok", "count": 3})
    parsed = json.loads(r.to_json())
    assert parsed["success"] is True
    assert "error" not in parsed
    assert parsed["message"] == "ok"
    assert parsed["count"] == 3


def test_envelope_shape_on_error() -> None:
    r = CliResult(success=False, error="something went wrong")
    parsed = json.loads(r.to_json())
    assert parsed["success"] is False
    assert parsed["error"] == "something went wrong"
    assert "message" not in parsed or parsed.get("message") == ""


def test_envelope_strips_message_when_error_set() -> None:
    r = CliResult(
        success=False,
        data={"message": "duplicate", "request_id": 5},
        error="duplicate",
    )
    parsed = json.loads(r.to_json())
    assert parsed == {"success": False, "error": "duplicate", "request_id": 5}


def test_grant_revoke_nonexistent_returns_json_error_envelope(monkeypatch, tmp_path) -> None:
    _setup(monkeypatch, tmp_path)
    result = runner.invoke(
        app,
        ["--output", "json", "grant", "--revoke", "nonexistent"],
    )
    assert result.exit_code != 0
    parsed = json.loads(result.stdout)
    assert parsed["success"] is False
    assert "error" in parsed
    assert "Token not found" in parsed["error"]
