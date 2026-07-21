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
                "subject": "Re: Data Deletion Request — TestBroker",
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
                "subject": "Data Deletion Request — OtherBroker",
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
    """Verify poll_inbox uses BODY.PEEK with header fields + truncated text."""

    def _make_header_bytes(self, subject: str = "Re: Test", from_addr: str = "b@x.com") -> bytes:
        lines = [
            f"Subject: {subject}",
            f"From: {from_addr}",
            "Date: Mon, 21 Jul 2026 10:00:00 +0000",
            "Message-ID: <test@example.com>",
            "",
            "",
        ]
        return "\r\n".join(lines).encode()

    @patch("symeraseme.adapters.email.smtp_imap._resolve_imap_password", return_value="pw")
    @patch("symeraseme.adapters.email.smtp_imap._imap_session")
    def test_fetch_command_uses_body_peek(self, mock_session_fn, _mock_pw):
        header_bytes = self._make_header_bytes()
        body_bytes = b"Hello, this is the body text."

        mock_mail = MagicMock()
        mock_mail.search.return_value = ("OK", [b"1"])
        mock_mail.fetch.return_value = (
            "OK",
            [
                (b"1 FETCH (FLAGS (\\Seen) BODY[HEADER.FIELDS ...] {100}", header_bytes),
                (b" BODY[TEXT]<0.4096> {30}", body_bytes),
            ],
        )
        mock_session_fn.return_value.__enter__ = MagicMock(return_value=mock_mail)
        mock_session_fn.return_value.__exit__ = MagicMock(return_value=False)

        result = poll_inbox(
            host="imap.test.com", port=993, username="u", password="pw", ssl=True,
        )

        assert len(result) == 1
        fetch_call = mock_mail.fetch.call_args
        fetch_cmd = fetch_call[0][1]
        assert "BODY.PEEK[HEADER.FIELDS" in fetch_cmd
        assert "BODY.PEEK[TEXT]<0.4096>" in fetch_cmd
        assert "RFC822" not in fetch_cmd

    @patch("symeraseme.adapters.email.smtp_imap._resolve_imap_password", return_value="pw")
    @patch("symeraseme.adapters.email.smtp_imap._imap_session")
    def test_returns_subject_and_body_from_peek(self, mock_session_fn, _mock_pw):
        header_bytes = self._make_header_bytes(subject="Re: Data Request", from_addr="broker@x.com")
        body_bytes = b"Your data has been removed."

        mock_mail = MagicMock()
        mock_mail.search.return_value = ("OK", [b"1"])
        mock_mail.fetch.return_value = (
            "OK",
            [
                (b"1 FETCH (FLAGS () BODY[HEADER.FIELDS ...] {100}", header_bytes),
                (b" BODY[TEXT]<0.4096> {30}", body_bytes),
            ],
        )
        mock_session_fn.return_value.__enter__ = MagicMock(return_value=mock_mail)
        mock_session_fn.return_value.__exit__ = MagicMock(return_value=False)

        result = poll_inbox(
            host="imap.test.com", port=993, username="u", password="pw", ssl=True,
        )

        assert len(result) == 1
        msg = result[0]
        assert msg["subject"] == "Re: Data Request"
        assert msg["from_addr"] == "broker@x.com"
        assert msg["body"] == "Your data has been removed."
        assert msg["message_id"] == "<test@example.com>"
