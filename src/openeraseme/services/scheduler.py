from __future__ import annotations

import json

import typer

from openeraseme.core.scheduler import (
    detect_platform,
    generate_scheduler_configs,
    get_schedule_status,
    write_scheduler_files,
)


def handle_generate_scheduler(
    platform: str = "",
    output_dir: str = "./schedules",
    tick_hour: int = 10,
    tick_minute: int = 0,
    poll_hours: str = "8,12,16,20",
    project_dir: str = "",
    openeraseme_bin: str = "",
    venv_activate: str = "",
    dry_run: bool = False,
    output_format: str = "text",
) -> str:
    poll_hours_list = [int(h.strip()) for h in poll_hours.split(",") if h.strip()]

    try:
        files = generate_scheduler_configs(
            platform_name=platform,
            output_dir=output_dir,
            tick_hour=tick_hour,
            tick_minute=tick_minute,
            poll_hours=poll_hours_list,
            project_dir=project_dir,
            openeraseme_bin=openeraseme_bin,
            venv_activate=venv_activate,
        )
    except ValueError as e:
        typer.echo(
            f"Scheduler error: {e}. "
            "Use --platform cron|launchd|systemd or ensure the platform is supported.",
            err=True,
        )
        raise typer.Exit(1) from e

    written = write_scheduler_files(files, output_dir, dry_run=dry_run)

    if output_format == "json":
        return json.dumps(
            {
                "platform": platform or "auto",
                "output_dir": output_dir,
                "files": written,
                "dry_run": dry_run,
            },
            indent=2,
        )

    if dry_run:
        lines = [f"[dry-run] Would generate {len(files)} file(s) for {platform or 'auto'}:"]
    else:
        lines = [f"Generated {len(written)} file(s) in {output_dir}:"]
    for f in written:
        lines.append(f"  {f}")
    return "\n".join(lines)


def handle_schedule_install(
    platform: str = "",
    tick_hour: int = 10,
    tick_minute: int = 0,
    yes: bool = False,
    dry_run: bool = False,
    output_format: str = "text",
) -> str:
    plat = platform or detect_platform()
    output_dir = "./schedules"

    if dry_run:
        files = generate_scheduler_configs(
            platform_name=plat,
            output_dir=output_dir,
            tick_hour=tick_hour,
            tick_minute=tick_minute,
        )
        if output_format == "json":
            return json.dumps(
                {
                    "platform": plat,
                    "output_dir": output_dir,
                    "files": list(files.keys()),
                    "dry_run": True,
                },
                indent=2,
            )
        lines = [f"[DRY RUN] Would generate schedule configs for {plat} in {output_dir}:"]
        for name in files:
            lines.append(f"  {name}")
        lines.append("")
        lines.append("To install:")
        suffix = "   # (installs crontab entries)" if plat == "cron" else ""
        lines.append(f"  bash {output_dir}/install.sh{suffix}")
        lines.append("")
        lines.append("To uninstall:")
        lines.append(f"  bash {output_dir}/uninstall.sh")
        return "\n".join(lines)

    if not yes:
        typer.echo(f"Platform detected: {plat}")
        typer.echo(f"Output directory: {output_dir}")
        typer.echo("Files will be generated and install helpers will be placed in the output dir.")
        typer.confirm("Continue?", abort=True)

    files = generate_scheduler_configs(
        platform_name=plat,
        output_dir=output_dir,
        tick_hour=tick_hour,
        tick_minute=tick_minute,
    )
    written = write_scheduler_files(files, output_dir)

    if output_format == "json":
        return json.dumps(
            {
                "platform": plat,
                "output_dir": output_dir,
                "files": written,
            },
            indent=2,
        )

    lines = [f"Schedule configs generated for {plat} in {output_dir}.", ""]
    lines.append("To install:")
    suffix = "   # (installs crontab entries)" if plat == "cron" else ""
    lines.append(f"  bash {output_dir}/install.sh{suffix}")
    lines.append("")
    lines.append("To uninstall:")
    lines.append(f"  bash {output_dir}/uninstall.sh")
    return "\n".join(lines)


def handle_schedule_uninstall(platform: str = "") -> str:
    plat = platform or detect_platform()
    lines = [f"Platform: {plat}"]
    lines.append("To uninstall, run the uninstall script from your schedules directory:")
    lines.append("  bash ./schedules/uninstall.sh")
    if plat == "launchd":
        lines.append("")
        lines.append("Or manually:")
        for label in ["com.openeraseme.tick", "com.openeraseme.poll", "com.openeraseme.rescan"]:
            lines.append(
                f"  launchctl unload ~/Library/LaunchAgents/{label}.plist 2>/dev/null; "
                f"rm -f ~/Library/LaunchAgents/{label}.plist"
            )
    return "\n".join(lines)


def handle_schedule_status(
    platform: str = "",
    output_format: str = "text",
) -> str:
    plat = platform or detect_platform()
    status = get_schedule_status(platform_name=plat)

    if output_format == "json":
        return json.dumps(status, indent=2, default=str)

    lines = [f"Platform: {status['platform']}", "Installed services:"]
    for svc in status["installed"]:
        label = svc.get("label", "?")
        installed = "✓ installed" if svc.get("installed") else "✗ not installed"
        path = svc.get("path", "")
        lines.append(f"  {label}: {installed}")
        if path:
            lines.append(f"    Path: {path}")
        if svc.get("error"):
            lines.append(f"    Error: {svc['error']}")
    return "\n".join(lines)
