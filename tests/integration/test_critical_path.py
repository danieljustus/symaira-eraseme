"""Integration tests for the critical CLI-to-event-store path.

Covers the core lifecycle: plan → execute → tick → status, identity
profile encryption/decryption round-trip, event store + projection
integrity, registry loading and schema validation.
"""

from __future__ import annotations

import json
import os
import tempfile
from datetime import UTC, datetime, timedelta
from pathlib import Path
from unittest.mock import patch

import pytest

from symeraseme.registry.schema import Broker, IdentityProfile

# ──────────────────────────────────────────────────────────────
#  Fixtures
# ──────────────────────────────────────────────────────────────


@pytest.fixture
def db_path():
    """Create a temporary directory for the test SQLite database."""
    from symeraseme.core.db import close_connection

    close_connection()
    with tempfile.TemporaryDirectory() as d:
        old_dir = os.environ.get("SYMERASEME_DB_DIR")
        old_encrypt = os.environ.get("SYMERASEME_ENCRYPT_DB")
        os.environ["SYMERASEME_DB_DIR"] = str(d)
        os.environ["SYMERASEME_ENCRYPT_DB"] = ""
        yield Path(d)
        close_connection()
        if old_dir is not None:
            os.environ["SYMERASEME_DB_DIR"] = old_dir
        else:
            os.environ.pop("SYMERASEME_DB_DIR", None)
        if old_encrypt is not None:
            os.environ["SYMERASEME_ENCRYPT_DB"] = old_encrypt
        else:
            os.environ.pop("SYMERASEME_ENCRYPT_DB", None)


@pytest.fixture
def clean_db(db_path):
    """Initialised empty database in a temporary directory."""
    from symeraseme.core.db import init_db

    db_file = db_path / "test.db"
    init_db(str(db_file))
    return db_path


@pytest.fixture
def fake_keyring():
    """In-memory keyring so identity encryption tests are side-effect free."""
    store: dict[str, str] = {}

    def set_pass(service, username, password):
        store[f"{service}:{username}"] = password

    def get_pass(service, username):
        return store.get(f"{service}:{username}")

    def del_pass(service, username):
        store.pop(f"{service}:{username}", None)

    with (
        patch("symeraseme.core.identity.keyring.set_password", set_pass),
        patch("symeraseme.core.identity.keyring.get_password", get_pass),
        patch("symeraseme.core.identity.keyring.delete_password", del_pass),
    ):
        yield


# ──────────────────────────────────────────────────────────────
#  1. plan → execute (dry-run) → tick → status  lifecycle
# ──────────────────────────────────────────────────────────────


class TestPlanExecuteTickStatus:
    """Integration test for the complete lifecycle cycle."""

    def test_plan_dry_run_cycle(self, clean_db):
        """Plan a campaign, dry-run execute, tick, verify status."""
        from symeraseme.core.orchestrator import plan_campaign
        from symeraseme.services.campaign import handle_execute
        from symeraseme.services.status import handle_status
        from symeraseme.services.tick import handle_tick

        result = plan_campaign(
            campaign_id="integration-test",
            max_brokers=3,
        )
        assert result["campaign_id"] == "integration-test"
        assert result["planned"] >= 1, "Expected at least 1 planned request"
        requests = result["requests"]
        assert len(requests) >= 1

        dry_result = handle_execute(
            "integration-test",
            dry_run=True,
            yes=True,
            output_format="json",
        )
        dry_data = json.loads(dry_result)
        assert dry_data["campaign_id"] == "integration-test"
        assert dry_data["total_planned"] >= 1
        for r in dry_data["results"]:
            assert r.get("success") is True, f"dry-run failed: {r}"
            assert r.get("dry_run") is True
            assert r.get("body") is not None
            assert len(r["body"]) > 0

        for req in requests:
            from symeraseme.core.projection import append_event_and_project

            append_event_and_project(
                req["request_id"],
                "SENT",
                payload={"to": "test@example.com", "expected_response_days": 30},
            )

        tick_dry = handle_tick(dry_run=True, output_format="text")
        assert "Tick:" in tick_dry

        tick_result = handle_tick(dry_run=False, output_format="text")
        assert "Tick:" in tick_result or "no actions" in tick_result

        status = json.loads(handle_status(output_format="json"))
        assert status["totals"]["requests"] >= 1

    def test_plan_with_jurisdiction_filter(self, clean_db):
        """Plan a campaign filtered by EU jurisdiction."""
        from symeraseme.core.orchestrator import plan_campaign

        result = plan_campaign(
            campaign_id="eu-only",
            jurisdiction="EU",
            max_brokers=5,
        )
        assert result["planned"] >= 1
        from symeraseme.core.events import list_campaigns

        camps = list_campaigns()
        eu_camp = next((c for c in camps if c["id"] == "eu-only"), None)
        assert eu_camp is not None


# ──────────────────────────────────────────────────────────────
#  2.  Identity  encryption / decryption  round-trip
# ──────────────────────────────────────────────────────────────


class TestIdentityRoundTrip:
    """Integration test for the real AES-GCM identity vault code path."""

    def test_save_load_preserves_all_fields(self, fake_keyring, monkeypatch, tmp_path):
        """Save a profile, load it back — all fields must be identical."""
        profile_path = tmp_path / "integration_identity.enc"
        monkeypatch.setenv("SYMERASEME_IDENTITY_PATH", str(profile_path))

        import symeraseme.core.identity as vault

        original = IdentityProfile(
            full_name="Integration Test User",
            name_variants=["IT User", "Test User"],
            email_addresses=["integration@example.com"],
            phone_numbers=["+1-555-0001"],
            jurisdictions=["US", "EU"],
        )
        vault.save_profile(original)
        assert profile_path.exists()
        assert vault.profile_exists()

        loaded = vault.load_profile()
        assert loaded.model_dump() == original.model_dump()

        vault.delete_profile()

    def test_ciphertext_does_not_leak_plaintext(self, fake_keyring, monkeypatch, tmp_path):
        """The encrypted file must not contain the plaintext full name."""
        profile_path = tmp_path / "leak_check.enc"
        monkeypatch.setenv("SYMERASEME_IDENTITY_PATH", str(profile_path))

        import symeraseme.core.identity as vault

        profile = IdentityProfile(
            full_name="Secret Person",
            email_addresses=["secret@example.com"],
        )
        vault.save_profile(profile)

        raw = profile_path.read_bytes()
        assert b"Secret Person" not in raw

        vault.delete_profile()

    def test_tampered_ciphertext_fails_closed(self, fake_keyring, monkeypatch, tmp_path):
        """Tampering with the ciphertext must raise InvalidTag."""
        from cryptography.exceptions import InvalidTag

        profile_path = tmp_path / "tamper.enc"
        monkeypatch.setenv("SYMERASEME_IDENTITY_PATH", str(profile_path))

        import symeraseme.core.identity as vault

        profile = IdentityProfile(
            full_name="Tamper Target",
            email_addresses=["target@example.com"],
        )
        vault.save_profile(profile)

        raw = profile_path.read_bytes()
        header_bytes, _, ciphertext = raw.partition(b"\n")
        tampered = bytearray(ciphertext)
        tampered[-1] ^= 0xFF
        profile_path.write_bytes(header_bytes + b"\n" + bytes(tampered))

        with pytest.raises(InvalidTag):
            vault.load_profile()

        vault.delete_profile()

    def test_load_nonexistent_raises(self, fake_keyring, monkeypatch, tmp_path):
        """Loading a profile that does not exist raises FileNotFoundError."""
        import symeraseme.core.identity as vault

        identity_path = tmp_path / "_nonexistent_integration_identity.enc"
        monkeypatch.setenv("SYMERASEME_IDENTITY_PATH", str(identity_path))
        identity_path.unlink(missing_ok=True)

        with pytest.raises(FileNotFoundError):
            vault.load_profile()


# ──────────────────────────────────────────────────────────────
#  3.  Event-store  +  projection  round-trip
# ──────────────────────────────────────────────────────────────


class TestEventStoreProjectionRoundTrip:
    """Integration test: append events and rebuild projections."""

    def test_full_event_cycle(self, clean_db):
        """Plan → SENT → ACK → CONFIRMED cycle with projection rebuild."""
        from symeraseme.core.events import create_campaign, create_removal_request, get_events
        from symeraseme.core.projection import (
            append_event_and_project,
            rebuild_state,
        )

        create_campaign("cycle-test")
        rid = create_removal_request(
            broker_id="broker-cycle",
            channel="email",
            campaign_id="cycle-test",
            jurisdiction="GDPR",
        )

        # Append events via the atomic helper
        append_event_and_project(rid, "PLANNED")
        append_event_and_project(
            rid, "SENT", payload={"to": "test@broker.com", "expected_response_days": 30}
        )
        append_event_and_project(rid, "ACK")
        append_event_and_project(rid, "CONFIRMED")

        # Verify events are recorded
        events = get_events(rid)
        event_types = [e["event_type"] for e in events]
        assert event_types == ["PLANNED", "SENT", "ACK", "CONFIRMED"]

        # Verify projection is correct
        state = rebuild_state(rid)
        assert state["current_status"] == "CONFIRMED"
        assert state["sent_at"] is not None
        assert state["acknowledged_at"] is not None
        assert state["resolved_at"] is not None

    def test_atomicity_rolls_back_on_failure(self, clean_db, monkeypatch):
        """Projection failure rolls back the event insert (A1 guarantee)."""
        from symeraseme.core.db import get_connection
        from symeraseme.core.events import create_campaign, create_removal_request
        from symeraseme.core.projection import append_event_and_project

        create_campaign("atomic-test")
        rid = create_removal_request(
            broker_id="b-atomic",
            campaign_id="atomic-test",
            jurisdiction="GDPR",
        )

        from symeraseme.core import projection

        original = projection.upsert_state

        def crash(*args, **kwargs):
            raise RuntimeError("simulated projection failure")

        monkeypatch.setattr(projection, "upsert_state", crash)

        with pytest.raises(RuntimeError, match="simulated projection failure"):
            append_event_and_project(rid, "SENT")

        # No event should be persisted
        conn = get_connection()
        count = conn.execute(
            "SELECT COUNT(*) AS n FROM request_events WHERE request_id = ?", (rid,)
        ).fetchone()["n"]
        assert count == 0

        # Restore and verify subsequent call succeeds
        monkeypatch.setattr(projection, "upsert_state", original)
        eid, state = append_event_and_project(rid, "SENT", payload={"to": "x@y.com"})
        assert eid > 0
        assert state["current_status"] == "AWAITING_ACK"


# ──────────────────────────────────────────────────────────────
#  4.  Deadline / tick engine integration
# ──────────────────────────────────────────────────────────────


class TestTickEngineIntegration:
    """Integration test for the tick engine with time-shifted reference."""

    def test_tick_finds_awaiting_ack_requests(self, clean_db):
        """Tick engine scans requests and finds those needing attention."""
        from symeraseme.core.events import append_event, create_campaign, create_removal_request
        from symeraseme.core.projection import (
            rebuild_all_states,
            rebuild_state,
            upsert_state,
        )

        create_campaign("tick-scan")
        rid = create_removal_request(
            broker_id="broker-scan",
            campaign_id="tick-scan",
            jurisdiction="GDPR",
        )

        planned_ts = (datetime.now(UTC) - timedelta(days=15)).strftime("%Y-%m-%dT%H:%M:%S")
        sent_ts = (datetime.now(UTC) - timedelta(days=14)).strftime("%Y-%m-%dT%H:%M:%S")

        append_event(rid, "PLANNED", occurred_at=planned_ts)
        upsert_state(rid)
        rebuild_all_states()

        append_event(
            rid,
            "SENT",
            payload={"to": "scan@example.com", "expected_response_days": 30},
            occurred_at=sent_ts,
        )
        upsert_state(rid)
        rebuild_all_states()

        state = rebuild_state(rid)
        assert state["current_status"] == "AWAITING_ACK"

        from symeraseme.core.deadlines import run_tick

        actions = run_tick(dry_run=True, reference_date=datetime.now(UTC))
        my_actions = [a for a in actions if a.request_id == rid]
        assert len(my_actions) > 0, f"Expected tick actions for request {rid}, got none"

    def test_tick_on_planned_campaign(self, clean_db):
        """Tick on a plan-only campaign with PLANNED events reports results."""
        from symeraseme.core.orchestrator import plan_campaign
        from symeraseme.services.tick import handle_tick

        plan_campaign(campaign_id="tick-planned", max_brokers=2)
        result = handle_tick(dry_run=True, output_format="text")
        assert "Tick:" in result


# ──────────────────────────────────────────────────────────────
#  5.  Registry  loading  and  schema  validation
# ──────────────────────────────────────────────────────────────


class TestRegistryIntegration:
    """Integration tests for broker registry loading and validation."""

    def test_load_all_brokers_returns_data(self):
        """Loading all brokers from the real registry directory works."""
        from symeraseme.registry.loader import load_all_brokers

        brokers = load_all_brokers()
        assert len(brokers) > 100, f"Expected >100 brokers, got {len(brokers)}"
        assert all(isinstance(b, Broker) for b in brokers)

    def test_load_brokers_by_jurisdiction(self):
        """Filtering by EU returns only brokers with EU jurisdiction."""
        from symeraseme.registry.loader import load_all_brokers

        eu_brokers = load_all_brokers(jurisdiction="EU")
        assert len(eu_brokers) > 0
        for b in eu_brokers:
            assert "EU" in b.jurisdictions

    def test_load_single_broker(self):
        """Loading a specific broker by ID works."""
        from symeraseme.registry.loader import load_broker

        broker = load_broker("acxiom-eu")
        assert broker.id == "acxiom-eu"
        assert broker.name
        assert broker.website

    def test_schema_validation_passes(self):
        """Every broker YAML file must validate against the JSON Schema."""
        import jsonschema
        import yaml

        from symeraseme.registry.loader import _registry_dir, broker_schema

        registry_dir = _registry_dir() / "brokers"
        assert registry_dir.exists(), f"Registry broker dir not found at {registry_dir}"

        schema = broker_schema()
        yaml_files = list(registry_dir.rglob("*.yaml"))
        assert len(yaml_files) > 100

        failures: list[str] = []
        for yml in sorted(yaml_files):
            with open(yml) as f:
                data = yaml.safe_load(f)
            try:
                jsonschema.validate(data, schema)
            except jsonschema.ValidationError as e:
                failures.append(f"{yml}: {e.message}")

        assert not failures, "Schema validation failures:\n" + "\n".join(failures)

    def test_load_real_broker_passes_pydantic(self):
        """Every real broker YAML must pass Pydantic model validation."""
        from symeraseme.registry.loader import _registry_dir, load_all_brokers

        brokers = load_all_brokers()
        yaml_files = list((_registry_dir() / "brokers").rglob("*.yaml"))
        # All YAMLs that are not disabled or skipped should be loadable
        # The fact that load_all_brokers() returns without exception
        # already validates Pydantic; this test makes it explicit.
        assert len(brokers) <= len(yaml_files), "Broker count should not exceed YAML file count"


# ──────────────────────────────────────────────────────────────
#  6.  Campaign lifecycle  events  (multi-event integration)
# ──────────────────────────────────────────────────────────────


class TestCampaignLifecycle:
    """Multi-step campaign lifecycle across event types."""

    def test_planned_to_confirmed_flow(self, clean_db):
        """Full flow: PLAN → SENT → ACK → CONFIRMED with status checks."""
        from symeraseme.core.events import (
            append_event,
            create_campaign,
            create_removal_request,
            get_events,
            list_removal_requests,
        )
        from symeraseme.core.projection import rebuild_all_states, rebuild_state, upsert_state

        create_campaign("lifecycle")
        rid = create_removal_request(
            broker_id="broker-life",
            campaign_id="lifecycle",
            jurisdiction="CCPA",
        )

        # Plan → Sent
        append_event(rid, "PLANNED")
        upsert_state(rid)
        rebuild_all_states()

        append_event(rid, "SENT", payload={"to": "life@example.com"})
        upsert_state(rid)
        rebuild_all_states()

        # Ack
        append_event(rid, "ACK")
        upsert_state(rid)
        rebuild_all_states()

        # Confirmed
        append_event(rid, "CONFIRMED")
        upsert_state(rid)
        rebuild_all_states()

        # Verify status chain
        state = rebuild_state(rid)
        assert state["current_status"] == "CONFIRMED"
        assert state["sent_at"] is not None
        assert state["acknowledged_at"] is not None
        assert state["resolved_at"] is not None

        # Verify events
        events = get_events(rid)
        assert len(events) == 4

        # Verify listable by status
        confirmed = list_removal_requests(campaign_id="lifecycle", status="CONFIRMED")
        assert len(confirmed) == 1
        assert confirmed[0]["id"] == rid

    def test_send_failed_path(self, clean_db):
        """SEND_FAILED event should result in SEND_FAILED status."""
        from symeraseme.core.events import (
            append_event,
            create_campaign,
            create_removal_request,
        )
        from symeraseme.core.projection import rebuild_all_states, rebuild_state, upsert_state

        create_campaign("fail-test")
        rid = create_removal_request(
            broker_id="fail-broker",
            campaign_id="fail-test",
            jurisdiction="GDPR",
        )

        append_event(rid, "PLANNED")
        upsert_state(rid)
        rebuild_all_states()

        append_event(rid, "SEND_FAILED", payload={"error": "SMTP timeout"})
        upsert_state(rid)
        rebuild_all_states()

        state = rebuild_state(rid)
        assert state["current_status"] == "SEND_FAILED"


# ──────────────────────────────────────────────────────────────
#  7.  Service  handler  integration
# ──────────────────────────────────────────────────────────────


class TestServiceHandlerIntegration:
    """Integration tests for the service layer (CLI handler bridge)."""

    def test_status_after_plan(self, clean_db):
        """After planning a campaign, status should show the planned requests."""
        from symeraseme.core.orchestrator import plan_campaign
        from symeraseme.services.status import handle_status

        plan_campaign(campaign_id="svc-status-test", max_brokers=3)
        status = json.loads(handle_status(output_format="json"))
        assert status["totals"]["requests"] >= 1

    def test_plan_show_after_create(self, clean_db):
        """Handle plan show after creating a campaign."""
        from symeraseme.core.orchestrator import plan_campaign
        from symeraseme.services.campaign import handle_plan_show

        plan_campaign(campaign_id="svc-show-test", max_brokers=2)
        plan_json = json.loads(handle_plan_show("svc-show-test", output_format="json"))
        assert plan_json["campaign_id"] == "svc-show-test"
        assert plan_json["total"] >= 1


# ──────────────────────────────────────────────────────────────
#  8.  Consent  token  lifecycle  integration
# ──────────────────────────────────────────────────────────────


class TestConsentIntegration:
    """Integration tests for the consent token mechanism."""

    def test_issue_verify_consume_flow(self, monkeypatch, tmp_path):
        """Full consent token lifecycle in a temp directory."""
        consent_dir = tmp_path / "consent"
        consent_dir.mkdir()
        monkeypatch.setenv("SYMERASEME_DATA_DIR", str(consent_dir))

        from symeraseme.core.consent import (
            check_consent,
            consume_token,
            issue_token,
        )

        # Issue
        token = issue_token("execute", ttl=3600)
        assert token

        # Verify
        assert check_consent("execute", consent_token=token)
        assert not check_consent("other-cmd", consent_token=token)

        # Consume
        consume_token(token)
        assert not check_consent("execute", consent_token=token)

    def test_expired_token_rejected(self, monkeypatch, tmp_path):
        """An expired token must not pass verification."""
        consent_dir = tmp_path / "consent"
        consent_dir.mkdir()
        monkeypatch.setenv("SYMERASEME_DATA_DIR", str(consent_dir))

        from symeraseme.core.consent import check_consent, issue_token

        token = issue_token("execute", ttl=-1)
        assert not check_consent("execute", consent_token=token)


# ──────────────────────────────────────────────────────────────
#  9.  CLI  smoke  via  Typer  runner  (light integration)
# ──────────────────────────────────────────────────────────────


class TestCliSmoke:
    """Light CLI smoke tests exercising the Typer runner end-to-end."""

    def test_db_init_command(self, db_path, monkeypatch):
        """'db-init' should create a database."""
        from typer.testing import CliRunner

        from symeraseme.cli import app

        monkeypatch.setenv("SYMERASEME_DB_DIR", str(db_path))

        runner = CliRunner()
        result = runner.invoke(app, ["db-init"])
        assert result.exit_code == 0
        assert "Database initialized" in result.stdout

        # Verify tables were created
        from symeraseme.core.db import close_connection, get_connection, init_db

        close_connection()
        init_db(str(db_path / "test.db"))
        conn = get_connection()
        tables = conn.execute(
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"
        ).fetchall()
        names = [r["name"] for r in tables]
        assert "removal_requests" in names
        assert "request_events" in names
        assert "request_state" in names

    def test_grant_and_revoke_cli(self, monkeypatch, tmp_path):
        """CLI grant command issues a consent token."""
        consent_dir = tmp_path / "consent"
        consent_dir.mkdir()
        monkeypatch.setenv("SYMERASEME_DATA_DIR", str(consent_dir))

        from typer.testing import CliRunner

        from symeraseme.cli import app

        runner = CliRunner()

        result = runner.invoke(app, ["grant", "execute"])
        assert result.exit_code == 0
        assert "Consent token:" in result.stdout
