"""File I/O and schedule status for generated configs."""

from __future__ import annotations

import logging
import stat
from pathlib import Path
from typing import Any

from symeraseme.core.scheduler.config import detect_platform

logger = logging.getLogger(__name__)


def write_scheduler_files(
    files: dict[str, str],
    output_dir: str,
    *,
    dry_run: bool = False,
) -> list[str]:
    """Write generated scheduler files to disk."""
    out_dir = Path(output_dir).expanduser().resolve()
    written: list[str] = []

    for filename, content in files.items():
        dest = out_dir / filename
        if dry_run:
            logger.info("[dry-run] Would write: %s (%d bytes)", dest, len(content))
            written.append(str(dest))
            continue

        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_text(content)

        exec_exts = (".sh", ".plist")
        exec_prefixes = ("install", "uninstall")
        if filename.endswith(exec_exts) or filename.startswith(exec_prefixes):
            dest.chmod(dest.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)

        written.append(str(dest))

    return written


def get_schedule_status(platform_name: str = "") -> dict[str, Any]:
    """Get current schedule status: installed configs and next run times."""
    if not platform_name:
        platform_name = detect_platform()

    platform_name = platform_name.lower()
    status: dict[str, Any] = {
        "platform": platform_name,
        "installed": [],
        "last_run": None,
        "next_run": None,
    }

    if platform_name == "launchd":
        import subprocess

        for label in ["com.symeraseme.tick", "com.symeraseme.poll", "com.symeraseme.rescan"]:
            plist_path = Path.home() / "Library" / "LaunchAgents" / f"{label}.plist"
            status["installed"].append(
                {
                    "label": label,
                    "installed": plist_path.exists(),
                    "path": str(plist_path) if plist_path.exists() else "",
                }
            )

    elif platform_name == "systemd":
        import subprocess

        for name in ["symeraseme-tick", "symeraseme-poll", "symeraseme-rescan"]:
            try:
                result = subprocess.run(
                    ["systemctl", "--user", "is-enabled", f"{name}.timer"],
                    capture_output=True,
                    text=True,
                    timeout=10,
                )
                enabled = result.returncode == 0
            except (FileNotFoundError, subprocess.TimeoutExpired):
                enabled = False
            status["installed"].append(
                {
                    "label": name,
                    "installed": enabled,
                    "path": f"{name}.timer",
                }
            )

    else:
        import subprocess

        try:
            result = subprocess.run(
                ["crontab", "-l"],
                capture_output=True,
                text=True,
                timeout=10,
            )
            has_entries = "symeraseme" in result.stdout
            status["installed"].append(
                {
                    "label": "cron",
                    "installed": has_entries,
                    "path": "crontab",
                }
            )
        except (FileNotFoundError, subprocess.TimeoutExpired):
            status["installed"].append(
                {
                    "label": "cron",
                    "installed": False,
                    "error": "crontab not available",
                }
            )

    return status
