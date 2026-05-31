"""Scheduler configuration and platform detection."""

from __future__ import annotations

import logging
import os
import platform
from dataclasses import dataclass, field
from datetime import time
from pathlib import Path

logger = logging.getLogger(__name__)

TICK_DEFAULT_TIME = time(10, 0)
POLL_TIMES = [time(8, 0), time(12, 0), time(16, 0), time(20, 0)]
RESCAN_INTERVAL_MONTHS = 3


@dataclass
class SchedulerConfig:
    """Combined schedule specification."""

    tick_time: time = TICK_DEFAULT_TIME
    poll_times: list[time] = field(default_factory=lambda: list(POLL_TIMES))
    rescan_interval_months: int = RESCAN_INTERVAL_MONTHS
    symeraseme_bin: str = ""
    venv_activate: str = ""
    output_dir: str = "./schedules"
    platform: str = ""


def detect_platform() -> str:
    """Detect the native scheduling platform."""
    system = platform.system().lower()
    if system == "darwin":
        return "launchd"
    if system == "linux":
        try:
            import subprocess

            result = subprocess.run(
                ["systemctl", "--version"],
                capture_output=True,
                text=True,
                timeout=5,
            )
            if result.returncode == 0:
                return "systemd"
        except (FileNotFoundError, subprocess.TimeoutExpired):
            pass
        return "cron"
    return "cron"


def _resolve_bin() -> str:
    """Resolve the symeraseme binary path."""
    from shutil import which

    return which("symeraseme") or os.environ.get("SYMERASEME_BIN", "/usr/local/bin/symeraseme")


def _resolve_venv() -> str:
    """Resolve the virtualenv activate path."""
    import sys

    if sys.prefix != sys.base_prefix:
        activate = Path(sys.prefix) / "bin" / "activate"
        if activate.exists():
            return str(activate)
    return os.environ.get("SYMERASEME_VENV", "")
