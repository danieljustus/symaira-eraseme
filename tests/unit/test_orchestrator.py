"""Tests for orchestrator (plan, execute, consent)."""

from __future__ import annotations

import os
import tempfile

import pytest
from typer.testing import CliRunner

from openeraseme.cli import app
from openeraseme.core.consent import check_consent, issue_token, verify_token
from openeraseme.core.db import close_connection, init_db
from openeraseme.core.events import list_removal_requests
from openeraseme.core.orchestrator import (
    execute_campaign,
    execute_request,
    get_plan,
    plan_campaign,
    submit_inbox_reply,
)

runner = CliRunner()


@pytest.fixture(autouse=True)
def _db(tmp_path: tempfile.TemporaryDirectory) -> None:
    os.environ["OPENERASEME_DB_DIR"] = str(tmp_path)
    os.environ["OPENERASEME_DATA_DIR"] = str(tmp_path)
    close_connection()
    init_db(str(tmp_path / "test.db"))
    yield
    close_connection()
    os.environ.pop("OPENERASEME_DB_DIR", None)
    os.environ.pop("OPENERASEME_DATA_DIR", None)


class TestPlanCampaign:
    def test_plan_creates_events(self):
        result = plan_campaign(campaign_id="test-plan", max_brokers=5)
        assert result["campaign_id"] == "test-plan"
        assert result["planned"] > 0
        assert len(result["requests"]) > 0

    def test_plan_respects_max(self):
        result = plan_campaign(campaign_id="test-max", max_brokers=2)
        assert result["planned"] <= 2

    def test_plan_requests_have_state(self):
        plan_campaign(campaign_id="test-state", max_brokers=3)
        requests = list_removal_requests(campaign_id="test-state")
        assert all(r.get("current_status") == "PLANNED" for r in requests)

    def test_plan_show(self):
        plan_campaign(campaign_id="show-test", max_brokers=2)
        plan = get_plan(campaign_id="show-test")
        assert plan["total"] == 2

    def test_plan_show_by_status(self):
        plan_campaign(campaign_id="status-test", max_brokers=2)
        plan = get_plan(campaign_id="status-test", status="PLANNED")
        assert plan["total"] == 2
        plan_done = get_plan(campaign_id="status-test", status="DONE")
        assert plan_done["total"] == 0


class TestExecuteCampaign:
    def test_dry_run_returns_body(self):
        plan_campaign(campaign_id="dry-test", max_brokers=1)
        result = execute_campaign("dry-test", dry_run=True)
        assert result["total_planned"] >= 1
        assert len(result["results"]) >= 1
        r = result["results"][0]
        assert r["success"] is True
        assert r.get("dry_run") is True

    def test_execute_send_failure_logged(self):
        plan_campaign(campaign_id="fail-test", max_brokers=1)
        requests = list_removal_requests(campaign_id="fail-test")
        assert len(requests) > 0

        result = execute_request(requests[0]["id"])
        # Will fail because Himalaya is not installed — that's expected
        assert result["success"] is False
        assert "error" in result


class TestConsent:
    def test_issue_and_verify(self):
        token = issue_token("execute")
        assert verify_token("execute", token) is True
        assert verify_token("plan", token) is False

    def test_expired_token(self):
        token = issue_token("execute", ttl=-1)
        assert verify_token("execute", token) is False

    def test_unknown_token(self):
        assert verify_token("execute", "nonexistent") is False

    def test_check_consent_yes(self):
        assert check_consent("execute", yes=True) is True

    def test_check_consent_token(self):
        token = issue_token("execute")
        assert check_consent("execute", consent_token=token) is True

    def test_check_consent_env(self):
        token = issue_token("execute")
        os.environ["OPENERASEME_CONSENT"] = token
        try:
            assert check_consent("execute") is True
        finally:
            os.environ.pop("OPENERASEME_CONSENT", None)

    def test_check_consent_no_consent(self):
        assert check_consent("execute") is False


class TestInboxReply:
    def test_submit_reply(self):
        result = submit_inbox_reply(
            "<msg@test.com>",
            request_id=None,
            from_addr="broker@example.com",
            subject="Re: Data Deletion Request",
            snippet="We have received your request",
            classified_as="ack",
        )
        assert result["reply_id"] > 0
        assert result["classified_as"] == "ack"

    def test_submit_reply_with_request_triggers_event(self):
        from openeraseme.core.events import create_campaign, create_removal_request, get_events

        create_campaign("reply-test")
        rid = create_removal_request(
            broker_id="b", campaign_id="reply-test", jurisdiction="GDPR"
        )
        submit_inbox_reply(
            "<msg2@test.com>",
            request_id=rid,
            from_addr="broker@example.com",
            subject="Your request is confirmed",
            snippet="Data has been deleted",
            classified_as="confirmed",
        )
        events = get_events(rid)
        assert any(e["event_type"] == "CONFIRMED" for e in events)


class TestCLIConsent:
    def test_execute_dry_run(self):
        result = runner.invoke(
            app,
            ["execute", "--campaign", "cli-dry", "--dry-run", "--yes"],
        )
        assert result.exit_code == 0

    def test_execute_refuses_without_consent(self):
        result = runner.invoke(
            app,
            ["execute", "--campaign", "no-consent"],
        )
        assert result.exit_code != 0
        output = (result.stdout + result.stderr).lower()
        assert "consent" in output

    def test_grant_command(self):
        result = runner.invoke(app, ["grant", "execute"])
        assert result.exit_code == 0
        assert "Consent token" in result.stdout

    def test_plan_cli(self):
        result = runner.invoke(
            app,
            ["plan", "create", "--campaign", "cli-plan-test", "--max", "3"],
        )
        assert result.exit_code == 0
        assert "Campaign" in result.stdout or "planned" in result.stdout.lower()

    def test_plan_show_cli(self):
        runner.invoke(app, ["plan", "create", "--campaign", "cli-show", "--max", "2"])
        result = runner.invoke(app, ["plan", "show", "--campaign", "cli-show"])
        assert result.exit_code == 0

    def test_events_show(self):
        from openeraseme.core.events import create_campaign, create_removal_request

        create_campaign("evt-test")
        rid = create_removal_request(broker_id="b", campaign_id="evt-test", jurisdiction="GDPR")
        result = runner.invoke(app, ["events", "show", str(rid)])
        assert result.exit_code == 0

    def test_requests_list(self):
        runner.invoke(app, ["plan", "create", "--campaign", "req-list", "--max", "2"])
        result = runner.invoke(app, ["requests", "list", "--campaign", "req-list"])
        assert result.exit_code == 0
