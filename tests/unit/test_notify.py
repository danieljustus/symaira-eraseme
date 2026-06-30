"""Tests for symeraseme.core.notify — notification backends and payload."""

from __future__ import annotations

import json
import urllib.error
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from symeraseme.core.notify import (
    CompositeNotificationBackend,
    FileNotificationBackend,
    InboxNotification,
    WebhookNotificationBackend,
    get_notification_backend,
)

FIXED_TS = "2026-01-15T10:30:00+00:00"


@pytest.fixture()
def notification() -> InboxNotification:
    """A sample InboxNotification for reuse across tests."""
    return InboxNotification(
        request_id=42,
        broker_id="spokeo",
        from_addr="reply@spokeo.com",
        subject="Removal confirmed",
        snippet="Your data has been removed.",
        received_at="2026-01-15T09:00:00+00:00",
    )


@pytest.fixture()
def notification_no_request_id() -> InboxNotification:
    """Notification without a request_id (edge case)."""
    return InboxNotification(
        request_id=None,
        broker_id="unknown",
        from_addr="noreply@broker.com",
        subject="Re: removal",
        snippet="",
        received_at="2026-01-15T09:00:00+00:00",
    )


# ── InboxNotification ──────────────────────────────────────────────


class TestInboxNotification:
    def test_to_dict_fields(self, notification: InboxNotification) -> None:
        d = notification.to_dict()
        assert d["type"] == "inbox_reply"
        assert d["request_id"] == 42
        assert d["broker_id"] == "spokeo"
        assert d["from_addr"] == "reply@spokeo.com"
        assert d["subject"] == "Removal confirmed"
        assert d["snippet"] == "Your data has been removed."
        assert d["received_at"] == "2026-01-15T09:00:00+00:00"
        # notified_at is auto-generated — just check it's present and ISO-ish
        assert "notified_at" in d
        assert isinstance(d["notified_at"], str)

    def test_to_dict_none_request_id(self, notification_no_request_id: InboxNotification) -> None:
        d = notification_no_request_id.to_dict()
        assert d["request_id"] is None

    def test_to_message_with_request_id(self, notification: InboxNotification) -> None:
        msg = notification.to_message()
        assert "spokeo" in msg
        assert "#42" in msg
        assert "New reply" in msg

    def test_to_message_without_request_id(self, notification_no_request_id: InboxNotification) -> None:
        msg = notification_no_request_id.to_message()
        assert "unmatched" in msg
        assert "unknown" in msg

    def test_frozen_dataclass(self) -> None:
        n = InboxNotification(
            request_id=1, broker_id="b", from_addr="a", subject="s", snippet="", received_at=""
        )
        with pytest.raises(AttributeError):
            n.request_id = 99  # type: ignore[misc]


# ── FileNotificationBackend ────────────────────────────────────────


class TestFileNotificationBackend:
    def test_send_success(self, notification: InboxNotification, tmp_path: Path) -> None:
        backend = FileNotificationBackend(events_dir=tmp_path)
        result = backend.send(notification)
        assert result is True

        files = list(tmp_path.glob("inbox_reply_*.json"))
        assert len(files) == 1

        content = json.loads(files[0].read_text())
        assert content["broker_id"] == "spokeo"
        assert content["request_id"] == 42

    def test_send_unknown_rid(self, notification_no_request_id: InboxNotification, tmp_path: Path) -> None:
        backend = FileNotificationBackend(events_dir=tmp_path)
        result = backend.send(notification_no_request_id)
        assert result is True

        files = list(tmp_path.glob("inbox_reply_*_unknown.json"))
        assert len(files) == 1

    def test_send_oserror_returns_false(self, notification: InboxNotification, tmp_path: Path) -> None:
        backend = FileNotificationBackend(events_dir=tmp_path)
        with patch.object(Path, "write_text", side_effect=OSError("disk full")):
            result = backend.send(notification)
        assert result is False

    def test_creates_events_dir(self, tmp_path: Path) -> None:
        missing = tmp_path / "nested" / "events"
        backend = FileNotificationBackend(events_dir=missing)
        assert missing.is_dir()

    def test_default_events_dir_uses_config(self) -> None:
        with patch("symeraseme.core.notify.get_config") as mock_cfg:
            mock_cfg.return_value.resolved_data_dir = Path("/tmp/test-symeraseme-data")
            backend = FileNotificationBackend()
            assert "events" in str(backend.events_dir)


# ── WebhookNotificationBackend ─────────────────────────────────────


class TestWebhookNotificationBackend:
    def test_send_success(self, notification: InboxNotification) -> None:
        mock_resp = MagicMock()
        mock_resp.status = 200
        mock_resp.__enter__ = lambda s: s
        mock_resp.__exit__ = MagicMock(return_value=False)

        backend = WebhookNotificationBackend("https://hooks.example.com/notify")
        with patch("symeraseme.core.notify.urllib.request.urlopen", return_value=mock_resp) as mock_open:
            result = backend.send(notification)
        assert result is True

        call_args = mock_open.call_args
        req = call_args[0][0]
        assert req.full_url == "https://hooks.example.com/notify"
        assert req.get_header("Content-type") == "application/json"

    def test_send_http_error_status(self, notification: InboxNotification) -> None:
        mock_resp = MagicMock()
        mock_resp.status = 500
        mock_resp.__enter__ = lambda s: s
        mock_resp.__exit__ = MagicMock(return_value=False)

        backend = WebhookNotificationBackend("https://hooks.example.com/notify")
        with patch("symeraseme.core.notify.urllib.request.urlopen", return_value=mock_resp):
            result = backend.send(notification)
        assert result is False

    def test_send_url_error(self, notification: InboxNotification) -> None:
        backend = WebhookNotificationBackend("https://hooks.example.com/notify")
        with patch(
            "symeraseme.core.notify.urllib.request.urlopen",
            side_effect=urllib.error.URLError("connection refused"),
        ):
            result = backend.send(notification)
        assert result is False

    def test_send_os_error(self, notification: InboxNotification) -> None:
        backend = WebhookNotificationBackend("https://hooks.example.com/notify")
        with patch(
            "symeraseme.core.notify.urllib.request.urlopen",
            side_effect=OSError("network unreachable"),
        ):
            result = backend.send(notification)
        assert result is False

    def test_custom_timeout(self, notification: InboxNotification) -> None:
        mock_resp = MagicMock()
        mock_resp.status = 204
        mock_resp.__enter__ = lambda s: s
        mock_resp.__exit__ = MagicMock(return_value=False)

        backend = WebhookNotificationBackend("https://hooks.example.com/notify", timeout=30)
        with patch("symeraseme.core.notify.urllib.request.urlopen", return_value=mock_resp) as mock_open:
            backend.send(notification)
        assert mock_open.call_args[1]["timeout"] == 30


# ── CompositeNotificationBackend ───────────────────────────────────


class TestCompositeNotificationBackend:
    def test_send_one_succeeds(self, notification: InboxNotification) -> None:
        ok_backend = MagicMock()
        ok_backend.send.return_value = True
        fail_backend = MagicMock()
        fail_backend.send.return_value = False

        composite = CompositeNotificationBackend([ok_backend, fail_backend])
        result = composite.send(notification)
        assert result is True
        ok_backend.send.assert_called_once_with(notification)
        fail_backend.send.assert_called_once_with(notification)

    def test_send_all_fail(self, notification: InboxNotification) -> None:
        fail1 = MagicMock()
        fail1.send.return_value = False
        fail2 = MagicMock()
        fail2.send.return_value = False

        composite = CompositeNotificationBackend([fail1, fail2])
        result = composite.send(notification)
        assert result is False

    def test_send_empty_backends(self, notification: InboxNotification) -> None:
        composite = CompositeNotificationBackend([])
        result = composite.send(notification)
        assert result is False


# ── get_notification_backend factory ────────────────────────────────


class TestGetNotificationBackend:
    def test_file_only(self) -> None:
        with patch.dict("os.environ", {}, clear=True):
            env_keys_to_clear = ["SYMERASEME_EVENTS_DIR", "SYMERASEME_WEBHOOK_URL"]
            for k in env_keys_to_clear:
                import os
                os.environ.pop(k, None)

            backend = get_notification_backend()
            assert isinstance(backend, FileNotificationBackend)

    def test_file_and_webhook(self) -> None:
        with patch.dict("os.environ", {"SYMERASEME_WEBHOOK_URL": "https://hooks.test"}):
            import os
            os.environ.pop("SYMERASEME_EVENTS_DIR", None)
            backend = get_notification_backend()
            assert isinstance(backend, CompositeNotificationBackend)

    def test_custom_events_dir(self) -> None:
        with patch.dict("os.environ", {"SYMERASEME_EVENTS_DIR": "/tmp/custom-events"}):
            import os
            os.environ.pop("SYMERASEME_WEBHOOK_URL", None)
            backend = get_notification_backend()
            assert isinstance(backend, FileNotificationBackend)
            assert str(backend.events_dir) == "/tmp/custom-events"
