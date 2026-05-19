"""Tests for aggregated campaign reports."""

from __future__ import annotations

import json
import os

import pytest


def _seed_db(tmp_path: str) -> None:
    """Seed a test database with multi-campaign data for report testing."""
    from openeraseme.core.db import close_connection, get_connection, init_db

    os.environ["OPENERASEME_DB_DIR"] = tmp_path
    close_connection()
    init_db(tmp_path + "/test.db")

    conn = get_connection()

    # Campaign 1
    conn.execute(
        "INSERT INTO campaigns (id, kind) VALUES ('camp-initial', 'initial')"
    )

    conn.execute(
        "INSERT INTO removal_requests (id, broker_id, channel, campaign_id, jurisdiction) "
        "VALUES (1, 'broker-a', 'email', 'camp-initial', 'GDPR')"
    )
    conn.execute(
        "INSERT INTO request_state (request_id, current_status, sent_at, resolved_at) "
        "VALUES (1, 'CONFIRMED', '2026-06-01T10:00:00', '2026-06-15T10:00:00')"
    )

    conn.execute(
        "INSERT INTO removal_requests (id, broker_id, channel, campaign_id, jurisdiction) "
        "VALUES (2, 'broker-b', 'email', 'camp-initial', 'CCPA')"
    )
    conn.execute(
        "INSERT INTO request_state (request_id, current_status, sent_at) "
        "VALUES (2, 'REJECTED_FINAL', '2026-06-01T10:00:00')"
    )

    conn.execute(
        "INSERT INTO removal_requests (id, broker_id, channel, campaign_id, jurisdiction) "
        "VALUES (3, 'broker-c', 'email', 'camp-initial', 'GDPR')"
    )
    conn.execute(
        "INSERT INTO request_state (request_id, current_status, sent_at, deadline_at) "
        "VALUES (3, 'OVERDUE', '2026-06-01T10:00:00', '2026-07-01T10:00:00')"
    )

    conn.execute(
        "INSERT INTO removal_requests (id, broker_id, channel, campaign_id, jurisdiction) "
        "VALUES (4, 'broker-d', 'email', 'camp-initial', 'GDPR')"
    )
    conn.execute(
        "INSERT INTO request_state (request_id, current_status) "
        "VALUES (4, 'PLANNED')"
    )

    # Campaign 2 (for historical comparison - older campaign)
    # Insert as an older campaign so it sorts correctly
    conn.execute(
        "INSERT INTO campaigns (id, kind, created_at) VALUES ('camp-rescan', 're-scan', '2026-01-01T00:00:00')"
    )

    conn.execute(
        "INSERT INTO removal_requests (id, broker_id, channel, campaign_id, jurisdiction) "
        "VALUES (5, 'broker-a', 'email', 'camp-rescan', 'GDPR')"
    )
    conn.execute(
        "INSERT INTO request_state (request_id, current_status, sent_at, resolved_at) "
        "VALUES (5, 'CONFIRMED', '2026-07-01T10:00:00', '2026-07-10T10:00:00')"
    )

    conn.execute(
        "INSERT INTO removal_requests (id, broker_id, channel, campaign_id, jurisdiction) "
        "VALUES (6, 'broker-e', 'web_form', 'camp-rescan', 'CCPA')"
    )
    conn.execute(
        "INSERT INTO request_state (request_id, current_status, sent_at) "
        "VALUES (6, 'AWAITING_ACK', '2026-07-01T10:00:00')"
    )

    # Events
    for req_id in range(1, 7):
        conn.execute(
            "INSERT INTO request_events (request_id, event_type, source, occurred_at) "
            "VALUES (?, 'PLANNED', 'system', ?)",
            (req_id, f"2026-06-0{req_id}T10:00:00"),
        )

    conn.commit()


def _clean_db() -> None:
    from openeraseme.core.db import close_connection

    close_connection()
    os.environ.pop("OPENERASEME_DB_DIR", None)


class TestGetReportData:
    def test_returns_dict_with_structure(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.reports import get_report_data

            data = get_report_data()
            assert "campaigns" in data
            assert "total_requests" in data
            assert "status_breakdown" in data
            assert "broker_leaderboard" in data
            assert "jurisdiction_stats" in data
            assert "success_metrics" in data
        finally:
            _clean_db()

    def test_empty_db_returns_empty_report(self):
        from openeraseme.core.reports import get_report_data

        data = get_report_data(campaign_id="nonexistent")
        assert data["total_requests"] == 0
        assert "error" in data

    def test_specific_campaign(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.reports import get_report_data

            data = get_report_data(campaign_id="camp-initial")
            assert len(data["campaigns"]) == 1
            assert data["campaigns"][0]["campaign_id"] == "camp-initial"
        finally:
            _clean_db()

    def test_all_campaigns(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.reports import get_report_data

            data = get_report_data(all_campaigns=True)
            assert data["total_campaigns"] == 2
        finally:
            _clean_db()

    def test_status_breakdown(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.reports import get_report_data

            data = get_report_data(campaign_id="camp-initial")
            sb = data["status_breakdown"]
            assert sb.get("CONFIRMED", 0) >= 1
            assert sb.get("REJECTED_FINAL", 0) >= 1
            assert sb.get("OVERDUE", 0) >= 1
            assert sb.get("PLANNED", 0) >= 1
        finally:
            _clean_db()

    def test_broker_leaderboard(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.reports import get_report_data

            data = get_report_data(all_campaigns=True)
            assert len(data["broker_leaderboard"]) >= 4
            leader = data["broker_leaderboard"][0]
            assert "broker_id" in leader
            assert "success_rate" in leader
            assert "avg_response_time_days" in leader
        finally:
            _clean_db()

    def test_jurisdiction_stats(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.reports import get_report_data

            data = get_report_data(all_campaigns=True)
            jurisdictions = {j["jurisdiction"] for j in data["jurisdiction_stats"]}
            assert "GDPR" in jurisdictions
            assert "CCPA" in jurisdictions
        finally:
            _clean_db()

    def test_success_metrics(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.reports import get_report_data

            data = get_report_data(all_campaigns=True)
            sm = data["success_metrics"]
            assert sm["overall_confirmation_rate"] > 0
            assert sm["avg_response_time_days"] is not None
            assert sm["median_response_time_days"] is not None
        finally:
            _clean_db()

    def test_historical_comparison(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.reports import get_report_data

            data = get_report_data(all_campaigns=True)
            hc = data["historical_comparison"]
            # camp-initial is more recent (created now), camp-rescan is older
            assert hc["latest_campaign"] == "camp-initial"
            assert hc["previous_campaign"] == "camp-rescan"
            assert "confirmation_rate_change" in hc
        finally:
            _clean_db()


class TestAggregateCampaign:
    def test_aggregates_counts_correctly(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.events import list_removal_requests
            from openeraseme.core.reports import _aggregate_campaign

            reqs = list_removal_requests(campaign_id="camp-initial")
            camp = {
                "id": "camp-initial",
                "requests": reqs,
            }
            agg = _aggregate_campaign(camp)
            assert agg["total"] == 4
            assert agg["confirmed"] == 1
            assert agg["rejected"] == 1
            assert agg["overdue"] == 1
            assert agg["planned"] == 1
            assert agg["confirmation_rate"] == 25.0
        finally:
            _clean_db()

    def test_empty_campaign_returns_zero_counts(self):
        from openeraseme.core.reports import _aggregate_campaign

        agg = _aggregate_campaign({"id": "empty", "requests": []})
        assert agg["total"] == 0
        assert agg["confirmed"] == 0
        assert agg["confirmation_rate"] == 0.0


class TestExportJson:
    def test_json_is_parseable(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.reports import export_json, get_report_data

            data = get_report_data(all_campaigns=True)
            json_str = export_json(data)
            parsed = json.loads(json_str)
            assert parsed["total_campaigns"] == 2
            assert "campaigns" in parsed
        finally:
            _clean_db()

    def test_json_empty_report(self):
        from openeraseme.core.reports import _empty_report, export_json

        data = _empty_report("nonexistent")
        json_str = export_json(data)
        parsed = json.loads(json_str)
        assert parsed["total_requests"] == 0


class TestExportCsv:
    def test_csv_has_header(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.reports import export_csv, get_report_data

            data = get_report_data(all_campaigns=True)
            csv_str = export_csv(data)
            assert "campaign_id" in csv_str
            assert "request_id" in csv_str
            assert "broker_id" in csv_str
            assert "status" in csv_str
        finally:
            _clean_db()

    def test_csv_contains_data_rows(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.reports import export_csv, get_report_data

            data = get_report_data(all_campaigns=True)
            csv_str = export_csv(data)
            rows = csv_str.strip().split("\n")
            # Header + data rows
            assert len(rows) >= 3
            assert "CONFIRMED" in csv_str or "PLANNED" in csv_str
        finally:
            _clean_db()

    def test_csv_empty_data(self):
        from openeraseme.core.reports import _empty_report, export_csv

        data = _empty_report("nonexistent")
        csv_str = export_csv(data)
        assert csv_str.strip() != ""


class TestExportHtml:
    def test_html_template_renders(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.reports import export_html, get_report_data

            data = get_report_data(all_campaigns=True)
            html = export_html(data)
            assert "<!DOCTYPE html>" in html
            assert "Campaign Report" in html
            assert "camp-initial" in html
        finally:
            _clean_db()

    def test_html_empty_data(self):
        from openeraseme.core.reports import _empty_report, export_html

        data = _empty_report("nonexistent")
        html = export_html(data)
        assert "<!DOCTYPE html>" in html
        assert "Campaign Report" in html


class TestSuccessMetrics:
    def test_empty_requests(self):
        from openeraseme.core.reports import _success_metrics

        result = _success_metrics([])
        assert result == {}

    def test_all_confirmed(self):
        from openeraseme.core.reports import _success_metrics

        requests = [
            {"id": 1, "current_status": "CONFIRMED", "sent_at": "2026-06-01T10:00:00", "resolved_at": "2026-06-15T10:00:00"},
            {"id": 2, "current_status": "CONFIRMED", "sent_at": "2026-06-01T10:00:00", "resolved_at": "2026-06-20T10:00:00"},
        ]
        sm = _success_metrics(requests)
        assert sm["overall_confirmation_rate"] == 100.0
        assert sm["overall_rejection_rate"] == 0.0
        assert sm["avg_response_time_days"] is not None
        assert sm["median_response_time_days"] is not None

    def test_mixed_statuses(self):
        from openeraseme.core.reports import _success_metrics

        requests = [
            {"id": 1, "current_status": "CONFIRMED"},
            {"id": 2, "current_status": "REJECTED_FINAL"},
            {"id": 3, "current_status": "OVERDUE"},
            {"id": 4, "current_status": "PLANNED"},
        ]
        sm = _success_metrics(requests)
        assert sm["overall_confirmation_rate"] == 25.0
        assert sm["overall_rejection_rate"] == 25.0
        assert sm["overdue_rate"] == 25.0

    def test_response_time_calculation(self):
        from openeraseme.core.reports import _success_metrics

        requests = [
            {"id": 1, "current_status": "CONFIRMED", "sent_at": "2026-06-01T10:00:00", "resolved_at": "2026-06-11T10:00:00"},
        ]
        sm = _success_metrics(requests)
        assert sm["avg_response_time_days"] == 10.0


class TestBrokerLeaderboard:
    def test_returns_sorted_by_total(self):
        from openeraseme.core.reports import _broker_leaderboard

        requests = [
            {"id": 1, "broker_id": "broker-a", "current_status": "CONFIRMED"},
            {"id": 2, "broker_id": "broker-a", "current_status": "CONFIRMED"},
            {"id": 3, "broker_id": "broker-b", "current_status": "REJECTED_FINAL"},
        ]
        board = _broker_leaderboard(requests)
        assert len(board) == 2
        assert board[0]["broker_id"] == "broker-a"
        assert board[1]["broker_id"] == "broker-b"
        assert board[0]["success_rate"] == 100.0
        assert board[1]["success_rate"] == 0.0

    def test_empty_requests(self):
        from openeraseme.core.reports import _broker_leaderboard

        assert _broker_leaderboard([]) == []


class TestJurisdictionBreakdown:
    def test_gdpr_and_ccpa(self):
        from openeraseme.core.reports import _jurisdiction_breakdown

        requests = [
            {"id": 1, "jurisdiction": "GDPR", "current_status": "CONFIRMED"},
            {"id": 2, "jurisdiction": "GDPR", "current_status": "REJECTED_FINAL"},
            {"id": 3, "jurisdiction": "CCPA", "current_status": "CONFIRMED"},
        ]
        stats = _jurisdiction_breakdown(requests)
        jm = {j["jurisdiction"]: j for j in stats}
        assert "GDPR" in jm
        assert "CCPA" in jm
        assert jm["GDPR"]["confirmation_rate"] == 50.0
        assert jm["CCPA"]["confirmation_rate"] == 100.0


class TestGenerateReport:
    def test_html_format(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.reports import generate_report, get_report_data

            data = get_report_data(all_campaigns=True)
            result = generate_report(data, format="html")
            assert isinstance(result, str)
            assert "Campaign Report" in result
        finally:
            _clean_db()

    def test_json_format(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.reports import generate_report, get_report_data

            data = get_report_data(all_campaigns=True)
            result = generate_report(data, format="json")
            assert isinstance(result, str)
            parsed = json.loads(result)
            assert parsed["total_campaigns"] == 2
        finally:
            _clean_db()

    def test_csv_format(self, tmp_path):
        _seed_db(str(tmp_path))
        try:
            from openeraseme.core.reports import generate_report, get_report_data

            data = get_report_data(all_campaigns=True)
            result = generate_report(data, format="csv")
            assert isinstance(result, str)
            assert "campaign_id" in result
        finally:
            _clean_db()

    def test_invalid_format_raises(self):
        from openeraseme.core.reports import generate_report

        with pytest.raises(ValueError, match="Unsupported format"):
            generate_report({"campaigns": []}, format="xml")


class TestMedian:
    def test_odd_count(self):
        from openeraseme.core.reports import _median

        assert _median([1.0, 2.0, 3.0]) == 2.0

    def test_even_count(self):
        from openeraseme.core.reports import _median

        assert _median([1.0, 2.0, 3.0, 4.0]) == 2.5

    def test_single_value(self):
        from openeraseme.core.reports import _median

        assert _median([42.0]) == 42.0

    def test_empty_returns_zero(self):
        from openeraseme.core.reports import _median

        assert _median([]) == 0.0
