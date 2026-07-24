"""Tests for IMAP polling and thread matching."""

from __future__ import annotations

from unittest.mock import MagicMock, patch

import pytest

from symeraseme.adapters.email.smtp_imap import (
    IMAPError,
    _resolve_imap_password,
    decode_mime_header,
    extract_thread_id,
    match_reply_to_request,
    normalize_subject,
    parse_email_body,
    poll_inbox,
    subject_matches,
)
from symeraseme.core.secrets import SecretResolutionError


class TestResolveImapPassword:
    def test_empty_password_returned_as_is(self):
        assert _resolve_imap_password("") == ""

    def test_literal_password_passed_through(self):
        assert _resolve_imap_password("plain-password") == "plain-password"

    @patch("symeraseme.adapters.email.smtp_imap.resolve_secret")
    def test_resolved_vault_secret_returned(self, mock_resolve):
        mock_resolve.return_value = "resolved-secret"
        assert _resolve_imap_password("vault://email/imap") == "resolved-secret"

    @patch("symeraseme.adapters.email.smtp_imap.resolve_secret")
    def test_unresolvable_vault_uri_raises_instead_of_using_literal(self, mock_resolve):
        mock_resolve.side_effect = SecretResolutionError("symvault not available")
        with pytest.raises(IMAPError, match="Cannot resolve IMAP password"):
            _resolve_imap_password("vault://email/imap")


class TestDecodeMimeHeader:
    def test_empty(self):
        assert decode_mime_header(None) == ""
        assert decode_mime_header("") == ""

    def test_plain_ascii(self):
        assert decode_mime_header("Hello World") == "Hello World"

    def test_encoded(self):
        result = decode_mime_header("=?UTF-8?Q?Re:_Data_Request?=")
        assert "Re: Data Request" in result


class TestNormalizeSubject:
    def test_removes_re_prefix(self):
        assert normalize_subject("Re: Hello") == "Hello"
        assert normalize_subject("RE: Hello") == "Hello"
        assert normalize_subject("Re: Re: Hello") == "Hello"

    def test_handles_multiple_prefixes(self):
        assert normalize_subject("AW: WG: Test") == "Test"

    def test_no_prefix(self):
        assert normalize_subject("Hello World") == "Hello World"

    def test_empty(self):
        assert normalize_subject("") == ""


class TestSubjectMatches:
    def test_exact_match(self):
        assert subject_matches("Data Deletion Request", "Data Deletion Request") is True

    def test_reply_match(self):
        assert subject_matches("Data Deletion Request", "Re: Data Deletion Request") is True

    def test_case_insensitive(self):
        assert subject_matches("Data Deletion", "data deletion") is True

    def test_no_match(self):
        assert subject_matches("Data Deletion", "Other Subject") is False


class TestExtractThreadId:
    def test_from_references(self):
        headers = {
            "References": "<ref1@a.com> <ref2@b.com>",
        }
        tid = extract_thread_id(headers)
        assert tid == "<ref1@a.com>"

    def test_from_in_reply_to(self):
        headers = {
            "In-Reply-To": "<parent@test.com>",
        }
        tid = extract_thread_id(headers)
        assert tid == "<parent@test.com>"

    def test_fallback_to_message_id(self):
        headers = {
            "Message-ID": "<self@test.com>",
        }
        tid = extract_thread_id(headers)
        assert tid == "<self@test.com>"

    def test_no_ids(self):
        assert extract_thread_id({}) is None


class TestParseEmailBody:
    def test_truncates_long_body(self):
        body = "x" * 1000
        result = parse_email_body(body, max_length=10)
        assert len(result) <= 13  # 10 + "..."
        assert result.endswith("...")

    def test_short_body(self):
        assert parse_email_body("Hello") == "Hello"

    def test_empty(self):
        assert parse_email_body("") == ""


class TestMatchReplyToRequest:
    def test_matches_by_thread_id(self):
        messages = [
            {
                "subject": "Re: Your data deletion request",
                "thread_id": "<abc123@example.com>",
            }
        ]
        requests = [
            {"id": 42, "broker_id": "TestBroker"},
        ]
        thread_map = {"<abc123@example.com>": 42}
        matched = match_reply_to_request(messages, requests, thread_map)
        assert matched[0]["request_id"] == 42
        assert matched[0]["match_method"] == "thread"

    def test_falls_back_to_subject_match(self):
        messages = [
            {
                "subject": "Re: Data Deletion Request \u2014 TestBroker",
                "thread_id": "",
            }
        ]
        requests = [
            {"id": 42, "broker_id": "TestBroker"},
        ]
        matched = match_reply_to_request(messages, requests)
        assert matched[0]["request_id"] == 42
        assert matched[0]["match_method"] == "subject"

    def test_unmatched_when_nothing_fits(self):
        messages = [
            {
                "subject": "Random email",
                "thread_id": "<unknown@example.com>",
            }
        ]
        requests = [
            {"id": 42, "broker_id": "TestBroker"},
        ]
        matched = match_reply_to_request(messages, requests)
        assert matched[0]["request_id"] is None
        assert matched[0]["match_method"] == "unmatched"

    def test_thread_match_takes_priority_over_subject(self):
        messages = [
            {
                "subject": "Data Deletion Request \u2014 OtherBroker",
                "thread_id": "<abc123@example.com>",
            }
        ]
        requests = [
            {"id": 1, "broker_id": "TestBroker"},
            {"id": 2, "broker_id": "OtherBroker"},
        ]
        thread_map = {"<abc123@example.com>": 1}
        matched = match_reply_to_request(messages, requests, thread_map)
        assert matched[0]["request_id"] == 1
        assert matched[0]["match_method"] == "thread"


class TestPollInboxFetchStrategy:
    """Verify poll_inbox uses UID-based search + batched UID FETCH."""

    def _make_header_bytes(self, subject="Re: Test", from_addr="b@x.com"):
        lines = [
            f"Subject: {subject}",
            f"From: {from_addr}",
            "Date: Mon, 21 Jul 2026 10:00:00 +0000",
            "Message-ID: <test@example.com>",
            "",
            "",
        ]
        return "\r\n".join(lines).encode()

    def _setup_mock_session(self, mock_session_fn):
        """Create a mock IMAP session that supports UID-based operations."""
        mock_mail = MagicMock()
        mock_mail.status.return_value = ("OK", [b"* STATUS INBOX (UIDVALIDITY 42)"])
        mock_session_fn.return_value.__enter__ = MagicMock(return_value=mock_mail)
        mock_session_fn.return_value.__exit__ = MagicMock(return_value=False)
        return mock_mail

    @patch("symeraseme.adapters.email.smtp_imap._resolve_imap_password", return_value="pw")
    @patch("symeraseme.adapters.email.smtp_imap._imap_session")
    @patch("symeraseme.core.repositories.inbox.get_imap_hwm", return_value=(None, None))
    @patch("symeraseme.core.repositories.inbox.set_imap_hwm")
    def test_fetch_command_uses_body_peek(
        self, _mock_set_hwm, _mock_get_hwm, mock_session_fn, _mock_pw
    ):
        header_bytes = self._make_header_bytes()
        body_bytes = b"Hello, this is the body text."

        mock_mail = self._setup_mock_session(mock_session_fn)

        def _mock_uid_side_effect(command, *args):
            if command == "SEARCH":
                return ("OK", [b"1"])
            if command == "FETCH":
                return (
                    "OK",
                    [
                        (
                            b"1 (UID 1 FLAGS (\\Seen) BODY[HEADER.FIELDS ...] {100}",
                            header_bytes,
                        ),
                        (b" UID 1 BODY[TEXT]<0.4096> {30}", body_bytes),
                    ],
                )
            return ("NO", [])

        mock_mail.uid = MagicMock(side_effect=_mock_uid_side_effect)

        result = poll_inbox(
            host="imap.test.com",
            port=993,
            username="u",
            password="pw",
            ssl=True,
        )

        assert len(result) == 1
        fetch_calls = [c for c in mock_mail.uid.call_args_list if c[0][0] == "FETCH"]
        assert len(fetch_calls) == 1
        fetch_cmd = fetch_calls[0][0][2]
        assert "BODY.PEEK[HEADER.FIELDS" in fetch_cmd
        assert "BODY.PEEK[TEXT]<0.4096>" in fetch_cmd
        assert "RFC822" not in fetch_cmd

    @patch("symeraseme.adapters.email.smtp_imap._resolve_imap_password", return_value="pw")
    @patch("symeraseme.adapters.email.smtp_imap._imap_session")
    @patch("symeraseme.core.repositories.inbox.get_imap_hwm", return_value=(None, None))
    @patch("symeraseme.core.repositories.inbox.set_imap_hwm")
    def test_returns_subject_and_body_from_peek(
        self, _mock_set_hwm, _mock_get_hwm, mock_session_fn, _mock_pw
    ):
        header_bytes = self._make_header_bytes(
            subject="Re: Data Request", from_addr="broker@x.com"
        )
        body_bytes = b"Your data has been removed."

        mock_mail = self._setup_mock_session(mock_session_fn)

        def _mock_uid_side_effect(command, *args):
            if command == "SEARCH":
                return ("OK", [b"1"])
            if command == "FETCH":
                return (
                    "OK",
                    [
                        (
                            b"1 (UID 1 FLAGS () BODY[HEADER.FIELDS ...] {100}",
                            header_bytes,
                        ),
                        (b" UID 1 BODY[TEXT]<0.4096> {30}", body_bytes),
                    ],
                )
            return ("NO", [])

        mock_mail.uid = MagicMock(side_effect=_mock_uid_side_effect)

        result = poll_inbox(
            host="imap.test.com",
            port=993,
            username="u",
            password="pw",
            ssl=True,
        )

        assert len(result) == 1
        msg = result[0]
        assert msg["subject"] == "Re: Data Request"
        assert msg["from_addr"] == "broker@x.com"
        assert msg["body"] == "Your data has been removed."
        assert msg["message_id"] == "<test@example.com>"

    @patch("symeraseme.adapters.email.smtp_imap._resolve_imap_password", return_value="pw")
    @patch("symeraseme.adapters.email.smtp_imap._imap_session")
    @patch("symeraseme.core.repositories.inbox.get_imap_hwm", return_value=(42, 5))
    @patch("symeraseme.core.repositories.inbox.set_imap_hwm")
    def test_uses_hwm_for_uid_range(
        self, mock_set_hwm, _mock_get_hwm, mock_session_fn, _mock_pw
    ):
        """When a valid HWM exists, SEARCH should use UID >= last_uid+1."""
        mock_mail = self._setup_mock_session(mock_session_fn)

        def _mock_uid_side_effect(command, *args):
            if command == "SEARCH":
                return ("OK", [b""])
            if command == "FETCH":
                return ("OK", [])
            return ("NO", [])

        mock_mail.uid = MagicMock(side_effect=_mock_uid_side_effect)

        result = poll_inbox(
            host="imap.test.com",
            port=993,
            username="u",
            password="pw",
            ssl=True,
        )

        assert result == []
        # SEARCH should use "6:*" (last_uid=5 -> 5+1=6)
        search_calls = [
            c for c in mock_mail.uid.call_args_list if c[0][0] == "SEARCH"
        ]
        assert len(search_calls) == 1
        assert search_calls[0][0][1] == "6:*"

    @patch("symeraseme.adapters.email.smtp_imap._resolve_imap_password", return_value="pw")
    @patch("symeraseme.adapters.email.smtp_imap._imap_session")
    @patch("symeraseme.core.repositories.inbox.get_imap_hwm", return_value=(99, 5))
    @patch("symeraseme.core.repositories.inbox.set_imap_hwm")
    def test_uidvalidity_mismatch_cold_starts(
        self, _mock_set_hwm, _mock_get_hwm, mock_session_fn, _mock_pw
    ):
        """When stored UIDVALIDITY != server UIDVALIDITY, starts from 1:*."""
        mock_mail = self._setup_mock_session(mock_session_fn)

        def _mock_uid_side_effect(command, *args):
            if command == "SEARCH":
                return ("OK", [b""])
            if command == "FETCH":
                return ("OK", [])
            return ("NO", [])

        mock_mail.uid = MagicMock(side_effect=_mock_uid_side_effect)

        result = poll_inbox(
            host="imap.test.com",
            port=993,
            username="u",
            password="pw",
            ssl=True,
        )

        assert result == []
        search_calls = [
            c for c in mock_mail.uid.call_args_list if c[0][0] == "SEARCH"
        ]
        assert len(search_calls) == 1
        assert search_calls[0][0][1] == "1:*"
