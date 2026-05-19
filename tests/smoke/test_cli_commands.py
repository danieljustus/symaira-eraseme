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
