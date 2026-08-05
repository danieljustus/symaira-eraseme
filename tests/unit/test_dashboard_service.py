"""Tests for the dashboard service handlers."""

from __future__ import annotations

import json
import sqlite3
from unittest.mock import MagicMock, patch
import pytest

from symeraseme.core.result_types import CliResult
from symeraseme.services.dashboard import (
    handle_get_dashboard_data,
    handle_list_requests,
    handle_get_events,
    handle_get_calendar,
    handle_list_brokers,
    handle_get_profile,
    handle_export_data,
)


class TestHandleGetDashboardData:
    def test_success(self):
        mock_data = {
            "campaigns": [{"id": "camp-1"}],
            "total_requests": 10,
            "broker_status": [],
            "recent_events": [],
            "generated_at": "2026-07-20T12:00:00",
        }
        with patch("symeraseme.core.dashboard.get_dashboard_data", return_value=mock_data) as mock_get:
            result = handle_get_dashboard_data()
            assert result.success is True
            assert result.data == mock_data
            mock_get.assert_called_once()

    def test_failure(self):
        with patch("symeraseme.core.dashboard.get_dashboard_data", side_effect=ValueError("Database error")) as mock_get:
            result = handle_get_dashboard_data()
            assert result.success is False
            assert "Failed to fetch dashboard data" in result.error
            assert "Database error" in result.error
            mock_get.assert_called_once()

    def test_sqlite_failure_sanitized(self):
        """Issue #619: raw sqlite text must never reach the client."""
        with patch(
            "symeraseme.core.dashboard.get_dashboard_data",
            side_effect=sqlite3.OperationalError("no such table: campaigns"),
        ) as mock_get:
            result = handle_get_dashboard_data()
            assert result.success is False
            assert result.error is not None
            assert "no such table" not in result.error
            assert "campaigns" not in result.error
            assert "Database not ready" in result.error
            mock_get.assert_called_once()


class TestHandleListRequests:
    def test_success(self):
        mock_items = [{"id": 1, "broker_id": "broker-a", "status": "PLANNED"}]
        with (
            patch("symeraseme.core.repositories.count_removal_requests", return_value=1) as mock_count,
            patch("symeraseme.core.repositories.list_removal_requests", return_value=mock_items) as mock_list,
        ):
            result = handle_list_requests(campaign_id="camp-1", status="PLANNED", broker_id="broker-a", page=2, page_size=10)
            assert result.success is True
            assert result.data["page"] == 2
            assert result.data["page_size"] == 10
            assert result.data["total"] == 1
            assert result.data["items"] == mock_items
            mock_count.assert_called_once_with(campaign_id="camp-1", status="PLANNED")
            mock_list.assert_called_once_with(
                campaign_id="camp-1",
                status="PLANNED",
                broker_id="broker-a",
                limit=10,
                offset=10,
            )

    def test_failure(self):
        with patch("symeraseme.core.repositories.count_removal_requests", side_effect=Exception("Connection lost")):
            result = handle_list_requests()
            assert result.success is False
            assert "Failed to list requests" in result.error


class TestHandleGetEvents:
    def test_success(self):
        mock_events = [{"id": 1, "event_type": "SENT", "payload": {}}]
        with patch("symeraseme.core.repositories.get_events", return_value=mock_events) as mock_get_events:
            result = handle_get_events(request_id=42, after_event_id=10)
            assert result.success is True
            assert result.data["request_id"] == 42
            assert result.data["events"] == mock_events
            mock_get_events.assert_called_once_with(42, after_event_id=10)

    def test_failure(self):
        with patch("symeraseme.core.repositories.get_events", side_effect=Exception("Read failure")):
            result = handle_get_events(request_id=42)
            assert result.success is False
            assert "Failed to get events" in result.error


class TestHandleGetCalendar:
    def test_success(self):
        mock_status = [{"campaign_id": "c1", "due_date": "2026-08-01"}]
        mock_action = MagicMock()
        mock_action.request_id = 1
        mock_action.broker_id = "b1"
        mock_action.campaign_id = "c1"
        mock_action.current_status = "SENT"
        mock_action.action_type = "check"
        mock_action.event_type = "tock"
        mock_action.description = "desc"
        mock_action.payload = {}
        mock_action.dry_run = True

        with (
            patch("symeraseme.core.reports.data.get_campaign_status", return_value=mock_status) as mock_status_fn,
            patch("symeraseme.core.deadlines.run_tick", return_value=[mock_action]) as mock_tick_fn,
        ):
            result = handle_get_calendar(weeks=3, campaign_id="camp-1")
            assert result.success is True
            assert result.data["upcoming_deadlines"] == mock_status
            assert len(result.data["tick_actions"]) == 1
            assert result.data["tick_actions"][0]["request_id"] == 1
            assert result.data["weeks"] == 3
            mock_status_fn.assert_called_once_with(campaign_id="camp-1")
            mock_tick_fn.assert_called_once_with(dry_run=True)

    def test_failure(self):
        with patch("symeraseme.core.reports.data.get_campaign_status", side_effect=Exception("Tick error")):
            result = handle_get_calendar()
            assert result.success is False
            assert "Failed to build calendar" in result.error


class TestHandleListBrokers:
    def test_success(self):
        mock_broker = MagicMock()
        mock_broker.model_dump.return_value = {"id": "broker-x", "name": "Broker X"}
        with patch("symeraseme.registry.loader.load_all_brokers", return_value=[mock_broker]) as mock_load:
            result = handle_list_brokers(
                jurisdiction="GDPR",
                law="some-law",
                priority="high",
                category="people-search",
                include_disabled=True,
            )
            assert result.success is True
            assert result.data["brokers"] == [{"id": "broker-x", "name": "Broker X"}]
            assert result.data["total"] == 1
            mock_load.assert_called_once_with(
                jurisdiction="GDPR",
                law="some-law",
                priority="high",
                category="people-search",
                include_disabled=True,
                status="active",
                include_inactive=False,
            )

    def test_failure(self):
        with patch("symeraseme.registry.loader.load_all_brokers", side_effect=Exception("Registry error")):
            result = handle_list_brokers()
            assert result.success is False
            assert "Failed to list brokers" in result.error


class TestHandleGetProfile:
    def test_success(self):
        mock_profile = MagicMock()
        mock_profile.model_dump_json.return_value = '{"first_name": "John"}'
        with (
            patch("symeraseme.core.identity.profile_exists", return_value=True) as mock_exists,
            patch("symeraseme.core.identity.load_profile", return_value=mock_profile) as mock_load,
        ):
            result = handle_get_profile()
            assert result.success is True
            assert result.data == {"first_name": "John"}
            mock_exists.assert_called_once()
            mock_load.assert_called_once()

    def test_profile_missing(self):
        with patch("symeraseme.core.identity.profile_exists", return_value=False) as mock_exists:
            result = handle_get_profile()
            assert result.success is False
            assert result.error == "No profile found"
            mock_exists.assert_called_once()

    def test_failure(self):
        with (
            patch("symeraseme.core.identity.profile_exists", return_value=True),
            patch("symeraseme.core.identity.load_profile", side_effect=Exception("Decrypt error")),
        ):
            result = handle_get_profile()
            assert result.success is False
            assert "Failed to load profile" in result.error


class TestHandleExportData:
    def test_success(self):
        mock_report = {"summary": "done"}
        with patch("symeraseme.core.reports.data.get_report_data", return_value=mock_report) as mock_get_report:
            result = handle_export_data(format="json", campaign_id="camp-2")
            assert result.success is True
            assert result.data["format"] == "json"
            assert result.data["data"] == mock_report
            mock_get_report.assert_called_once_with(campaign_id="camp-2")

    def test_failure(self):
        with patch("symeraseme.core.reports.data.get_report_data", side_effect=Exception("Export failed")):
            result = handle_export_data()
            assert result.success is False
            assert "Failed to export data" in result.error


class TestWithDbDecoration:
    """Issue #619: dashboard read handlers guarantee schema creation.

    ``@with_db`` must ensure the schema exists even when the server was
    started by an older version (or another tool) that skipped init_db().
    """

    def test_dashboard_handler_creates_schema(self, tmp_path, monkeypatch):
        monkeypatch.setenv("SYMERASEME_DATA_DIR", str(tmp_path))
        with patch("symeraseme.core.dashboard.get_dashboard_data", return_value={}):
            result = handle_get_dashboard_data()
        assert result.success is True

        conn = sqlite3.connect(tmp_path / "symeraseme.db")
        try:
            tables = {
                row[0]
                for row in conn.execute(
                    "SELECT name FROM sqlite_master WHERE type='table'"
                )
            }
        finally:
            conn.close()
        assert {"campaigns", "removal_requests", "request_events"} <= tables

    def test_list_brokers_does_not_require_db(self):
        """list_brokers is registry-only; @with_db must not break it."""
        mock_broker = MagicMock()
        mock_broker.model_dump.return_value = {"id": "broker-x", "name": "Broker X"}
        with patch("symeraseme.registry.loader.load_all_brokers", return_value=[mock_broker]):
            result = handle_list_brokers()
        assert result.success is True
        assert result.data["total"] == 1
