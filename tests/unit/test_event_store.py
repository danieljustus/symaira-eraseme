"""Tests for event store (db, events, projection)."""

from __future__ import annotations

import os
import tempfile

import pytest

from openeraseme.core.db import close_connection, get_connection, init_db
from openeraseme.core.events import (
    EVENT_TYPES,
    append_event,
    create_campaign,
    create_removal_request,
    get_events,
    get_removal_request,
    list_campaigns,
    list_removal_requests,
)
from openeraseme.core.projection import rebuild_all_states, rebuild_state, upsert_state


@pytest.fixture(autouse=True)
def _db(tmp_path: tempfile.TemporaryDirectory) -> None:
    db_file = tmp_path / "test.db"
    old = os.environ.get("OPENERASEME_DB_DIR")
    os.environ["OPENERASEME_DB_DIR"] = str(tmp_path)
    close_connection()
    init_db(str(db_file))
    yield
    close_connection()
    if old:
        os.environ["OPENERASEME_DB_DIR"] = old
    else:
        os.environ.pop("OPENERASEME_DB_DIR", None)


class TestDBInit:
    def test_init_creates_tables(self):
        conn = get_connection()
        tables = conn.execute(
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"
        ).fetchall()
        names = [r["name"] for r in tables]
        assert "removal_requests" in names
        assert "request_events" in names
        assert "request_state" in names
        assert "campaigns" in names
        assert "inbox_replies" in names

    def test_init_is_idempotent(self):
        init_db()
        init_db()  # second call must not raise


class TestCampaigns:
    def test_create_and_list(self):
        create_campaign("test-campaign", kind="initial", notes="test")
        camps = list_campaigns()
        assert len(camps) >= 1
        assert camps[0]["id"] == "test-campaign"
        assert camps[0]["kind"] == "initial"

    def test_create_duplicate_is_ignored(self):
        create_campaign("dup")
        create_campaign("dup")  # no error
        camps = list_campaigns()
        assert sum(1 for c in camps if c["id"] == "dup") == 1


class TestRemovalRequests:
    def test_create_and_retrieve(self):
        create_campaign("camp1")
        rid = create_removal_request(
            broker_id="acxiom",
            campaign_id="camp1",
            jurisdiction="GDPR-DE",
            template_id="gdpr-art17.de.md.j2",
        )
        assert rid > 0
        req = get_removal_request(rid)
        assert req is not None
        assert req["broker_id"] == "acxiom"
        assert req["campaign_id"] == "camp1"

    def test_list_by_campaign(self):
        create_campaign("camp-a")
        create_campaign("camp-b")
        create_removal_request(broker_id="b1", campaign_id="camp-a", jurisdiction="GDPR")
        create_removal_request(broker_id="b2", campaign_id="camp-a", jurisdiction="CCPA")
        create_removal_request(broker_id="b3", campaign_id="camp-b", jurisdiction="GDPR")

        camp_a = list_removal_requests(campaign_id="camp-a")
        assert len(camp_a) == 2
        camp_b = list_removal_requests(campaign_id="camp-b")
        assert len(camp_b) == 1

    def test_list_by_status(self):
        create_campaign("c1")
        rid = create_removal_request(broker_id="b1", campaign_id="c1", jurisdiction="GDPR")
        append_event(rid, "SENT", payload={"to": "test@example.com"})
        upsert_state(rid)

        results = list_removal_requests(status="AWAITING_ACK")
        assert any(r["id"] == rid for r in results)

    def test_list_by_broker(self):
        create_campaign("c1")
        create_removal_request(broker_id="broker-a", campaign_id="c1", jurisdiction="GDPR")
        create_removal_request(broker_id="broker-b", campaign_id="c1", jurisdiction="GDPR")
        results = list_removal_requests(broker_id="broker-a")
        assert len(results) == 1
        assert results[0]["broker_id"] == "broker-a"


class TestEventStore:
    def test_append_event(self):
        create_campaign("c")
        rid = create_removal_request(broker_id="b", campaign_id="c", jurisdiction="GDPR")
        eid = append_event(rid, "PLANNED", payload={"plan": "test"})
        assert eid > 0

    def test_append_event_validates_type(self):
        create_campaign("c")
        rid = create_removal_request(broker_id="b", campaign_id="c", jurisdiction="GDPR")
        with pytest.raises(ValueError, match="Unknown event type"):
            append_event(rid, "INVALID_TYPE")

    def test_get_events(self):
        create_campaign("c")
        rid = create_removal_request(broker_id="b", campaign_id="c", jurisdiction="GDPR")
        append_event(rid, "PLANNED", payload={"plan": "test"})
        append_event(rid, "SENT", payload={"to": "x@y.com"})
        events = get_events(rid)
        assert len(events) == 2
        assert events[0]["event_type"] == "PLANNED"
        assert events[1]["event_type"] == "SENT"

    def test_events_ordered_by_occurred_at(self):
        create_campaign("c")
        rid = create_removal_request(broker_id="b", campaign_id="c", jurisdiction="GDPR")
        append_event(rid, "SENT", occurred_at="2025-01-02T00:00:00")
        append_event(rid, "PLANNED", occurred_at="2025-01-01T00:00:00")
        events = get_events(rid)
        assert events[0]["event_type"] == "PLANNED"
        assert events[1]["event_type"] == "SENT"

    def test_get_events_after_id(self):
        create_campaign("c")
        rid = create_removal_request(broker_id="b", campaign_id="c", jurisdiction="GDPR")
        e1 = append_event(rid, "PLANNED")
        append_event(rid, "SENT")
        later = get_events(rid, after_event_id=e1)
        assert len(later) == 1
        assert later[0]["event_type"] == "SENT"

    def test_event_types_are_frozen(self):
        assert isinstance(EVENT_TYPES, frozenset)
        assert "PLANNED" in EVENT_TYPES
        assert "SENT" in EVENT_TYPES
        assert "CONFIRMED" in EVENT_TYPES


class TestProjection:
    def test_rebuild_state_planned(self):
        create_campaign("c")
        rid = create_removal_request(broker_id="b", campaign_id="c", jurisdiction="GDPR")
        append_event(rid, "PLANNED")
        state = rebuild_state(rid)
        assert state["current_status"] == "PLANNED"
        assert state["last_event_id"] > 0

    def test_rebuild_state_sent(self):
        create_campaign("c")
        rid = create_removal_request(broker_id="b", campaign_id="c", jurisdiction="GDPR")
        append_event(rid, "PLANNED")
        append_event(rid, "SENT", payload={"to": "x@y.com", "expected_response_days": 30})
        state = rebuild_state(rid)
        assert state["current_status"] == "AWAITING_ACK"
        assert state["sent_at"] is not None
        assert state["deadline_at"] is not None

    def test_rebuild_state_full_flow(self):
        create_campaign("c")
        rid = create_removal_request(broker_id="b", campaign_id="c", jurisdiction="GDPR")
        append_event(rid, "PLANNED")
        append_event(rid, "SENT")
        append_event(rid, "ACK")
        append_event(rid, "CONFIRMED")
        state = rebuild_state(rid)
        assert state["current_status"] == "CONFIRMED"
        assert state["resolved_at"] is not None

    def test_rebuild_state_send_failed(self):
        create_campaign("c")
        rid = create_removal_request(broker_id="b", campaign_id="c", jurisdiction="GDPR")
        append_event(rid, "PLANNED")
        append_event(rid, "SEND_FAILED", payload={"error": "SMTP timeout"})
        state = rebuild_state(rid)
        assert state["current_status"] == "SEND_FAILED"

    def test_upsert_state_persists(self):
        create_campaign("c")
        rid = create_removal_request(broker_id="b", campaign_id="c", jurisdiction="GDPR")
        append_event(rid, "SENT")
        upsert_state(rid)
        conn = get_connection()
        row = conn.execute(
            "SELECT current_status, sent_at FROM request_state WHERE request_id = ?",
            (rid,),
        ).fetchone()
        assert row is not None
        assert row["current_status"] == "AWAITING_ACK"
        assert row["sent_at"] is not None

    def test_rebuild_all_states(self):
        create_campaign("c")
        r1 = create_removal_request(broker_id="b1", campaign_id="c", jurisdiction="GDPR")
        r2 = create_removal_request(broker_id="b2", campaign_id="c", jurisdiction="GDPR")
        append_event(r1, "PLANNED")
        append_event(r2, "PLANNED")
        count = rebuild_all_states()
        assert count == 2
