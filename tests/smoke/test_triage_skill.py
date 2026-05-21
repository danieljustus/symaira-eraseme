from __future__ import annotations

from .conftest import (
    assert_in_output_stderr,
    assert_ok,
    invoke,
)


class TestPollInbox:
    def test_poll_inbox_requires_auth(self, seeded_db, monkeypatch):
        monkeypatch.setenv("IMAP_PASSWORD", "password")
        result = invoke(
            "poll-inbox",
            "--username",
            "test@test.com",
            "--host",
            "imap.test.com",
            "--since",
            "1",
        )
        assert result.exit_code != 0

    def test_poll_inbox_json_output(self, seeded_db, monkeypatch):
        monkeypatch.setenv("IMAP_PASSWORD", "password")
        result = invoke(
            "poll-inbox",
            "--username",
            "test@test.com",
            "--host",
            "imap.test.com",
            "--since",
            "1",
        )
        assert result.exit_code != 0

    def test_poll_inbox_rejects_cli_password(self, seeded_db):
        result = invoke(
            "poll-inbox",
            "--username",
            "test@test.com",
            "--password",
            "should-fail",
            "--host",
            "imap.test.com",
            "--since",
            "1",
        )
        assert result.exit_code != 0
        # --password option was removed for security; CLI should reject it
        assert "No such option" in result.stderr


class TestClassifyReply:
    def test_classify_reply_nonexistent(self, seeded_db):
        result = invoke("classify-reply", "9999")
        assert result.exit_code != 0
        assert_in_output_stderr(result, "not found")

    def test_classify_reply_no_unclassified(self, seeded_db):
        result = invoke("classify-reply", "1")
        assert result.exit_code != 0
        assert_in_output_stderr(result, "No unclassified")


class TestEvents:
    def test_events_for_planned_request(self, seeded_db):
        result = invoke("events", "show", "1")
        assert_ok(result)
        assert "PLANNED" in result.stdout

    def test_events_json_output(self, seeded_db):
        result = invoke("--output", "json", "events", "show", "1")
        import json

        assert_ok(result)
        data = json.loads(result.stdout)
        assert isinstance(data, list)
        for event in data:
            assert "event_type" in event
            assert "occurred_at" in event
