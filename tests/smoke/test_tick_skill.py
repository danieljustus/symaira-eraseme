from __future__ import annotations

from datetime import UTC, datetime

from .conftest import (
    assert_ok,
    invoke,
)


class TestTick:
    def test_tick_dry_run(self, seeded_db):
        result = invoke("plan", "tick", "--dry-run")
        assert_ok(result)

    def test_tick_dry_run_json(self, seeded_db):
        result = invoke("--output", "json", "plan", "tick", "--dry-run")
        import json

        assert_ok(result)
        data = json.loads(result.stdout)
        assert "total_actions" in data

    def test_tick_with_sent_event(self, seeded_db):
        from symeraseme.core.db import get_connection, init_db

        init_db()
        conn = get_connection()
        now = datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%S")
        conn.execute(
            "INSERT INTO request_events (request_id, occurred_at, recorded_at, event_type, source) "
            "VALUES (1, ?, ?, 'SENT', 'system')",
            (now, now),
        )
        conn.commit()
        from symeraseme.core.projection import upsert_state

        upsert_state(1)
        result = invoke("plan", "tick", "--dry-run")
        assert_ok(result)

    def test_tick_no_actions(self, tmp_home):
        result = invoke("plan", "tick", "--dry-run")
        assert_ok(result)
        assert "no actions needed" in result.stdout

    def test_tick_json_empty(self, tmp_home):
        result = invoke("--output", "json", "plan", "tick", "--dry-run")
        import json

        assert_ok(result)
        data = json.loads(result.stdout)
        assert data["total_actions"] == 0


class TestRequestsByStatus:
    def test_requests_by_status_planned(self, seeded_db):
        result = invoke("requests", "list", "--status", "PLANNED")
        assert_ok(result)

    def test_requests_by_status_json(self, seeded_db):
        result = invoke("--output", "json", "requests", "list", "--status", "PLANNED")
        import json

        assert_ok(result)
        data = json.loads(result.stdout)
        assert isinstance(data, dict)
        requests = data.get("data", [])
        assert isinstance(requests, list)
