"""Tests for the manual task CLI handlers in ``services/manual_task.py``."""

from __future__ import annotations

from unittest.mock import MagicMock, patch

import pytest


# Helper: import the module under test once
def _mt():
    import symeraseme.services.manual_task as m

    return m


class TestHandleManualTasksList:
    """Coverage for ``handle_manual_tasks_list``."""

    def test_no_tasks_returns_empty_message(self):
        mod = _mt()
        with (
            patch.object(mod, "list_manual_tasks", return_value=[]),
            patch("symeraseme.core.db_connection.init_db"),
        ):
            result = mod.handle_manual_tasks_list()

        assert result.success is True
        assert result.data["message"] == "No manual tasks found."
        assert result.data["tasks"] == []

    def test_with_tasks_returns_formatted_list(self):
        mod = _mt()
        mock_tasks = [
            {
                "id": 1,
                "status": "pending",
                "broker_name": "TestBroker",
                "broker_id": "test-broker",
                "reason": "unknown_captcha",
                "created_at": "2024-01-01T00:00:00",
            },
            {
                "id": 2,
                "status": "completed",
                "broker_name": "AnotherBroker",
                "broker_id": "another-broker",
                "reason": "timeout",
                "created_at": "2024-01-02T00:00:00",
            },
        ]
        with (
            patch.object(mod, "list_manual_tasks", return_value=mock_tasks),
            patch("symeraseme.core.db_connection.init_db"),
        ):
            result = mod.handle_manual_tasks_list()

        assert result.success is True
        assert "Manual tasks (2):" in result.data["message"]
        assert "#1" in result.data["message"]
        assert "#2" in result.data["message"]

    def test_filters_by_status_and_request_id(self):
        mod = _mt()
        with (
            patch.object(mod, "list_manual_tasks", return_value=[]) as mock_list,
            patch("symeraseme.core.db_connection.init_db"),
        ):
            mod.handle_manual_tasks_list(status="pending", request_id=42)

        mock_list.assert_called_once_with(status="pending", request_id=42)


class TestHandleManualTasksShow:
    """Coverage for ``handle_manual_tasks_show``."""

    def test_task_not_found_returns_error(self):
        mod = _mt()
        with (
            patch.object(mod, "get_manual_task", return_value=None),
            patch("symeraseme.core.db_connection.init_db"),
        ):
            result = mod.handle_manual_tasks_show(task_id=999)

        assert result.success is False
        assert "not found" in (result.error or "")
        assert "999" in (result.error or "")

    def test_task_found_returns_details(self):
        mod = _mt()
        mock_task = {
            "id": 1,
            "broker_name": "TestBroker",
            "broker_id": "test-broker",
            "form_url": "https://example.com/optout",
            "reason": "unknown_captcha",
            "status": "pending",
            "created_at": "2024-01-01T00:00:00",
            "instructions": "Please complete manually",
            "notes": "",
            "completed_at": None,
            "screenshot_path": None,
            "html_snapshot_path": None,
        }
        with (
            patch.object(mod, "get_manual_task", return_value=mock_task),
            patch("symeraseme.core.db_connection.init_db"),
        ):
            result = mod.handle_manual_tasks_show(task_id=1)

        assert result.success is True
        assert "TestBroker" in result.data["message"]
        assert result.data["id"] == 1

    def test_task_with_all_optional_fields(self):
        mod = _mt()
        mock_task = {
            "id": 2,
            "broker_name": "WithNotes",
            "broker_id": "with-notes",
            "form_url": "https://example.com/form",
            "reason": "timeout",
            "status": "completed",
            "created_at": "2024-01-01T00:00:00",
            "completed_at": "2024-01-02T00:00:00",
            "screenshot_path": "/tmp/screen.png",
            "html_snapshot_path": "/tmp/snapshot.html",
            "instructions": "Do the thing",
            "notes": "User notes here",
        }
        with (
            patch.object(mod, "get_manual_task", return_value=mock_task),
            patch("symeraseme.core.db_connection.init_db"),
        ):
            result = mod.handle_manual_tasks_show(task_id=2)

        assert result.success is True
        assert "Completed:" in result.data["message"]
        assert "Screenshot:" in result.data["message"]
        assert "HTML:" in result.data["message"]
        assert "Notes:" in result.data["message"]


class TestHandleManualTasksComplete:
    """Coverage for ``handle_manual_tasks_complete``."""

    def test_complete_task_not_found(self):
        mod = _mt()
        with (
            patch.object(mod, "resume_from_manual", return_value=None),
            patch("symeraseme.core.db_connection.init_db"),
        ):
            result = mod.handle_manual_tasks_complete(task_id=999, notes="done")

        assert result.success is False
        assert "not found" in (result.error or "")

    def test_complete_task_success(self):
        mod = _mt()
        with (
            patch.object(mod, "resume_from_manual", return_value=MagicMock()),
            patch("symeraseme.core.db_connection.init_db"),
        ):
            result = mod.handle_manual_tasks_complete(task_id=1, notes="completed manually")

        assert result.success is True
        assert "marked as completed" in result.data["message"]
        assert result.data["task_id"] == 1

    def test_complete_passes_notes(self):
        mod = _mt()
        with (
            patch.object(mod, "resume_from_manual", return_value=MagicMock()) as mock_resume,
            patch("symeraseme.core.db_connection.init_db"),
        ):
            mod.handle_manual_tasks_complete(task_id=1, notes="user notes here")

        mock_resume.assert_called_once_with(1, notes="user notes here", completed=True)


class TestHandleManualTasksCleanup:
    """Coverage for ``handle_manual_tasks_cleanup``."""

    def test_no_tasks_dir_dry_run(self):
        mod = _mt()
        result = mod.handle_manual_tasks_cleanup(dry_run=True)
        assert result.success is True
        assert "No manual tasks directory found" in result.data["message"]

    def test_dry_run_counts_skipped(self, tmp_path):
        tasks_dir = tmp_path / "data" / "manual_tasks"
        tasks_dir.mkdir(parents=True)
        (tasks_dir / "snap.png").write_bytes(b"PNG")
        (tasks_dir / "page.html").write_bytes(b"HTML")
        (tasks_dir / "data.json").write_bytes(b"{}")

        import symeraseme.core.config as config_mod

        with patch.object(config_mod, "get_config") as mc:
            mc.return_value.resolved_data_dir = tmp_path / "data"
            mod = _mt()
            result = mod.handle_manual_tasks_cleanup(dry_run=True)

        assert result.success is True
        assert result.data["skipped"] == 3
        assert result.data["removed"] == 0
        assert result.data["dry_run"] is True

    def test_actual_cleanup_removes_files(self, tmp_path):
        tasks_dir = tmp_path / "data" / "manual_tasks"
        tasks_dir.mkdir(parents=True)
        (tasks_dir / "snap.png").write_bytes(b"PNG")
        (tasks_dir / "page.html").write_bytes(b"HTML")
        (tasks_dir / "readme.txt").write_text("keep me")

        import symeraseme.core.config as config_mod

        with patch.object(config_mod, "get_config") as mc:
            mc.return_value.resolved_data_dir = tmp_path / "data"
            mod = _mt()
            result = mod.handle_manual_tasks_cleanup(dry_run=False)

        assert result.success is True
        assert result.data["removed"] == 2
        assert not (tasks_dir / "snap.png").exists()
        assert not (tasks_dir / "page.html").exists()
        assert (tasks_dir / "readme.txt").exists()

    def test_cleanup_with_no_matching_files(self, tmp_path):
        tasks_dir = tmp_path / "data" / "manual_tasks"
        tasks_dir.mkdir(parents=True)
        (tasks_dir / "some_other.txt").write_text("keep")

        import symeraseme.core.config as config_mod

        with patch.object(config_mod, "get_config") as mc:
            mc.return_value.resolved_data_dir = tmp_path / "data"
            mod = _mt()
            result = mod.handle_manual_tasks_cleanup(dry_run=False)

        assert result.success is True
        assert result.data["removed"] == 0
