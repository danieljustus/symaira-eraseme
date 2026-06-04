"""Tests for the manual fallback module."""

from __future__ import annotations

from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from symeraseme.core.manual_fallback import (
    FALLBACK_REASONS,
    FormState,
    ManualTask,
    _instructions_for_reason,
    _redact_identity_values,
    _tasks_dir,
    create_manual_task,
    list_manual_tasks,
    resume_from_manual,
)


class TestFallbackReasons:
    def test_known_reasons(self):
        assert "unknown_captcha" in FALLBACK_REASONS
        assert "timeout" in FALLBACK_REASONS
        assert "login_required" in FALLBACK_REASONS
        assert "generic_error" in FALLBACK_REASONS

    def test_reasons_are_frozen(self):
        with pytest.raises(AttributeError):
            FALLBACK_REASONS.add("new_reason")


class TestFormState:
    def test_defaults(self):
        state = FormState(url="https://example.com/form")
        assert state.url == "https://example.com/form"
        assert state.screenshot_path is None
        assert state.html_snapshot == ""
        assert state.form_fields == {}
        assert state.error_message == ""

    def test_full_state(self):
        state = FormState(
            url="https://broker.com/optout",
            screenshot_path="/tmp/screen.png",
            html_snapshot="<html></html>",
            form_fields={"name": "John", "email": "john@test.com"},
            field_selectors=["#name", "#email"],
            error_message="Timeout after 30s",
            reason="timeout",
            step_index=2,
            total_steps=5,
            broker_name="Test Broker",
            broker_id="test-broker",
        )
        assert state.reason == "timeout"
        assert state.step_index == 2
        assert state.total_steps == 5
        assert state.broker_name == "Test Broker"
        assert state.form_fields["name"] == "John"


class TestInstructionsForReason:
    def test_unknown_captcha_instructions(self):
        result = _instructions_for_reason("unknown_captcha", "Test Broker", "https://example.com")
        assert "unknown CAPTCHA" in result
        assert "Test Broker" in result

    def test_timeout_instructions(self):
        result = _instructions_for_reason("timeout", "DataCo", "https://dataco.com/optout")
        assert "timed out" in result.lower()
        assert "DataCo" in result

    def test_login_required_instructions(self):
        result = _instructions_for_reason(
            "login_required", "SecureCorp", "https://securecorp.com/privacy"
        )
        assert "login" in result.lower()
        assert "SecureCorp" in result

    def test_generic_error_instructions(self):
        result = _instructions_for_reason("generic_error", "Unknown Broker", "https://unknown.com")
        assert "unexpected error" in result.lower() or "manually" in result.lower()

    def test_unknown_reason_falls_back(self):
        result = _instructions_for_reason("made_up_reason", "Test", "https://test.com")
        assert "manually" in result.lower()

    def test_dynamic_form_instructions(self):
        result = _instructions_for_reason("dynamic_form", "AJAX Corp", "https://ajax.com")
        assert "dynamic" in result.lower()

    def test_assertion_failed_instructions(self):
        result = _instructions_for_reason("assertion_failed", "VerifyCo", "https://verify.com")
        assert "confirmation" in result.lower() or "verify" in result.lower()


class TestCreateManualTask:
    @patch("symeraseme.core.repositories.manual_tasks.get_connection")
    def test_creates_task_in_db(self, mock_get_conn):
        mock_conn = MagicMock()
        mock_cursor = MagicMock()
        mock_cursor.lastrowid = 42
        mock_conn.execute.return_value = mock_cursor
        mock_get_conn.return_value = mock_conn

        task = create_manual_task(
            request_id=1,
            broker_id="test-broker",
            broker_name="Test Broker",
            form_url="https://test.com/optout",
            reason="timeout",
            error_message="Navigation timed out",
        )

        assert task.id == 42
        assert task.request_id == 1
        assert task.broker_id == "test-broker"
        assert task.status == "pending"
        assert task.created_at is not None

        # Should have inserted into manual_tasks
        insert_call = mock_conn.execute.call_args_list[0]
        assert "INSERT INTO manual_tasks" in insert_call[0][0]

    @patch("symeraseme.core.repositories.manual_tasks.get_connection")
    def test_unknown_reason_normalized(self, mock_get_conn):
        mock_conn = MagicMock()
        mock_cursor = MagicMock()
        mock_cursor.lastrowid = 1
        mock_conn.execute.return_value = mock_cursor
        mock_get_conn.return_value = mock_conn

        task = create_manual_task(
            request_id=1,
            reason="made_up_reason",
            form_url="https://test.com",
        )
        assert task.reason == "generic_error"

    @patch("symeraseme.core.repositories.manual_tasks.get_connection")
    def test_saves_html_snapshot(self, mock_get_conn, tmp_path):
        mock_conn = MagicMock()
        mock_cursor = MagicMock()
        mock_cursor.lastrowid = 1
        mock_conn.execute.return_value = mock_cursor
        mock_get_conn.return_value = mock_conn

        with patch(
            "symeraseme.core.manual_fallback._tasks_dir",
            return_value=tmp_path,
        ):
            task = create_manual_task(
                request_id=1,
                reason="timeout",
                form_url="https://test.com",
                html_snapshot="<html><body>Form content</body></html>",
            )

        assert task.html_snapshot_path != ""
        saved_file = tmp_path / Path(task.html_snapshot_path).name
        assert saved_file.exists()
        assert "<html><body>Form content</body></html>" in saved_file.read_text()


class TestResumeFromManual:
    @patch("symeraseme.core.repositories.manual_tasks.get_connection")
    def test_completes_task(self, mock_get_conn):
        mock_conn = MagicMock()
        existing_row = {
            "id": 1,
            "request_id": 1,
            "broker_id": "test-broker",
            "broker_name": "Test Broker",
            "form_url": "https://test.com/optout",
            "reason": "timeout",
            "instructions": "Please complete manually",
            "screenshot_path": "",
            "html_snapshot_path": "",
            "form_fields_json": "{}",
            "status": "pending",
            "created_at": "2026-01-01T00:00:00",
            "notes": "",
        }
        mock_conn.execute.return_value.fetchone.return_value = existing_row
        mock_get_conn.return_value = mock_conn

        result = resume_from_manual(1, notes="Completed manually")
        assert result is not None
        assert result.status == "completed"
        assert result.notes == "Completed manually"

    @patch("symeraseme.core.repositories.manual_tasks.get_connection")
    def test_cancels_task(self, mock_get_conn):
        mock_conn = MagicMock()
        existing_row = {
            "id": 2,
            "request_id": None,
            "broker_id": "",
            "broker_name": "",
            "form_url": "",
            "reason": "",
            "instructions": "",
            "screenshot_path": "",
            "html_snapshot_path": "",
            "form_fields_json": "{}",
            "status": "pending",
            "created_at": "2026-01-01T00:00:00",
            "notes": "",
        }
        mock_conn.execute.return_value.fetchone.return_value = existing_row
        mock_get_conn.return_value = mock_conn

        result = resume_from_manual(2, completed=False)
        assert result is not None
        assert result.status == "cancelled"

    @patch("symeraseme.core.repositories.manual_tasks.get_connection")
    def test_nonexistent_task(self, mock_get_conn):
        mock_conn = MagicMock()
        mock_conn.execute.return_value.fetchone.return_value = None
        mock_get_conn.return_value = mock_conn

        result = resume_from_manual(999)
        assert result is None


class TestListManualTasks:
    @patch("symeraseme.core.repositories.manual_tasks.get_connection")
    def test_list_all(self, mock_get_conn):
        mock_conn = MagicMock()
        mock_conn.execute.return_value.fetchall.return_value = [
            {
                "id": 1,
                "status": "pending",
                "broker_name": "Test Broker",
                "request_id": 1,
                "broker_id": "test",
                "form_url": "https://test.com",
                "reason": "timeout",
                "instructions": "",
                "screenshot_path": "",
                "html_snapshot_path": "",
                "form_fields_json": "{}",
                "created_at": "2026-01-01",
                "completed_at": None,
                "notes": "",
            },
            {
                "id": 2,
                "status": "completed",
                "broker_name": "Other",
                "request_id": 2,
                "broker_id": "other",
                "form_url": "https://other.com",
                "reason": "captcha_failed",
                "instructions": "",
                "screenshot_path": "",
                "html_snapshot_path": "",
                "form_fields_json": "{}",
                "created_at": "2026-01-02",
                "completed_at": "2026-01-03",
                "notes": "Done",
            },
        ]
        mock_get_conn.return_value = mock_conn

        tasks = list_manual_tasks()
        assert len(tasks) == 2
        assert tasks[0]["status"] == "pending"
        assert tasks[1]["status"] == "completed"

    @patch("symeraseme.core.repositories.manual_tasks.get_connection")
    def test_filter_by_status(self, mock_get_conn):
        mock_conn = MagicMock()
        mock_conn.execute.return_value.fetchall.return_value = [
            {"id": 1, "status": "pending", "broker_name": "Test"},
        ]
        mock_get_conn.return_value = mock_conn

        tasks = list_manual_tasks(status="pending")
        assert len(tasks) == 1
        # Verify the SQL had WHERE clause
        call_args = mock_conn.execute.call_args[0]
        assert "WHERE" in call_args[0]
        assert "status = ?" in call_args[0]

    @patch("symeraseme.core.repositories.manual_tasks.get_connection")
    def test_empty_result(self, mock_get_conn):
        mock_conn = MagicMock()
        mock_conn.execute.return_value.fetchall.return_value = []
        mock_get_conn.return_value = mock_conn

        tasks = list_manual_tasks()
        assert tasks == []


class TestManualTask:
    def test_defaults(self):
        task = ManualTask(
            id=1,
            broker_id="test",
            broker_name="Test",
            form_url="https://test.com",
            reason="timeout",
            instructions="Do it manually",
        )
        assert task.id == 1
        assert task.status == "pending"
        assert task.created_at == ""
        assert task.completed_at is None

    def test_completed_task(self):
        task = ManualTask(
            id=2,
            broker_id="test",
            broker_name="Test",
            form_url="https://test.com",
            reason="captcha_failed",
            instructions="Solve captcha",
            status="completed",
            created_at="2026-01-01T00:00:00",
            completed_at="2026-01-02T00:00:00",
            notes="Done",
        )
        assert task.status == "completed"
        assert task.completed_at == "2026-01-02T00:00:00"
        assert task.notes == "Done"


class TestDbIntegration:
    def test_init_db_creates_manual_tasks_table(self):
        """Verify the manual_tasks table exists after init_db."""
        from symeraseme.core.db import close_connection, get_connection, init_db

        close_connection()

        import tempfile

        with tempfile.NamedTemporaryFile(suffix=".db", delete=False) as f:
            db_path = f.name

        try:
            init_db(db_path)
            conn = get_connection(db_path)

            # Check table exists
            result = conn.execute(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='manual_tasks'"
            ).fetchone()
            assert result is not None
            assert result["name"] == "manual_tasks"

            # Check columns
            columns = conn.execute("PRAGMA table_info(manual_tasks)").fetchall()
            col_names = [c["name"] for c in columns]
            assert "id" in col_names
            assert "request_id" in col_names
            assert "broker_id" in col_names
            assert "form_url" in col_names
            assert "reason" in col_names
            assert "status" in col_names
            assert "instructions" in col_names
            assert "created_at" in col_names
            assert "completed_at" in col_names
        finally:
            import os

            os.unlink(db_path)
            close_connection()


class TestRedactionAndPermissions:
    """Tests for PII redaction and restrictive file permissions."""

    def test_redact_identity_values_with_profile(self):
        from symeraseme.registry.schema import IdentityProfile

        profile = IdentityProfile(
            full_name="Jane Doe",
            email_addresses=["jane@example.com"],
            phone_numbers=["+1-555-1234"],
        )
        html = "<html>Jane Doe jane@example.com +1-555-1234</html>"
        redacted = _redact_identity_values(html, profile)
        assert "Jane Doe" not in redacted
        assert "jane@example.com" not in redacted
        assert "+1-555-1234" not in redacted
        assert "[REDACTED-NAME]" in redacted
        assert "[REDACTED-EMAIL]" in redacted
        assert "[REDACTED-PHONE]" in redacted

    def test_redact_identity_values_fallback_regex(self):
        html = "<html>Contact us at john@test.com or 555-123-4567</html>"
        redacted = _redact_identity_values(html)
        assert "john@test.com" not in redacted
        assert "555-123-4567" not in redacted
        assert "[REDACTED-EMAIL]" in redacted
        assert "[REDACTED-PHONE]" in redacted

    def test_tasks_dir_created_with_restrictive_permissions(self, tmp_path):
        with patch("symeraseme.core.manual_fallback.MANUAL_TASKS_DIR", str(tmp_path / "tasks")):
            tasks_dir = _tasks_dir()
            assert tasks_dir.exists()
            perms = tasks_dir.stat().st_mode & 0o777
            assert perms == 0o700, f"Expected 0o700 permissions, got {oct(perms)}"

    @patch("symeraseme.core.repositories.manual_tasks.get_connection")
    def test_html_snapshot_has_restrictive_permissions(self, mock_get_conn, tmp_path):
        mock_conn = MagicMock()
        mock_cursor = MagicMock()
        mock_cursor.lastrowid = 1
        mock_conn.execute.return_value = mock_cursor
        mock_get_conn.return_value = mock_conn

        with patch("symeraseme.core.manual_fallback._tasks_dir", return_value=tmp_path):
            task = create_manual_task(
                request_id=1,
                reason="timeout",
                form_url="https://test.com",
                html_snapshot="<html><body>Form</body></html>",
            )

        assert task.html_snapshot_path != ""
        saved_file = tmp_path / Path(task.html_snapshot_path).name
        assert saved_file.exists()
        perms = saved_file.stat().st_mode & 0o777
        assert perms == 0o600, f"Expected 0o600 permissions, got {oct(perms)}"
