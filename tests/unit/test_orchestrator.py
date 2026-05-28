"""Tests for orchestrator (plan, execute, consent)."""

from __future__ import annotations

import os
import tempfile

import pytest
from typer.testing import CliRunner

from symeraseme.cli import app
from symeraseme.core.consent import check_consent, issue_token, verify_token
from symeraseme.core.db import close_connection, init_db
from symeraseme.core.events import list_removal_requests
from symeraseme.core.orchestrator import (
    execute_campaign,
    execute_campaign_async,
    execute_request,
    get_plan,
    plan_campaign,
    submit_inbox_reply,
)

runner = CliRunner()


@pytest.fixture(autouse=True)
def _db(tmp_path: tempfile.TemporaryDirectory) -> None:
    os.environ["SYMERASEME_DB_DIR"] = str(tmp_path)
    os.environ["SYMERASEME_DATA_DIR"] = str(tmp_path)
    close_connection()
    init_db(str(tmp_path / "test.db"))
    yield
    close_connection()
    os.environ.pop("SYMERASEME_DB_DIR", None)
    os.environ.pop("SYMERASEME_DATA_DIR", None)


@pytest.fixture()
def _fake_profile(monkeypatch):
    """Provide a mock identity profile for execute tests."""
    from unittest.mock import MagicMock

    from symeraseme.registry.schema import IdentityProfile

    profile = IdentityProfile(
        full_name="Jane Doe",
        email_addresses=["jane@example.com"],
        phone_numbers=["+1-555-1234"],
        jurisdictions=["EU"],
    )
    monkeypatch.setattr(
        "symeraseme.core.identity.load_profile",
        lambda: profile,
    )
    monkeypatch.setattr(
        "symeraseme.core.orchestrator.load_profile",
        lambda: profile,
    )
    return profile


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

    def test_plan_populates_identity_hash(self, _fake_profile):
        from symeraseme.core.identity import hash_profile

        profile = _fake_profile
        expected_hash = hash_profile(profile)
        plan_campaign(campaign_id="test-hash", max_brokers=2)
        requests = list_removal_requests(campaign_id="test-hash")
        assert len(requests) > 0
        assert all(r.get("identity_snapshot_hash") == expected_hash for r in requests)

    def test_plan_without_profile_has_empty_hash(self, monkeypatch):
        monkeypatch.setattr(
            "symeraseme.core.identity.load_profile",
            lambda: (_ for _ in ()).throw(FileNotFoundError("no profile")),
        )
        plan_campaign(campaign_id="test-no-hash", max_brokers=2)
        requests = list_removal_requests(campaign_id="test-no-hash")
        assert len(requests) > 0
        assert all(r.get("identity_snapshot_hash") == "" for r in requests)

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
    def test_dry_run_returns_body(self, _fake_profile):
        plan_campaign(campaign_id="dry-test", max_brokers=1)
        result = execute_campaign("dry-test", dry_run=True)
        assert result["total_planned"] >= 1
        assert len(result["results"]) >= 1
        r = result["results"][0]
        assert r["success"] is True
        assert r.get("dry_run") is True
        if "to" in r:
            assert "Jane Doe" in r["body"]
            assert "jane@example.com" in r["body"]
        else:
            assert "steps" in r or "url" in r

    def test_execute_send_failure_logged(self, _fake_profile):
        plan_campaign(campaign_id="fail-test", max_brokers=1)
        requests = list_removal_requests(campaign_id="fail-test")
        assert len(requests) > 0

        email_requests = [r for r in requests if r.get("channel") == "email"]
        if not email_requests:
            pytest.skip("No email requests in campaign")

        result = execute_request(email_requests[0]["id"])
        assert result["success"] is False
        assert "error" in result

    def test_dry_run_without_profile_fails(self, monkeypatch):
        monkeypatch.setattr(
            "symeraseme.core.identity.load_profile",
            lambda: (_ for _ in ()).throw(FileNotFoundError("no profile")),
        )
        plan_campaign(campaign_id="no-profile", max_brokers=1)
        result = execute_campaign("no-profile", dry_run=True)
        assert result["total_planned"] >= 1
        email_results = [r for r in result["results"] if "to" in r]
        if not email_results:
            pytest.skip("No email requests in campaign")
        r = email_results[0]
        assert r["success"] is False
        assert "init-profile" in r["error"]

    def test_dry_run_missing_required_fields_fails(self, monkeypatch):
        from symeraseme.registry.schema import IdentityProfile

        profile = IdentityProfile(
            full_name="",
            email_addresses=[],
        )
        monkeypatch.setattr(
            "symeraseme.core.identity.load_profile",
            lambda: profile,
        )
        plan_campaign(campaign_id="missing-fields", max_brokers=1)
        result = execute_campaign("missing-fields", dry_run=True)
        assert result["total_planned"] >= 1
        email_results = [r for r in result["results"] if "to" in r]
        if not email_results:
            pytest.skip("No email requests in campaign")
        r = email_results[0]
        assert r["success"] is False
        assert "init-profile" in r["error"]

    def test_execute_includes_identity_hash_in_event(self, _fake_profile, monkeypatch):
        from symeraseme.core.events import get_events
        from symeraseme.core.identity import hash_profile

        profile = _fake_profile
        expected_hash = hash_profile(profile)

        monkeypatch.setattr(
            "symeraseme.adapters.email.himalaya.send_email",
            lambda **_: {"message_id": "<test@msg>"},
        )

        plan_campaign(campaign_id="hash-test", max_brokers=3)
        requests = list_removal_requests(campaign_id="hash-test")
        assert len(requests) > 0

        email_requests = [r for r in requests if r.get("channel") == "email"]
        if not email_requests:
            pytest.skip("No email requests in campaign")

        result = execute_request(email_requests[0]["id"])
        assert result["success"] is True

        events = get_events(email_requests[0]["id"])
        sent_events = [e for e in events if e["event_type"] == "SENT"]
        assert len(sent_events) == 1
        assert sent_events[0]["payload_json"].get("identity_snapshot_hash") == expected_hash

    def test_web_form_execution_dispatch(self, monkeypatch, tmp_path):
        import os

        os.environ["SYMERASEME_DB_DIR"] = str(tmp_path)
        os.environ["SYMERASEME_DATA_DIR"] = str(tmp_path)

        from symeraseme.core.db import close_connection, init_db

        close_connection()
        init_db(str(tmp_path / "test.db"))

        plan_campaign(campaign_id="web-form-test", max_brokers=5)
        requests = list_removal_requests(campaign_id="web-form-test")
        web_form_requests = [r for r in requests if r.get("channel") == "web_form"]
        if not web_form_requests:
            pytest.skip("No web-form requests in campaign")

        import asyncio

        async def mock_run_form(**kwargs):
            return type(
                "Result",
                (),
                {
                    "success": True,
                    "step_index": 0,
                    "total_steps": 1,
                    "error": "",
                    "screenshot_path": "",
                    "dry_run": False,
                },
            )()

        monkeypatch.setattr(
            "symeraseme.services.web_form._run_form",
            mock_run_form,
        )
        monkeypatch.setattr(
            "symeraseme.services.web_form.profile_exists",
            lambda: False,
        )

        result = execute_request(web_form_requests[0]["id"])
        assert result["success"] is True

    @pytest.mark.asyncio
    async def test_async_batch_renders_profile_data(self, _fake_profile, monkeypatch):
        """Regression test: execute_campaign_async must pass profile to render_template."""
        plan_campaign(campaign_id="async-profile", max_brokers=5)
        requests = list_removal_requests(campaign_id="async-profile")
        email_requests = [r for r in requests if r.get("channel") == "email"]
        if not email_requests:
            pytest.skip("No email requests in campaign")

        sent_messages: list[tuple[str, str]] = []

        async def mock_send_batch(messages, **kwargs):
            for msg in messages:
                sent_messages.append((msg.to, msg.body))
            return [{"to": to, "success": True} for to, _ in sent_messages]

        monkeypatch.setattr(
            "symeraseme.adapters.email.himalaya.send_messages_batch",
            mock_send_batch,
        )

        from symeraseme.adapters.email.himalaya import SmtpConfig

        monkeypatch.setattr(
            "symeraseme.adapters.email.himalaya.load_smtp_config",
            lambda: SmtpConfig(
                host="localhost",
                port=1025,
                username="",
                password="",
                use_tls=False,
                from_addr="test@example.com",
            ),
        )

        monkeypatch.setattr(
            "symeraseme.core.orchestrator.list_removal_requests",
            lambda campaign_id=None, status=None: email_requests,
        )

        result = await execute_campaign_async("async-profile", batch_size=5)
        assert result["total_planned"] >= 1
        assert result["batch_size"] >= 1
        assert len(sent_messages) >= 1

        profile = _fake_profile
        for to_addr, body in sent_messages:
            assert profile.full_name in body, (
                f"Rendered email to {to_addr} does not contain user's full name "
                f"({profile.full_name!r}). Body: {body[:200]!r}"
            )


class TestHandleExecuteRouting:
    def test_routes_to_async_when_no_account(self, monkeypatch, tmp_path):
        import os

        from symeraseme.services.campaign import handle_execute

        os.environ["SYMERASEME_DB_DIR"] = str(tmp_path)
        os.environ["SYMERASEME_DATA_DIR"] = str(tmp_path)

        from symeraseme.core.db import close_connection, init_db

        close_connection()
        init_db(str(tmp_path / "test.db"))

        async_called = []

        async def mock_execute_campaign_async(campaign_id, **kwargs):
            async_called.append(True)
            return {
                "campaign_id": campaign_id,
                "total_planned": 0,
                "batch_size": 0,
                "results": [],
            }

        monkeypatch.setattr(
            "symeraseme.services.campaign.execute_campaign_async",
            mock_execute_campaign_async,
        )

        handle_execute("test-campaign", yes=True)
        assert len(async_called) == 1

    def test_routes_to_sync_when_account_given(self, monkeypatch, tmp_path):
        import os

        from symeraseme.services.campaign import handle_execute

        os.environ["SYMERASEME_DB_DIR"] = str(tmp_path)
        os.environ["SYMERASEME_DATA_DIR"] = str(tmp_path)

        from symeraseme.core.db import close_connection, init_db

        close_connection()
        init_db(str(tmp_path / "test.db"))

        sync_called = []

        def mock_execute_campaign(*args, **kwargs):
            sync_called.append(True)
            return {
                "campaign_id": kwargs.get("campaign_id", "test"),
                "total_planned": 0,
                "batch_size": 0,
                "results": [],
            }

        monkeypatch.setattr(
            "symeraseme.services.campaign.execute_campaign",
            mock_execute_campaign,
        )

        handle_execute("test-campaign", account="gmail", yes=True)
        assert len(sync_called) == 1


class TestBrokerIdIndex:
    def test_load_broker_uses_index(self, monkeypatch):
        from symeraseme.registry.loader import _BROKER_ID_INDEX, load_broker

        mock_broker = type("Broker", (), {"id": "test-broker"})()
        load_calls = []

        def mock_load_yaml(path):
            load_calls.append(str(path))
            return mock_broker

        monkeypatch.setattr(
            "symeraseme.registry.loader.load_broker_yaml",
            mock_load_yaml,
        )

        _BROKER_ID_INDEX.clear()
        _BROKER_ID_INDEX["test-broker"] = "/fake/path.yaml"
        result = load_broker("test-broker")
        assert result == mock_broker
        assert len(load_calls) == 1
        assert load_calls[0] == "/fake/path.yaml"


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
        os.environ["SYMERASEME_CONSENT"] = token
        try:
            assert check_consent("execute") is True
        finally:
            os.environ.pop("SYMERASEME_CONSENT", None)

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

    def test_submit_reply_does_not_create_event(self):
        from symeraseme.core.events import create_campaign, create_removal_request, get_events

        create_campaign("reply-test")
        rid = create_removal_request(broker_id="b", campaign_id="reply-test", jurisdiction="GDPR")
        submit_inbox_reply(
            "<msg2@test.com>",
            request_id=rid,
            from_addr="broker@example.com",
            subject="Your request is confirmed",
            snippet="Data has been deleted",
            classified_as="confirmed",
        )
        events = get_events(rid)
        assert not any(e["event_type"] == "CONFIRMED" for e in events)


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
        from symeraseme.core.events import create_campaign, create_removal_request

        create_campaign("evt-test")
        rid = create_removal_request(broker_id="b", campaign_id="evt-test", jurisdiction="GDPR")
        result = runner.invoke(app, ["events", "show", str(rid)])
        assert result.exit_code == 0

    def test_requests_list(self):
        runner.invoke(app, ["plan", "create", "--campaign", "req-list", "--max", "2"])
        result = runner.invoke(app, ["requests", "list", "--campaign", "req-list"])
        assert result.exit_code == 0
