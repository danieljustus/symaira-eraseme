"""Tests for the repository layer."""

from __future__ import annotations

import os

import pytest

from symeraseme.core.db_connection import close_connection, init_db
from symeraseme.core.repositories import (
    append_event,
    create_campaign,
    create_removal_request,
    get_events,
    get_events_for_requests,
    get_removal_request,
    list_campaigns,
    list_removal_requests,
)


@pytest.fixture(autouse=True)
def _db(tmp_path) -> None:
    old = os.environ.get("SYMERASEME_DB_DIR")
    os.environ["SYMERASEME_DB_DIR"] = str(tmp_path)
    close_connection()
    init_db()
    yield
    close_connection()
    if old:
        os.environ["SYMERASEME_DB_DIR"] = old
    else:
        os.environ.pop("SYMERASEME_DB_DIR", None)


class TestCampaignRepository:
    def test_create_and_list(self):
        create_campaign("repo-campaign", kind="initial", notes="test")
        camps = list_campaigns()
        assert any(c["id"] == "repo-campaign" for c in camps)

    def test_create_duplicate_is_detected(self):
        assert create_campaign("dup-repo") is True
        assert create_campaign("dup-repo") is False
        camps = list_campaigns()
        assert sum(1 for c in camps if c["id"] == "dup-repo") == 1


class TestRequestRepository:
    def test_create_and_retrieve(self):
        create_campaign("camp-repo")
        rid = create_removal_request(
            broker_id="acxiom",
            campaign_id="camp-repo",
            jurisdiction="GDPR-DE",
        )
        assert rid > 0
        req = get_removal_request(rid)
        assert req is not None
        assert req["broker_id"] == "acxiom"

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

    def test_list_by_broker(self):
        create_campaign("c1")
        create_removal_request(broker_id="broker-a", campaign_id="c1", jurisdiction="GDPR")
        create_removal_request(broker_id="broker-b", campaign_id="c1", jurisdiction="GDPR")
        results = list_removal_requests(broker_id="broker-a")
        assert len(results) == 1
        assert results[0]["broker_id"] == "broker-a"

    def test_pagination_limit(self):
        create_campaign("pag-camp")
        for i in range(10):
            create_removal_request(broker_id=f"b{i}", campaign_id="pag-camp", jurisdiction="GDPR")
        results = list_removal_requests(campaign_id="pag-camp", limit=5)
        assert len(results) == 5
        # Verify stable ordering (oldest first)
        assert results[0]["broker_id"] == "b0"
        assert results[4]["broker_id"] == "b4"

    def test_pagination_offset(self):
        create_campaign("pag-off")
        for i in range(10):
            create_removal_request(broker_id=f"b{i}", campaign_id="pag-off", jurisdiction="GDPR")
        results = list_removal_requests(campaign_id="pag-off", limit=3, offset=3)
        assert len(results) == 3
        assert results[0]["broker_id"] == "b3"
        assert results[2]["broker_id"] == "b5"

    def test_pagination_offset_without_limit_uses_negative_one(self):
        create_campaign("pag-nolimit")
        for i in range(5):
            create_removal_request(broker_id=f"b{i}", campaign_id="pag-nolimit", jurisdiction="GDPR")
        results = list_removal_requests(campaign_id="pag-nolimit", offset=2)
        assert len(results) == 3
        assert results[0]["broker_id"] == "b2"

    def test_pagination_limit_zero(self):
        create_campaign("pag-zero")
        create_removal_request(broker_id="b0", campaign_id="pag-zero", jurisdiction="GDPR")
        results = list_removal_requests(campaign_id="pag-zero", limit=0)
        assert len(results) == 0

    def test_pagination_offset_beyond_results(self):
        create_campaign("pag-beyond")
        for i in range(3):
            create_removal_request(broker_id=f"b{i}", campaign_id="pag-beyond", jurisdiction="GDPR")
        results = list_removal_requests(campaign_id="pag-beyond", limit=5, offset=10)
        assert len(results) == 0


class TestEventRepository:
    def test_append_and_get(self):
        create_campaign("c")
        rid = create_removal_request(broker_id="b", campaign_id="c", jurisdiction="GDPR")
        eid = append_event(rid, "PLANNED", payload={"plan": "test"})
        assert eid > 0

        events = get_events(rid)
        assert len(events) == 1
        assert events[0]["event_type"] == "PLANNED"

    def test_get_events_for_requests(self):
        create_campaign("c")
        rid1 = create_removal_request(broker_id="b1", campaign_id="c", jurisdiction="GDPR")
        rid2 = create_removal_request(broker_id="b2", campaign_id="c", jurisdiction="GDPR")
        append_event(rid1, "PLANNED")
        append_event(rid2, "SENT")

        result = get_events_for_requests([rid1, rid2])
        assert len(result[rid1]) == 1
        assert len(result[rid2]) == 1
        assert result[rid1][0]["event_type"] == "PLANNED"
        assert result[rid2][0]["event_type"] == "SENT"

    def test_get_events_after_id(self):
        create_campaign("c")
        rid = create_removal_request(broker_id="b", campaign_id="c", jurisdiction="GDPR")
        e1 = append_event(rid, "PLANNED")
        append_event(rid, "SENT")
        later = get_events(rid, after_event_id=e1)
        assert len(later) == 1
        assert later[0]["event_type"] == "SENT"
