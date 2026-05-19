from __future__ import annotations

from openeraseme.registry.schema import IdentityProfile

from .conftest import assert_ok, invoke


class TestInitProfile:
    def test_init_profile_creates_vault(self, tmp_home):
        result = invoke("init-profile", "--full-name", "Jane Doe", "--email", "jane@test.com")
        assert_ok(result)

    def test_show_profile_after_init(self, tmp_home):
        invoke("init-profile", "--full-name", "Jane Doe", "--email", "jane@test.com")
        result = invoke("show-profile")
        assert_ok(result)
        assert "Jane Doe" in result.stdout

    def test_show_profile_no_profile(self, tmp_home):
        result = invoke("show-profile")
        assert result.exit_code != 0

    def test_init_profile_overwrites(self, tmp_home):
        invoke("init-profile", "--full-name", "Old Name", "--email", "old@test.com")
        invoke("init-profile", "--full-name", "New Name", "--email", "new@test.com")
        result = invoke("show-profile")
        assert_ok(result)
        assert "New Name" in result.stdout


class TestDbInit:
    def test_db_init_creates_database(self, tmp_home):
        result = invoke("db-init")
        assert_ok(result)
        assert "Database initialized" in result.stdout

    def test_db_init_is_idempotent(self, tmp_home):
        invoke("db-init")
        result = invoke("db-init")
        assert_ok(result)

    def test_db_init_creates_tables(self, tmp_home):
        invoke("db-init")
        from openeraseme.core.db import get_connection

        conn = get_connection()
        tables = conn.execute(
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"
        ).fetchall()
        names = [r["name"] for r in tables]
        for expected in [
            "removal_requests",
            "request_events",
            "request_state",
            "campaigns",
            "inbox_replies",
            "reply_drafts",
            "manual_tasks",
        ]:
            assert expected in names, f"Missing table: {expected}"


class TestIdentityProfileModel:
    def test_minimal_profile(self):
        profile = IdentityProfile(full_name="Test User", email_addresses=["test@test.com"])
        assert profile.full_name == "Test User"

    def test_profile_roundtrip(self):
        original = IdentityProfile(full_name="Alice", email_addresses=["a@b.com"])
        data = original.model_dump()
        restored = IdentityProfile.model_validate(data)
        assert restored.full_name == original.full_name

    def test_profile_custom_fields(self):
        profile = IdentityProfile(
            full_name="Jane Doe",
            name_variants=["Jane Roe"],
            email_addresses=["jane@test.com"],
            phone_numbers=["+1-555-1234"],
            jurisdictions=["US", "EU"],
        )
        assert profile.jurisdictions == ["US", "EU"]
