from __future__ import annotations

from .conftest import (
    assert_in_output_stderr,
    assert_ok,
    invoke,
)


class TestAutoConfirm:
    def test_auto_confirm_dry_run(self, seeded_db):
        result = invoke("auto-confirm", "1", "--dry-run")
        assert result.exit_code in (0, 1)

    def test_auto_confirm_nonexistent(self, seeded_db):
        result = invoke("auto-confirm", "9999")
        assert result.exit_code != 0
        assert_in_output_stderr(result, "not found")


class TestManualTasks:
    def test_manual_tasks_list_empty(self, seeded_db):
        result = invoke("manual-tasks", "list")
        assert_ok(result)
        assert "No manual tasks" in result.stdout

    def test_manual_tasks_show_nonexistent(self, seeded_db):
        result = invoke("manual-tasks", "show", "9999")
        assert result.exit_code != 0
        assert_in_output_stderr(result, "not found")

    def test_manual_tasks_complete_nonexistent(self, seeded_db):
        result = invoke("manual-tasks", "complete", "9999")
        assert result.exit_code != 0
        assert_in_output_stderr(result, "not found")

    def test_manual_tasks_with_pending(self, seeded_db):
        from symeraseme.core.db import get_connection, init_db

        init_db()
        conn = get_connection()
        conn.execute(
            "INSERT INTO manual_tasks "
            "(request_id, broker_id, broker_name, form_url, reason, instructions, status) "
            "VALUES (1, 'acxiom-eu', 'Acxiom (EU)', 'https://acxiom.com/opt-out', "
            "'captcha_required', 'Navigate to URL and fill form', 'pending')"
        )
        conn.commit()
        result = invoke("manual-tasks", "list")
        assert_ok(result)
        assert "Acxiom" in result.stdout

    def test_manual_tasks_complete_with_pending(self, seeded_db):
        from symeraseme.core.db import get_connection, init_db

        init_db()
        conn = get_connection()
        conn.execute(
            "INSERT INTO manual_tasks (request_id, broker_id, broker_name, form_url, "
            "reason, instructions, status) "
            "VALUES (2, 'oracle', 'Oracle', 'https://oracle.com/opt-out', "
            "'multi_step_form', 'Complete opt-out manually', 'pending')"
        )
        conn.commit()
        task_id = conn.execute("SELECT id FROM manual_tasks WHERE request_id = 2").fetchone()[0]
        result = invoke("manual-tasks", "complete", str(task_id))
        assert_ok(result)
        assert "marked as completed" in result.stdout
