from __future__ import annotations

import os
import tempfile

import pytest

from symeraseme.core.db import close_connection, get_connection, init_db
from symeraseme.core.events import append_event, create_removal_request
from symeraseme.core.projection import upsert_state
from symeraseme.core.reply_manager import (
    CLASSIFICATIONS_NEEDING_REPLY,
    _fallback_rebuttal,
    draft_reply,
    get_reply,
    list_replies,
    send_reply,
)


@pytest.fixture(autouse=True)
def _db(tmp_path: tempfile.TemporaryDirectory) -> None:
    db_file = tmp_path / "test.db"
    old = os.environ.get("SYMERASEME_DB_DIR")
    os.environ["SYMERASEME_DB_DIR"] = str(tmp_path)
    close_connection()
    init_db(str(db_file))
    yield
    close_connection()
    if old:
        os.environ["SYMERASEME_DB_DIR"] = old
    else:
        os.environ.pop("SYMERASEME_DB_DIR", None)


def _ensure_request(conn, request_id: int) -> None:
    cur = conn.execute("SELECT id FROM removal_requests WHERE id = ?", (request_id,))
    if cur.fetchone() is None:
        conn.execute(
            "INSERT INTO removal_requests (id, broker_id, channel, campaign_id, jurisdiction) "
            "VALUES (?, ?, ?, ?, ?)",
            (request_id, "test-broker", "email", "test", "GDPR"),
        )
        conn.commit()


def _insert_inbox_reply(
    conn,
    *,
    request_id: int | None = None,
    classified_as: str | None = None,
    from_addr: str = "broker@example.com",
    subject: str = "Re: Data Deletion Request",
    snippet: str = "We have received your request.",
) -> int:
    if request_id is not None:
        _ensure_request(conn, request_id)
    cur = conn.execute(
        """INSERT INTO inbox_replies
           (request_id, message_id, from_addr, subject, snippet, classified_as)
           VALUES (?, ?, ?, ?, ?, ?)""",
        (request_id, f"msg-{os.urandom(4).hex()}", from_addr, subject, snippet, classified_as),
    )
    conn.commit()
    return cur.lastrowid


class TestListReplies:
    def test_empty(self):
        assert list_replies() == []

    def test_all_replies(self):
        conn = get_connection()
        _insert_inbox_reply(conn, classified_as="ack")
        _insert_inbox_reply(conn, classified_as="verification")
        replies = list_replies()
        assert len(replies) == 2

    def test_filter_unclassified(self):
        conn = get_connection()
        _insert_inbox_reply(conn, classified_as="ack")
        _insert_inbox_reply(conn, classified_as=None)
        replies = list_replies(status="unclassified")
        assert len(replies) == 1
        assert replies[0]["classified_as"] is None

    def test_filter_classified(self):
        conn = get_connection()
        _insert_inbox_reply(conn, classified_as="ack")
        _insert_inbox_reply(conn, classified_as=None)
        replies = list_replies(status="classified")
        assert len(replies) == 1
        assert replies[0]["classified_as"] == "ack"

    def test_filter_needs_reply(self):
        conn = get_connection()
        _insert_inbox_reply(conn, classified_as="ack")
        _insert_inbox_reply(conn, classified_as="rejected")
        _insert_inbox_reply(conn, classified_as="verification")
        _insert_inbox_reply(conn, classified_as="confirmed")
        replies = list_replies(status="needs_reply")
        assert len(replies) == 2
        labels = {r["classified_as"] for r in replies}
        assert labels == {"rejected", "verification"}

    def test_filter_needs_verification(self):
        conn = get_connection()
        _insert_inbox_reply(conn, classified_as="verification")
        _insert_inbox_reply(conn, classified_as="ack")
        replies = list_replies(status="needs_verification")
        assert len(replies) == 1
        assert replies[0]["classified_as"] == "verification"

    def test_filter_by_request_id(self):
        conn = get_connection()
        _insert_inbox_reply(conn, request_id=1, classified_as="ack")
        _insert_inbox_reply(conn, request_id=2, classified_as="ack")
        replies = list_replies(request_id=1)
        assert len(replies) == 1

    def test_filter_drafted(self):
        conn = get_connection()
        r1 = _insert_inbox_reply(conn, classified_as="rejected")
        _insert_inbox_reply(conn, classified_as="rejected")
        _ensure_request(conn, 1)
        conn.execute(
            "INSERT INTO reply_drafts (reply_id, request_id, draft_body, subject) "
            "VALUES (?, 1, 'draft body', 'subject')",
            (r1,),
        )
        conn.commit()
        replies = list_replies(status="drafted")
        assert len(replies) == 1
        assert replies[0]["id"] == r1

    def test_filter_sent(self):
        conn = get_connection()
        r1 = _insert_inbox_reply(conn, classified_as="rejected")
        _insert_inbox_reply(conn, classified_as="rejected")
        _ensure_request(conn, 1)
        conn.execute(
            "INSERT INTO reply_drafts (reply_id, request_id, draft_body, subject, sent_at) "
            "VALUES (?, 1, 'draft body', 'subject', datetime('now'))",
            (r1,),
        )
        conn.commit()
        replies = list_replies(status="sent")
        assert len(replies) == 1
        assert replies[0]["id"] == r1


class TestGetReply:
    def test_not_found(self):
        assert get_reply(999) is None

    def test_basic_fields(self):
        conn = get_connection()
        rid = _insert_inbox_reply(conn, classified_as="ack", subject="Test Subject")
        reply = get_reply(rid)
        assert reply is not None
        assert reply["id"] == rid
        assert reply["classified_as"] == "ack"
        assert reply["subject"] == "Test Subject"

    def test_includes_draft(self):
        conn = get_connection()
        rid = _insert_inbox_reply(conn, classified_as="rejected")
        _ensure_request(conn, 1)
        conn.execute(
            "INSERT INTO reply_drafts (reply_id, request_id, draft_body, subject) "
            "VALUES (?, 1, 'draft text', 'Re: subject')",
            (rid,),
        )
        conn.commit()
        reply = get_reply(rid)
        assert reply is not None
        assert reply["draft_body"] == "draft text"
        assert reply["draft_subject"] == "Re: subject"


class TestDraftReply:
    def test_reply_not_found(self):
        with pytest.raises(ValueError, match="not found"):
            draft_reply(999)

    def test_reply_no_request(self):
        conn = get_connection()
        rid = _insert_inbox_reply(conn, request_id=None, classified_as="rejected")
        with pytest.raises(ValueError, match="not linked"):
            draft_reply(rid)

    def test_creates_draft(self):
        conn = get_connection()
        req_id = create_removal_request(
            broker_id="test-broker",
            channel="email",
            campaign_id="test",
            jurisdiction="GDPR",
        )
        append_event(req_id, "PLANNED", payload={"broker_name": "Test Broker"})
        upsert_state(req_id)
        rid = _insert_inbox_reply(conn, request_id=req_id, classified_as="rejected")

        result = draft_reply(rid)
        assert result["reply_id"] == rid
        assert result["request_id"] == req_id
        assert "draft_body" in result
        assert "Re: Data Deletion Request" in result["subject"]

    def test_idempotent(self):
        conn = get_connection()
        req_id = create_removal_request(
            broker_id="test-broker",
            channel="email",
            campaign_id="test",
            jurisdiction="GDPR",
        )
        append_event(req_id, "PLANNED")
        upsert_state(req_id)
        rid = _insert_inbox_reply(conn, request_id=req_id, classified_as="rejected")

        r1 = draft_reply(rid)
        r2 = draft_reply(rid)
        assert r1["draft_id"] == r2["draft_id"]

    def test_creates_event(self):
        conn = get_connection()
        req_id = create_removal_request(
            broker_id="test-broker",
            channel="email",
            campaign_id="test",
            jurisdiction="GDPR",
        )
        append_event(req_id, "PLANNED")
        upsert_state(req_id)
        rid = _insert_inbox_reply(conn, request_id=req_id, classified_as="rejected")

        draft_reply(rid)
        from symeraseme.core.events import get_events

        events = get_events(req_id)
        event_types = [e["event_type"] for e in events]
        assert "REPLY_DRAFTED" in event_types


class TestSendReply:
    def test_reply_not_found(self):
        with pytest.raises(ValueError, match="not found"):
            send_reply(999)

    def test_reply_no_request(self):
        conn = get_connection()
        rid = _insert_inbox_reply(conn, request_id=None, classified_as="ack")
        with pytest.raises(ValueError, match="not linked"):
            send_reply(rid)

    def test_dry_run_creates_draft(self):
        conn = get_connection()
        req_id = create_removal_request(
            broker_id="test-broker",
            channel="email",
            campaign_id="test",
            jurisdiction="GDPR",
        )
        append_event(req_id, "PLANNED")
        upsert_state(req_id)
        rid = _insert_inbox_reply(conn, request_id=req_id, classified_as="verification")

        result = send_reply(rid, dry_run=True)
        assert result["dry_run"] is True
        assert result["reply_id"] == rid

    def test_already_sent(self):
        conn = get_connection()
        req_id = create_removal_request(
            broker_id="test-broker",
            channel="email",
            campaign_id="test",
            jurisdiction="GDPR",
        )
        append_event(req_id, "PLANNED")
        upsert_state(req_id)
        rid = _insert_inbox_reply(conn, request_id=req_id, classified_as="ack")
        conn.execute(
            "INSERT INTO reply_drafts (reply_id, request_id, draft_body, subject, sent_at) "
            "VALUES (?, ?, 'draft', 'subj', datetime('now'))",
            (rid, req_id),
        )
        conn.commit()

        result = send_reply(rid)
        assert result["already_sent"] is True

    def test_no_from_addr_raises(self):
        conn = get_connection()
        req_id = create_removal_request(
            broker_id="test-broker",
            channel="email",
            campaign_id="test",
            jurisdiction="GDPR",
        )
        append_event(req_id, "PLANNED")
        upsert_state(req_id)
        rid = _insert_inbox_reply(conn, request_id=req_id, classified_as="ack", from_addr="")

        with pytest.raises(ValueError, match="no sender"):
            send_reply(rid, dry_run=False)


class TestFallbackRebutal:
    def test_general_rebuttal(self):
        text = _fallback_rebuttal(
            broker_name="TestBroker",
            classification="rejected",
            reply_snippet="Your request is denied.",
        )
        assert "TestBroker" in text
        assert "denied" in text
        assert "data protection law" in text

    def test_verification_rebuttal(self):
        text = _fallback_rebuttal(
            broker_name="TestBroker",
            classification="verification",
            reply_snippet="Please verify your identity.",
        )
        assert "additional information" in text
        assert "verify my identity" in text
        assert "confirmation" in text


class TestClassificationsNeedingReply:
    def test_contains_expected(self):
        assert "rejected" in CLASSIFICATIONS_NEEDING_REPLY
        assert "verification" in CLASSIFICATIONS_NEEDING_REPLY
        assert "human_required" in CLASSIFICATIONS_NEEDING_REPLY
        assert "unclear" in CLASSIFICATIONS_NEEDING_REPLY
        assert "ack" not in CLASSIFICATIONS_NEEDING_REPLY
        assert "confirmed" not in CLASSIFICATIONS_NEEDING_REPLY
