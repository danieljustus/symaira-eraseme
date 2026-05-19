"""Basic CLI smoke tests."""

from pathlib import Path

from typer.testing import CliRunner

from openeraseme.cli import app

runner = CliRunner()


def test_version() -> None:
    result = runner.invoke(app, ["version"])
    assert result.exit_code == 0
    assert "OpenEraseMe" in result.stdout


def test_help() -> None:
    result = runner.invoke(app, ["--help"])
    assert result.exit_code == 0
    assert "openeraseme" in result.stdout


class TestGrant:
    @staticmethod
    def _setup(monkeypatch, tmp_path) -> Path:
        consent_dir = tmp_path / "consent"
        consent_dir.mkdir()
        monkeypatch.setenv("OPENERASEME_DATA_DIR", str(consent_dir))
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
