"""Scheduler configuration generators for cron, launchd, and systemd."""

from datetime import time

from symeraseme.core.scheduler.config import (
    RESCAN_INTERVAL_MONTHS,
    TICK_DEFAULT_TIME,
    SchedulerConfig,
    detect_platform,
)
from symeraseme.core.scheduler.cron import generate_cron
from symeraseme.core.scheduler.launchd import generate_launchd
from symeraseme.core.scheduler.systemd import generate_systemd
from symeraseme.core.scheduler.writer import (
    get_schedule_status,
    write_scheduler_files,
)


def generate_scheduler_configs(
    platform_name: str = "",
    *,
    output_dir: str = "./schedules",
    tick_hour: int = TICK_DEFAULT_TIME.hour,
    tick_minute: int = TICK_DEFAULT_TIME.minute,
    poll_hours: list[int] | None = None,
    project_dir: str = "",
    symeraseme_bin: str = "",
    venv_activate: str = "",
) -> dict[str, str]:
    """Generate scheduler config files for the given platform."""
    if not platform_name:
        platform_name = detect_platform()

    platform_name = platform_name.lower()
    if platform_name not in ("cron", "launchd", "systemd"):
        msg = f"Unsupported platform: {platform_name}. Choose cron, launchd, or systemd."
        raise ValueError(msg)

    poll_times_list = [time(h, 0) for h in (poll_hours or [8, 12, 16, 20])]

    config = SchedulerConfig(
        tick_time=time(tick_hour, tick_minute),
        poll_times=poll_times_list,
        symeraseme_bin=symeraseme_bin,
        venv_activate=venv_activate,
        output_dir=output_dir,
        platform=platform_name,
    )

    generators = {
        "cron": generate_cron,
        "launchd": generate_launchd,
        "systemd": generate_systemd,
    }

    generator = generators[platform_name]
    return generator(config, project_dir=project_dir)


__all__ = [
    "RESCAN_INTERVAL_MONTHS",
    "TICK_DEFAULT_TIME",
    "SchedulerConfig",
    "detect_platform",
    "generate_cron",
    "generate_launchd",
    "generate_scheduler_configs",
    "generate_systemd",
    "get_schedule_status",
    "write_scheduler_files",
]
