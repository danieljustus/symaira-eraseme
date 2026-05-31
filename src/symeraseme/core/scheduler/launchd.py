"""Launchd (macOS) scheduler configuration generator."""

from __future__ import annotations

from symeraseme.core.scheduler.config import SchedulerConfig, _resolve_bin, _resolve_venv
from symeraseme.core.scheduler.wrapper import _wrapper_script

_LAUNCHD_PLIST_TEMPLATE = """<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>/bin/bash</string>
        <string>{wrapper_path}</string>
    </array>
    <key>StartCalendarInterval</key>
{interval_xml}
    <key>StandardOutPath</key>
    <string>/tmp/{label}.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/{label}.err</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>SYMERASEME_HEADLESS</key>
        <string>1</string>
    </dict>
    <key>RunAtLoad</key>
    <false/>
    <key>KeepAlive</key>
    <false/>
</dict>
</plist>"""


def _plist_content(
    label: str,
    wrapper_path: str,
    schedule_interval: str,
    *,
    wrap_array: bool = False,
) -> str:
    if wrap_array:
        interval_xml = f"    <array>\n{schedule_interval}\n    </array>"
    else:
        interval_xml = f"    {schedule_interval}"
    return _LAUNCHD_PLIST_TEMPLATE.format(
        label=label,
        wrapper_path=wrapper_path,
        interval_xml=interval_xml,
    )


def _plist_calendar(hour: int, minute: int) -> str:
    return f"""    <dict>
        <key>Hour</key>
        <integer>{hour}</integer>
        <key>Minute</key>
        <integer>{minute}</integer>
    </dict>"""


def _launchd_quarter_dates(hour: int, minute: int) -> str:
    entries = []
    for m in (1, 4, 7, 10):
        entries.append(
            f"    <dict>\n"
            f"        <key>Month</key>\n"
            f"        <integer>{m}</integer>\n"
            f"        <key>Day</key>\n"
            f"        <integer>1</integer>\n"
            f"        <key>Hour</key>\n"
            f"        <integer>{hour}</integer>\n"
            f"        <key>Minute</key>\n"
            f"        <integer>{minute}</integer>\n"
            f"    </dict>"
        )
    return "\n".join(entries)


def generate_launchd(
    config: SchedulerConfig,
    project_dir: str = "",
) -> dict[str, str]:
    """Generate launchd .plist files and wrapper scripts."""
    bin_path = config.symeraseme_bin or _resolve_bin()
    venv = config.venv_activate or _resolve_venv()
    out: dict[str, str] = {}

    out["symeraseme-tick.sh"] = _wrapper_script(
        f"{bin_path} tick --output json",
        project_dir=project_dir,
        venv_activate=venv,
    )
    out["com.symeraseme.tick.plist"] = _plist_content(
        label="com.symeraseme.tick",
        wrapper_path="__WRAPPER_DIR__/symeraseme-tick.sh",
        schedule_interval=_plist_calendar(config.tick_time.hour, config.tick_time.minute),
    )

    out["symeraseme-poll.sh"] = _wrapper_script(
        f"{bin_path} poll-inbox --output json",
        project_dir=project_dir,
        venv_activate=venv,
    )
    poll_entries = "\n".join(_plist_calendar(t.hour, t.minute) for t in config.poll_times)
    out["com.symeraseme.poll.plist"] = _plist_content(
        label="com.symeraseme.poll",
        wrapper_path="__WRAPPER_DIR__/symeraseme-poll.sh",
        schedule_interval=poll_entries,
        wrap_array=True,
    )

    out["symeraseme-rescan.sh"] = _wrapper_script(
        f"{bin_path} tick --output json",
        project_dir=project_dir,
        venv_activate=venv,
    )
    out["com.symeraseme.rescan.plist"] = _plist_content(
        label="com.symeraseme.rescan",
        wrapper_path="__WRAPPER_DIR__/symeraseme-rescan.sh",
        schedule_interval=_launchd_quarter_dates(config.tick_time.hour, config.tick_time.minute),
        wrap_array=True,
    )

    out["install.sh"] = _launchd_install_script()
    out["uninstall.sh"] = _launchd_uninstall_script()
    return out


def _launchd_install_script() -> str:
    return """#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

install_plist() {
    local src="$1"
    local name
    name=$(basename "$src")
    local dest="$HOME/Library/LaunchAgents/$name"

    # Replace wrapper dir placeholder
    sed "s|__WRAPPER_DIR__|$SCRIPT_DIR|g" "$src" > /tmp/"$name"
    cp /tmp/"$name" "$dest"
    chmod 644 "$dest"
    chmod +x "$SCRIPT_DIR/symeraseme-tick.sh"
    chmod +x "$SCRIPT_DIR/symeraseme-poll.sh"
    chmod +x "$SCRIPT_DIR/symeraseme-rescan.sh"

    launchctl load "$dest" 2>/dev/null || true
    launchctl start "$(basename "$name" .plist)" 2>/dev/null || true
    echo "Installed: $name"
}

for plist in "$SCRIPT_DIR"/*.plist; do
    [ -f "$plist" ] && install_plist "$plist"
done
echo ""
echo "launchd jobs installed. Wrappers in: $SCRIPT_DIR"
echo "Logs: /tmp/com.symeraseme.*.log"
"""


def _launchd_uninstall_script() -> str:
    return """#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

for plist in "$SCRIPT_DIR"/*.plist; do
    [ -f "$plist" ] || continue
    name=$(basename "$plist" .plist)
    dest="$HOME/Library/LaunchAgents/$(basename "$plist")"

    if [ -f "$dest" ]; then
        launchctl unload "$dest" 2>/dev/null || true
        rm -f "$dest"
        echo "Removed: $name"
    fi
done
echo "launchd jobs uninstalled."
"""
