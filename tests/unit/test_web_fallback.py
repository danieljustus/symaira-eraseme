"""Tests for the web fallback adapter."""

from __future__ import annotations

from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from symeraseme.adapters.web._fallback import capture_form_state


@patch("symeraseme.adapters.web._fallback._async_get_content")
@patch("symeraseme.adapters.web._fallback._async_extract_form_fields")
@patch("symeraseme.adapters.web._fallback._async_save_screenshot")
class TestCaptureFormState:
    def test_captures_url_and_reason(self, mock_save, mock_fields, mock_content):
        mock_page = MagicMock()
        mock_page.url = "https://broker.com/optout"
        mock_content.return_value = "<html><form>..."
        mock_fields.return_value = ({"email": "test@test.com"}, ["#email"])

        state = capture_form_state(
            mock_page,
            reason="timeout",
            error_message="Navigation timed out",
            broker_name="Test Broker",
        )
        assert state.url == "https://broker.com/optout"
        assert state.reason == "timeout"
        assert state.error_message == "Navigation timed out"
        assert state.broker_name == "Test Broker"

    def test_unknown_reason_normalized(self, mock_save, mock_fields, mock_content):
        mock_page = MagicMock()
        mock_page.url = "https://example.com"
        mock_fields.return_value = ({}, [])
        state = capture_form_state(mock_page, reason="some_crazy_error")
        assert state.reason == "generic_error"

    def test_with_screenshot_dir(self, mock_save, mock_fields, mock_content, tmp_path):
        mock_page = MagicMock()
        mock_page.url = "https://example.com"
        mock_save.return_value = str(tmp_path / "screenshot.png")
        mock_fields.return_value = ({}, [])

        state = capture_form_state(mock_page, screenshot_dir=tmp_path)
        assert state.screenshot_path is not None
        mock_save.assert_called_once()

    def test_failed_content_does_not_crash(self, mock_save, mock_fields, mock_content):
        from symeraseme.adapters.web._compat import PlaywrightError

        mock_page = MagicMock()
        mock_page.url = "https://example.com"
        mock_content.side_effect = PlaywrightError("Failed to get content")
        mock_fields.return_value = ({}, [])

        state = capture_form_state(mock_page)
        assert state.html_snapshot == ""
