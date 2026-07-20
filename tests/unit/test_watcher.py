"""Tests for symeraseme.services.watcher — InboxWatcher and run_watch_loop."""

from __future__ import annotations

import threading
from unittest.mock import MagicMock, patch

import pytest

from symeraseme.core.result_types import CliResult
from symeraseme.services.watcher import (
    DEFAULT_INTERVAL_SECONDS,
    InboxWatcher,
    run_watch_loop,
)

# get_connection is imported locally inside _count_replies and
# _send_notifications_for_new from symeraseme.core.db_connection.
_PATCH_GET_CONN = "symeraseme.core.db_connection.get_connection"


@pytest.fixture()
def poll_kwargs() -> dict:
    """Minimal kwargs for InboxWatcher poll calls."""
    return {
        "host": "imap.test.com",
        "port": 993,
        "username": "user@test.com",
        "since_days": 7,
        "ssl": True,
        "campaign_id": None,
        "password": "secret",
    }


@pytest.fixture()
def watcher(poll_kwargs: dict) -> InboxWatcher:
    return InboxWatcher(interval_seconds=60, poll_kwargs=poll_kwargs)


def _make_result(data: dict | None = None) -> CliResult:
    return CliResult(success=True, data=data or {"replies": []})


# ── InboxWatcher.__init__ ──────────────────────────────────────────


class TestInboxWatcherInit:
    def test_valid_interval(self, poll_kwargs: dict) -> None:
        w = InboxWatcher(interval_seconds=60, poll_kwargs=poll_kwargs)
        assert w._interval == 60
        assert w._poll_kwargs == poll_kwargs
        assert w._last_reply_count == 0
        assert w._thread is None

    def test_below_minimum_raises(self, poll_kwargs: dict) -> None:
        with pytest.raises(ValueError, match="interval_seconds must be >= 60"):
            InboxWatcher(interval_seconds=59, poll_kwargs=poll_kwargs)

    def test_exactly_minimum(self, poll_kwargs: dict) -> None:
        w = InboxWatcher(interval_seconds=60, poll_kwargs=poll_kwargs)
        assert w._interval == 60

    def test_default_interval(self, poll_kwargs: dict) -> None:
        w = InboxWatcher(poll_kwargs=poll_kwargs)
        assert w._interval == DEFAULT_INTERVAL_SECONDS


# ── InboxWatcher.is_running ────────────────────────────────────────


class TestIsRunning:
    def test_not_running_initially(self, watcher: InboxWatcher) -> None:
        assert watcher.is_running is False


# ── InboxWatcher.start / stop ──────────────────────────────────────


class TestStartStop:
    @patch("symeraseme.services.watcher.init_db")
    @patch("symeraseme.services.watcher.threading.Thread")
    def test_start_creates_thread(
        self, mock_thread_cls: MagicMock, mock_init_db: MagicMock, watcher: InboxWatcher
    ) -> None:
        mock_thread = MagicMock()
        mock_thread.is_alive.return_value = False
        mock_thread_cls.return_value = mock_thread

        with patch.object(watcher, "_count_replies", return_value=0):
            watcher.start()

        mock_init_db.assert_called_once()
        mock_thread.start.assert_called_once()
        assert watcher._thread is mock_thread

    @patch("symeraseme.services.watcher.init_db")
    @patch("symeraseme.services.watcher.threading.Thread")
    def test_start_already_running_logs_warning(
        self, mock_thread_cls: MagicMock, mock_init_db: MagicMock, watcher: InboxWatcher
    ) -> None:
        # Simulate already-running thread
        mock_thread = MagicMock()
        mock_thread.is_alive.return_value = True
        watcher._thread = mock_thread

        watcher.start()  # should return early
        mock_init_db.assert_not_called()

    @patch("symeraseme.services.watcher.init_db")
    def test_stop_clean(self, mock_init_db: MagicMock, watcher: InboxWatcher) -> None:
        mock_thread = MagicMock()
        mock_thread.join.return_value = None
        watcher._thread = mock_thread

        watcher.stop()

        mock_thread.join.assert_called_once()
        assert watcher._thread is None

    def test_stop_when_not_started(self, watcher: InboxWatcher) -> None:
        # Should not raise
        watcher.stop()
        assert watcher._thread is None


# ── InboxWatcher.poll_once ─────────────────────────────────────────


class TestPollOnce:
    @patch("symeraseme.services.watcher.handle_poll_inbox")
    @patch.object(InboxWatcher, "_count_replies", return_value=0)
    def test_returns_data_dict(
        self, mock_count: MagicMock, mock_poll: MagicMock, watcher: InboxWatcher
    ) -> None:
        mock_poll.return_value = _make_result({"replies": [], "fetched": 3})
        result = watcher.poll_once()
        assert isinstance(result, dict)
        assert result["fetched"] == 3
        assert result["watcher_new_replies"] == 0

    @patch("symeraseme.services.watcher.handle_poll_inbox")
    @patch.object(InboxWatcher, "_count_replies", return_value=5)
    def test_non_dict_data_handled(
        self, mock_count: MagicMock, mock_poll: MagicMock, watcher: InboxWatcher
    ) -> None:
        # Set last_reply_count to match count so new_replies == 0
        watcher._last_reply_count = 5
        # CliResult with list data — falls back to empty dict
        mock_poll.return_value = CliResult(success=True, data=["a", "b"])
        result = watcher.poll_once()
        assert result["watcher_new_replies"] == 0


# ── InboxWatcher._do_poll with new replies ─────────────────────────


class TestDoPollNewReplies:
    @patch("symeraseme.services.watcher.InboxWatcher._send_notifications_for_new")
    @patch("symeraseme.services.watcher.handle_poll_inbox")
    @patch.object(InboxWatcher, "_count_replies")
    def test_new_replies_trigger_notification(
        self,
        mock_count: MagicMock,
        mock_poll: MagicMock,
        mock_send: MagicMock,
        watcher: InboxWatcher,
    ) -> None:
        # last_reply_count starts at 0; _count_replies returns 3
        mock_count.return_value = 3
        mock_poll.return_value = _make_result({"replies": [1, 2, 3]})

        result = watcher.poll_once()

        assert result["watcher_new_replies"] == 3
        mock_send.assert_called_once_with(3)

    @patch("symeraseme.services.watcher.InboxWatcher._send_notifications_for_new")
    @patch("symeraseme.services.watcher.handle_poll_inbox")
    @patch.object(InboxWatcher, "_count_replies")
    def test_no_new_replies_no_notification(
        self,
        mock_count: MagicMock,
        mock_poll: MagicMock,
        mock_send: MagicMock,
        watcher: InboxWatcher,
    ) -> None:
        watcher._last_reply_count = 5
        mock_count.return_value = 5
        mock_poll.return_value = _make_result()

        result = watcher.poll_once()

        assert result["watcher_new_replies"] == 0
        mock_send.assert_not_called()


# ── InboxWatcher._send_notifications_for_new ───────────────────────


class TestSendNotificationsForNew:
    @patch("symeraseme.services.watcher.get_notification_backend")
    @patch(_PATCH_GET_CONN)
    def test_sends_for_each_new_row(
        self, mock_get_conn: MagicMock, mock_get_backend: MagicMock, watcher: InboxWatcher
    ) -> None:
        mock_conn = MagicMock()
        mock_get_conn.return_value = mock_conn

        rows = [
            {
                "request_id": 1,
                "from_addr": "a@b.com",
                "subject": "Hi",
                "snippet": "...",
                "received_at": "2026-01-01",
                "broker_id": "broker1",
            },
            {
                "request_id": 2,
                "from_addr": "c@d.com",
                "subject": "Re: Hi",
                "snippet": "bye",
                "received_at": "2026-01-02",
                "broker_id": "broker2",
            },
        ]
        mock_conn.execute.return_value.fetchall.return_value = rows

        mock_backend = MagicMock()
        mock_get_backend.return_value = mock_backend

        watcher._last_reply_count = 0
        watcher._send_notifications_for_new(current_count=2)

        assert mock_backend.send.call_count == 2

    @patch(_PATCH_GET_CONN)
    def test_db_error_handled(self, mock_get_conn: MagicMock, watcher: InboxWatcher) -> None:
        mock_conn = MagicMock()
        mock_conn.execute.side_effect = Exception("db locked")
        mock_get_conn.return_value = mock_conn

        # Should not raise
        watcher._send_notifications_for_new(current_count=5)


# ── InboxWatcher._count_replies ────────────────────────────────────


class TestCountReplies:
    @patch(_PATCH_GET_CONN)
    def test_returns_count(self, mock_get_conn: MagicMock, watcher: InboxWatcher) -> None:
        mock_conn = MagicMock()
        mock_conn.execute.return_value.fetchone.return_value = [42]
        mock_get_conn.return_value = mock_conn

        assert watcher._count_replies() == 42

    @patch(_PATCH_GET_CONN)
    def test_empty_result_returns_zero(self, mock_get_conn: MagicMock, watcher: InboxWatcher) -> None:
        mock_conn = MagicMock()
        mock_conn.execute.return_value.fetchone.return_value = None
        mock_get_conn.return_value = mock_conn

        assert watcher._count_replies() == 0

    @patch(_PATCH_GET_CONN)
    def test_db_error_returns_zero(self, mock_get_conn: MagicMock, watcher: InboxWatcher) -> None:
        mock_conn = MagicMock()
        mock_conn.execute.side_effect = Exception("no such table")
        mock_get_conn.return_value = mock_conn

        assert watcher._count_replies() == 0


# ── run_watch_loop ─────────────────────────────────────────────────


class TestRunWatchLoop:
    def test_sets_signal_handlers_and_restores(self) -> None:
        """run_watch_loop sets SIGINT/SIGTERM handlers and restores them."""
        with (
            patch("symeraseme.services.watcher.signal.signal") as mock_signal,
            patch("symeraseme.services.watcher.time.sleep"),
            patch.object(InboxWatcher, "stop"),
            patch.object(InboxWatcher, "start"),
        ):
            with patch.object(
                InboxWatcher, "is_running",
                new_callable=lambda: property(lambda self: False),
            ):
                run_watch_loop(
                    interval_seconds=60,
                    poll_kwargs={"host": "x", "port": 1, "username": "u", "since_days": 1, "ssl": False, "campaign_id": None, "password": "p"},
                )

            # 2 sets + 2 restores
            assert mock_signal.call_count == 4

    def test_keyboard_interrupt_stops_cleanly(self) -> None:
        """KeyboardInterrupt during sleep triggers clean stop."""
        call_count = 0

        def sleep_side_effect(_sec):
            nonlocal call_count
            call_count += 1
            if call_count >= 1:
                raise KeyboardInterrupt

        with (
            patch("symeraseme.services.watcher.signal.signal"),
            patch("symeraseme.services.watcher.time.sleep", side_effect=sleep_side_effect),
            patch.object(InboxWatcher, "stop") as mock_stop,
            patch.object(InboxWatcher, "start"),
        ):
            with patch.object(
                InboxWatcher, "is_running",
                new_callable=lambda: property(lambda self: True),
            ):
                try:
                    run_watch_loop(
                        interval_seconds=60,
                        poll_kwargs={"host": "x", "port": 1, "username": "u", "since_days": 1, "ssl": False, "campaign_id": None, "password": "p"},
                    )
                except KeyboardInterrupt:
                    pass
            mock_stop.assert_called()


# ── InboxWatcher._run_loop ─────────────────────────────────────────


class TestRunLoop:
    def test_loop_exits_when_event_set(self, watcher: InboxWatcher) -> None:
        """The background loop exits when _stop_event is set before first poll."""
        watcher._stop_event.set()
        with patch.object(watcher, "_do_poll") as mock_poll:
            watcher._run_loop()
            # Loop exits before polling since event is already set
            mock_poll.assert_not_called()

    def test_loop_polls_then_stops(self, watcher: InboxWatcher) -> None:
        """The loop polls once then exits when event is set after poll."""
        call_count = 0

        def do_poll_side_effect():
            nonlocal call_count
            call_count += 1
            watcher._stop_event.set()  # stop after first poll
            return {}

        with patch.object(watcher, "_do_poll", side_effect=do_poll_side_effect) as mock_poll:
            watcher._run_loop()
            mock_poll.assert_called_once()

    def test_loop_continues_after_exception(self, watcher: InboxWatcher) -> None:
        """The loop catches exceptions and continues."""
        watcher._interval = 0.001
        call_count = 0

        def side_effect():
            nonlocal call_count
            call_count += 1
            if call_count == 1:
                raise RuntimeError("transient error")
            watcher._stop_event.set()
            return {}

        with patch.object(watcher, "_do_poll", side_effect=side_effect):
            watcher._run_loop()
        assert call_count == 2
