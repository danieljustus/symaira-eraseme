"""Tests for the centralized configuration object."""

from __future__ import annotations

from pathlib import Path

from symeraseme.core.config import Config, get_config


class TestConfigDefaults:
    def test_default_data_dir(self):
        c = Config()
        assert c.data_dir == "~/.local/share/symeraseme"

    def test_default_config_dir(self):
        c = Config()
        assert c.config_dir == "~/.config/symeraseme"

    def test_default_db_name(self):
        c = Config()
        assert c.db_name == "symeraseme.db"


class TestConfigEnvOverrides:
    def test_data_dir_override(self, monkeypatch, tmp_path):
        monkeypatch.setenv("SYMERASEME_DATA_DIR", str(tmp_path))
        c = Config()
        assert c.resolved_data_dir == tmp_path

    def test_db_dir_uses_data_dir_when_no_override(self, monkeypatch, tmp_path):
        monkeypatch.setenv("SYMERASEME_DATA_DIR", str(tmp_path))
        c = Config()
        assert c.db_dir == tmp_path

    def test_db_dir_override(self, monkeypatch, tmp_path):
        db_dir = tmp_path / "db"
        monkeypatch.setenv("SYMERASEME_DB_DIR", str(db_dir))
        c = Config()
        assert c.db_dir == db_dir

    def test_identity_path_override(self, monkeypatch, tmp_path):
        identity = tmp_path / "identity.enc"
        monkeypatch.setenv("SYMERASEME_IDENTITY_PATH", str(identity))
        c = Config()
        assert c.identity_path == identity

    def test_identity_path_default(self, monkeypatch):
        monkeypatch.delenv("SYMERASEME_IDENTITY_PATH", raising=False)
        c = Config()
        assert c.identity_path == Path("~/.config/symeraseme/identity.enc").expanduser()


class TestConfigPaths:
    def test_db_path(self, monkeypatch, tmp_path):
        monkeypatch.setenv("SYMERASEME_DATA_DIR", str(tmp_path))
        c = Config()
        assert c.db_path == tmp_path / "symeraseme.db"

    def test_consent_dir_matches_data_dir(self, monkeypatch, tmp_path):
        monkeypatch.setenv("SYMERASEME_DATA_DIR", str(tmp_path))
        c = Config()
        assert c.consent_dir == tmp_path

    def test_resolved_config_dir_expands(self):
        c = Config()
        assert c.resolved_config_dir == Path("~/.config/symeraseme").expanduser()


class TestGetConfig:
    def test_returns_config_instance(self):
        c = get_config()
        assert isinstance(c, Config)

    def test_returns_fresh_instance(self):
        c1 = get_config()
        c2 = get_config()
        assert c1 is not c2
        assert c1 == c2
