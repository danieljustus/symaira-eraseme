from __future__ import annotations

from .conftest import (
    assert_in_output_stderr,
    assert_ok,
    invoke,
)


class TestExecute:
    def test_execute_dry_run(self, seeded_db):
        result = invoke("execute", "--campaign", "smoke-test", "--dry-run")
        assert_ok(result)

    def test_execute_dry_run_json(self, seeded_db):
        result = invoke("--output", "json", "execute", "--campaign", "smoke-test", "--dry-run")
        import json

        assert_ok(result)
        data = json.loads(result.stdout)
        assert data["campaign_id"] == "smoke-test"

    def test_execute_dry_run_batch_size(self, seeded_db):
        result = invoke("execute", "--campaign", "smoke-test", "--dry-run", "--batch-size", "3")
        assert_ok(result)

    def test_execute_without_consent_fails(self, seeded_db):
        result = invoke("execute", "--campaign", "smoke-test")
        assert result.exit_code != 0
        assert_in_output_stderr(result, "consent")

    def test_execute_nonexistent_campaign(self, seeded_db):
        result = invoke("execute", "--campaign", "nonexistent", "--dry-run")
        assert_ok(result)


class TestGrant:
    def test_grant_issues_token(self, tmp_home):
        result = invoke("grant", "execute")
        assert_ok(result)
        assert "Consent token:" in result.stdout

    def test_grant_with_ttl(self, tmp_home):
        result = invoke("grant", "execute", "--ttl", "3600")
        assert_ok(result)
        assert "TTL: 3600s" in result.stdout

    def test_grant_list_tokens(self, tmp_home):
        invoke("grant", "execute")
        result = invoke("grant", "--list")
        assert_ok(result)
        assert "cmd=execute" in result.stdout

    def test_grant_revoke_token(self, tmp_home):
        invoke("grant", "execute")
        list_result = invoke("grant", "--list")
        token = list_result.stdout.split()[0].strip()
        result = invoke("grant", "--revoke", token)
        assert_ok(result)

    def test_grant_revoke_all(self, tmp_home):
        invoke("grant", "execute")
        invoke("grant", "execute", "--ttl", "7200")
        result = invoke("grant", "--revoke-all")
        assert_ok(result)
        assert "Revoked" in result.stdout

    def test_grant_revoke_nonexistent(self, tmp_home):
        result = invoke("grant", "--revoke", "nonexistent")
        assert result.exit_code != 0
        assert_in_output_stderr(result, "not found")

    def test_grant_list_empty(self, tmp_home):
        result = invoke("grant", "--list")
        assert_ok(result)
        assert "No active tokens" in result.stdout

    def test_grant_revoke_all_empty(self, tmp_home):
        result = invoke("grant", "--revoke-all")
        assert_ok(result)
        assert "No active tokens to revoke" in result.stdout

    def test_grant_list_with_tokens(self, tmp_home):
        invoke("grant", "execute")
        result = invoke("grant", "--list")
        assert_ok(result)
        assert "cmd=execute" in result.stdout

    def test_execute_with_consent(self, seeded_db):
        invoke("grant", "execute", "--ttl", "3600")
        list_result = invoke("grant", "--list")
        assert_ok(list_result)
