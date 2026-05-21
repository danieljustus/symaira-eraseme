"""Tests for IMAP polling and thread matching."""

from __future__ import annotations

from openeraseme.adapters.email.smtp_imap import (
    decode_mime_header,
    extract_thread_id,
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
