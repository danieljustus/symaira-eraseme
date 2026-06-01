"""Tests for the consent token mechanism."""

import json
import os
import time
from pathlib import Path

import pytest

from symeraseme.core.consent import (
    TOKEN_TTL,
    _token_filename,
    check_consent,
    consume_token,
    issue_token,
    list_tokens,
    revoke_token,
    tty_available,
    verify_token,
)


@pytest.fixture(autouse=True)
def _isolated_consent_dir(monkeypatch, tmp_path) -> Path:
    """Use an isolated temp directory for all consent token files."""
    consent_dir = tmp_path / "consent"
    consent_dir.mkdir()
    monkeypatch.setenv("SYMERASEME_DATA_DIR", str(consent_dir))
    return consent_dir


class TestIssueToken:
    def test_issue_returns_token_string(self):
        token = issue_token("execute")
        assert isinstance(token, str)
        assert len(token) > 0

    def test_issue_stores_token_in_payload(self, _isolated_consent_dir):
        token = issue_token("execute")
        token_file = _isolated_consent_dir / _token_filename(token)
        payload = json.loads(token_file.read_text())
        assert payload["token"] == token

    def test_issue_creates_token_file(self, _isolated_consent_dir):
        token = issue_token("execute")
        token_file = _isolated_consent_dir / _token_filename(token)
        assert token_file.exists()

    def test_issue_stores_payload(self, _isolated_consent_dir):
        token = issue_token("send-reply", ttl=3600)
        token_file = _isolated_consent_dir / _token_filename(token)
        payload = json.loads(token_file.read_text())
        assert payload["command"] == "send-reply"
        assert payload["expires_at"] - payload["issued_at"] == 3600

    def test_issue_default_ttl(self):
        token = issue_token("execute")
        dir_path = Path(os.environ["SYMERASEME_DATA_DIR"])
        payload = json.loads((dir_path / _token_filename(token)).read_text())
        assert payload["expires_at"] - payload["issued_at"] == TOKEN_TTL

    def test_multiple_tokens_are_unique(self):
        t1 = issue_token("execute")
        t2 = issue_token("execute")
        assert t1 != t2

    def test_issue_sets_file_permissions(self, _isolated_consent_dir):
        token = issue_token("execute")
        token_file = _isolated_consent_dir / _token_filename(token)
        mode = token_file.stat().st_mode & 0o777
        assert mode == 0o600


class TestVerifyToken:
    def test_valid_token_returns_true(self):
        token = issue_token("execute")
        assert verify_token("execute", token) is True

    def test_wrong_command_returns_false(self):
        token = issue_token("execute")
        assert verify_token("send-reply", token) is False

    def test_nonexistent_token_returns_false(self):
        assert verify_token("execute", "nonexistent123456") is False

    def test_expired_token_returns_false(self, monkeypatch):
        token = issue_token("execute", ttl=1)
        _now = int(time.time())
        monkeypatch.setattr(time, "time", lambda: _now + 10)
        assert verify_token("execute", token) is False

    def test_expired_token_file_removed(self, _isolated_consent_dir, monkeypatch):
        token = issue_token("execute", ttl=1)
        _now = int(time.time())
        monkeypatch.setattr(time, "time", lambda: _now + 10)
        verify_token("execute", token)
        token_file = _isolated_consent_dir / _token_filename(token)
        assert not token_file.exists()

    def test_token_mismatch_rejected(self, _isolated_consent_dir):
        """Token stored in payload differs from token provided — forgery attempt."""
        # Create a token file with a stored token that differs from the filename
        token = "realtoken1234567890abc"
        payload = {
            "command": "execute",
            "token": "differenttoken12345678",
            "issued_at": int(time.time()),
            "expires_at": int(time.time()) + 3600,
        }
        token_file = _isolated_consent_dir / f"consent_{token}.json"
        token_file.write_text(json.dumps(payload))
        assert verify_token("execute", token) is False

    def test_old_format_token_still_works(self, _isolated_consent_dir):
        """Backward compat: token file without stored token field still verifies."""
        token = "oldformatok12345678"
        payload = {
            "command": "execute",
            "issued_at": int(time.time()),
            "expires_at": int(time.time()) + 3600,
        }
        token_file = _isolated_consent_dir / f"consent_{token}.json"
        token_file.write_text(json.dumps(payload))
        assert verify_token("execute", token) is True

    def test_verify_fixes_permissions(self, _isolated_consent_dir):
        """verify_token should fix permissions on existing token files."""
        token = issue_token("execute")
        token_file = _isolated_consent_dir / _token_filename(token)
        os.chmod(token_file, 0o644)
        assert verify_token("execute", token) is True
        mode = token_file.stat().st_mode & 0o777
        assert mode == 0o600


class TestConsumeToken:
    def test_consume_removes_token_file(self, _isolated_consent_dir):
        token = issue_token("execute")
        consume_token(token)
        token_file = _isolated_consent_dir / _token_filename(token)
        assert not token_file.exists()

    def test_consume_nonexistent_does_not_raise(self):
        consume_token("nonexistent123456")


class TestRevokeToken:
    def test_revoke_existing_returns_true(self):
        token = issue_token("execute")
        assert revoke_token(token) is True

    def test_revoke_existing_removes_file(self, _isolated_consent_dir):
        token = issue_token("execute")
        revoke_token(token)
        token_file = _isolated_consent_dir / _token_filename(token)
        assert not token_file.exists()

    def test_revoke_nonexistent_returns_false(self):
        assert revoke_token("nonexistent123456") is False

    def test_revoked_token_no_longer_valid(self):
        token = issue_token("execute")
        revoke_token(token)
        assert verify_token("execute", token) is False


class TestListTokens:
    def test_list_empty_when_no_tokens(self):
        assert list_tokens() == []

    def test_list_returns_active_tokens(self):
        token_a = issue_token("execute")
        token_b = issue_token("send-reply", ttl=7200)
        tokens = list_tokens()
        assert len(tokens) == 2
        token_ids = {t["token"] for t in tokens}
        assert token_a in token_ids
        assert token_b in token_ids

    def test_list_skips_expired_tokens(self, monkeypatch):
        issue_token("execute", ttl=1)
        _now = int(time.time())
        monkeypatch.setattr(time, "time", lambda: _now + 10)
        tokens = list_tokens()
        assert len(tokens) == 0

    def test_list_includes_metadata(self):
        token = issue_token("send-reply", ttl=3600)
        tokens = list_tokens()
        t = next(t for t in tokens if t["token"] == token)
        assert t["command"] == "send-reply"
        assert t["expires_at"] - t["issued_at"] == 3600

    def test_list_fixes_permissions(self, _isolated_consent_dir):
        """list_tokens should fix permissions on existing token files."""
        token = issue_token("execute")
        token_file = _isolated_consent_dir / _token_filename(token)
        os.chmod(token_file, 0o644)
        list_tokens()
        mode = token_file.stat().st_mode & 0o777
        assert mode == 0o600


class TestCheckConsent:
    def test_yes_flag_returns_true(self):
        assert check_consent("execute", yes=True) is True

    def test_valid_consent_token_returns_true(self):
        token = issue_token("execute")
        assert check_consent("execute", consent_token=token) is True

    def test_invalid_consent_token_returns_false(self):
        assert check_consent("execute", consent_token="badbadbadbad1234") is False

    def test_valid_env_var_returns_true(self, monkeypatch):
        token = issue_token("execute")
        monkeypatch.setenv("SYMERASEME_CONSENT", token)
        assert check_consent("execute") is True

    def test_wrong_command_env_var_returns_false(self, monkeypatch):
        token = issue_token("execute")
        monkeypatch.setenv("SYMERASEME_CONSENT", token)
        assert check_consent("send-reply") is False

    def test_no_token_returns_false_when_not_interactive(self, monkeypatch):
        monkeypatch.setattr("symeraseme.core.consent.tty_available", lambda: False)
        assert check_consent("execute", interactive=True) is False

    def test_no_token_returns_false_when_interactive_disabled(self):
        assert check_consent("execute", interactive=False) is False

    def test_interactive_affirmative(self, monkeypatch):
        monkeypatch.setattr("symeraseme.core.consent._tty_prompt", lambda _: True)
        assert check_consent("execute", interactive=True) is True

    def test_interactive_negative(self, monkeypatch):
        monkeypatch.setattr("symeraseme.core.consent._tty_prompt", lambda _: False)
        assert check_consent("execute", interactive=True) is False


class TestTTYAvailable:
    def test_tty_available_false_when_stdin_not_a_tty(self, monkeypatch):
        monkeypatch.setattr("sys.stdin.isatty", lambda: False)
        monkeypatch.setattr("sys.stdout.isatty", lambda: True)
        assert tty_available() is False

    def test_tty_available_false_when_stdout_not_a_tty(self, monkeypatch):
        monkeypatch.setattr("sys.stdin.isatty", lambda: True)
        monkeypatch.setattr("sys.stdout.isatty", lambda: False)
        assert tty_available() is False

    def test_tty_available_true_when_both_tty(self, monkeypatch):
        monkeypatch.setattr("sys.stdin.isatty", lambda: True)
        monkeypatch.setattr("sys.stdout.isatty", lambda: True)
        assert tty_available() is True


class TestEdgeCases:
    def test_token_file_with_missing_command(self, _isolated_consent_dir):
        """Token file without a command field should not verify."""
        token = "missingcmd12345678"
        payload = {"not_command": "execute", "expires_at": int(time.time()) + 3600}
        (_isolated_consent_dir / f"consent_{token}.json").write_text(json.dumps(payload))
        assert verify_token("execute", token) is False

    def test_token_file_with_missing_expiry(self, _isolated_consent_dir):
        """Token file without expires_at should be treated as expired."""
        token = "noexpiry12345678"
        payload = {"command": "execute"}
        (_isolated_consent_dir / f"consent_{token}.json").write_text(json.dumps(payload))
        assert verify_token("execute", token) is False

    def test_whitespace_token(self):
        assert verify_token("execute", "  ") is False
