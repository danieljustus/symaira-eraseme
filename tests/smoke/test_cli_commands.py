from __future__ import annotations

from .conftest import assert_ok, invoke


class TestVersion:
    def test_version_output(self):
        result = invoke("version")
        assert_ok(result)
        assert "OpenEraseMe" in result.stdout

    def test_version_contains_number(self):
        result = invoke("version")
        assert_ok(result)
        assert any(c.isdigit() for c in result.stdout)


class TestHelp:
    def test_help_shows_commands(self):
        result = invoke("--help")
        assert_ok(result)
        assert "init-profile" in result.stdout

    def test_help_no_args(self):
        result = invoke("--help")
        assert_ok(result)


class TestOutputFormat:
    def test_json_from_plan_create(self, seeded_db):
        result = invoke("--output", "json", "plan", "create", "--campaign", "fmt-test")
        import json

        assert_ok(result)
        data = json.loads(result.stdout)
        assert data["campaign_id"] == "fmt-test"

    def test_json_from_tick(self, seeded_db):
        result = invoke("--output", "json", "tick", "--dry-run")
        import json

        assert_ok(result)
        data = json.loads(result.stdout)
        assert "total_actions" in data


class TestNonExistentCommand:
    def test_unknown_command_fails(self):
        result = invoke("nonexistent-command")
        assert result.exit_code != 0


class TestBrokersList:
    def test_list_default_returns_brokers(self):
        result = invoke("brokers", "list")
        assert_ok(result)
        assert "broker(s)" in result.stdout

    def test_list_json_shape(self):
        from .conftest import assert_json_output

        result = invoke("--output", "json", "brokers", "list")
        data = assert_json_output(result)
        assert data["schema_version"] == 1
        assert "count" in data
        assert "brokers" in data
        assert data["count"] == len(data["brokers"])
        assert data["filters"]["include_disabled"] is False

    def test_list_filter_by_priority(self):
        from .conftest import assert_json_output

        result = invoke("--output", "json", "brokers", "list", "--priority", "high")
        data = assert_json_output(result)
        for broker in data["brokers"]:
            assert broker["priority"] == "high"

    def test_list_filter_by_jurisdiction(self):
        from .conftest import assert_json_output

        result = invoke("--output", "json", "brokers", "list", "--jurisdiction", "DE")
        data = assert_json_output(result)
        for broker in data["brokers"]:
            assert "DE" in broker["jurisdictions"]

    def test_list_excludes_disabled_by_default(self):
        from .conftest import assert_json_output

        result = invoke("--output", "json", "brokers", "list")
        data = assert_json_output(result)
        for broker in data["brokers"]:
            assert broker["disabled"] is False, (
                f"Disabled broker {broker['id']} leaked into default list"
            )

    def test_list_include_disabled(self):
        from .conftest import assert_json_output

        result_default = invoke("--output", "json", "brokers", "list")
        result_all = invoke("--output", "json", "brokers", "list", "--include-disabled")
        default = assert_json_output(result_default)
        full = assert_json_output(result_all)
        assert full["count"] >= default["count"]
        disabled = [b for b in full["brokers"] if b["disabled"]]
        assert disabled, "registry should have at least one disabled broker for this test"


class TestBrokersShow:
    def test_show_known_broker_text(self):
        result = invoke("brokers", "show", "acxiom-eu")
        assert_ok(result)
        assert "Acxiom" in result.stdout
        assert "acxiom-eu" in result.stdout

    def test_show_known_broker_json(self):
        from .conftest import assert_json_output

        result = invoke("--output", "json", "brokers", "show", "acxiom-eu")
        data = assert_json_output(result)
        assert data["schema_version"] == 1
        assert data["broker"]["id"] == "acxiom-eu"
        assert data["broker"]["name"] == "Acxiom (EU)"

    def test_show_disabled_broker_still_visible(self):
        """`show` is informational — it shows disabled brokers too."""
        from .conftest import assert_json_output

        result = invoke("--output", "json", "brokers", "show", "beenverified-us")
        data = assert_json_output(result)
        assert data["broker"]["id"] == "beenverified-us"
        assert data["broker"]["disabled"] is True

    def test_show_unknown_broker_exits_nonzero(self):
        result = invoke("brokers", "show", "this-broker-does-not-exist")
        assert result.exit_code != 0


class TestStatusCommand:
    def test_status_empty_db_text(self, tmp_home):
        result = invoke("status")
        assert_ok(result)
        assert "Total: 0" in result.stdout

    def test_status_empty_db_json(self, tmp_home):
        from .conftest import assert_json_output

        result = invoke("--output", "json", "status")
        data = assert_json_output(result)
        assert data["schema_version"] == 1
        assert data["totals"]["requests"] == 0
        assert data["scope"]["campaign_id"] == "all"
        assert "by_status" in data
        assert "upcoming" in data

    def test_status_with_seeded_data(self, seeded_db):
        from .conftest import assert_json_output

        result = invoke("--output", "json", "status")
        data = assert_json_output(result)
        assert data["totals"]["requests"] == 7  # 5 + 2 from seeded_db
        assert data["by_status"].get("PLANNED", 0) == 7
        assert data["by_channel"].get("email", 0) == 7

    def test_status_scoped_to_campaign(self, seeded_db):
        from .conftest import assert_json_output

        result = invoke("--output", "json", "status", "--campaign", "smoke-test-ccpa")
        data = assert_json_output(result)
        assert data["scope"]["campaign_id"] == "smoke-test-ccpa"
        assert data["totals"]["requests"] == 2

    def test_status_unknown_campaign_is_zero(self, seeded_db):
        from .conftest import assert_json_output

        result = invoke("--output", "json", "status", "--campaign", "does-not-exist")
        data = assert_json_output(result)
        assert data["totals"]["requests"] == 0


class TestValidateCommand:
    def test_validate_bundled_registry_passes(self):
        result = invoke("validate")
        assert_ok(result)
        assert "OK" in result.stdout

    def test_validate_json_shape(self):
        from .conftest import assert_json_output

        result = invoke("--output", "json", "validate")
        data = assert_json_output(result)
        assert data["schema_version"] == 1
        assert data["ok"] is True
        assert data["totals"]["valid"] >= 30
        assert data["totals"]["failed"] == 0
        assert data["totals"]["duplicate_ids"] == 0

    def test_validate_failure_exits_nonzero(self, tmp_path):
        """Point validate at a directory with a broken YAML — must exit nonzero."""
        broken_dir = tmp_path / "brokers"
        broken_dir.mkdir()
        (broken_dir / "broken.yaml").write_text(
            "id: broken\nname: Broken\nwebsite: not-a-url\n"
            "category: people-search\njurisdictions: []\nlaws: []\n"
            "priority: high\nopt_out: []\n"
        )
        result = invoke("validate", "--registry-dir", str(broken_dir))
        assert result.exit_code != 0

    def test_validate_failure_lists_file(self, tmp_path):
        from .conftest import assert_in_output_stderr

        broken_dir = tmp_path / "brokers"
        broken_dir.mkdir()
        (broken_dir / "broken.yaml").write_text(
            "id: broken\nname: Broken\nwebsite: https://x.com\n"
            "category: people-search\njurisdictions: []\nlaws: []\n"
            "priority: high\nopt_out: []\n"
        )
        result = invoke("validate", "--registry-dir", str(broken_dir))
        assert_in_output_stderr(result, "broken.yaml")


class TestCalendarCommand:
    def test_calendar_empty_db_text(self, tmp_home):
        result = invoke("calendar")
        assert_ok(result)
        assert "Nothing scheduled" in result.stdout

    def test_calendar_default_weeks_is_4(self, tmp_home):
        from .conftest import assert_json_output

        result = invoke("--output", "json", "calendar")
        data = assert_json_output(result)
        assert data["horizon_weeks"] == 4

    def test_calendar_custom_horizon(self, tmp_home):
        from .conftest import assert_json_output

        result = invoke("--output", "json", "calendar", "--weeks", "12")
        data = assert_json_output(result)
        assert data["horizon_weeks"] == 12

    def test_calendar_clamps_to_minimum_1(self, tmp_home):
        from .conftest import assert_json_output

        result = invoke("--output", "json", "calendar", "--weeks", "0")
        data = assert_json_output(result)
        assert data["horizon_weeks"] == 1

    def test_calendar_picks_up_deadline(self, tmp_home):
        """Seed a SENT request, run tick to project deadline, calendar must see it."""
        from openeraseme.core.db import init_db
        from openeraseme.core.events import create_campaign, create_removal_request
        from openeraseme.core.projection import append_event_and_project

        init_db()
        create_campaign("cal-test")
        rid = create_removal_request(
            broker_id="acxiom", campaign_id="cal-test", jurisdiction="GDPR-DE"
        )
        append_event_and_project(
            rid,
            "SENT",
            payload={"to": "x@y.com", "expected_response_days": 14},
        )

        from .conftest import assert_json_output

        result = invoke("--output", "json", "calendar", "--weeks", "8")
        data = assert_json_output(result)
        assert data["totals"]["entries"] >= 1
        # Find our request in the entries
        all_ids = [e["request_id"] for week in data["weeks"] for e in week["entries"]]
        assert rid in all_ids


class TestExportCommand:
    def test_export_empty_db_to_stdout(self, tmp_home):
        result = invoke("--output", "json", "export")
        assert_ok(result)
        import json as _json

        data = _json.loads(result.stdout)
        assert data["totals"]["requests"] == 0

    def test_export_rejects_unknown_format(self, tmp_home):
        result = invoke("export", "--format", "yaml")
        assert result.exit_code != 0

    def test_export_json_to_file(self, seeded_db, tmp_path):
        out = tmp_path / "campaign.json"
        result = invoke("export", "--output-file", str(out))
        assert_ok(result)
        assert out.exists()
        import json as _json

        data = _json.loads(out.read_text())
        assert data["schema_version"] == 1
        assert data["totals"]["requests"] == 7
        # Each request must have an `events` list
        for r in data["requests"]:
            assert "events" in r

    def test_export_csv_to_file(self, seeded_db, tmp_path):
        out = tmp_path / "campaign.csv"
        result = invoke("export", "--format", "csv", "--output-file", str(out))
        assert_ok(result)
        assert out.exists()
        content = out.read_text()
        assert "request_id,broker_id" in content
        assert "PLANNED" in content

    def test_export_scoped_to_campaign(self, seeded_db, tmp_path):
        out = tmp_path / "ccpa.json"
        result = invoke("export", "--campaign", "smoke-test-ccpa", "--output-file", str(out))
        assert_ok(result)
        import json as _json

        data = _json.loads(out.read_text())
        assert data["scope"]["campaign_id"] == "smoke-test-ccpa"
        assert data["totals"]["requests"] == 2


class TestRegistrySyncCommand:
    def test_sync_runs_in_git_checkout(self):
        """The bundled registry lives in a git checkout — sync must succeed."""
        from .conftest import assert_json_output

        result = invoke("--output", "json", "registry", "sync")
        data = assert_json_output(result)
        assert data["schema_version"] == 1
        assert data["mode"] == "git"
        assert data["ok"] is True

    def test_sync_text_mode(self):
        result = invoke("registry", "sync")
        assert_ok(result)
        assert "Registry sync" in result.stdout

    def test_sync_accepts_verify_signatures_flag(self):
        """v0.1 documents --verify-signatures as a no-op stub."""
        from .conftest import assert_json_output

        result = invoke("--output", "json", "registry", "sync", "--verify-signatures")
        data = assert_json_output(result)
        assert data["signature_verification"].startswith("skipped")

    def test_sync_pip_mode_when_no_git_root(self, monkeypatch):
        """If the registry lives outside any git tree, sync reports pip-install mode."""
        from openeraseme.registry import sync as sync_mod

        monkeypatch.setattr(sync_mod, "_find_git_root", lambda _p: None)

        from .conftest import assert_json_output

        result = invoke("--output", "json", "registry", "sync")
        data = assert_json_output(result)
        assert data["mode"] == "pip"
        assert "upgrade" in data["message"].lower()
