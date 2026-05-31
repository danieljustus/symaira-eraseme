"""Systemd (Linux) scheduler configuration generator."""

from __future__ import annotations

from symeraseme.core.scheduler.config import SchedulerConfig, _resolve_bin, _resolve_venv
from symeraseme.core.scheduler.wrapper import _wrapper_script


def _systemd_service(
    description: str,
    wrapper_path: str,
    user_service: bool = True,
) -> str:
    return f"""[Unit]
Description={description}
After=network.target

[Service]
Type=oneshot
ExecStart=/bin/bash {wrapper_path}
Environment=SYMERASEME_HEADLESS=1
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=default.target
"""


def _systemd_timer(
    description: str,
    on_calendar: str,
) -> str:
    return f"""[Unit]
Description={description}

[Timer]
OnCalendar={on_calendar}
Persistent=true

[Install]
WantedBy=timers.target
"""


def generate_systemd(
    config: SchedulerConfig,
    project_dir: str = "",
) -> dict[str, str]:
    """Generate systemd service + timer pairs and wrapper scripts."""
    bin_path = config.symeraseme_bin or _resolve_bin()
    venv = config.venv_activate or _resolve_venv()
    out: dict[str, str] = {}

    tick_cmd = f"{bin_path} tick --output json"
    out["symeraseme-tick.sh"] = _wrapper_script(
        tick_cmd, project_dir=project_dir, venv_activate=venv
    )
    out["symeraseme-tick.service"] = _systemd_service(
        "Symaira EraseMe daily tick",
        "__WRAPPER_DIR__/symeraseme-tick.sh",
    )
    out["symeraseme-tick.timer"] = _systemd_timer(
        "Symaira EraseMe daily tick",
        f"Daily-{config.tick_time.hour:02d}:{config.tick_time.minute:02d}",
    )

    poll_cmd = f"{bin_path} poll-inbox --output json"
    out["symeraseme-poll.sh"] = _wrapper_script(
        poll_cmd, project_dir=project_dir, venv_activate=venv
    )
    out["symeraseme-poll.service"] = _systemd_service(
        "Symaira EraseMe hourly inbox poll",
        "__WRAPPER_DIR__/symeraseme-poll.sh",
    )
    out["symeraseme-poll.timer"] = _systemd_timer(
        "Symaira EraseMe hourly inbox poll",
        "Hourly",
    )

    rescan_cmd = f"{bin_path} tick --output json"
    out["symeraseme-rescan.sh"] = _wrapper_script(
        rescan_cmd, project_dir=project_dir, venv_activate=venv
    )
    out["symeraseme-rescan.service"] = _systemd_service(
        "Symaira EraseMe quarterly re-scan",
        "__WRAPPER_DIR__/symeraseme-rescan.sh",
    )
    out["symeraseme-rescan.timer"] = _systemd_timer(
        "Symaira EraseMe quarterly re-scan",
        "Quarterly",
    )

    out["install.sh"] = _systemd_install_script()
    out["uninstall.sh"] = _systemd_uninstall_script()
    return out


def _systemd_install_script() -> str:
    return """#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
USER_MODE="${1:---user}"

chmod +x "$SCRIPT_DIR"/*.sh

for unit in "$SCRIPT_DIR"/*.service "$SCRIPT_DIR"/*.timer; do
    [ -f "$unit" ] || continue
    name=$(basename "$unit")
    dest="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/$name"
    mkdir -p "$(dirname "$dest")"
    sed "s|__WRAPPER_DIR__|$SCRIPT_DIR|g" "$unit" > "$dest"
    echo "Installed: $name"
done

systemctl $USER_MODE daemon-reload
systemctl $USER_MODE enable --now symeraseme-tick.timer
systemctl $USER_MODE enable --now symeraseme-poll.timer
systemctl $USER_MODE enable --now symeraseme-rescan.timer
echo ""
echo "Systemd timers installed and enabled."
echo "Wrappers in: $SCRIPT_DIR"
echo ""
systemctl $USER_MODE list-timers | grep -i symeraseme || true
"""


def _systemd_uninstall_script() -> str:
    return """#!/usr/bin/env bash
set -euo pipefail
USER_MODE="${1:---user}"

for unit in symeraseme-tick symeraseme-poll symeraseme-rescan; do
    systemctl $USER_MODE disable --now "$unit.timer" 2>/dev/null || true
done

for unit in "$SCRIPT_DIR"/*.service "$SCRIPT_DIR"/*.timer; do
    [ -f "$unit" ] || continue
    name=$(basename "$unit")
    dest="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/$name"
    rm -f "$dest"
    echo "Removed: $name"
done

systemctl $USER_MODE daemon-reload
echo "Systemd timers uninstalled."
"""
