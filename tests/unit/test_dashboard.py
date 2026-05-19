"""Tests for the HTML status dashboard."""

from __future__ import annotations

import json
import os

import pytest


def _seed_db(tmp_path: str) -> None:
    """Seed a test database with campaign and request data."""
    from openeraseme.core.db import close_connection, get_connection, init_db

    os.environ["OPENERASEME_DB_DIR"] = tmp_path
    close_connection()
    init_db(tmp_path + "/test.db")

    conn = get_connection()

    conn.execute("INSERT INTO campaigns (id, kind) VALUES ('test-camp-1', 'initial')")
    conn.execute("INSERT INTO campaigns (id, kind) VALUES ('test-camp-2', 're-scan')")

    conn.execute(
        "INSERT INTO removal_requests (id, broker_id, channel, campaign_id, jurisdiction) "
        "VALUES (1, 'broker-a', 'email', 'test-camp-1', 'GDPR')"
    )
    conn.execute(
        "INSERT INTO request_state (request_id, current_status, sent_at) "
        "VALUES (1, 'CONFIRMED', '2026-06-01T10:00:00')"
    )

    conn.execute(
        "INSERT INTO removal_requests (id, broker_id, channel, campaign_id, jurisdiction) "
        "VALUES (2, 'broker-b', 'email', 'test-camp-1', 'CCPA')"
    )
    conn.execute(
        "INSERT INTO request_state (request_id, current_status, sent_at) "
        "VALUES (2, 'REJECTED_FINAL', '2026-06-01T10:00:00')"
    )

    conn.execute(
        "INSERT INTO removal_requests (id, broker_id, channel, campaign_id, jurisdiction) "
        "VALUES (3, 'broker-c', 'web_form', 'test-camp-2', 'GDPR')"
    )
    conn.execute(
        "INSERT INTO request_state (request_id, current_status, sent_at) "
        "VALUES (3, 'OVERDUE', '2026-06-01T10:00:00')"
    )

    conn.execute(
        "INSERT INTO removal_requests (id, broker_id, channel, campaign_id, jurisdiction) "
        "VALUES (4, 'broker-a', 'email', 'test-camp-2', 'GDPR')"
    )
    conn.execute(
        "INSERT INTO request_state (request_id, current_status, sent_at) "
        "VALUES (4, 'AWAITING_ACK', '2026-06-01T10:00:00')"
    )

    conn.execute(
        "INSERT INTO removal_requests (id, broker_id, channel, campaign_id, jurisdiction) "
        "VALUES (5, 'broker-d', 'web_form', 'test-camp-1', 'GDPR')"
    )
    conn.execute(
        "INSERT INTO request_state (request_id, current_status) "
        "VALUES (5, 'PLANNED')"
    )

    # Add some events
    for req_id in range(1, 6):
        conn.execute(
            "INSERT INTO request_events (request_id, event_type, source) "
            "VALUES (?, 'PLANNED', 'system')",
            (req_id,),
        )

    conn.commit()


def _clean_db() -> None:
    from openeraseme.core.db import close_connection

    close_connection()
    os.environ.pop("OPENERASEME_DB_DIR", None)


class TestGetDashboardData:
    def test_returns_dict_with_structure(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.dashboard import get_dashboard_data

            data = get_dashboard_data()

            assert "campaigns" in data
            assert "total_requests" in data
            assert "broker_status" in data
            assert "recent_events" in data
            assert "generated_at" in data
        finally:
            _clean_db()

    def test_returns_correct_counts(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.dashboard import get_dashboard_data

            data = get_dashboard_data()
            assert data["total_requests"] >= 5
            assert data["confirmed"] >= 1
            assert data["rejected"] >= 1
        finally:
            _clean_db()

    def test_campaign_specific_filter(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.dashboard import get_dashboard_data

            data = get_dashboard_data(campaign_id="test-camp-1")
            assert len(data["campaigns"]) == 1
            assert data["campaigns"][0]["id"] == "test-camp-1"
        finally:
            _clean_db()

    def test_handles_no_db(self, tmp_path):
        db_path = str(tmp_path / "empty.db")
        os.environ["OPENERASEME_DB_DIR"] = str(tmp_path)
        from openeraseme.core.db import close_connection, init_db

        close_connection()
        init_db(db_path)
        try:
            from openeraseme.core.dashboard import get_dashboard_data

            data = get_dashboard_data()
            assert data["total_requests"] == 0
            assert data["campaigns"] == []
        finally:
            _clean_db()

    def test_broker_status_aggregation(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.dashboard import get_dashboard_data

            data = get_dashboard_data()
            broker_ids = {b["broker_id"] for b in data["broker_status"]}
            assert "broker-a" in broker_ids
            assert "broker-b" in broker_ids
            assert "broker-c" in broker_ids
        finally:
            _clean_db()

    def test_recent_events_included(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.dashboard import get_dashboard_data

            data = get_dashboard_data()
            assert len(data["recent_events"]) >= 5
        finally:
            _clean_db()


class TestGenerateDashboard:
    def test_returns_html_string(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.dashboard import generate_dashboard, get_dashboard_data

            data = get_dashboard_data()
            html = generate_dashboard(data)
            assert isinstance(html, str)
            assert len(html) > 100
        finally:
            _clean_db()

    def test_html_contains_expected_elements(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.dashboard import generate_dashboard, get_dashboard_data

            data = get_dashboard_data()
            html = generate_dashboard(data)
            assert "OpenEraseMe Dashboard" in html
            assert "Campaigns" in html
            assert "Broker Status" in html
            assert "Recent Events" in html
        finally:
            _clean_db()

    def test_html_is_valid_doctype(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.dashboard import generate_dashboard, get_dashboard_data

            data = get_dashboard_data()
            html = generate_dashboard(data)
            assert html.startswith("<!DOCTYPE html>")
            assert "</html>" in html
        finally:
            _clean_db()

    def test_auto_refresh_meta_included(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.dashboard import generate_dashboard, get_dashboard_data

            data = get_dashboard_data()
            html = generate_dashboard(data, auto_refresh_seconds=30)
            assert "http-equiv=\"refresh\"" in html
            assert "content=\"30\"" in html
        finally:
            _clean_db()

    def test_empty_data_renders_no_error(self):
        from openeraseme.core.dashboard import generate_dashboard

        data = {
            "campaigns": [],
            "total_requests": 0,
            "planned": 0,
            "sent": 0,
            "awaiting_ack": 0,
            "awaiting_response": 0,
            "confirmed": 0,
            "rejected": 0,
            "overdue": 0,
            "broker_status": [],
            "recent_events": [],
            "generated_at": "2026-01-01T00:00:00",
        }
        html = generate_dashboard(data)
        assert "No requests yet" in html or "No campaigns" in html

    def test_has_dark_mode_support(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.dashboard import generate_dashboard, get_dashboard_data

            data = get_dashboard_data()
            html = generate_dashboard(data)
            assert "prefers-color-scheme: dark" in html
        finally:
            _clean_db()

    def test_has_responsive_viewport(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.dashboard import generate_dashboard, get_dashboard_data

            data = get_dashboard_data()
            html = generate_dashboard(data)
            assert "viewport" in html
        finally:
            _clean_db()
