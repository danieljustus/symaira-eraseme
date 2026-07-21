from __future__ import annotations

from unittest.mock import patch

from symeraseme.adapters.email.smtp_imap import IMAPError
from symeraseme.services.inbox import handle_poll_inbox

SR = "symeraseme.services.inbox"

_BASE_KWARGS = dict(
    host="imap.example.com",
    port=993,
    username="user@example.com",
    since_days=7,
    ssl=True,
    campaign_id=None,
    password="app-password",
)

_MSG_1 = {
    "message_id": "<abc@example.com>",
    "from_addr": "broker@spokeo.com",
    "subject": "Re: Data Deletion Request",
    "body": "Your data has been deleted.",
}
_MSG_2 = {
    "message_id": "<def@example.com>",
    "from_addr": "broker@intelius.com",
    "subject": "Re: Data Deletion Request",
    "body": "Please verify your identity.",
}
_MSG_3 = {
    "message_id": "<ghi@example.com>",
    "from_addr": "noreply@unknown.com",
    "subject": "Newsletter",
    "body": "This month's deals...",
}

DB = "symeraseme.core.db_connection.init_db"


def _match_call_arg(mock_call, index: int):
    """Get the Nth positional argument from a mock call."""
    return mock_call.args[index] if mock_call.args else mock_call[0][index]


class TestHandlePollInbox:
    """Tests for ``handle_poll_inbox`` gateway — the inbox polling service handler."""

    # ------------------------------------------------------------------
    # IMAP error path  (lines 39-48)
    # ------------------------------------------------------------------

    def test_imap_error_returns_error_result(self):
        """IMAPError from ``_poll`` produces ``success=False`` with a user-facing message."""
        with (
            patch(DB),
            patch(f"{SR}._poll", side_effect=IMAPError("Connection refused")),
        ):
            result = handle_poll_inbox(**_BASE_KWARGS)

        assert result.success is False
        assert result.error is not None
        assert "IMAP error" in result.error
        assert "Connection refused" in result.error
        # Assert it includes the troubleshooting hint
        assert "Check your credentials" in result.error
        assert "app password" in result.error

    def test_imap_error_logs_debug(self, caplog):
        """IMAPError from ``_poll`` is logged at debug with exc_info."""
        import logging

        caplog.set_level(logging.DEBUG)

        with (
            patch(DB),
            patch(f"{SR}._poll", side_effect=IMAPError("Timeout")),
        ):
            handle_poll_inbox(**_BASE_KWARGS)

        assert any("IMAP poll failed" in rec.message for rec in caplog.records)

    # ------------------------------------------------------------------
    # Empty inbox path  (messages falsy → line 77-78)
    # ------------------------------------------------------------------

    def test_empty_inbox_returns_success_with_no_messages(self):
        """When ``_poll`` returns an empty list, the result reports zero messages."""
        with (
            patch(DB),
            patch(f"{SR}._poll", return_value=[]),
        ):
            result = handle_poll_inbox(**_BASE_KWARGS)

        assert result.success is True
        assert result.data["total_fetched"] == 0
        assert result.data["total_matched"] == 0
        assert result.data["messages"] == []
        assert "No new messages found." in result.data["message"]
        assert "Fetched 0 messages" in result.data["message"]

    # ------------------------------------------------------------------
    # Successful poll — message matching flow  (lines 50-76)
    # ------------------------------------------------------------------

    def test_poll_with_matched_messages_submits_replies(self):
        """Messages matched to requests are passed to ``submit_inbox_reply``."""
        matched = [
            {**_MSG_1, "request_id": 1, "match_method": "thread"},
            {**_MSG_2, "request_id": 2, "match_method": "subject"},
        ]

        with (
            patch(DB),
            patch(f"{SR}._poll", return_value=[_MSG_1, _MSG_2]),
            patch(
                f"{SR}.list_removal_requests",
                return_value=[
                    {"id": 1, "broker_id": "spokeo"},
                    {"id": 2, "broker_id": "intelius"},
                ],
            ),
            patch(f"{SR}.get_events_for_requests", return_value={}),
            patch(f"{SR}.match_reply_to_request", return_value=matched),
            patch(f"{SR}.insert_inbox_reply") as submit_mock,
        ):
            result = handle_poll_inbox(**_BASE_KWARGS)

        assert result.success is True
        assert result.data["total_fetched"] == 2
        assert result.data["total_matched"] == 2
        assert len(result.data["messages"]) == 2
        assert submit_mock.call_count == 2

        # Verify insert_inbox_reply was called with correct data
        call_1 = submit_mock.call_args_list[0]
        assert call_1.kwargs["request_id"] == 1
        assert call_1.kwargs["from_addr"] == "broker@spokeo.com"
        assert call_1.kwargs["subject"] == "Re: Data Deletion Request"
        # Body truncated to 200 chars
        assert len(call_1.kwargs["snippet"]) <= 200

    def test_poll_with_unmatched_messages_counts_them_correctly(self):
        """Messages with ``request_id=None`` are counted as unmatched
        and displayed as ``[None]`` (the key exists, default not used)."""
        matched = [
            {**_MSG_1, "request_id": 1, "match_method": "thread"},
            {**_MSG_3, "request_id": None, "match_method": None},  # unmatched
        ]

        with (
            patch(DB),
            patch(f"{SR}._poll", return_value=[_MSG_1, _MSG_3]),
            patch(f"{SR}.list_removal_requests", return_value=[{"id": 1, "broker_id": "spokeo"}]),
            patch(f"{SR}.get_events_for_requests", return_value={}),
            patch(f"{SR}.match_reply_to_request", return_value=matched),
            patch(f"{SR}.insert_inbox_reply"),
        ):
            result = handle_poll_inbox(**_BASE_KWARGS)

        assert result.data["total_fetched"] == 2
        assert result.data["total_matched"] == 1  # only the one with request_id
        assert "[None]" in result.data["message"]
        assert "[1]" in result.data["message"]

    def test_poll_submits_only_matched_messages(self):
        """Only messages returned by ``match_reply_to_request`` are submitted."""
        matched = [{**_MSG_1, "request_id": 1, "match_method": "thread"}]

        with (
            patch(DB),
            patch(f"{SR}._poll", return_value=[_MSG_1, _MSG_2, _MSG_3]),
            patch(f"{SR}.list_removal_requests", return_value=[{"id": 1, "broker_id": "spokeo"}]),
            patch(f"{SR}.get_events_for_requests", return_value={}),
            patch(f"{SR}.match_reply_to_request", return_value=matched),
            patch(f"{SR}.insert_inbox_reply") as submit_mock,
        ):
            result = handle_poll_inbox(**_BASE_KWARGS)

        assert submit_mock.call_count == 1
        assert result.data["total_matched"] == 1

    def test_poll_with_no_matching_requests_still_succeeds(self):
        """Messages exist but no requests → no match, no submissions, zero matched."""
        with (
            patch(DB),
            patch(f"{SR}._poll", return_value=[_MSG_1, _MSG_2]),
            patch(f"{SR}.list_removal_requests", return_value=[]),
            patch(f"{SR}.get_events_for_requests"),
            patch(f"{SR}.match_reply_to_request", return_value=[]),
            patch(f"{SR}.insert_inbox_reply") as submit_mock,
        ):
            result = handle_poll_inbox(**_BASE_KWARGS)

        assert result.success is True
        assert result.data["total_fetched"] == 2
        assert result.data["total_matched"] == 0
        assert result.data["messages"] == []
        submit_mock.assert_not_called()

    # ------------------------------------------------------------------
    # Thread-map building from SENT events  (lines 58-66)
    # ------------------------------------------------------------------

    def test_thread_map_built_from_sent_events(self):
        """SENT events populate the thread_map for ``match_reply_to_request``."""
        events = {
            1: [
                {
                    "event_type": "SENT",
                    "payload_json": {"message_id": "<sent-msg-1@ex.com>"},
                },
            ],
            2: [
                {
                    "event_type": "SENT",
                    "payload_json": {"message_id": "<sent-msg-2@ex.com>"},
                },
                {
                    "event_type": "PLANNED",
                    "payload_json": {},
                },
            ],
        }

        with (
            patch(DB),
            patch(f"{SR}._poll", return_value=[_MSG_1]),
            patch(
                f"{SR}.list_removal_requests",
                return_value=[
                    {"id": 1, "broker_id": "spokeo"},
                    {"id": 2, "broker_id": "intelius"},
                ],
            ),
            patch(f"{SR}.get_events_for_requests", return_value=events),
            patch(f"{SR}.match_reply_to_request") as match_mock,
            patch(f"{SR}.insert_inbox_reply"),
        ):
            handle_poll_inbox(**_BASE_KWARGS)

        # ``match_reply_to_request`` receives thread_map as 3rd positional arg
        _call_args = match_mock.call_args.args
        thread_map = _call_args[2] if len(_call_args) >= 3 else {}
        assert thread_map == {
            "<sent-msg-1@ex.com>": 1,
            "<sent-msg-2@ex.com>": 2,
        }

    def test_no_sent_events_leaves_thread_map_empty(self):
        """When no SENT events exist, an empty thread_map is passed to the matcher."""
        events = {
            1: [{"event_type": "PLANNED", "payload_json": {}}],
        }

        with (
            patch(DB),
            patch(f"{SR}._poll", return_value=[_MSG_1]),
            patch(
                f"{SR}.list_removal_requests",
                return_value=[
                    {"id": 1, "broker_id": "spokeo"},
                ],
            ),
            patch(f"{SR}.get_events_for_requests", return_value=events),
            patch(f"{SR}.match_reply_to_request") as match_mock,
            patch(f"{SR}.insert_inbox_reply"),
        ):
            handle_poll_inbox(**_BASE_KWARGS)

        _call_args = match_mock.call_args.args
        thread_map = _call_args[2] if len(_call_args) >= 3 else {}
        assert thread_map == {}

    def test_payload_with_no_message_id_skipped_in_thread_map(self):
        """SENT events without a ``message_id`` in ``payload_json`` are ignored."""
        events = {
            1: [
                {
                    "event_type": "SENT",
                    "payload_json": {},  # no message_id
                },
            ],
        }

        with (
            patch(DB),
            patch(f"{SR}._poll", return_value=[_MSG_1]),
            patch(
                f"{SR}.list_removal_requests",
                return_value=[
                    {"id": 1, "broker_id": "spokeo"},
                ],
            ),
            patch(f"{SR}.get_events_for_requests", return_value=events),
            patch(f"{SR}.match_reply_to_request") as match_mock,
            patch(f"{SR}.insert_inbox_reply"),
        ):
            handle_poll_inbox(**_BASE_KWARGS)

        _call_args = match_mock.call_args.args
        thread_map = _call_args[2] if len(_call_args) >= 3 else {}
        assert thread_map == {}

    def test_payload_with_non_dict_payload_json_skipped(self):
        """SENT events with a non-dict ``payload_json`` are skipped gracefully."""
        events = {
            1: [
                {
                    "event_type": "SENT",
                    "payload_json": "some-string",  # not a dict
                },
            ],
        }

        with (
            patch(DB),
            patch(f"{SR}._poll", return_value=[_MSG_1]),
            patch(
                f"{SR}.list_removal_requests",
                return_value=[
                    {"id": 1, "broker_id": "spokeo"},
                ],
            ),
            patch(f"{SR}.get_events_for_requests", return_value=events),
            patch(f"{SR}.match_reply_to_request") as match_mock,
            patch(f"{SR}.insert_inbox_reply"),
        ):
            handle_poll_inbox(**_BASE_KWARGS)

        _call_args = match_mock.call_args.args
        thread_map = _call_args[2] if len(_call_args) >= 3 else {}
        assert thread_map == {}

    # ------------------------------------------------------------------
    # Request list edge cases
    # ------------------------------------------------------------------

    def test_no_requests_skips_event_lookup(self):
        """When ``list_removal_requests`` returns nothing, ``get_events_for_requests``
        is not called, but ``match_reply_to_request`` still runs."""
        with (
            patch(DB),
            patch(f"{SR}._poll", return_value=[_MSG_1]),
            patch(f"{SR}.list_removal_requests", return_value=[]),
            patch(f"{SR}.get_events_for_requests") as events_mock,
            patch(f"{SR}.match_reply_to_request") as match_mock,
            patch(f"{SR}.insert_inbox_reply"),
        ):
            handle_poll_inbox(**_BASE_KWARGS)

        events_mock.assert_not_called()
        match_mock.assert_called_once()
        _call_args = match_mock.call_args.args
        assert _call_args[1] == []  # requests is 2nd positional arg
        # thread_map is 3rd positional arg (may be absent if mock returns 2 args)
        if len(_call_args) >= 3:
            assert _call_args[2] == {}

    def test_requests_without_id_are_skipped(self):
        """Requests missing ``id``/``request_id`` keys are excluded from event lookup."""
        with (
            patch(DB),
            patch(f"{SR}._poll", return_value=[_MSG_1]),
            patch(
                f"{SR}.list_removal_requests",
                return_value=[
                    {"broker_id": "spokeo"},  # no id field!
                    {"request_id": 2, "broker_id": "intelius"},
                ],
            ),
            patch(f"{SR}.get_events_for_requests") as events_mock,
            patch(f"{SR}.match_reply_to_request"),
            patch(f"{SR}.insert_inbox_reply"),
        ):
            handle_poll_inbox(**_BASE_KWARGS)

        # Only request with id=2 should be passed
        events_mock.assert_called_once_with([2])

    # ------------------------------------------------------------------
    # Result formatting  (lines 80-97)
    # ------------------------------------------------------------------

    def test_result_message_contains_matched_subjects(self):
        """The output message includes subject lines for matched messages."""
        matched = [
            {**_MSG_1, "request_id": 1, "match_method": "thread"},
        ]

        with (
            patch(DB),
            patch(f"{SR}._poll", return_value=[_MSG_1]),
            patch(f"{SR}.list_removal_requests", return_value=[{"id": 1}]),
            patch(f"{SR}.get_events_for_requests", return_value={}),
            patch(f"{SR}.match_reply_to_request", return_value=matched),
            patch(f"{SR}.insert_inbox_reply"),
        ):
            result = handle_poll_inbox(**_BASE_KWARGS)

        msg = result.data["message"]
        assert "Fetched 1 messages" in msg
        assert "Matched to requests: 1" in msg
        assert "[1]" in msg
        assert "Re: Data Deletion Request" in msg

    def test_result_format_type_is_cli_result(self):
        """The function returns a ``CliResult`` named-tuple-like object."""
        with (
            patch(DB),
            patch(f"{SR}._poll", return_value=[_MSG_1]),
            patch(f"{SR}.list_removal_requests", return_value=[{"id": 1}]),
            patch(f"{SR}.get_events_for_requests", return_value={}),
            patch(
                f"{SR}.match_reply_to_request",
                return_value=[{**_MSG_1, "request_id": 1, "match_method": "thread"}],
            ),
            patch(f"{SR}.insert_inbox_reply"),
        ):
            result = handle_poll_inbox(**_BASE_KWARGS)

        from symeraseme.core.result_types import CliResult

        assert isinstance(result, CliResult)
        assert result.success is True
        assert isinstance(result.data, dict)

    # ------------------------------------------------------------------
    # Database dependency (@with_db)
    # ------------------------------------------------------------------

    def test_with_db_calls_init_db(self):
        """The ``@with_db`` decorator triggers ``init_db()`` before the handler."""
        with (
            patch(DB) as init_mock,
            patch(f"{SR}._poll", return_value=[]),
        ):
            handle_poll_inbox(**_BASE_KWARGS)

        init_mock.assert_called_once()

    def test_campaign_id_passed_to_list_removal_requests(self):
        """``campaign_id`` is forwarded to ``list_removal_requests``."""
        with (
            patch(DB),
            patch(f"{SR}._poll", return_value=[_MSG_1]),
            patch(f"{SR}.list_removal_requests") as list_mock,
            patch(f"{SR}.get_events_for_requests", return_value={}),
            patch(
                f"{SR}.match_reply_to_request",
                return_value=[{**_MSG_1, "request_id": 1, "match_method": "thread"}],
            ),
            patch(f"{SR}.insert_inbox_reply"),
        ):
            handle_poll_inbox(**{**_BASE_KWARGS, "campaign_id": "initial"})

        list_mock.assert_called_once_with(campaign_id="initial")

    def test_password_not_leaked_in_result(self):
        """Sensitive credential (password) must not appear in the returned result."""
        with (
            patch(DB),
            patch(f"{SR}._poll", side_effect=IMAPError("fail")),
        ):
            result = handle_poll_inbox(**_BASE_KWARGS)

        assert "app-password" not in (result.error or "")

    # ------------------------------------------------------------------
    # Parameter forwarding to ``_poll``
    # ------------------------------------------------------------------

    def test_poll_parameters_forwarded(self):
        """Host, port, username, password, ssl, and since_days are forwarded to ``_poll``."""
        with (
            patch(DB),
            patch(f"{SR}._poll") as poll_mock,
            patch(f"{SR}.list_removal_requests", return_value=[]),
            patch(f"{SR}.match_reply_to_request", return_value=[]),
            patch(f"{SR}.insert_inbox_reply"),
        ):
            handle_poll_inbox(**_BASE_KWARGS)

        poll_mock.assert_called_once_with(
            host="imap.example.com",
            port=993,
            username="user@example.com",
            password="app-password",
            ssl=True,
            folder="INBOX",
            since_days=7,
        )

    # ------------------------------------------------------------------
    # Interaction: matched messages produce formatted output line per message
    # ------------------------------------------------------------------

    def test_each_matched_message_has_output_line(self):
        """Every matched message gets a line in the output message."""
        matched = [
            {**_MSG_1, "request_id": 1, "match_method": "thread"},
            {**_MSG_2, "request_id": 2, "match_method": "subject"},
            {**_MSG_3, "request_id": None, "match_method": None},
        ]

        with (
            patch(DB),
            patch(f"{SR}._poll", return_value=[_MSG_1, _MSG_2, _MSG_3]),
            patch(f"{SR}.list_removal_requests", return_value=[]),
            patch(f"{SR}.get_events_for_requests", return_value={}),
            patch(f"{SR}.match_reply_to_request", return_value=matched),
            patch(f"{SR}.insert_inbox_reply"),
        ):
            result = handle_poll_inbox(**_BASE_KWARGS)

        lines = result.data["message"].split("\n")
        msg_lines = [l for l in lines if l.startswith("  [")]
        assert len(msg_lines) == 3
        assert "[1]" in msg_lines[0]
        assert "[2]" in msg_lines[1]
        assert "[None]" in msg_lines[2]
