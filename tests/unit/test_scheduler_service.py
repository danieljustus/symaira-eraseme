"""Tests for scheduler service handlers."""

from __future__ import annotations

from unittest.mock import patch

import pytest
import typer

from symeraseme.core.result_types import CliResult
from symeraseme.services.scheduler import (
    handle_generate_scheduler,
    handle_schedule_install,
    handle_schedule_status,
    handle_schedule_uninstall,
)

SRV = "symeraseme.services.scheduler"


class TestHandleGenerateScheduler:
    """Tests for handle_generate_scheduler (lines 14-64)."""

    def test_success_with_explicit_platform(self):
        """Generates configs and writes them for a given platform."""
        mock_files = {
            "install.sh": "#!/bin/bash\n# install script",
            "uninstall.sh": "#!/bin/bash\n# uninstall script",
            "symeraseme-tick.cron": "# tick entry",
        }
        mock_written = [
            "/tmp/schedules/install.sh",
            "/tmp/schedules/uninstall.sh",
            "/tmp/schedules/symeraseme-tick.cron",
        ]

        with (
            patch(f"{SRV}.generate_scheduler_configs", return_value=mock_files) as mock_gen,
            patch(f"{SRV}.write_scheduler_files", return_value=mock_written) as mock_write,
        ):
            result = handle_generate_scheduler(
                platform="cron",
                output_dir="/tmp/schedules",
                tick_hour=10,
                tick_minute=0,
                poll_hours="8,12,16,20",
                project_dir="/project",
                symeraseme_bin="symeraseme",
                venv_activate="/project/.venv/bin/activate",
                dry_run=False,
            )

        assert result.success is True
        assert result.data["platform"] == "cron"
        assert result.data["output_dir"] == "/tmp/schedules"
        assert result.data["files"] == mock_written
        assert result.data["dry_run"] is False
        assert "Generated 3 file(s) in /tmp/schedules:" in result.message
        for f in mock_written:
            assert f in result.message

        mock_gen.assert_called_once_with(
            platform_name="cron",
            output_dir="/tmp/schedules",
            tick_hour=10,
            tick_minute=0,
            poll_hours=[8, 12, 16, 20],
            project_dir="/project",
            symeraseme_bin="symeraseme",
            venv_activate="/project/.venv/bin/activate",
        )
        mock_write.assert_called_once_with(mock_files, "/tmp/schedules", dry_run=False)

    def test_dry_run(self):
        """With dry_run=True message says [dry-run] and does not write."""
        mock_files = {"install.sh": "content", "uninstall.sh": "content"}

        with (
            patch(f"{SRV}.generate_scheduler_configs", return_value=mock_files) as mock_gen,
            patch(f"{SRV}.write_scheduler_files", return_value=[]) as mock_write,
        ):
            result = handle_generate_scheduler(
                platform="launchd",
                output_dir="./schedules",
                dry_run=True,
            )

        assert result.success is True
        assert result.data["dry_run"] is True
        assert result.data["platform"] == "launchd"
        assert "[dry-run] Would generate 2 file(s)" in result.message

        mock_gen.assert_called_once()
        mock_write.assert_called_once_with(mock_files, "./schedules", dry_run=True)

    def test_value_error_from_generate(self):
        """ValueError from generate_scheduler_configs returns error CliResult."""
        with patch(
            f"{SRV}.generate_scheduler_configs",
            side_effect=ValueError("Unsupported platform: windows"),
        ):
            result = handle_generate_scheduler(platform="windows")

        assert result.success is False
        assert result.error is not None
        assert "Scheduler error" in result.error
        assert "Unsupported platform: windows" in result.error
        assert "--platform cron|launchd|systemd" in result.error

    def test_empty_platform_shows_auto(self):
        """When platform is empty, result.data['platform'] is 'auto'."""
        with (
            patch(f"{SRV}.generate_scheduler_configs", return_value={"f": "c"}),
            patch(f"{SRV}.write_scheduler_files", return_value=["/tmp/f"]),
        ):
            result = handle_generate_scheduler(platform="")

        assert result.success is True
        assert result.data["platform"] == "auto"

    def test_poll_hours_parsing(self):
        """poll_hours string is parsed into int list and passed to generate."""
        with (
            patch(f"{SRV}.generate_scheduler_configs") as mock_gen,
            patch(f"{SRV}.write_scheduler_files", return_value=[]),
        ):
            handle_generate_scheduler(
                platform="cron", poll_hours="6, 18, 22", dry_run=True
            )

        mock_gen.assert_called_once()
        assert mock_gen.call_args.kwargs["poll_hours"] == [6, 18, 22]


class TestHandleScheduleInstall:
    """Tests for handle_schedule_install (lines 67-130)."""

    def test_dry_run_returns_plan_without_writing(self):
        """Dry_run mode shows plan and does NOT call write_scheduler_files."""
        mock_files = {
            "install.sh": "content",
            "uninstall.sh": "content",
            "symeraseme-tick.cron": "content",
        }

        with (
            patch(f"{SRV}.generate_scheduler_configs", return_value=mock_files) as mock_gen,
            patch(f"{SRV}.write_scheduler_files") as mock_write,
        ):
            result = handle_schedule_install(platform="cron", dry_run=True)

        assert result.success is True
        assert result.data["dry_run"] is True
        assert result.data["platform"] == "cron"
        assert result.data["files"] == list(mock_files.keys())
        assert "[DRY RUN]" in result.message
        assert "install.sh" in result.message
        assert "bash ./schedules/install.sh" in result.message
        assert "(installs crontab entries)" in result.message
        assert "bash ./schedules/uninstall.sh" in result.message

        mock_gen.assert_called_once_with(
            platform_name="cron",
            output_dir="./schedules",
            tick_hour=10,
            tick_minute=0,
        )
        mock_write.assert_not_called()

    def test_install_with_yes_skips_confirm(self):
        """With yes=True, typer.confirm is not called."""
        mock_files = {"install.sh": "c", "uninstall.sh": "c"}
        mock_written = ["./schedules/install.sh", "./schedules/uninstall.sh"]

        with (
            patch(f"{SRV}.generate_scheduler_configs", return_value=mock_files),
            patch(f"{SRV}.write_scheduler_files", return_value=mock_written),
            patch(f"{SRV}.typer.echo"),
            patch(f"{SRV}.typer.confirm") as mock_confirm,
        ):
            result = handle_schedule_install(platform="cron", yes=True)

        assert result.success is True
        assert "dry_run" not in result.data
        assert "Schedule configs generated" in result.message
        assert "bash ./schedules/install.sh" in result.message
        mock_confirm.assert_not_called()

    def test_confirm_called_when_not_yes(self):
        """When yes=False, typer.confirm is called with abort=True."""
        with (
            patch(f"{SRV}.generate_scheduler_configs", return_value={"f": "c"}),
            patch(f"{SRV}.write_scheduler_files", return_value=["/f"]),
            patch(f"{SRV}.typer.echo"),
            patch(f"{SRV}.typer.confirm", return_value=True) as mock_confirm,
        ):
            result = handle_schedule_install(platform="launchd", yes=False)

        assert result.success is True
        mock_confirm.assert_called_once_with("Continue?", abort=True)

    def test_confirm_abort_raises_typer_abort(self):
        """When user declines confirm, typer.Abort is raised."""
        with (
            patch(f"{SRV}.typer.echo"),
            patch(f"{SRV}.typer.confirm", side_effect=typer.Abort()),
        ):
            with pytest.raises(typer.Abort):
                handle_schedule_install(platform="cron")

    def test_empty_platform_calls_detect(self):
        """When platform is empty, detect_platform is called."""
        with (
            patch(f"{SRV}.detect_platform", return_value="cron") as mock_detect,
            patch(f"{SRV}.generate_scheduler_configs", return_value={"f": "c"}),
            patch(f"{SRV}.write_scheduler_files", return_value=["/f"]),
            patch(f"{SRV}.typer.echo"),
            patch(f"{SRV}.typer.confirm", return_value=True),
        ):
            result = handle_schedule_install(platform="", yes=True)

        assert result.success is True
        assert result.data["platform"] == "cron"
        mock_detect.assert_called_once()


class TestHandleScheduleUninstall:
    """Tests for handle_schedule_uninstall (lines 133-150)."""

    def test_default_platform_instructions(self):
        """Uninstall gives bash script instructions for default platform."""
        with patch(f"{SRV}.detect_platform", return_value="cron"):
            result = handle_schedule_uninstall()

        assert result.success is True
        assert result.data["platform"] == "cron"
        assert "To uninstall" in result.data["message"]
        assert "bash ./schedules/uninstall.sh" in result.data["message"]

    def test_launchd_includes_manual_steps(self):
        """launchd platform includes manual plist removal commands."""
        with patch(f"{SRV}.detect_platform", return_value="launchd"):
            result = handle_schedule_uninstall()

        assert result.success is True
        assert result.data["platform"] == "launchd"
        msg = result.data["message"]
        assert "com.symeraseme.tick" in msg
        assert "com.symeraseme.poll" in msg
        assert "com.symeraseme.rescan" in msg
        assert "launchctl unload" in msg
        assert "rm -f" in msg

    def test_explicit_platform(self):
        """Explicit platform is used without calling detect_platform."""
        with patch(f"{SRV}.detect_platform") as mock_detect:
            result = handle_schedule_uninstall(platform="systemd")

        assert result.success is True
        assert result.data["platform"] == "systemd"
        assert "bash ./schedules/uninstall.sh" in result.data["message"]
        mock_detect.assert_not_called()


class TestHandleScheduleStatus:
    """Tests for handle_schedule_status (lines 153-171)."""

    def test_status_formats_installed_services(self):
        """Installed services show ✓ and their path."""
        mock_status = {
            "platform": "cron",
            "installed": [
                {"label": "cron", "installed": True, "path": "crontab"},
            ],
            "last_run": None,
            "next_run": None,
        }

        with patch(f"{SRV}.get_schedule_status", return_value=mock_status):
            result = handle_schedule_status(platform="cron")

        assert result.success is True
        assert result.data["platform"] == "cron"
        msg = result.data["message"]
        assert "✓ installed" in msg
        assert "Path: crontab" in msg

    def test_status_shows_not_installed(self):
        """Non-installed services show ✗."""
        mock_status = {
            "platform": "systemd",
            "installed": [
                {"label": "symeraseme-tick", "installed": False, "path": "symeraseme-tick.timer"},
                {"label": "symeraseme-poll", "installed": True, "path": "symeraseme-poll.timer"},
            ],
            "last_run": None,
            "next_run": None,
        }

        with patch(f"{SRV}.get_schedule_status", return_value=mock_status):
            result = handle_schedule_status(platform="systemd")

        assert result.success is True
        msg = result.data["message"]
        assert "✗ not installed" in msg
        assert "✓ installed" in msg

    def test_status_shows_errors(self):
        """Services with an error field show the error text."""
        mock_status = {
            "platform": "cron",
            "installed": [
                {"label": "cron", "installed": False, "error": "crontab not available"},
            ],
            "last_run": None,
            "next_run": None,
        }

        with patch(f"{SRV}.get_schedule_status", return_value=mock_status):
            result = handle_schedule_status()

        assert result.success is True
        assert "Error: crontab not available" in result.data["message"]

    def test_empty_platform_calls_detect(self):
        """When platform is empty, detect_platform is called."""
        mock_status = {
            "platform": "launchd",
            "installed": [],
            "last_run": None,
            "next_run": None,
        }

        with (
            patch(f"{SRV}.detect_platform", return_value="launchd") as mock_detect,
            patch(f"{SRV}.get_schedule_status", return_value=mock_status),
        ):
            result = handle_schedule_status(platform="")

        assert result.success is True
        assert result.data["platform"] == "launchd"
        mock_detect.assert_called_once()

    def test_returns_cliresult(self):
        """Handler always returns a CliResult instance."""
        with patch(f"{SRV}.get_schedule_status", return_value={"platform": "x", "installed": []}):
            result = handle_schedule_status(platform="x")

        assert isinstance(result, CliResult)
