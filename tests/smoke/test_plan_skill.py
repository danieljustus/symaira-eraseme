from __future__ import annotations

from .conftest import (
    assert_ok,
    invoke,
)


class TestPlanCreate:
    def test_plan_create_with_campaign(self, seeded_db):
        result = invoke("plan", "create", "--campaign", "plan-test")
        assert_ok(result)
        assert "Campaign: plan-test" in result.stdout

    def test_plan_create_json(self, seeded_db):
        result = invoke("--output", "json", "plan", "create", "--campaign", "plan-json")
        import json

        assert_ok(result)
        data = json.loads(result.stdout)
        assert data["campaign_id"] == "plan-json"

    def test_plan_create_with_jurisdiction(self, seeded_db):
        result = invoke("plan", "create", "--campaign", "plan-gdpr", "--jurisdiction", "GDPR")
        assert_ok(result)

    def test_plan_create_with_max(self, seeded_db):
        result = invoke("plan", "create", "--campaign", "plan-max", "--max", "3")
        assert_ok(result)

    def test_plan_create_descriptive_id(self, seeded_db):
        result = invoke("plan", "create", "--campaign", "quarterly-rescan-q3-2026")
        assert_ok(result)
        assert "q3-2026" in result.stdout


class TestPlanShow:
    def test_plan_show_all(self, seeded_db):
        result = invoke("plan", "show")
        assert_ok(result)
        assert "Plan:" in result.stdout

    def test_plan_show_by_campaign(self, seeded_db):
        result = invoke("plan", "show", "--campaign", "smoke-test")
        assert_ok(result)
        assert "smoke-test" in result.stdout

    def test_plan_show_json(self, seeded_db):
        result = invoke("--output", "json", "plan", "show")
        import json

        assert_ok(result)
        data = json.loads(result.stdout)
        assert "total" in data
        assert "requests" in data

    def test_plan_show_empty_campaign(self, seeded_db):
        result = invoke("plan", "show", "--campaign", "nonexistent")
        assert_ok(result)


class TestRequestsList:
    def test_requests_list_all(self, seeded_db):
        result = invoke("requests", "list")
        assert_ok(result)
        assert "#" in result.stdout or "No requests" in result.stdout

    def test_requests_list_by_campaign(self, seeded_db):
        result = invoke("requests", "list", "--campaign", "smoke-test")
        assert_ok(result)

    def test_requests_list_by_status(self, seeded_db):
        result = invoke("requests", "list", "--status", "PLANNED")
        assert_ok(result)

    def test_requests_list_json(self, seeded_db):
        result = invoke("--output", "json", "requests", "list")
        import json

        assert_ok(result)
        data = json.loads(result.stdout)
        assert isinstance(data, list)

    def test_requests_list_by_broker(self, seeded_db):
        result = invoke("requests", "list", "--broker", "acxiom-eu")
        assert_ok(result)

    def test_requests_list_empty(self, tmp_home):
        result = invoke("requests", "list")
        assert_ok(result)
        assert "No requests found" in result.stdout


class TestEventsShow:
    def test_events_show_exists(self, seeded_db):
        result = invoke("events", "show", "1")
        assert_ok(result)
        assert "Events for request" in result.stdout

    def test_events_show_json(self, seeded_db):
        result = invoke("--output", "json", "events", "show", "1")
        import json

        assert_ok(result)
        data = json.loads(result.stdout)
        assert isinstance(data, list)
        for event in data:
            assert "event_type" in event

    def test_events_show_nonexistent(self):
        result = invoke("events", "show", "9999")
        assert_ok(result)
        assert "No events found" in result.stdout
