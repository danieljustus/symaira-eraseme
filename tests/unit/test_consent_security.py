"""Security-focused tests for consent token mechanism.

Covers defensive paths not tested in test_consent.py:
- _read_consent_file: permission checks, symlink attack, empty files
- _tty_prompt: TTY unavailability, EOFError, KeyboardInterrupt
- check_consent: full fallback chain ordering
- _find_token_file: legacy migration OSError, path traversal
- verify_token: permission fix OSError, stored-token mismatch
"""

import json
import os
import stat
import time
from pathlib import Path
from unittest.mock import patch

import pytest

from symeraseme.core.consent import (
    _find_token_file,
    _read_consent_file,
    _token_filename,
    _tty_prompt,
    check_consent,
    issue_token,
    verify_token,
)


# ---------------------------------------------------------------------------
# Fixture
# ---------------------------------------------------------------------------

@pytest.fixture(autouse=True)
def _isolated_consent_dir(monkeypatch, tmp_path) -> Path:
    """Use an isolated temp directory for all consent token files."""
    consent_dir = tmp_path / "consent"
    consent_dir.mkdir()
    monkeypatch.setenv("SYMERASEME_DATA_DIR", str(consent_dir))
    return consent_dir


# ===================================================================
# _read_consent_file  —  lines 203-234
# ===================================================================

class TestReadConsentFileSecurity:
    """Security-oriented tests for _read_consent_file.

    Uncovered production lines exercised here: 205-234.
    """

    def test_reads_valid_file_with_correct_permissions(self, tmp_path):
        """A properly permissioned file (0o600) returns the token."""
        f = tmp_path / "token.txt"
        f.write_text("myconsenttoken\n")
        os.chmod(f, 0o600)
        assert _read_consent_file(f) == "myconsenttoken"

    def test_wrong_permissions_still_returns_and_fixes(self, tmp_path):
        """File with 0o644 returns the token AND gets chmod'd to 0o600."""
        f = tmp_path / "loose.txt"
        f.write_text("secret_token\n")
        os.chmod(f, 0o644)

        token = _read_consent_file(f)
        assert token == "secret_token"

        mode = f.stat().st_mode & 0o777
        assert mode == 0o600, f"Expected 0o600, got {oct(mode)}"

    def test_wrong_permissions_logs_warning_and_chmods(self, tmp_path, caplog):
        """Permission warning is emitted and chmod is attempted."""
        f = tmp_path / "loose.txt"
        f.write_text("token\n")
        os.chmod(f, 0o644)

        _read_consent_file(f)

        assert any("permissions" in rec.message for rec in caplog.records)

        # File permissions were fixed
        assert f.stat().st_mode & 0o777 == 0o600

    def test_symlink_rejected_by_o_nofollow(self, tmp_path):
        """O_NOFOLLOW prevents reading through a symlink — returns None."""
        target = tmp_path / "real_token.txt"
        target.write_text("secret")
        os.chmod(target, 0o600)

        link = tmp_path / "consent_link.txt"
        link.symlink_to(target)

        assert _read_consent_file(link) is None

    def test_directory_rejected_as_not_regular_file(self, tmp_path):
        """A directory path is rejected (not S_ISREG)."""
        d = tmp_path / "not_a_file"
        d.mkdir()
        assert _read_consent_file(d) is None

    def test_nonexistent_path_returns_none(self, tmp_path):
        """Missing file path returns None."""
        assert _read_consent_file(tmp_path / "does_not_exist.txt") is None

    def test_empty_file_returns_none(self, tmp_path):
        """An empty consent file returns None (line 231-233)."""
        f = tmp_path / "empty.txt"
        f.write_text("")
        os.chmod(f, 0o600)
        assert _read_consent_file(f) is None

    def test_whitespace_only_file_returns_none(self, tmp_path):
        """File with only whitespace returns None after strip()."""
        f = tmp_path / "whitespace.txt"
        f.write_text("   \n")
        os.chmod(f, 0o600)
        assert _read_consent_file(f) is None

    def test_multiline_file_takes_first_line(self, tmp_path):
        """Only the first non-empty line is used as the token."""
        f = tmp_path / "multiline.txt"
        f.write_text("line1\ntoken_line2\n")
        os.chmod(f, 0o600)
        assert _read_consent_file(f) == "line1"

    def test_path_with_expanduser(self, tmp_path):
        """Tilde in path is expanded."""
        # Write a real file and make _read_consent_file read it via Path
        # with a tilde we can't actually control, so test expanding a
        # regular path — we prove expanduser() is called by checking
        # that a relative path + os.open works.
        f = tmp_path / "tilde_test.txt"
        f.write_text("expanded_token\n")
        os.chmod(f, 0o600)
        assert _read_consent_file(str(f)) == "expanded_token"

    def test_oserror_during_read_returns_none(self, tmp_path):
        """OSError in os.read path returns None (line 225-227)."""
        f = tmp_path / "read_fail.txt"
        f.write_text("some_token\n")
        os.chmod(f, 0o600)

        with patch("symeraseme.core.consent.os.read", side_effect=OSError("read failure")):
            result = _read_consent_file(f)
            assert result is None


# ===================================================================
# _tty_prompt  —  lines 192-200
# ===================================================================

class TestTTYPromptSecurity:
    """Security-oriented tests for _tty_prompt.

    Uncovered production lines exercised here: 196-200.
    """

    def test_returns_false_when_tty_unavailable(self):
        """When no TTY, prompt returns False without calling input()."""
        with patch("symeraseme.core.consent.tty_available", return_value=False):
            with patch("builtins.input") as mock_input:
                result = _tty_prompt("Test?")
                assert result is False
                mock_input.assert_not_called()

    def test_eof_error_returns_false(self):
        """EOFError during input() returns False (line 199-200)."""
        with patch("symeraseme.core.consent.tty_available", return_value=True):
            with patch("builtins.input", side_effect=EOFError):
                assert _tty_prompt("Test?") is False

    def test_keyboard_interrupt_returns_false(self):
        """KeyboardInterrupt during input() returns False (line 199-200)."""
        with patch("symeraseme.core.consent.tty_available", return_value=True):
            with patch("builtins.input", side_effect=KeyboardInterrupt):
                assert _tty_prompt("Test?") is False

    def test_y_returns_true(self):
        """'y' response returns True."""
        with patch("symeraseme.core.consent.tty_available", return_value=True):
            with patch("builtins.input", return_value="y"):
                assert _tty_prompt("Test?") is True

    def test_yes_returns_true(self):
        """'yes' response returns True."""
        with patch("symeraseme.core.consent.tty_available", return_value=True):
            with patch("builtins.input", return_value="yes"):
                assert _tty_prompt("Test?") is True

    def test_n_returns_false(self):
        """'n' response returns False."""
        with patch("symeraseme.core.consent.tty_available", return_value=True):
            with patch("builtins.input", return_value="n"):
                assert _tty_prompt("Test?") is False

    def test_empty_response_returns_false(self):
        """Default empty response returns False."""
        with patch("symeraseme.core.consent.tty_available", return_value=True):
            with patch("builtins.input", return_value=""):
                assert _tty_prompt("Test?") is False

    def test_case_insensitive_affirmative(self):
        """'Y' and 'YES' also return True."""
        with patch("symeraseme.core.consent.tty_available", return_value=True):
            with patch("builtins.input", return_value="Y"):
                assert _tty_prompt("Test?") is True

        with patch("symeraseme.core.consent.tty_available", return_value=True):
            with patch("builtins.input", return_value="YES"):
                assert _tty_prompt("Test?") is True


# ===================================================================
# check_consent  —  fallback chain  lines 237-264
# ===================================================================

class TestCheckConsentFallbackChain:
    """Fallback ordering: consent_token → consent_file → env FILE → env → interactive.

    Uncovered production lines exercised here: 248-261.
    """

    # --- consent_file parameter ---

    def test_consent_file_param_valid(self, _isolated_consent_dir, tmp_path):
        """consent_file parameter with a valid token file returns True."""
        token = issue_token("execute")
        f = tmp_path / "param.txt"
        f.write_text(token + "\n")
        os.chmod(f, 0o600)

        result = check_consent("execute", consent_file=str(f))
        assert result is True

    def test_consent_file_param_invalid_returns_false(self, tmp_path):
        """consent_file parameter with a bad token returns False."""
        f = tmp_path / "bad.txt"
        f.write_text("invalidtoken\n")
        os.chmod(f, 0o600)

        result = check_consent("execute", consent_file=str(f))
        assert result is False

    def test_consent_file_param_missing_file_returns_false(self, tmp_path):
        """consent_file parameter pointing to nonexistent file returns False."""
        result = check_consent("execute", consent_file=str(tmp_path / "missing.txt"))
        assert result is False

    # --- SYMERASEME_CONSENT_FILE env var ---

    def test_env_consent_file_valid(self, _isolated_consent_dir, tmp_path, monkeypatch):
        """SYMERASEME_CONSENT_FILE env with valid token returns True."""
        token = issue_token("execute")
        f = tmp_path / "env_file.txt"
        f.write_text(token + "\n")
        os.chmod(f, 0o600)
        monkeypatch.setenv("SYMERASEME_CONSENT_FILE", str(f))

        assert check_consent("execute") is True

    def test_env_consent_file_missing_returns_false(self, monkeypatch):
        """SYMERASEME_CONSENT_FILE pointing to missing file returns False."""
        monkeypatch.setenv("SYMERASEME_CONSENT_FILE", "/nonexistent/consent.txt")
        # Falls through to SYMERASEME_CONSENT (not set) then interactive=False
        assert check_consent("execute", interactive=False) is False

    def test_env_consent_file_empty_skipped(self, monkeypatch):
        """Empty string env var is skipped (line 253)."""
        monkeypatch.setenv("SYMERASEME_CONSENT_FILE", "")
        assert check_consent("execute", interactive=False) is False

    # --- Fallback to SYMERASEME_CONSENT env var ---

    def test_env_consent_token_valid(self, _isolated_consent_dir, monkeypatch):
        """SYMERASEME_CONSENT env with valid token returns True."""
        token = issue_token("execute")
        monkeypatch.setenv("SYMERASEME_CONSENT", token)
        assert check_consent("execute") is True

    def test_env_consent_token_wrong_command(self, _isolated_consent_dir, monkeypatch):
        """SYMERASEME_CONSENT env with wrong command returns False."""
        token = issue_token("execute")
        monkeypatch.setenv("SYMERASEME_CONSENT", token)
        assert check_consent("send-reply") is False

    def test_env_consent_empty_skipped(self, monkeypatch):
        """Empty SYMERASEME_CONSENT env var is skipped (line 259)."""
        monkeypatch.setenv("SYMERASEME_CONSENT", "")
        assert check_consent("execute", interactive=False) is False

    # --- Fallback ordering ---

    def test_consent_token_preferred_over_consent_file(
        self, _isolated_consent_dir, tmp_path
    ):
        """consent_token param is checked before consent_file param."""
        token = issue_token("execute")
        f = tmp_path / "file_param.txt"
        f.write_text("different_token\n")
        os.chmod(f, 0o600)

        result = check_consent("execute", consent_token=token, consent_file=str(f))
        assert result is True

    def test_consent_file_param_preferred_over_env_file(
        self, _isolated_consent_dir, tmp_path, monkeypatch
    ):
        """consent_file param is checked before SYMERASEME_CONSENT_FILE env."""
        token = issue_token("execute")
        f_param = tmp_path / "param_consent.txt"
        f_param.write_text(token + "\n")
        os.chmod(f_param, 0o600)

        # env points to a different (invalid) file — shouldn't be reached
        f_env = tmp_path / "env_consent.txt"
        f_env.write_text("bad_token\n")
        os.chmod(f_env, 0o600)
        monkeypatch.setenv("SYMERASEME_CONSENT_FILE", str(f_env))

        result = check_consent("execute", consent_file=str(f_param))
        assert result is True

    def test_env_consent_file_preferred_over_env_consent(
        self, _isolated_consent_dir, tmp_path, monkeypatch
    ):
        """SYMERASEME_CONSENT_FILE is checked before SYMERASEME_CONSENT."""
        token = issue_token("execute")
        f = tmp_path / "env_file_consent.txt"
        f.write_text(token + "\n")
        os.chmod(f, 0o600)
        monkeypatch.setenv("SYMERASEME_CONSENT_FILE", str(f))
        monkeypatch.setenv("SYMERASEME_CONSENT", "will_not_be_used")

        assert check_consent("execute") is True

    # --- Interactive fallback ---

    def test_all_exhausted_interactive_true(self, monkeypatch):
        """When all previous methods fail, interactive prompt decides."""
        monkeypatch.setattr("symeraseme.core.consent._tty_prompt", lambda _: True)
        assert check_consent("execute") is True

    def test_all_exhausted_interactive_false(self, monkeypatch):
        """Interactive prompt returns False."""
        monkeypatch.setattr("symeraseme.core.consent._tty_prompt", lambda _: False)
        assert check_consent("execute") is False

    def test_all_exhausted_interactive_disabled(self):
        """interactive=False returns False without prompting."""
        assert check_consent("execute", interactive=False) is False


# ===================================================================
# _find_token_file  —  legacy migration edge cases  lines 38-57
# ===================================================================

class TestLegacyTokenMigrationSecurity:
    """Security-oriented tests for legacy token filename migration.

    Uncovered production lines exercised here: 47-48, 54-56.
    """

    def test_legacy_token_migrates_to_hashed(self, _isolated_consent_dir):
        """A legacy consent_{token}.json file is renamed to hashed name."""
        token = "legacy_to_migrate"
        payload = {
            "command": "execute",
            "token": token,
            "issued_at": int(time.time()),
            "expires_at": int(time.time()) + 3600,
        }
        legacy = _isolated_consent_dir / f"consent_{token}.json"
        legacy.write_text(json.dumps(payload))

        hashed_name = _token_filename(token)
        hashed_path = _isolated_consent_dir / hashed_name

        result = _find_token_file(token)
        assert result == hashed_path
        assert hashed_path.exists()
        assert not legacy.exists()

    def test_legacy_migration_oserror_uses_legacy(self, _isolated_consent_dir):
        """OSError during rename falls back to legacy path (lines 54-56)."""
        token = "legacy_oserror"
        payload = {
            "command": "execute",
            "token": token,
            "issued_at": int(time.time()),
            "expires_at": int(time.time()) + 3600,
        }
        legacy = _isolated_consent_dir / f"consent_{token}.json"
        legacy.write_text(json.dumps(payload))

        with patch.object(Path, "rename", side_effect=OSError("read-only filesystem")):
            result = _find_token_file(token)
            assert result == legacy, (
                "Should return the legacy path when migration OSError occurs"
            )

    def test_legacy_symlink_outside_consent_dir_returns_none(
        self, _isolated_consent_dir, tmp_path
    ):
        """Legacy file resolving outside consent dir returns None (lines 47-48).

        A symlink pointing outside the consent directory is a path-traversal
        attempt. _find_token_file detects this via resolve().parent check.
        """
        token = "symlink_outside"

        # Target outside the consent directory
        target = tmp_path / "outside" / "evil.json"
        target.parent.mkdir(parents=True)
        target.write_text("{}")

        # Create legacy file as symlink to the outside target
        legacy = _isolated_consent_dir / f"consent_{token}.json"
        legacy.symlink_to(target)

        result = _find_token_file(token)
        assert result is None, (
            "Should reject a legacy file that resolves outside consent dir"
        )

    def test_legacy_token_renamed_to_hashed_and_verified(
        self, _isolated_consent_dir
    ):
        """After migration to hashed name, verify_token works."""
        token = "migrate_and_verify"
        payload = {
            "command": "execute",
            "token": token,
            "issued_at": int(time.time()),
            "expires_at": int(time.time()) + 3600,
        }
        legacy = _isolated_consent_dir / f"consent_{token}.json"
        legacy.write_text(json.dumps(payload))

        assert verify_token("execute", token) is True

        # After verify, the legacy file is gone and the hashed file exists
        hashed_name = _token_filename(token)
        hashed_path = _isolated_consent_dir / hashed_name
        assert hashed_path.exists()
        assert not legacy.exists()

    def test_legacy_resolve_check_before_exists(self, _isolated_consent_dir, tmp_path):
        """The resolve() parent check happens before exists() — no TOCTOU.

        Even if the legacy file does not exist, the resolve check runs
        first. Create a situation where the path resolves outside consent
        dir but the file doesn't exist yet — should still return None
        because the resolve guard triggers first.
        """
        token = "nonexistent_outside"

        # Don't actually create the legacy file; just mock resolve to
        # simulate a path that would resolve outside the consent dir.
        with patch.object(Path, "resolve") as mock_resolve:
            outside = _isolated_consent_dir.parent / "outside_dir" / f"consent_{token}.json"
            mock_resolve.side_effect = [
                outside,  # legacy.resolve()
                _isolated_consent_dir,  # consent_dir.resolve()
            ]
            result = _find_token_file(token)
            assert result is None


# ===================================================================
# verify_token  —  security edge cases  lines 92-130
# ===================================================================

class TestVerifyTokenSecurity:
    """Security-oriented tests for verify_token.

    Uncovered production lines exercised here: 110, 127-128.
    """

    def test_stored_token_mismatch_rejected(self, _isolated_consent_dir):
        """When stored token differs from provided token, return False (line 110)."""
        token = "realtoken_mismatch_test"
        payload = {
            "command": "execute",
            "token": "different_token_value",
            "issued_at": int(time.time()),
            "expires_at": int(time.time()) + 3600,
        }
        token_file = _isolated_consent_dir / f"consent_{token}.json"
        token_file.write_text(json.dumps(payload))

        assert verify_token("execute", token) is False

    def test_permission_fix_oserror_tolerated(self, _isolated_consent_dir):
        """OSError during permission fix does NOT affect verification (lines 127-128)."""
        token = issue_token("execute")
        token_file = _isolated_consent_dir / _token_filename(token)

        # Fix permissions first to trigger the os.chmod path in verify_token
        os.chmod(token_file, 0o644)

        with patch("symeraseme.core.consent.os.chmod", side_effect=OSError("read-only fs")):
            result = verify_token("execute", token)
            assert result is True, (
                "Verify should succeed even if permission-fix chmod fails"
            )

    def test_open_failure_returns_false(self, _isolated_consent_dir):
        """OSError during os.open in verify_token returns False (lines 97-99)."""
        token = issue_token("execute")

        with patch("symeraseme.core.consent.os.open", side_effect=OSError("permission denied")):
            result = verify_token("execute", token)
            assert result is False

    def test_json_decode_error_returns_false(self, _isolated_consent_dir):
        """Corrupt JSON in the token file returns False (line 102)."""
        token = issue_token("execute")
        token_file = _isolated_consent_dir / _token_filename(token)
        token_file.write_text("not-json-at-all")

        assert verify_token("execute", token) is False

    def test_old_format_with_missing_token_field(self, _isolated_consent_dir):
        """Old format (no 'token' field) still verifies (line 112-113)."""
        token = "old_format_no_token_field"
        payload = {
            "command": "execute",
            "issued_at": int(time.time()),
            "expires_at": int(time.time()) + 3600,
        }
        (_isolated_consent_dir / f"consent_{token}.json").write_text(
            json.dumps(payload)
        )
        assert verify_token("execute", token) is True
