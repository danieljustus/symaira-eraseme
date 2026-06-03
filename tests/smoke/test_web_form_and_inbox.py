"""Smoke tests for web-form automation and manual-tasks commands."""

from __future__ import annotations

import os
from unittest.mock import patch

from .conftest import assert_ok, invoke


class TestRunWebForm:
    def test_run_web_form_dry_run(self, seeded_db):
        with patch("symeraseme.cli.commands.web_form_commands.handle_run_web_form") as mock_handler:
            from symeraseme.core.result_types import CliResult

            mock_handler.return_value = CliResult(
                data={"broker_id": "acxiom-eu", "steps": 3},
                message="Web form dry-run for acxiom-eu: 3 steps",
            )
            result = invoke("run-web-form", "acxiom-eu", "--dry-run")
            assert_ok(result)
            assert "acxiom-eu" in result.stdout

    def test_run_web_form_dry_run_json(self, seeded_db):
        with patch("symeraseme.cli.commands.web_form_commands.handle_run_web_form") as mock_handler:
            from symeraseme.core.result_types import CliResult

            mock_handler.return_value = CliResult(
                data={"broker_id": "acxiom-eu", "steps": 3},
                message="Web form dry-run for acxiom-eu: 3 steps",
            )
            result = invoke("--output", "json", "run-web-form", "acxiom-eu", "--dry-run")
            assert_ok(result)
            import json

            data = json.loads(result.stdout)
            assert data["broker_id"] == "acxiom-eu"


class TestAutoConfirm:
    def test_auto_confirm_dry_run(self, seeded_db):
        with patch("symeraseme.cli.commands.web_form_commands.handle_auto_confirm") as mock_handler:
            from symeraseme.core.result_types import CliResult

            mock_handler.return_value = CliResult(
                data={"request_id": 1, "confirmed": False},
                message="Auto-confirm dry-run for request #1",
            )
            result = invoke("auto-confirm", "1", "--dry-run")
            assert_ok(result)


class TestSolveCaptcha:
    def test_solve_captcha_dry_run(self, seeded_db):
        with patch(
            "symeraseme.cli.commands.web_form_commands.handle_solve_captcha"
        ) as mock_handler:
            from symeraseme.core.result_types import CliResult

            mock_handler.return_value = CliResult(
                data={"provider": "capsolver", "site_key": "test-key"},
                message="Captcha solve dry-run",
            )
            result = invoke(
                "solve-captcha",
                "--site-key",
                "test-key",
                "--page-url",
                "https://example.com",
                "--dry-run",
            )
            assert_ok(result)


class TestManualTasks:
    def test_manual_tasks_list_empty(self, tmp_home):
        result = invoke("manual-tasks", "list")
        assert_ok(result)

    def test_manual_tasks_list_json(self, tmp_home):
        result = invoke("--output", "json", "manual-tasks", "list")
        assert_ok(result)
        import json

        data = json.loads(result.stdout)
        assert isinstance(data, dict)
        assert "tasks" in data or "message" in data

    def test_manual_tasks_show_nonexistent(self, tmp_home):
        result = invoke("manual-tasks", "show", "9999")
        assert result.exit_code != 0

    def test_manual_tasks_cleanup_dry_run(self, tmp_home):
        result = invoke("manual-tasks", "cleanup", "--dry-run")
        assert_ok(result)

    def test_manual_tasks_cleanup_json(self, tmp_home):
        result = invoke("--output", "json", "manual-tasks", "cleanup", "--dry-run")
        assert_ok(result)
        import json

        data = json.loads(result.stdout)
        assert isinstance(data, dict)


class TestPollInbox:
    def test_poll_inbox_dry_run_mocked(self, tmp_home):
        with patch("symeraseme.cli.commands.monitoring_commands.handle_poll_inbox") as mock_handler:
            from symeraseme.core.result_types import CliResult

            mock_handler.return_value = CliResult(
                data={"messages": 0, "matched": 0},
                message="Inbox poll complete: 0 messages, 0 matched",
            )
            os.environ["IMAP_PASSWORD"] = "test-password"
            result = invoke(
                "poll-inbox",
                "--username",
                "test@example.com",
                "--host",
                "imap.example.com",
            )
            assert_ok(result)
            assert "poll complete" in result.stdout.lower()

    def test_poll_inbox_json(self, tmp_home):
        with patch("symeraseme.cli.commands.monitoring_commands.handle_poll_inbox") as mock_handler:
            from symeraseme.core.result_types import CliResult

            mock_handler.return_value = CliResult(
                data={"messages": 2, "matched": 1},
                message="Inbox poll complete: 2 messages, 1 matched",
            )
            os.environ["IMAP_PASSWORD"] = "test-password"
            result = invoke(
                "--output",
                "json",
                "poll-inbox",
                "--username",
                "test@example.com",
            )
            assert_ok(result)
            import json

            data = json.loads(result.stdout)
            assert data["messages"] == 2
