"""Maintenance commands."""

from __future__ import annotations

import typer

from symeraseme.cli.console import console, print_success, render_error, render_result
from symeraseme.registry.sync import handle_registry_sync
from symeraseme.core.db import init_db
from symeraseme.services.export import handle_export
from symeraseme.services.scheduler import (
    handle_generate_scheduler,
    handle_schedule_install,
    handle_schedule_status,
    handle_schedule_uninstall,
)
from symeraseme.services.validate import handle_validate

schedule_app = typer.Typer(
    name="schedule",
    help="Manage scheduler configuration (install, uninstall, status)",
    no_args_is_help=True,
)
registry_app = typer.Typer(
    name="registry",
    help="Manage the broker registry (sync)",
    no_args_is_help=True,
)


def db_init() -> None:
    path = init_db()
    print_success(f"Database initialized at {path}")


def generate_scheduler_cmd(
    ctx: typer.Context,
    platform: str = typer.Option(
        "",
        "--platform",
        help="Target platform: cron, launchd, systemd (auto-detect if empty)",
    ),
    output_dir: str = typer.Option(
        "./schedules",
        "--output-dir",
        help="Output directory for generated files",
    ),
    tick_hour: int = typer.Option(
        10,
        "--tick-hour",
        help="Hour for daily tick (0-23)",
    ),
    tick_minute: int = typer.Option(
        0,
        "--tick-minute",
        help="Minute for daily tick (0-59)",
    ),
    poll_hours: str = typer.Option(
        "8,12,16,20",
        "--poll-hours",
        help="Comma-separated hours for poll-inbox",
    ),
    project_dir: str = typer.Option(
        "",
        "--project-dir",
        help="Project directory",
    ),
    symeraseme_bin: str = typer.Option(
        "",
        "--bin",
        help="Path to symeraseme binary",
    ),
    venv_activate: str = typer.Option(
        "",
        "--venv",
        help="Path to virtualenv activate script",
    ),
    dry_run: bool = typer.Option(
        False,
        "--dry-run",
        help="Preview files without writing",
    ),
) -> None:
    result = handle_generate_scheduler(
        platform,
        output_dir,
        tick_hour,
        tick_minute,
        poll_hours,
        project_dir,
        symeraseme_bin,
        venv_activate,
        dry_run,
        ctx.obj["output"],
    )
    render_result(ctx.obj["output"], result)


@schedule_app.command()
def schedule_install(
    ctx: typer.Context,
    platform: str = typer.Option(
        "",
        "--platform",
        help="Target platform: cron, launchd, systemd (auto-detect)",
    ),
    tick_hour: int = typer.Option(
        10,
        "--tick-hour",
        help="Hour for daily tick (0-23)",
    ),
    tick_minute: int = typer.Option(
        0,
        "--tick-minute",
        help="Minute for daily tick (0-59)",
    ),
    yes: bool = typer.Option(
        False,
        "--yes",
        help="Skip confirmation prompt",
    ),
    dry_run: bool = typer.Option(
        False,
        "--dry-run",
        help="Show scheduler configs without installing",
    ),
) -> None:
    result = handle_schedule_install(
        platform,
        tick_hour,
        tick_minute,
        yes,
        dry_run,
        ctx.obj["output"],
    )
    render_result(ctx.obj["output"], result)


@schedule_app.command(name="uninstall")
def schedule_uninstall(
    platform: str = typer.Option(
        "",
        "--platform",
        help="Target platform: cron, launchd, systemd (auto-detect)",
    ),
) -> None:
    result = handle_schedule_uninstall(platform)
    render_result("text", result)


@schedule_app.command()
def schedule_status(
    ctx: typer.Context,
    platform: str = typer.Option(
        "",
        "--platform",
        help="Target platform: cron, launchd, systemd (auto-detect)",
    ),
) -> None:
    result = handle_schedule_status(platform, ctx.obj["output"])
    render_result(ctx.obj["output"], result)


@registry_app.command(name="sync")
def registry_sync_cmd(
    ctx: typer.Context,
    verify_signatures: bool = typer.Option(
        False,
        "--verify-signatures",
        help="(v0.2) Verify maintainer GPG signatures on registry HEAD. "
        "Currently a no-op accepted for forward compatibility.",
    ),
) -> None:
    """Pull the latest broker definitions (git pull --ff-only for source installs)."""
    result = handle_registry_sync(
        verify_signatures=verify_signatures,
        output_format=ctx.obj["output"],
    )
    render_result(ctx.obj["output"], result)


def export_cmd(
    ctx: typer.Context,
    fmt: str = typer.Option(
        "json",
        "--format",
        help="Output format: json or csv (default: json).",
    ),
    output_file: str = typer.Option(
        None,
        "--output-file",
        help="Path to write the export to (default: print to stdout).",
    ),
    campaign: str = typer.Option(
        None,
        "--campaign",
        help="Restrict to one campaign id (default: all).",
    ),
) -> None:
    """Export removal requests with full event history (GDPR record-keeping).

    Use --output-file to write the file directly; otherwise the payload is
    streamed to stdout (raw json/csv when --output text, wrapped when --output json).
    """
    if fmt not in ("json", "csv"):
        render_error(f"Unsupported --format {fmt!r}. Use 'json' or 'csv'.")
    result = handle_export(
        output_file=output_file,
        fmt=fmt,
        campaign_id=campaign,
        output_format=ctx.obj["output"],
    )
    # When writing to a file in text mode we get a one-line summary; print raw.
    # Otherwise raw payload for piping.
    console.print(result, markup=False, soft_wrap=True)


def validate(
    ctx: typer.Context,
    registry_dir: str = typer.Option(
        None,
        "--registry-dir",
        help="Path to registry/brokers (default: bundled registry).",
    ),
) -> None:
    """Validate every broker YAML against the JSON Schema and Pydantic model.

    Exits non-zero if any file fails validation or duplicate ids are found.
    """
    result = handle_validate(registry_dir=registry_dir, output_format=ctx.obj["output"])
    render_result(ctx.obj["output"], result)
