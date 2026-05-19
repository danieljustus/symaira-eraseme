from __future__ import annotations

import json
from unittest.mock import patch

import pytest

from openeraseme.adapters.email.himalaya import (
    HimalayaError,
    HimalayaNotInstalledError,
    Message,
    get_message,
    hismalaya_available,
    list_messages,
    send_message,
)


def _mock_result(stdout: str = "", stderr: str = "", returncode: int = 0):
    import types

    return types.SimpleNamespace(stdout=stdout, stderr=stderr, returncode=returncode)


class TestHimalayaAvailable:
    @patch("openeraseme.adapters.email.himalaya.shutil.which", return_value="/usr/bin/himalaya")
    def test_available_when_on_path(self, _mock):
        assert hismalaya_available() is True

    @patch("openeraseme.adapters.email.himalaya.shutil.which", return_value=None)
    def test_not_available_when_missing(self, _mock):
        assert hismalaya_available() is False


class TestListMessages:
    @patch("openeraseme.adapters.email.himalaya.subprocess.run")
    @patch("openeraseme.adapters.email.himalaya.shutil.which", return_value="/usr/bin/himalaya")
    def test_lists_envelopes(self, _which, mock_run):
        fake_data: list[dict] = [
            {
                "id": "1",
                "subject": "Hello",
                "from": {"name": "Alice"},
                "to": {"name": "Bob"},
                "date": "2026-01-15T10:00:00+0000",
                "flags": [],
            },
            {
                "id": "2",
                "subject": "Re: Hello",
                "from": {"name": "Bob"},
                "to": {"name": "Alice"},
                "date": "2026-01-15T11:00:00+0000",
                "flags": ["SEEN"],
            },
        ]
        mock_run.return_value = _mock_result(stdout=json.dumps(fake_data))

        result = list_messages()
        assert len(result) == 2
        assert result[0].id == "1"
        assert result[0].subject == "Hello"
        assert result[1].flags == ["SEEN"]

    @patch("openeraseme.adapters.email.himalaya.subprocess.run")
    @patch("openeraseme.adapters.email.himalaya.shutil.which", return_value="/usr/bin/himalaya")
    def test_empty_inbox(self, _which, mock_run):
        mock_run.return_value = _mock_result(stdout="")

        result = list_messages()
        assert result == []

    @patch("openeraseme.adapters.email.himalaya.subprocess.run")
    @patch("openeraseme.adapters.email.himalaya.shutil.which", return_value="/usr/bin/himalaya")
    def test_raises_on_subprocess_error(self, _which, mock_run):
        mock_run.return_value = _mock_result(stderr="connection failed", returncode=1)

        with pytest.raises(HimalayaError, match="connection failed"):
            list_messages()


class TestGetMessage:
    @patch("openeraseme.adapters.email.himalaya.subprocess.run")
    @patch("openeraseme.adapters.email.himalaya.shutil.which", return_value="/usr/bin/himalaya")
    def test_returns_message(self, _which, mock_run):
        fake_body = "<div>Hello World</div>"
        fake_data = {
            "id": "42",
            "subject": "Your Request",
            "from": {"name": "Broker"},
            "to": {"name": "User"},
            "date": "2026-01-15T10:00:00+0000",
            "body": fake_body,
            "flags": [],
        }
        mock_run.return_value = _mock_result(stdout=json.dumps(fake_data))

        msg = get_message("42")
        assert isinstance(msg, Message)
        assert msg.id == "42"
        assert msg.subject == "Your Request"
        assert msg.body == fake_body

    @patch("openeraseme.adapters.email.himalaya.subprocess.run")
    @patch("openeraseme.adapters.email.himalaya.shutil.which", return_value="/usr/bin/himalaya")
    def test_raises_on_empty_output(self, _which, mock_run):
        mock_run.return_value = _mock_result(stdout="")

        with pytest.raises(HimalayaError, match="not found"):
            get_message("999")


class TestSendMessage:
    @patch("openeraseme.adapters.email.himalaya.subprocess.run")
    @patch("openeraseme.adapters.email.himalaya.shutil.which", return_value="/usr/bin/himalaya")
    def test_sends_successfully(self, _which, mock_run):
        mock_run.return_value = _mock_result(stdout="Message sent")

        result = send_message(to="test@example.com", subject="Test", body="Hello")
        assert result == "Message sent"

    @patch("openeraseme.adapters.email.himalaya.subprocess.run")
    @patch("openeraseme.adapters.email.himalaya.shutil.which", return_value="/usr/bin/himalaya")
    def test_raises_on_failure(self, _which, mock_run):
        mock_run.return_value = _mock_result(stderr="auth failed", returncode=1)

        with pytest.raises(HimalayaError, match="auth failed"):
            send_message(to="test@example.com", subject="Test", body="Hello")


class TestHimalayaNotInstalled:
    @patch("openeraseme.adapters.email.himalaya.shutil.which", return_value=None)
    def test_raises_on_list(self, _which):
        with pytest.raises(HimalayaNotInstalledError):
            list_messages()

    @patch("openeraseme.adapters.email.himalaya.shutil.which", return_value=None)
    def test_raises_on_send(self, _which):
        with pytest.raises(HimalayaNotInstalledError):
            send_message(to="a@b.com", subject="test", body="test")
