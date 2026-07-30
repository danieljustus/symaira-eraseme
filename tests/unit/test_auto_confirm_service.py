"""Tests for the auto-confirm service handler in ``services/auto_confirm.py``."""

from __future__ import annotations

from unittest.mock import AsyncMock, MagicMock, patch

import pytest


def _ac():
    import symeraseme.services.auto_confirm as m

    return m


class TestHandleAutoConfirm:
    """Coverage for ``handle_auto_confirm`` — all branches."""

    def test_request_not_found(self):
        mod = _ac()
        with (
            patch.object(mod, "get_removal_request", return_value=None),
            patch("symeraseme.core.db_connection.init_db"),
        ):
            result = mod.handle_auto_confirm(request_id=999)

        assert result.success is False
        assert "not found" in (result.error or "")

    def test_no_events_found(self):
        mod = _ac()
        with (
            patch.object(mod, "get_removal_request", return_value={"id": 1}),
            patch.object(mod, "get_events", return_value=[]),
            patch("symeraseme.core.db_connection.init_db"),
        ):
            result = mod.handle_auto_confirm(request_id=1)

        assert result.success is False
        assert "No events found" in (result.error or "")

    def test_dry_run_no_reply(self):
        mod = _ac()
        confirm_result = MagicMock()
        confirm_result.success = True
        confirm_result.clicked_url = "https://example.com/confirm"
        confirm_result.step = "dry_run"
        confirm_result.error = None
        confirm_result.dry_run = True
        confirm_result.screenshot_before = ""
        confirm_result.screenshot_after = ""

        mock_conn = MagicMock()
        mock_conn.execute.return_value.fetchone.return_value = None

        with (
            patch.object(mod, "get_removal_request", return_value={"id": 1}),
            patch.object(
                mod,
                "get_events",
                return_value=[{"payload_json": {"snippet": "Click https://example.com/confirm"}}],
            ),
            patch.object(mod, "get_connection", return_value=mock_conn),
            patch.object(mod, "auto_confirm", new_callable=AsyncMock, return_value=confirm_result),
            patch("symeraseme.core.db_connection.init_db"),
        ):
            result = mod.handle_auto_confirm(request_id=1, dry_run=True)

        assert result.success is True
        assert "[DRY RUN]" in result.data["message"]

    def test_success_path_logs_event(self):
        mod = _ac()
        confirm_result = MagicMock()
        confirm_result.success = True
        confirm_result.clicked_url = "https://example.com/confirm"
        confirm_result.step = "clicked_button"
        confirm_result.error = None
        confirm_result.dry_run = False
        confirm_result.screenshot_before = "/tmp/before.png"
        confirm_result.screenshot_after = "/tmp/after.png"

        mock_conn = MagicMock()
        mock_conn.execute.return_value.fetchone.return_value = None

        with (
            patch.object(mod, "get_removal_request", return_value={"id": 1}),
            patch.object(
                mod,
                "get_events",
                return_value=[{"payload_json": {"template": "Click https://example.com/confirm"}}],
            ),
            patch.object(mod, "get_connection", return_value=mock_conn),
            patch.object(mod, "auto_confirm", new_callable=AsyncMock, return_value=confirm_result),
            patch.object(mod, "append_event_and_project") as mock_append,
            patch("symeraseme.core.db_connection.init_db"),
        ):
            result = mod.handle_auto_confirm(request_id=1)

        assert result.success is True
        assert "clicked" in result.data["message"]
        mock_append.assert_called_once()

    def test_failure_path_logs_event(self):
        mod = _ac()
        confirm_result = MagicMock()
        confirm_result.success = False
        confirm_result.clicked_url = "https://example.com/confirm"
        confirm_result.step = "no_element"
        confirm_result.error = "No clickable element found"
        confirm_result.dry_run = False
        confirm_result.screenshot_before = None
        confirm_result.screenshot_after = None

        mock_conn = MagicMock()
        mock_conn.execute.return_value.fetchone.return_value = None

        with (
            patch.object(mod, "get_removal_request", return_value={"id": 1}),
            patch.object(
                mod,
                "get_events",
                return_value=[{"payload_json": {"snippet": "Link https://example.com/confirm"}}],
            ),
            patch.object(mod, "get_connection", return_value=mock_conn),
            patch.object(mod, "auto_confirm", new_callable=AsyncMock, return_value=confirm_result),
            patch.object(mod, "append_event_and_project") as mock_append,
            patch("symeraseme.core.db_connection.init_db"),
        ):
            result = mod.handle_auto_confirm(request_id=1)

        assert result.success is False
        assert "failed" in (result.error or "")
        mock_append.assert_called_once()
        assert mock_append.call_args[0][1] == "NOTE_ADDED"

    def test_uses_reply_from_db(self):
        mod = _ac()
        confirm_result = MagicMock()
        confirm_result.success = True
        confirm_result.clicked_url = "https://example.com/confirm"
        confirm_result.step = "dry_run"
        confirm_result.error = None
        confirm_result.dry_run = True
        confirm_result.screenshot_before = None
        confirm_result.screenshot_after = None

        mock_conn = MagicMock()
        mock_conn.execute.return_value.fetchone.return_value = {
            "id": 10,
            "snippet": "Confirmation: https://example.com/confirm",
            "from_addr": "broker@example.com",
        }

        with (
            patch.object(mod, "get_removal_request", return_value={"id": 1}),
            patch.object(
                mod,
                "get_events",
                return_value=[{"payload_json": {"snippet": "old snippet"}}],
            ),
            patch.object(mod, "get_connection", return_value=mock_conn),
            patch.object(mod, "auto_confirm", new_callable=AsyncMock, return_value=confirm_result) as mock_ac,
            patch("symeraseme.core.db_connection.init_db"),
        ):
            result = mod.handle_auto_confirm(request_id=1, dry_run=True)

        assert result.success is True
        assert mock_ac.call_args[1].get("from_addr") == "broker@example.com"

    def test_failure_with_url_in_message(self):
        mod = _ac()
        confirm_result = MagicMock()
        confirm_result.success = False
        confirm_result.clicked_url = "https://example.com/fail"
        confirm_result.step = "no_element"
        confirm_result.error = "Something went wrong"
        confirm_result.dry_run = False
        confirm_result.screenshot_before = None
        confirm_result.screenshot_after = None

        mock_conn = MagicMock()
        mock_conn.execute.return_value.fetchone.return_value = None

        with (
            patch.object(mod, "get_removal_request", return_value={"id": 1}),
            patch.object(mod, "get_events", return_value=[{"payload_json": {}}]),
            patch.object(mod, "get_connection", return_value=mock_conn),
            patch.object(mod, "auto_confirm", new_callable=AsyncMock, return_value=confirm_result),
            patch.object(mod, "append_event_and_project"),
            patch("symeraseme.core.db_connection.init_db"),
        ):
            result = mod.handle_auto_confirm(request_id=1)

        assert result.success is False
        assert "URL: https://example.com/fail" in (result.error or "")
