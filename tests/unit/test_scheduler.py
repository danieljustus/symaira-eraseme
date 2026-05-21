"""Tests for the scheduler generator (cron, launchd, systemd)."""

from __future__ import annotations

from datetime import time

from openeraseme.core.scheduler import (
    SchedulerConfig,
    detect_platform,
    generate_cron,
    generate_launchd,
    generate_scheduler_configs,
    generate_systemd,
    get_schedule_status,
    write_scheduler_files,
)


class TestDetectPlatform:
    def test_returns_string(self):
        plat = detect_platform()
        assert plat in ("cron", "launchd", "systemd")


class TestGenerateCron:
    def test_returns_dict_with_key_files(self):
        config = SchedulerConfig(platform="cron")
        files = generate_cron(config)
        assert "openeraseme-tick.sh" in files
        assert "openeraseme-poll.sh" in files
        assert "openeraseme-rescan.sh" in files
        assert "crontab.txt" in files
        assert "install.sh" in files
        assert "uninstall.sh" in files

    def test_tick_wrapper_contains_command(self):
        config = SchedulerConfig(platform="cron")
        files = generate_cron(config)
        content = files["openeraseme-tick.sh"]
        assert "openeraseme" in content
        assert "tick" in content
        assert "#!/usr/bin/env bash" in content

    def test_poll_wrapper_checks_times(self):
        config = SchedulerConfig(platform="cron", poll_times=[time(8, 0), time(20, 0)])
        files = generate_cron(config)
        content = files["openeraseme-poll.sh"]
        assert "08:00" in content or "8:00" in content
        assert "20:00" in content

    def test_crontab_has_entries(self):
        config = SchedulerConfig(platform="cron")
        files = generate_cron(config)
        content = files["crontab.txt"]
        assert "openeraseme" in content
        assert "tick" in content
        assert "poll" in content
        assert "rescan" in content

    def test_install_uninstall_are_shell_scripts(self):
        config = SchedulerConfig(platform="cron")
        files = generate_cron(config)
        assert files["install.sh"].startswith("#!/usr/bin/env bash")
        assert files["uninstall.sh"].startswith("#!/usr/bin/env bash")

    def test_venv_block_included_when_set(self):
        config = SchedulerConfig(platform="cron", venv_activate="/path/to/venv/bin/activate")
        files = generate_cron(config)
        content = files["openeraseme-tick.sh"]
        assert "source" in content and "activate" in content

    def test_bin_override(self):
        config = SchedulerConfig(platform="cron", openeraseme_bin="/custom/bin/openeraseme")
        files = generate_cron(config)
        content = files["openeraseme-tick.sh"]
        assert "/custom/bin/openeraseme" in content


class TestGenerateLaunchd:
    def test_returns_dict_with_key_files(self):
        config = SchedulerConfig(platform="launchd")
        files = generate_launchd(config)
        assert "openeraseme-tick.sh" in files
        assert "com.openeraseme.tick.plist" in files
        assert "com.openeraseme.poll.plist" in files
        assert "com.openeraseme.rescan.plist" in files
        assert "install.sh" in files
        assert "uninstall.sh" in files

    def test_tick_plist_is_valid_xml(self):
        config = SchedulerConfig(platform="launchd")
        files = generate_launchd(config)
        content = files["com.openeraseme.tick.plist"]
        assert "<?xml" in content
        assert "<plist" in content
        assert "com.openeraseme.tick" in content
        assert "</plist>" in content

    def test_poll_plist_has_multiple_calendar_intervals(self):
        config = SchedulerConfig(
            platform="launchd", poll_times=[time(8, 0), time(12, 0), time(16, 0), time(20, 0)]
        )
        files = generate_launchd(config)
        content = files["com.openeraseme.poll.plist"]
        # Should have multiple StartCalendarInterval entries
        assert content.count("<key>Hour</key>") >= 4

    def test_rescan_plist_has_quarterly_dates(self):
        config = SchedulerConfig(platform="launchd")
        files = generate_launchd(config)
        content = files["com.openeraseme.rescan.plist"]
        assert "Month" in content
        # Jan, Apr, Jul, Oct
        assert "<integer>1</integer>" in content
        assert "<integer>4</integer>" in content

    def test_install_script_loads_plists(self):
        config = SchedulerConfig(platform="launchd")
        files = generate_launchd(config)
        content = files["install.sh"]
        assert "launchctl load" in content
        assert "LaunchAgents" in content


class TestGenerateSystemd:
    def test_returns_dict_with_key_files(self):
        config = SchedulerConfig(platform="systemd")
        files = generate_systemd(config)
        assert "openeraseme-tick.sh" in files
        assert "openeraseme-tick.service" in files
        assert "openeraseme-tick.timer" in files
        assert "openeraseme-poll.service" in files
        assert "openeraseme-poll.timer" in files
        assert "openeraseme-rescan.service" in files
        assert "openeraseme-rescan.timer" in files

    def test_service_has_unit_section(self):
        config = SchedulerConfig(platform="systemd")
        files = generate_systemd(config)
        for key in files:
            if key.endswith(".service"):
                assert "[Unit]" in files[key]
                assert "[Service]" in files[key]
                assert "ExecStart" in files[key]

    def test_timer_has_timer_section(self):
        config = SchedulerConfig(platform="systemd")
        files = generate_systemd(config)
        for key in files:
            if key.endswith(".timer"):
                assert "[Unit]" in files[key]
                assert "[Timer]" in files[key]
                assert "OnCalendar" in files[key]

    def test_tick_timer_daily(self):
        config = SchedulerConfig(platform="systemd")
        files = generate_systemd(config)
        content = files["openeraseme-tick.timer"]
        assert "Daily" in content

    def test_poll_timer_hourly(self):
        config = SchedulerConfig(platform="systemd")
        files = generate_systemd(config)
        content = files["openeraseme-poll.timer"]
        assert "Hourly" in content

    def test_rescan_timer_quarterly(self):
        config = SchedulerConfig(platform="systemd")
        files = generate_systemd(config)
        content = files["openeraseme-rescan.timer"]
        assert "Quarterly" in content


class TestGenerateSchedulerConfigs:
    def test_auto_detect(self):
        files = generate_scheduler_configs()
        assert isinstance(files, dict)
        assert len(files) > 0

    def test_cron_platform(self):
        files = generate_scheduler_configs(platform_name="cron")
        assert "crontab.txt" in files

    def test_launchd_platform(self):
        files = generate_scheduler_configs(platform_name="launchd")
        assert any(f.endswith(".plist") for f in files)

    def test_systemd_platform(self):
        files = generate_scheduler_configs(platform_name="systemd")
        assert any(f.endswith(".timer") for f in files)

    def test_invalid_platform_raises(self):
        import pytest

        with pytest.raises(ValueError, match="Unsupported platform"):
            generate_scheduler_configs(platform_name="windows")

    def test_custom_tick_time(self):
        files = generate_scheduler_configs(platform_name="cron", tick_hour=14, tick_minute=30)
        content = files["crontab.txt"]
        assert "14" in content or "30" in content

    def test_custom_poll_hours(self):
        files = generate_scheduler_configs(platform_name="cron", poll_hours=[6, 18])
        content = files["openeraseme-poll.sh"]
        assert "06:00" in content or "18:00" in content


class TestWriteSchedulerFiles:
    def test_writes_to_directory(self, tmp_path):
        files = {"test.sh": "#!/usr/bin/env bash\necho hello"}
        out_dir = str(tmp_path / "schedules")
        written = write_scheduler_files(files, out_dir)
        assert len(written) == 1
        assert (tmp_path / "schedules" / "test.sh").exists()
        assert (tmp_path / "schedules" / "test.sh").stat().st_mode & 0o111

    def test_dry_run_does_not_write(self, tmp_path):
        files = {"test.sh": "#!/usr/bin/env bash\necho hello"}
        out_dir = str(tmp_path / "schedules")
        written = write_scheduler_files(files, out_dir, dry_run=True)
        assert len(written) == 1
        assert not (tmp_path / "schedules" / "test.sh").exists()

    def test_creates_parent_dirs(self, tmp_path):
        files = {"sub/test.sh": "#!/usr/bin/env bash\necho hello"}
        out_dir = str(tmp_path / "deep/schedules")
        write_scheduler_files(files, out_dir)
        assert (tmp_path / "deep" / "schedules" / "sub" / "test.sh").exists()


class TestGetScheduleStatus:
    def test_returns_dict_for_cron(self):
        status = get_schedule_status("cron")
        assert status["platform"] == "cron"
        assert "installed" in status

    def test_returns_dict_for_launchd(self):
        status = get_schedule_status("launchd")
        assert status["platform"] == "launchd"
        assert len(status["installed"]) == 3

    def test_returns_dict_for_systemd(self):
        status = get_schedule_status("systemd")
        assert status["platform"] == "systemd"
        assert len(status["installed"]) >= 1


class TestSchedulerConfig:
    def test_default_values(self):
        cfg = SchedulerConfig()
        assert cfg.tick_time == time(10, 0)
        assert cfg.poll_times == [time(8, 0), time(12, 0), time(16, 0), time(20, 0)]
        assert cfg.rescan_interval_months == 3
        assert cfg.output_dir == "./schedules"

    def test_custom_values(self):
        cfg = SchedulerConfig(
            tick_time=time(6, 30),
            poll_times=[time(9, 0), time(17, 0)],
            rescan_interval_months=6,
        )
        assert cfg.tick_time == time(6, 30)
        assert cfg.poll_times == [time(9, 0), time(17, 0)]
        assert cfg.rescan_interval_months == 6
