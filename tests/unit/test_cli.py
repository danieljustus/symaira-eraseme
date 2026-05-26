"""Basic CLI smoke tests."""

from pathlib import Path

from typer.testing import CliRunner

from symeraseme.cli import app
from symeraseme.cli.types import CliResult

runner = CliRunner()


def test_version() -> None:
    result = runner.invoke(app, ["version"])
    assert result.exit_code == 0
    assert "Symaira EraseMe" in result.stdout


def test_help() -> None:
    result = runner.invoke(app, ["--help"])
    assert result.exit_code == 0
    assert "symeraseme" in result.stdout


class TestGrant:
    @staticmethod
    def _setup(monkeypatch, tmp_path) -> Path:
        consent_dir = tmp_path / "consent"
        consent_dir.mkdir()
        monkeypatch.setenv("SYMERASEME_DATA_DIR", str(consent_dir))
        return consent_dir

    def test_grant_issues_token(self, monkeypatch, tmp_path):
        self._setup(monkeypatch, tmp_path)
        result = runner.invoke(app, ["grant", "execute"])
        assert result.exit_code == 0
        assert "Consent token:" in result.stdout

    def test_grant_with_ttl(self, monkeypatch, tmp_path):
        self._setup(monkeypatch, tmp_path)
        result = runner.invoke(app, ["grant", "execute", "--ttl", "3600"])
        assert result.exit_code == 0
        assert "TTL: 3600s" in result.stdout

    def test_grant_list_empty(self, monkeypatch, tmp_path):
        self._setup(monkeypatch, tmp_path)
        result = runner.invoke(app, ["grant", "--list"])
        assert result.exit_code == 0
        assert "No active tokens" in result.stdout

    def test_grant_revoke_nonexistent(self, monkeypatch, tmp_path):
        self._setup(monkeypatch, tmp_path)
        result = runner.invoke(app, ["grant", "--revoke", "nonexistent"])
        assert result.exit_code != 0
        assert "Token not found" in result.stderr

    def test_grant_revoke_all_empty(self, monkeypatch, tmp_path):
        self._setup(monkeypatch, tmp_path)
        result = runner.invoke(app, ["grant", "--revoke-all"])
        assert result.exit_code == 0
        assert "No active tokens to revoke" in result.stdout


class TestCliResult:
    """Tests for the structured CliResult type."""

    def test_defaults(self) -> None:
        r = CliResult()
        assert r.success is True
        assert r.data == {}
        assert r.error is None
        assert r.message == ""

    def test_with_message(self) -> None:
        r = CliResult(data={"message": "done"})
        assert r.message == "done"

    def test_with_error(self) -> None:
        r = CliResult(success=False, error="something went wrong")
        assert r.success is False
        assert r.message == "something went wrong"

    def test_to_json_success(self) -> None:
        r = CliResult(data={"message": "ok", "count": 3})
        j = r.to_json()
        assert '"success": true' in j
        assert '"message": "ok"' in j
        assert '"count": 3' in j

    def test_to_json_error(self) -> None:
        r = CliResult(success=False, error="fail")
        j = r.to_json()
        assert '"success": false' in j
        assert '"error": "fail"' in j
