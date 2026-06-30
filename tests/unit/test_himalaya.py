from __future__ import annotations

import json
import os
import subprocess
import types
from datetime import UTC, datetime
from email.mime.text import MIMEText
from pathlib import Path
from unittest.mock import MagicMock, patch as mock_patch

import pytest

from symeraseme.adapters.email._types import Envelope, Message, SmtpConfig
from symeraseme.adapters.email.himalaya import (
    EmailMessage,
    HimalayaError,
    HimalayaNotInstalledError,
    SmtpError,
    _build_mime,
    _detect_himalaya_version,
    _extract_address,
    _is_v1_plus,
    _parse_date,
    _read_himalaya_account_email,
    _run_himalaya,
    get_email_backend,
    get_message,
    himalaya_available,
    hismalaya_available,
    list_messages,
    load_smtp_config,
    send_email,
    send_message,
    send_message_smtp,
    send_raw_email,
)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _mock_result(stdout: str = "", stderr: str = "", returncode: int = 0):
    return types.SimpleNamespace(stdout=stdout, stderr=stderr, returncode=returncode)


def _fake_envelope_json(n: int = 1) -> str:
    """Return a JSON array of n envelope dicts (v1.x format)."""
    items = []
    for i in range(1, n + 1):
        items.append({
            "id": str(i),
            "subject": f"Subject {i}",
            "from": {"name": f"Sender{i}", "addr": f"sender{i}@example.com"},
            "to": {"name": "Recipient", "addr": "user@example.com"},
            "date": f"2026-01-{15:02d}T{10 + i:02d}:00:00+0000",
            "flags": ["SEEN"] if i % 2 == 0 else [],
        })
    return json.dumps(items)


def _patch_version(major: int, minor: int, patch_version: int):
    """Return a context manager that patches _detect_himalaya_version to return a specific version."""
    return mock_patch(
        "symeraseme.adapters.email.himalaya._detect_himalaya_version",
        return_value=(major, minor, patch_version),
    )


def _patch_which():
    return mock_patch("symeraseme.adapters.email.himalaya.shutil.which", return_value="/usr/bin/himalaya")


# ---------------------------------------------------------------------------
# Version detection
# ---------------------------------------------------------------------------

class TestDetectHimalayaVersion:
    """Tests for _detect_himalaya_version."""

    def setup_method(self):
        """Clear lru_cache between tests."""
        _detect_himalaya_version.cache_clear()

    def teardown_method(self):
        _detect_himalaya_version.cache_clear()

    @_patch_which()
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_detects_v1_2_0(self, mock_run, _which):
        mock_run.return_value = _mock_result(stdout="himalaya 1.2.0\n")
        assert _detect_himalaya_version() == (1, 2, 0)

    @_patch_which()
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_detects_v0_x_with_v_prefix(self, mock_run, _which):
        mock_run.return_value = _mock_result(stdout="himalaya v0.3.1\n")
        assert _detect_himalaya_version() == (0, 3, 1)

    @_patch_which()
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_returns_0_0_0_on_unparseable_output(self, mock_run, _which):
        mock_run.return_value = _mock_result(stdout="unknown output\n")
        assert _detect_himalaya_version() == (0, 0, 0)

    @_patch_which()
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_returns_0_0_0_on_timeout(self, mock_run, _which):
        mock_run.side_effect = subprocess.TimeoutExpired(cmd="himalaya", timeout=10)
        assert _detect_himalaya_version() == (0, 0, 0)

    @_patch_which()
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_returns_0_0_0_on_os_error(self, mock_run, _which):
        mock_run.side_effect = OSError("no such binary")
        assert _detect_himalaya_version() == (0, 0, 0)


class TestIsV1Plus:
    def setup_method(self):
        _detect_himalaya_version.cache_clear()

    def teardown_method(self):
        _detect_himalaya_version.cache_clear()

    @_patch_which()
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_true_for_v1_2_0(self, mock_run, _which):
        mock_run.return_value = _mock_result(stdout="himalaya 1.2.0\n")
        assert _is_v1_plus() is True

    @_patch_which()
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_false_for_v0_3_1(self, mock_run, _which):
        mock_run.return_value = _mock_result(stdout="himalaya v0.3.1\n")
        assert _is_v1_plus() is False

    @_patch_which()
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_false_for_unparseable(self, mock_run, _which):
        mock_run.return_value = _mock_result(stdout="unknown\n")
        assert _is_v1_plus() is False


# ---------------------------------------------------------------------------
# _extract_address
# ---------------------------------------------------------------------------

class TestExtractAddress:
    def test_dict_with_name(self):
        assert _extract_address({"name": "Alice", "addr": "a@b.com"}) == "Alice"

    def test_dict_with_addr_only(self):
        assert _extract_address({"addr": "a@b.com"}) == "a@b.com"

    def test_dict_empty(self):
        assert _extract_address({}) == ""

    def test_string_value(self):
        assert _extract_address("plain@email.com") == "plain@email.com"

    def test_none_value(self):
        assert _extract_address(None) == ""

    def test_integer_value(self):
        assert _extract_address(42) == "42"


# ---------------------------------------------------------------------------
# _parse_date
# ---------------------------------------------------------------------------

class TestParseDate:
    def test_none_returns_none(self):
        assert _parse_date(None) is None

    def test_empty_string_returns_none(self):
        assert _parse_date("") is None

    def test_iso_datetime_string(self):
        result = _parse_date("2026-01-15T10:30:00+0000")
        assert result is not None
        assert result.year == 2026
        assert result.month == 1
        assert result.day == 15

    def test_datetime_object_passthrough(self):
        dt = datetime(2026, 6, 1, tzinfo=UTC)
        assert _parse_date(dt) is dt

    def test_unparseable_returns_none(self):
        assert _parse_date("not-a-date") is None


# ---------------------------------------------------------------------------
# _run_himalaya — command construction for v1.x vs v0.x
# ---------------------------------------------------------------------------

class TestRunHimalaya:
    """Test _run_himalaya command construction for v1.x and v0.x."""

    def setup_method(self):
        _detect_himalaya_version.cache_clear()

    def teardown_method(self):
        _detect_himalaya_version.cache_clear()

    @_patch_which()
    @_patch_version(1, 2, 0)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_v1_x_places_account_after_subcommand(self, mock_run, _ver, _which):
        mock_run.return_value = _mock_result(stdout="ok")
        _run_himalaya(["envelope", "list"], account="myaccount")
        cmd = mock_run.call_args[0][0]
        # v1.x: himalaya envelope --account myaccount list
        assert cmd == ["himalaya", "envelope", "--account", "myaccount", "list"]

    @_patch_which()
    @_patch_version(1, 2, 0)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_v1_x_no_account(self, mock_run, _ver, _which):
        mock_run.return_value = _mock_result(stdout="ok")
        _run_himalaya(["envelope", "list"])
        cmd = mock_run.call_args[0][0]
        assert cmd == ["himalaya", "envelope", "list"]

    @_patch_which()
    @_patch_version(1, 2, 0)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_v1_x_empty_args_with_account(self, mock_run, _ver, _which):
        mock_run.return_value = _mock_result(stdout="ok")
        _run_himalaya([], account="myaccount")
        cmd = mock_run.call_args[0][0]
        assert cmd == ["himalaya", "--account", "myaccount"]

    @_patch_which()
    @_patch_version(1, 2, 0)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_v1_x_with_config_path(self, mock_run, _ver, _which):
        mock_run.return_value = _mock_result(stdout="ok")
        _run_himalaya(["envelope", "list"], config_path="/etc/himalaya.toml")
        cmd = mock_run.call_args[0][0]
        assert cmd == ["himalaya", "--config", "/etc/himalaya.toml", "envelope", "list"]

    @_patch_which()
    @_patch_version(0, 3, 1)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_v0_x_places_account_globally(self, mock_run, _ver, _which):
        mock_run.return_value = _mock_result(stdout="ok")
        _run_himalaya(["list"], account="myaccount")
        cmd = mock_run.call_args[0][0]
        # v0.x: himalaya --account myaccount list
        assert cmd == ["himalaya", "--account", "myaccount", "list"]

    @_patch_which()
    @_patch_version(0, 3, 1)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_v0_x_no_account(self, mock_run, _ver, _which):
        mock_run.return_value = _mock_result(stdout="ok")
        _run_himalaya(["list"])
        cmd = mock_run.call_args[0][0]
        assert cmd == ["himalaya", "list"]

    @_patch_which()
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_timeout_raises_himalaya_error(self, mock_run, _which):
        mock_run.side_effect = subprocess.TimeoutExpired(cmd="himalaya", timeout=30)
        with pytest.raises(HimalayaError, match="timed out"):
            _run_himalaya(["list"])

    @_patch_which()
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_nonzero_exit_raises_himalaya_error(self, mock_run, _which):
        mock_run.return_value = _mock_result(stderr="permission denied", returncode=2)
        with pytest.raises(HimalayaError, match="permission denied"):
            _run_himalaya(["list"])


# ---------------------------------------------------------------------------
# list_messages — v1.x envelope list format
# ---------------------------------------------------------------------------

class TestListMessagesV1:
    def setup_method(self):
        _detect_himalaya_version.cache_clear()

    def teardown_method(self):
        _detect_himalaya_version.cache_clear()

    @_patch_which()
    @_patch_version(1, 2, 0)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_v1_x_uses_envelope_list_subcommand(self, mock_run, _ver, _which):
        mock_run.return_value = _mock_result(stdout=_fake_envelope_json(1))
        list_messages(folder="SENT", page_size=10, page=2)
        cmd = mock_run.call_args[0][0]
        assert "envelope" in cmd
        assert "list" in cmd
        assert "--folder" in cmd
        assert "SENT" in cmd
        assert "--page-size" in cmd
        assert "10" in cmd
        assert "--page" in cmd
        assert "2" in cmd
        assert "--output" in cmd
        assert "json" in cmd

    @_patch_which()
    @_patch_version(1, 2, 0)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_v1_x_parses_envelope_fields(self, mock_run, _ver, _which):
        mock_run.return_value = _mock_result(stdout=_fake_envelope_json(1))
        envelopes = list_messages()
        assert len(envelopes) == 1
        env = envelopes[0]
        assert env.id == "1"
        assert env.subject == "Subject 1"
        assert env.from_ == "Sender1"
        assert env.to == "Recipient"
        assert env.flags == []

    @_patch_which()
    @_patch_version(0, 3, 1)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_v0_x_uses_legacy_list_subcommand(self, mock_run, _ver, _which):
        mock_run.return_value = _mock_result(stdout=_fake_envelope_json(1))
        list_messages()
        cmd = mock_run.call_args[0][0]
        assert cmd[1] == "list"  # v0.x: himalaya list ...

    @_patch_which()
    @_patch_version(1, 2, 0)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_json_decode_error_raises(self, mock_run, _ver, _which):
        mock_run.return_value = _mock_result(stdout="not json {{{")
        with pytest.raises(HimalayaError, match="Failed to parse"):
            list_messages()

    @_patch_which()
    @_patch_version(1, 2, 0)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_empty_stdout_returns_empty_list(self, mock_run, _ver, _which):
        mock_run.return_value = _mock_result(stdout="")
        assert list_messages() == []

    @_patch_which()
    @_patch_version(1, 2, 0)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_v1_x_with_account_and_config(self, mock_run, _ver, _which):
        mock_run.return_value = _mock_result(stdout=_fake_envelope_json(1))
        list_messages(account="work", config_path="/tmp/cfg.toml")
        cmd = mock_run.call_args[0][0]
        assert "--account" in cmd
        assert "work" in cmd
        assert "--config" in cmd
        assert "/tmp/cfg.toml" in cmd


# ---------------------------------------------------------------------------
# get_message — v1.x vs v0.x
# ---------------------------------------------------------------------------

class TestGetMessage:
    def setup_method(self):
        _detect_himalaya_version.cache_clear()

    def teardown_method(self):
        _detect_himalaya_version.cache_clear()

    @_patch_which()
    @_patch_version(1, 2, 0)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_v1_x_uses_message_read(self, mock_run, _ver, _which):
        fake = {"id": "42", "subject": "Hi", "from": "a@b.com", "to": "c@d.com",
                "date": "2026-01-15T10:00:00+0000", "body": "hello", "flags": []}
        mock_run.return_value = _mock_result(stdout=json.dumps(fake))
        msg = get_message("42")
        cmd = mock_run.call_args[0][0]
        assert "message" in cmd
        assert "read" in cmd
        assert "42" in cmd
        assert "--output" in cmd
        assert "json" in cmd
        assert msg.id == "42"
        assert msg.body == "hello"

    @_patch_which()
    @_patch_version(0, 3, 1)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_v0_x_uses_legacy_get(self, mock_run, _ver, _which):
        fake = {"id": "42", "subject": "Hi", "from": "a@b.com", "to": "c@d.com",
                "date": "2026-01-15T10:00:00+0000", "body": "hello", "flags": []}
        mock_run.return_value = _mock_result(stdout=json.dumps(fake))
        get_message("42")
        cmd = mock_run.call_args[0][0]
        assert cmd[1] == "get"
        assert "42" in cmd

    @_patch_which()
    @_patch_version(1, 2, 0)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_empty_output_raises_not_found(self, mock_run, _ver, _which):
        mock_run.return_value = _mock_result(stdout="")
        with pytest.raises(HimalayaError, match="not found"):
            get_message("999")

    @_patch_which()
    @_patch_version(1, 2, 0)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_invalid_json_raises(self, mock_run, _ver, _which):
        mock_run.return_value = _mock_result(stdout="<<<bad>>>")
        with pytest.raises(HimalayaError, match="Failed to parse"):
            get_message("1")


# ---------------------------------------------------------------------------
# send_message — v1.x MIME pipe path
# ---------------------------------------------------------------------------

class TestSendMessageV1:
    def setup_method(self):
        _detect_himalaya_version.cache_clear()
        _read_himalaya_account_email.cache_clear()

    def teardown_method(self):
        _detect_himalaya_version.cache_clear()
        _read_himalaya_account_email.cache_clear()

    @_patch_which()
    @_patch_version(1, 2, 0)
    @mock_patch("symeraseme.adapters.email.himalaya._read_himalaya_account_email", return_value="sender@example.com")
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_v1_x_builds_mime_and_pipes(self, mock_run, mock_email, _ver, _which):
        mock_run.return_value = _mock_result(stdout="sent")
        result = send_message(to="a@b.com", subject="Test", body="Hello")
        cmd = mock_run.call_args[0][0]
        assert cmd == ["himalaya", "message", "send"]
        # Input should be MIME text
        input_text = mock_run.call_args[1].get("input") or mock_run.call_args.kwargs.get("input", "")
        assert "From:" in input_text
        assert "To:" in input_text
        assert "Subject: Test" in input_text
        assert result["result"] == "sent"
        assert "message_id" in result

    @_patch_which()
    @_patch_version(1, 2, 0)
    @mock_patch("symeraseme.adapters.email.himalaya._read_himalaya_account_email", return_value="sender@example.com")
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_v1_x_with_account_flag(self, mock_run, mock_email, _ver, _which):
        mock_run.return_value = _mock_result(stdout="ok")
        send_message(to="a@b.com", subject="T", body="B", account="work")
        cmd = mock_run.call_args[0][0]
        assert "--account" in cmd
        assert "work" in cmd

    @_patch_which()
    @_patch_version(1, 2, 0)
    @mock_patch("symeraseme.adapters.email.himalaya._read_himalaya_account_email", return_value="sender@example.com")
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_v1_x_with_config_path(self, mock_run, mock_email, _ver, _which):
        mock_run.return_value = _mock_result(stdout="ok")
        send_message(to="a@b.com", subject="T", body="B", config_path="/tmp/cfg.toml")
        cmd = mock_run.call_args[0][0]
        assert "--config" in cmd
        assert "/tmp/cfg.toml" in cmd

    @_patch_which()
    @_patch_version(1, 2, 0)
    @mock_patch("symeraseme.adapters.email.himalaya._read_himalaya_account_email", return_value="")
    def test_v1_x_raises_when_no_sender_email(self, _mock_email, _ver, _which):
        with pytest.raises(HimalayaError, match="Cannot determine sender email"):
            send_message(to="a@b.com", subject="T", body="B")

    @_patch_which()
    @_patch_version(1, 2, 0)
    @mock_patch("symeraseme.adapters.email.himalaya._read_himalaya_account_email", return_value="s@e.com")
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_v1_x_timeout_raises(self, mock_run, _mock_email, _ver, _which):
        mock_run.side_effect = subprocess.TimeoutExpired(cmd="himalaya", timeout=30)
        with pytest.raises(HimalayaError, match="timed out"):
            send_message(to="a@b.com", subject="T", body="B")

    @_patch_which()
    @_patch_version(1, 2, 0)
    @mock_patch("symeraseme.adapters.email.himalaya._read_himalaya_account_email", return_value="s@e.com")
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_v1_x_nonzero_exit_raises(self, mock_run, _mock_email, _ver, _which):
        mock_run.return_value = _mock_result(stderr="smtp error", returncode=1)
        with pytest.raises(HimalayaError, match="smtp error"):
            send_message(to="a@b.com", subject="T", body="B")

    @_patch_which()
    @_patch_version(0, 3, 1)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_v0_x_uses_legacy_send_flags(self, mock_run, _ver, _which):
        mock_run.return_value = _mock_result(stdout="sent")
        result = send_message(to="a@b.com", subject="T", body="B")
        cmd = mock_run.call_args[0][0]
        assert "send" in cmd
        assert "--to" in cmd
        assert "a@b.com" in cmd
        assert "--subject" in cmd
        assert "T" in cmd
        # Input is the body
        input_text = mock_run.call_args[1].get("input") or mock_run.call_args.kwargs.get("input", "")
        assert input_text == "B"

    @_patch_which()
    @_patch_version(0, 3, 1)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_v0_x_with_cc_and_bcc(self, mock_run, _ver, _which):
        mock_run.return_value = _mock_result(stdout="ok")
        send_message(to="a@b.com", subject="T", body="B", cc="c@c.com", bcc="d@d.com")
        cmd = mock_run.call_args[0][0]
        assert "--cc" in cmd
        assert "c@c.com" in cmd
        assert "--bcc" in cmd
        assert "d@d.com" in cmd

    @_patch_which()
    @_patch_version(0, 3, 1)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_v0_x_timeout_raises(self, mock_run, _ver, _which):
        mock_run.side_effect = subprocess.TimeoutExpired(cmd="himalaya", timeout=30)
        with pytest.raises(HimalayaError, match="timed out"):
            send_message(to="a@b.com", subject="T", body="B")

    @_patch_which()
    @_patch_version(0, 3, 1)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_v0_x_nonzero_exit_raises(self, mock_run, _ver, _which):
        mock_run.return_value = _mock_result(stderr="auth fail", returncode=1)
        with pytest.raises(HimalayaError, match="auth fail"):
            send_message(to="a@b.com", subject="T", body="B")


# ---------------------------------------------------------------------------
# send_raw_email
# ---------------------------------------------------------------------------

class TestSendRawEmail:
    def setup_method(self):
        _detect_himalaya_version.cache_clear()

    def teardown_method(self):
        _detect_himalaya_version.cache_clear()

    @_patch_which()
    @_patch_version(1, 2, 0)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_v1_x_uses_message_send(self, mock_run, _ver, _which):
        mock_run.return_value = _mock_result(stdout="ok")
        result = send_raw_email(to="a@b.com", raw_message="MIME raw content")
        cmd = mock_run.call_args[0][0]
        assert cmd == ["himalaya", "message", "send"]
        assert result == "ok"

    @_patch_which()
    @_patch_version(0, 3, 1)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_v0_x_uses_legacy_send(self, mock_run, _ver, _which):
        mock_run.return_value = _mock_result(stdout="ok")
        send_raw_email(to="a@b.com", raw_message="raw")
        cmd = mock_run.call_args[0][0]
        assert cmd == ["himalaya", "send"]

    @_patch_which()
    @_patch_version(1, 2, 0)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_nonzero_exit_raises(self, mock_run, _ver, _which):
        mock_run.return_value = _mock_result(stderr="fail", returncode=1)
        with pytest.raises(HimalayaError, match="fail"):
            send_raw_email(to="a@b.com", raw_message="raw")

    @_patch_which()
    @_patch_version(1, 2, 0)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_with_account_and_config(self, mock_run, _ver, _which):
        mock_run.return_value = _mock_result(stdout="ok")
        send_raw_email(to="a@b.com", raw_message="raw", account="w", config_path="/c.toml")
        cmd = mock_run.call_args[0][0]
        assert "--account" in cmd
        assert "w" in cmd
        assert "--config" in cmd
        assert "/c.toml" in cmd


# ---------------------------------------------------------------------------
# _build_mime
# ---------------------------------------------------------------------------

class TestBuildMime:
    def test_basic_message(self):
        msg = EmailMessage(to="a@b.com", subject="Hi", body="Hello")
        mime_str, message_id = _build_mime(msg, "from@example.com")
        assert "From: from@example.com" in mime_str
        assert "To: a@b.com" in mime_str
        assert "Subject: Hi" in mime_str
        assert "Hello" in mime_str
        assert message_id.startswith("<")

    def test_with_cc(self):
        msg = EmailMessage(to="a@b.com", subject="T", body="B", cc="c@c.com")
        mime_str, _ = _build_mime(msg, "f@e.com")
        assert "Cc: c@c.com" in mime_str

    def test_without_cc(self):
        msg = EmailMessage(to="a@b.com", subject="T", body="B")
        mime_str, _ = _build_mime(msg, "f@e.com")
        assert "Cc:" not in mime_str


# ---------------------------------------------------------------------------
# load_smtp_config
# ---------------------------------------------------------------------------

class TestLoadSmtpConfig:
    def test_defaults(self, monkeypatch):
        for key in ("SYMERASEME_SMTP_HOST", "SYMERASEME_SMTP_PORT",
                     "SYMERASEME_SMTP_USER", "SYMERASEME_SMTP_PASSWORD",
                     "SYMERASEME_SMTP_TLS", "SYMERASEME_SMTP_FROM"):
            monkeypatch.delenv(key, raising=False)
        cfg = load_smtp_config()
        assert cfg.host == "localhost"
        assert cfg.port == 587
        assert cfg.username == ""
        assert cfg.password == ""
        assert cfg.use_tls is True
        assert cfg.from_addr == ""

    def test_custom_values(self, monkeypatch):
        monkeypatch.setenv("SYMERASEME_SMTP_HOST", "smtp.example.com")
        monkeypatch.setenv("SYMERASEME_SMTP_PORT", "465")
        monkeypatch.setenv("SYMERASEME_SMTP_USER", "user")
        monkeypatch.setenv("SYMERASEME_SMTP_PASSWORD", "pass")
        monkeypatch.setenv("SYMERASEME_SMTP_TLS", "0")
        monkeypatch.setenv("SYMERASEME_SMTP_FROM", "me@example.com")
        cfg = load_smtp_config()
        assert cfg.host == "smtp.example.com"
        assert cfg.port == 465
        assert cfg.username == "user"
        assert cfg.password == "pass"
        assert cfg.use_tls is False
        assert cfg.from_addr == "me@example.com"

    def test_tls_true_string(self, monkeypatch):
        monkeypatch.setenv("SYMERASEME_SMTP_TLS", "yes")
        assert load_smtp_config().use_tls is True

    def test_tls_true_string_uppercase(self, monkeypatch):
        monkeypatch.setenv("SYMERASEME_SMTP_TLS", "TRUE")
        assert load_smtp_config().use_tls is True


# ---------------------------------------------------------------------------
# get_email_backend
# ---------------------------------------------------------------------------

class TestGetEmailBackend:
    def test_default_smtp(self, monkeypatch):
        monkeypatch.delenv("SYMERASEME_EMAIL_BACKEND", raising=False)
        assert get_email_backend() == "smtp"

    def test_himalaya(self, monkeypatch):
        monkeypatch.setenv("SYMERASEME_EMAIL_BACKEND", "himalaya")
        assert get_email_backend() == "himalaya"

    def test_unknown_falls_back_to_smtp(self, monkeypatch):
        monkeypatch.setenv("SYMERASEME_EMAIL_BACKEND", "weird")
        assert get_email_backend() == "smtp"

    def test_case_insensitive(self, monkeypatch):
        monkeypatch.setenv("SYMERASEME_EMAIL_BACKEND", "SMTP")
        assert get_email_backend() == "smtp"


# ---------------------------------------------------------------------------
# himalaya_available / hismalaya_available (deprecated)
# ---------------------------------------------------------------------------

class TestHismalayaAvailable:
    def test_deprecated_warns(self):
        with pytest.warns(DeprecationWarning, match="deprecated"):
            with mock_patch("symeraseme.adapters.email.himalaya.shutil.which", return_value=None):
                hismalaya_available()

    def test_deprecated_returns_false(self):
        with pytest.warns(DeprecationWarning):
            with mock_patch("symeraseme.adapters.email.himalaya.shutil.which", return_value=None):
                assert hismalaya_available() is False


# ---------------------------------------------------------------------------
# send_email dispatch
# ---------------------------------------------------------------------------

class TestSendEmail:
    def setup_method(self):
        _detect_himalaya_version.cache_clear()
        _read_himalaya_account_email.cache_clear()

    def teardown_method(self):
        _detect_himalaya_version.cache_clear()
        _read_himalaya_account_email.cache_clear()

    @mock_patch("symeraseme.adapters.email.himalaya.send_message")
    def test_dispatches_to_himalaya(self, mock_send):
        mock_send.return_value = {"result": "ok", "message_id": "<x>"}
        result = send_email(to="a@b.com", subject="T", body="B", backend="himalaya")
        mock_send.assert_called_once()
        assert result["result"] == "ok"

    @mock_patch("symeraseme.adapters.email.himalaya.send_message_smtp")
    def test_dispatches_to_smtp(self, mock_smtp):
        mock_smtp.return_value = {"result": "sent", "message_id": "<y>"}
        result = send_email(to="a@b.com", subject="T", body="B", backend="smtp")
        mock_smtp.assert_called_once()
        assert result["result"] == "sent"

    @mock_patch("symeraseme.adapters.email.himalaya.get_email_backend", return_value="himalaya")
    @mock_patch("symeraseme.adapters.email.himalaya.send_message")
    def test_uses_env_backend_when_none(self, mock_send, mock_backend):
        mock_send.return_value = {"result": "ok", "message_id": "<z>"}
        send_email(to="a@b.com", subject="T", body="B")
        mock_send.assert_called_once()


# ---------------------------------------------------------------------------
# send_message_smtp
# ---------------------------------------------------------------------------

class TestSendSmtp:
    def test_raises_when_no_from(self):
        cfg = SmtpConfig(from_addr="")
        with pytest.raises(SmtpError, match="SYMERASEME_SMTP_FROM"):
            send_message_smtp(to="a@b.com", subject="T", body="B", smtp_config=cfg)

    @mock_patch("smtplib.SMTP")
    def test_sends_successfully(self, mock_smtp_cls):
        mock_smtp = MagicMock()
        mock_smtp_cls.return_value.__enter__ = MagicMock(return_value=mock_smtp)
        mock_smtp_cls.return_value.__exit__ = MagicMock(return_value=False)
        cfg = SmtpConfig(host="smtp.test.com", port=587, from_addr="me@test.com", use_tls=False)
        result = send_message_smtp(to="a@b.com", subject="T", body="B", smtp_config=cfg)
        assert result["result"] == "Message sent"
        assert "message_id" in result
        mock_smtp.sendmail.assert_called_once()

    @mock_patch("smtplib.SMTP")
    def test_smtp_exception_raises_smtp_error(self, mock_smtp_cls):
        import smtplib
        mock_smtp = MagicMock()
        mock_smtp.sendmail.side_effect = smtplib.SMTPException("send failed")
        mock_smtp_cls.return_value.__enter__ = MagicMock(return_value=mock_smtp)
        mock_smtp_cls.return_value.__exit__ = MagicMock(return_value=False)
        cfg = SmtpConfig(host="smtp.test.com", port=587, from_addr="me@test.com")
        with pytest.raises(SmtpError, match="send failed"):
            send_message_smtp(to="a@b.com", subject="T", body="B", smtp_config=cfg)

    @mock_patch("smtplib.SMTP")
    def test_os_error_raises_smtp_error(self, mock_smtp_cls):
        mock_smtp_cls.return_value.__enter__ = MagicMock(side_effect=OSError("connection refused"))
        mock_smtp_cls.return_value.__exit__ = MagicMock(return_value=False)
        cfg = SmtpConfig(host="smtp.test.com", port=587, from_addr="me@test.com")
        with pytest.raises(SmtpError, match="connection refused"):
            send_message_smtp(to="a@b.com", subject="T", body="B", smtp_config=cfg)

    @mock_patch("smtplib.SMTP")
    def test_with_cc_and_bcc(self, mock_smtp_cls):
        mock_smtp = MagicMock()
        mock_smtp_cls.return_value.__enter__ = MagicMock(return_value=mock_smtp)
        mock_smtp_cls.return_value.__exit__ = MagicMock(return_value=False)
        cfg = SmtpConfig(host="smtp.test.com", port=587, from_addr="me@test.com", use_tls=False)
        send_message_smtp(to="a@b.com", subject="T", body="B", cc="c@c.com", bcc="d@d.com", smtp_config=cfg)
        recipients = mock_smtp.sendmail.call_args[0][1]
        assert "c@c.com" in recipients
        assert "d@d.com" in recipients

    @mock_patch("smtplib.SMTP")
    def test_uses_env_when_no_config(self, mock_smtp_cls, monkeypatch):
        monkeypatch.setenv("SYMERASEME_SMTP_FROM", "env@test.com")
        monkeypatch.setenv("SYMERASEME_SMTP_HOST", "smtp.env.com")
        monkeypatch.setenv("SYMERASEME_SMTP_PORT", "25")
        monkeypatch.setenv("SYMERASEME_SMTP_TLS", "0")
        mock_smtp = MagicMock()
        mock_smtp_cls.return_value.__enter__ = MagicMock(return_value=mock_smtp)
        mock_smtp_cls.return_value.__exit__ = MagicMock(return_value=False)
        send_message_smtp(to="a@b.com", subject="T", body="B")
        mock_smtp_cls.assert_called_with("smtp.env.com", 25, timeout=30)


# ---------------------------------------------------------------------------
# _read_himalaya_account_email
# ---------------------------------------------------------------------------

class TestReadHimalayaAccountEmail:
    def setup_method(self):
        _read_himalaya_account_email.cache_clear()

    def teardown_method(self):
        _read_himalaya_account_email.cache_clear()

    @mock_patch("symeraseme.adapters.email.himalaya._config_path_for_account")
    def test_returns_empty_when_no_config(self, mock_path):
        mock_path.return_value = Path("/nonexistent/config.toml")
        assert _read_himalaya_account_email() == ""

    @mock_patch("symeraseme.adapters.email.himalaya._config_path_for_account")
    def test_returns_empty_when_no_accounts(self, mock_path, tmp_path):
        config = tmp_path / "config.toml"
        config.write_text("# no accounts\n")
        mock_path.return_value = config
        assert _read_himalaya_account_email() == ""

    @mock_patch("symeraseme.adapters.email.himalaya._config_path_for_account")
    def test_reads_first_account_email(self, mock_path, tmp_path):
        config = tmp_path / "config.toml"
        config.write_text('[accounts.default]\nemail = "test@example.com"\n')
        mock_path.return_value = config
        assert _read_himalaya_account_email() == "test@example.com"

    @mock_patch("symeraseme.adapters.email.himalaya._config_path_for_account")
    def test_reads_named_account(self, mock_path, tmp_path):
        config = tmp_path / "config.toml"
        config.write_text(
            '[accounts.default]\nemail = "def@e.com"\n\n'
            '[accounts.work]\nemail = "work@e.com"\n'
        )
        mock_path.return_value = config
        assert _read_himalaya_account_email("work") == "work@e.com"

    @mock_patch("symeraseme.adapters.email.himalaya._config_path_for_account")
    def test_falls_back_to_first_when_account_not_found(self, mock_path, tmp_path):
        config = tmp_path / "config.toml"
        config.write_text('[accounts.default]\nemail = "first@e.com"\n')
        mock_path.return_value = config
        assert _read_himalaya_account_email("nonexistent") == "first@e.com"


# ---------------------------------------------------------------------------
# Not-installed errors
# ---------------------------------------------------------------------------

class TestNotInstalled:
    def setup_method(self):
        _detect_himalaya_version.cache_clear()

    def teardown_method(self):
        _detect_himalaya_version.cache_clear()

    @mock_patch("symeraseme.adapters.email.himalaya.shutil.which", return_value=None)
    def test_detect_version_raises(self, _which):
        # _detect_himalaya_version calls _check_himalaya_installed first
        _detect_himalaya_version.cache_clear()
        with pytest.raises(HimalayaNotInstalledError):
            _detect_himalaya_version()

    @mock_patch("symeraseme.adapters.email.himalaya.shutil.which", return_value=None)
    def test_list_messages_raises(self, _which):
        with pytest.raises(HimalayaNotInstalledError):
            list_messages()

    @mock_patch("symeraseme.adapters.email.himalaya.shutil.which", return_value=None)
    def test_send_message_raises(self, _which):
        with pytest.raises(HimalayaNotInstalledError):
            send_message(to="a@b.com", subject="T", body="B")

    @mock_patch("symeraseme.adapters.email.himalaya.shutil.which", return_value=None)
    def test_get_message_raises(self, _which):
        with pytest.raises(HimalayaNotInstalledError):
            get_message("1")

    @mock_patch("symeraseme.adapters.email.himalaya.shutil.which", return_value=None)
    def test_send_raw_email_raises(self, _which):
        with pytest.raises(HimalayaNotInstalledError):
            send_raw_email(to="a@b.com", raw_message="raw")


# ---------------------------------------------------------------------------
# Envelope and Message from_address with addr field
# ---------------------------------------------------------------------------

class TestEnvelopeFromAddrField:
    def setup_method(self):
        _detect_himalaya_version.cache_clear()

    def teardown_method(self):
        _detect_himalaya_version.cache_clear()

    @_patch_which()
    @_patch_version(1, 2, 0)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_from_uses_addr_when_no_name(self, mock_run, _ver, _which):
        data = [{"id": "1", "subject": "S", "from": {"addr": "a@b.com"}, "to": "r@b.com", "date": None, "flags": []}]
        mock_run.return_value = _mock_result(stdout=json.dumps(data))
        envelopes = list_messages()
        assert envelopes[0].from_ == "a@b.com"

    @_patch_which()
    @_patch_version(1, 2, 0)
    @mock_patch("symeraseme.adapters.email.himalaya.subprocess.run")
    def test_from_string_value(self, mock_run, _ver, _which):
        data = [{"id": "1", "subject": "S", "from": "plain@addr.com", "to": "r@b.com", "date": None, "flags": []}]
        mock_run.return_value = _mock_result(stdout=json.dumps(data))
        envelopes = list_messages()
        assert envelopes[0].from_ == "plain@addr.com"


# ---------------------------------------------------------------------------
# himalaya_available (redundant with existing but kept for completeness)
# ---------------------------------------------------------------------------

class TestHimalayaAvailable:
    @mock_patch("symeraseme.adapters.email.himalaya.shutil.which", return_value="/usr/bin/himalaya")
    def test_true(self, _mock):
        assert himalaya_available() is True

    @mock_patch("symeraseme.adapters.email.himalaya.shutil.which", return_value=None)
    def test_false(self, _mock):
        assert himalaya_available() is False
