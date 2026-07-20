"""Tests for the reporting service handlers."""

from __future__ import annotations

import os
from pathlib import Path
from unittest.mock import MagicMock, patch
import pytest

from symeraseme.core.result_types import CliResult
from symeraseme.services.reporting import (
    handle_generate_dashboard,
    handle_generate_report,
)

SRV = "symeraseme.services.reporting"


class TestHandleGenerateDashboard:
    def test_success_no_auto_open(self, tmp_path):
        dummy_data = {
            "campaigns": [{"id": "camp-1"}],
            "total_requests": 5,
        }
        dummy_html = "<html>Dashboard</html>"
        output_file = tmp_path / "dashboard.html"

        with (
            patch(f"{SRV}.get_dashboard_data", return_value=dummy_data) as mock_get_data,
            patch(f"{SRV}.generate_dashboard", return_value=dummy_html) as mock_gen,
            patch.object(Path, "write_text") as mock_write,
            patch.object(os, "chmod") as mock_chmod,
            patch("webbrowser.open") as mock_open,
        ):
            result = handle_generate_dashboard(
                output=str(output_file),
                auto_open=False,
                auto_refresh=30,
            )

            assert result.success is True
            assert result.data["output_file"] == str(output_file.resolve())
            assert result.data["size_bytes"] == len(dummy_html)
            assert result.data["campaigns"] == 1
            assert result.data["requests"] == 5
            assert "Dashboard generated" in result.data["message"]

            mock_get_data.assert_called_once()
            mock_gen.assert_called_once_with(dummy_data, auto_refresh_seconds=30)
            mock_write.assert_called_once_with(dummy_html)
            mock_chmod.assert_called_once_with(str(output_file), 0o600)
            mock_open.assert_not_called()

    def test_success_with_auto_open(self, tmp_path):
        dummy_data = {"campaigns": [], "total_requests": 0}
        dummy_html = "<html>Dashboard</html>"
        output_file = tmp_path / "dashboard.html"

        with (
            patch(f"{SRV}.get_dashboard_data", return_value=dummy_data),
            patch(f"{SRV}.generate_dashboard", return_value=dummy_html),
            patch.object(Path, "write_text"),
            patch.object(os, "chmod"),
            patch("webbrowser.open") as mock_open,
        ):
            result = handle_generate_dashboard(
                output=str(output_file),
                auto_open=True,
            )

            assert result.success is True
            mock_open.assert_called_once_with(f"file://{output_file.resolve()}")


class TestHandleGenerateReport:
    def test_json_format_with_output(self, tmp_path):
        dummy_data = {"some": "data"}
        dummy_report = {"summary": "report"}
        output_file = tmp_path / "report.json"

        with (
            patch(f"{SRV}.get_report_data", return_value=dummy_data) as mock_get_data,
            patch(f"{SRV}.generate_report", return_value=dummy_report) as mock_gen,
            patch.object(Path, "write_text") as mock_write,
        ):
            result = handle_generate_report(
                campaign_id="camp-123",
                format="json",
                output=str(output_file),
                all_campaigns=False,
            )

            assert result.success is True
            assert result.data["output_file"] == str(output_file.resolve())
            assert "Report written to" in result.data["message"]

            mock_get_data.assert_called_once_with(campaign_id="camp-123", all_campaigns=False)
            mock_gen.assert_called_once_with(dummy_data, format="json")
            # JSON format dumps with indent=2
            mock_write.assert_called_once()
            written_arg = mock_write.call_args[0][0]
            assert "summary" in written_arg

    def test_json_format_without_output(self):
        dummy_data = {"some": "data"}
        dummy_report = {"summary": "report"}

        with (
            patch(f"{SRV}.get_report_data", return_value=dummy_data),
            patch(f"{SRV}.generate_report", return_value=dummy_report),
        ):
            result = handle_generate_report(
                format="json",
                output="",
            )

            assert result.success is True
            assert result.data["report"] == dummy_report
            assert result.data["message"] == "Report generated."

    def test_html_format_with_output(self, tmp_path):
        dummy_data = {"some": "data"}
        dummy_report = "<html>Report</html>"
        output_file = tmp_path / "report.html"

        with (
            patch(f"{SRV}.get_report_data", return_value=dummy_data),
            patch(f"{SRV}.generate_report", return_value=dummy_report),
            patch.object(Path, "write_text") as mock_write,
        ):
            result = handle_generate_report(
                format="html",
                output=str(output_file),
            )

            assert result.success is True
            assert result.data["output_file"] == str(output_file.resolve())
            mock_write.assert_called_once_with(dummy_report)

    def test_html_format_without_output(self):
        dummy_data = {"some": "data"}
        dummy_report = "<html>Report</html>"

        with (
            patch(f"{SRV}.get_report_data", return_value=dummy_data),
            patch(f"{SRV}.generate_report", return_value=dummy_report),
            patch.object(Path, "write_text") as mock_write,
        ):
            result = handle_generate_report(
                campaign_id="my-camp",
                format="html",
                output="",
            )

            assert result.success is True
            expected_name = "report-my-camp.html"
            assert result.data["output_file"] == str(Path(expected_name).resolve())
            mock_write.assert_called_once_with(dummy_report)
