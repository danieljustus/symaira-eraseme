"""Tests for IMAP polling and thread matching."""

from __future__ import annotations

from symeraseme.adapters.email.smtp_imap import (
    decode_mime_header,
    extract_thread_id,
    match_reply_to_request,
    normalize_subject,
    parse_email_body,
    subject_matches,
)


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
