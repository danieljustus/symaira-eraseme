"""Tests for the tick engine (deadlines, reminders, escalation, re-scan)."""

from __future__ import annotations

from datetime import UTC, datetime, timedelta
from typing import Any

from symeraseme.core.deadlines import (
    _check_deadline,
    _check_dpa_escalation,
    _check_reminder,
    _check_rescan,
    apply_tick_actions,
    run_tick,
)


def _make_req(overrides: dict[str, Any] | None = None) -> dict[str, Any]:
    base = {
        "id": 1,
        "broker_id": "test-broker",
        "campaign_id": "test-campaign",
        "jurisdiction": "GDPR",
        "current_status": "AWAITING_ACK",
        "sent_at": None,
        "deadline_at": None,
        "next_action_at": None,
        "acknowledged_at": None,
        "resolved_at": None,
        "reminders_sent": 0,
        "escalation_level": 0,
    }
    if overrides:
        base.update(overrides)
    return base


class TestCheckReminder:
    def test_no_reminder_if_recent(self):
        now = datetime(2026, 6, 1, tzinfo=UTC)
        sent = now - timedelta(days=3)
        req = _make_req({"sent_at": sent.isoformat()})
        result = _check_reminder(1, req, sent, now, 0)
        assert result is None

    def test_sends_reminder_after_7_days(self):
        now = datetime(2026, 6, 10, tzinfo=UTC)
        sent = now - timedelta(days=8)
        req = _make_req({"sent_at": sent.isoformat()})
        result = _check_reminder(1, req, sent, now, 0)
        assert result is not None
        assert result.action_type == "send_reminder"
        assert result.event_type == "REMINDER_SENT"

    def test_exponential_backoff(self):
        now = datetime(2026, 6, 20, tzinfo=UTC)
        sent = now - timedelta(days=10)
        req = _make_req({"sent_at": sent.isoformat()})
        # Already sent 1 reminder, next at 14 days (2^1 * 7)
        result = _check_reminder(1, req, sent, now, 1)
        assert result is None

    def test_backoff_eventually_triggers(self):
        now = datetime(2026, 6, 30, tzinfo=UTC)
        sent = now - timedelta(days=21)
        req = _make_req({"sent_at": sent.isoformat()})
        # 21 days, reminders_sent=1, next threshold = 2^1 * 7 = 14
        result = _check_reminder(1, req, sent, now, 1)
        assert result is not None

    def test_no_reminder_without_sent_at(self):
        now = datetime(2026, 6, 10, tzinfo=UTC)
        result = _check_reminder(1, _make_req(), None, now, 0)
        assert result is None


class TestCheckDeadline:
    def test_deadline_not_reached(self):
        now = datetime(2026, 7, 1, tzinfo=UTC)
        deadline = now + timedelta(days=10)
        result = _check_deadline(1, _make_req(), None, deadline, 30, now)
        assert result is None

    def test_deadline_reached_explicit(self):
        now = datetime(2026, 7, 1, tzinfo=UTC)
        deadline = now - timedelta(days=1)
        result = _check_deadline(1, _make_req(), None, deadline, 30, now)
        assert result is not None
        assert result.action_type == "mark_overdue"
        assert result.event_type == "DEADLINE_REACHED"

    def test_deadline_implicit_from_sent(self):
        now = datetime(2026, 7, 1, tzinfo=UTC)
        sent = now - timedelta(days=31)
        result = _check_deadline(1, _make_req(), sent, None, 30, now)
        assert result is not None
        assert result.event_type == "DEADLINE_REACHED"

    def test_deadline_not_reached_implicit(self):
        now = datetime(2026, 7, 1, tzinfo=UTC)
        sent = now - timedelta(days=20)
        result = _check_deadline(1, _make_req(), sent, None, 30, now)
        assert result is None


class TestCheckDPAEscalation:
    def test_no_escalation_without_deadline(self):
        now = datetime(2026, 7, 1, tzinfo=UTC)
        result = _check_dpa_escalation(1, _make_req(), None, now, 0)
        assert result is None

    def test_no_escalation_if_recent(self):
        now = datetime(2026, 7, 1, tzinfo=UTC)
        deadline = now - timedelta(days=5)
        result = _check_dpa_escalation(1, _make_req(), deadline, now, 0)
        assert result is None

    def test_escalation_after_14_days(self):
        now = datetime(2026, 7, 1, tzinfo=UTC)
        deadline = now - timedelta(days=15)
        result = _check_dpa_escalation(1, _make_req(), deadline, now, 0)
        assert result is not None
        assert result.action_type == "draft_dpa_complaint"
        assert result.event_type == "DPA_COMPLAINT_DRAFTED"

    def test_already_escalated(self):
        now = datetime(2026, 7, 1, tzinfo=UTC)
        deadline = now - timedelta(days=15)
        result = _check_dpa_escalation(1, _make_req(), deadline, now, 2)
        assert result is None


class TestCheckRescan:
    def test_no_rescan_if_recent(self):
        now = datetime(2026, 7, 1, tzinfo=UTC)
        resolved = now - timedelta(days=30)
        result = _check_rescan(1, _make_req(), resolved, now)
        assert result is None

    def test_triggers_rescan_after_90_days(self):
        now = datetime(2026, 7, 1, tzinfo=UTC)
        resolved = now - timedelta(days=91)
        result = _check_rescan(1, _make_req(), resolved, now)
        assert result is not None
        assert result.action_type == "trigger_rescan"
        assert result.event_type == "RE_SCAN_TRIGGERED"

    def test_no_rescan_without_resolved_at(self):
        now = datetime(2026, 7, 1, tzinfo=UTC)
        result = _check_rescan(1, _make_req(), None, now)
        assert result is None


class TestRunTick:
    def test_no_requests_returns_empty(self, tmp_path):
        import os

        from symeraseme.core.db import close_connection, init_db

        os.environ["SYMERASEME_DB_DIR"] = str(tmp_path)
        close_connection()
        init_db(str(tmp_path / "test.db"))

        try:
            actions = run_tick()
            assert actions == []
        finally:
            close_connection()
            os.environ.pop("SYMERASEME_DB_DIR", None)

    def test_dry_run_returns_actions(self, tmp_path):
        import os

        from symeraseme.core.db import close_connection, get_connection, init_db

        os.environ["SYMERASEME_DB_DIR"] = str(tmp_path)
        close_connection()
        init_db(str(tmp_path / "test.db"))

        now = datetime(2026, 7, 1, tzinfo=UTC)
        sent = now - timedelta(days=10)
        deadline = now - timedelta(days=1)

        try:
            conn = get_connection()
            conn.execute(
                "INSERT INTO removal_requests (id, broker_id, campaign_id, jurisdiction) "
                "VALUES (1, 'b', 'c', 'GDPR')"
            )
            conn.execute(
                """INSERT INTO request_state (request_id, current_status, sent_at, deadline_at,
                   reminders_sent, escalation_level, next_action_at)
                   VALUES (1, 'AWAITING_RESPONSE', ?, ?, 0, 0, ?)""",
                (sent.isoformat(), deadline.isoformat(), deadline.isoformat()),
            )
            conn.commit()

            actions = run_tick(reference_date=now)
            assert len(actions) == 1
            assert actions[0].action_type == "mark_overdue"
        finally:
            close_connection()
            os.environ.pop("SYMERASEME_DB_DIR", None)

    def test_multiple_states_ticked(self, tmp_path):
        import os

        from symeraseme.core.db import close_connection, get_connection, init_db

        os.environ["SYMERASEME_DB_DIR"] = str(tmp_path)
        close_connection()
        init_db(str(tmp_path / "test.db"))

        now = datetime(2026, 7, 1, tzinfo=UTC)
        sent = now - timedelta(days=10)
        resolved = now - timedelta(days=95)

        try:
            conn = get_connection()

            conn.execute(
                "INSERT INTO removal_requests (id, broker_id, campaign_id, jurisdiction) "
                "VALUES (1, 'a', 'c', 'GDPR')"
            )
            conn.execute(
                """INSERT INTO request_state (request_id, current_status, sent_at,
                   reminders_sent, escalation_level, next_action_at)
                   VALUES (1, 'AWAITING_ACK', ?, 0, 0, ?)""",
                (sent.isoformat(), sent.isoformat()),
            )

            conn.execute(
                "INSERT INTO removal_requests (id, broker_id, campaign_id, jurisdiction) "
                "VALUES (2, 'b', 'c', 'CCPA')"
            )
            conn.execute(
                """INSERT INTO request_state (request_id, current_status, resolved_at,
                   reminders_sent, escalation_level, next_action_at)
                   VALUES (2, 'CONFIRMED', ?, 0, 0, ?)""",
                (resolved.isoformat(), resolved.isoformat()),
            )

            conn.commit()

            actions = run_tick(reference_date=now)
            assert len(actions) >= 1
        finally:
            close_connection()
            os.environ.pop("SYMERASEME_DB_DIR", None)


class TestApplyTickActions:
    def test_empty_actions(self, tmp_path):
        import os

        from symeraseme.core.db import close_connection, init_db

        os.environ["SYMERASEME_DB_DIR"] = str(tmp_path)
        close_connection()
        init_db(str(tmp_path / "test.db"))

        try:
            results = apply_tick_actions([])
            assert results == []
        finally:
            close_connection()
            os.environ.pop("SYMERASEME_DB_DIR", None)

    def test_dry_run_actions_not_executed(self):
        from symeraseme.core.deadlines import TickAction

        actions = [
            TickAction(
                request_id=1,
                broker_id="b",
                campaign_id="c",
                current_status="AWAITING_ACK",
                action_type="send_reminder",
                event_type="REMINDER_SENT",
                description="Test",
                dry_run=True,
            )
        ]
        results = apply_tick_actions(actions)
        assert len(results) == 1
        assert results[0]["dry_run"] is True
        assert results[0]["executed"] is False


class TestCCPADeadline:
    def test_ccpa_has_45_day_deadline(self):
        from symeraseme.core.deadlines import JURISDICTION_DEADLINES

        assert JURISDICTION_DEADLINES.get("CCPA") == 45


class TestJurisdictionDefaults:
    def test_unknown_jurisdiction_defaults_to_30(self):
        from symeraseme.core.deadlines import JURISDICTION_DEADLINES

        val = JURISDICTION_DEADLINES.get("UNKNOWN", 30)
        assert val == 30
