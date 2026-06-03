"""Maintenance commands."""

from __future__ import annotations

import csv
import io
import json
from pathlib import Path
from typing import Any

import typer

from symeraseme.cli.console import print_success, render_error, render_result
from symeraseme.core.db import init_db
from symeraseme.registry.sync import handle_registry_sync
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
    )
    render_result(ctx.obj["output"], result)


@schedule_app.command(name="uninstall")
def schedule_uninstall(
    ctx: typer.Context,
    platform: str = typer.Option(
        "",
        "--platform",
        help="Target platform: cron, launchd, systemd (auto-detect)",
    ),
) -> None:
    result = handle_schedule_uninstall(platform)
    render_result(ctx.obj["output"], result)


@schedule_app.command()
def schedule_status(
    ctx: typer.Context,
    platform: str = typer.Option(
        "",
        "--platform",
        help="Target platform: cron, launchd, systemd (auto-detect)",
    ),
) -> None:
    result = handle_schedule_status(platform)
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

    init_db()
    campaign_id: str | None = campaign

    # Collect export data
    reqs, flat_events = _collect_export_data(campaign_id)

    # Serialize
    if fmt == "json":
        serialized = _format_export_json(reqs, campaign_id)
    else:
        serialized = _format_export_csv(reqs, flat_events)

    if output_file:
        _write_export_file(output_file, serialized)

    from symeraseme.core.result_types import CliResult

    summary: dict[str, Any] = {
        "schema_version": 1,
        "format": fmt,
        "scope": {"campaign_id": campaign_id or "all"},
        "totals": {
            "requests": len(reqs),
            "events": sum(len(r.get("events", [])) for r in reqs),
        },
        "output_file": str(Path(output_file).expanduser().resolve()) if output_file else None,
    }

    if output_file:
        message = (
            f"Exported {summary['totals']['requests']} request(s) "
            f"and {summary['totals']['events']} event(s) "
            f"to {summary['output_file']} ({fmt})."
        )
        data = summary
    else:
        message = serialized
        data = {**summary, "payload": serialized}

    render_result(ctx.obj["output"], CliResult(data=data, message=message))


def _collect_export_data(
    campaign_id: str | None,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """Query removal requests and events via the repository layer.

    Returns (requests, flat_events) where each request has its events attached.
    """
    from symeraseme.core.repositories import get_events_for_requests, list_removal_requests

    request_rows = list_removal_requests(campaign_id=campaign_id)

    requests: list[dict[str, Any]] = []
    flat_events: list[dict[str, Any]] = []

    req_ids = [r["id"] for r in request_rows]
    if req_ids:
        events_by_rid_raw = get_events_for_requests(req_ids)
        events_by_rid: dict[int, list[dict]] = {}
        for rid, evs in events_by_rid_raw.items():
            transformed = []
            for ev in evs:
                evd = dict(ev)
                evd["payload"] = evd.pop("payload_json", {})
                transformed.append(evd)
                flat_events.append({"request_id": rid, **evd})
            events_by_rid[rid] = transformed
    else:
        events_by_rid = {}

    for r in request_rows:
        req = dict(r)
        req["events"] = events_by_rid.get(req["id"], [])
        requests.append(req)

    return requests, flat_events


def _format_export_json(
    requests: list[dict[str, Any]],
    campaign_id: str | None,
) -> str:
    """Serialize the export payload as JSON."""
    payload = {
        "schema_version": 1,
        "scope": {"campaign_id": campaign_id or "all"},
        "totals": {
            "requests": len(requests),
            "events": sum(len(r["events"]) for r in requests),
        },
        "requests": requests,
    }
    return json.dumps(payload, indent=2, default=str)


def _format_export_csv(
    requests: list[dict[str, Any]],
    flat_events: list[dict[str, Any]],
) -> str:
    """Flatten the event log into CSV rows (one row per event)."""
    buf = io.StringIO()
    writer = csv.writer(buf)
    writer.writerow(
        [
            "request_id",
            "broker_id",
            "campaign_id",
            "jurisdiction",
            "current_status",
            "event_id",
            "occurred_at",
            "event_type",
            "source",
            "payload_json",
        ]
    )
    req_by_id = {r["id"]: r for r in requests}
    if not flat_events:
        for req in requests:
            writer.writerow(
                [
                    req["id"],
                    req["broker_id"],
                    req["campaign_id"],
                    req["jurisdiction"],
                    req.get("current_status", ""),
                    "",
                    "",
                    "",
                    "",
                    "",
                ]
            )
    else:
        for e in flat_events:
            req = req_by_id.get(e["request_id"], {})
            writer.writerow(
                [
                    e["request_id"],
                    req.get("broker_id", ""),
                    req.get("campaign_id", ""),
                    req.get("jurisdiction", ""),
                    req.get("current_status", ""),
                    e["id"],
                    e["occurred_at"],
                    e["event_type"],
                    e["source"],
                    json.dumps(e.get("payload", {}), default=str),
                ]
            )
    return buf.getvalue()


def _write_export_file(output_file: str, serialized: str) -> None:
    """Write the serialized export payload to *output_file*."""
    path = Path(output_file).expanduser().resolve()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(serialized, encoding="utf-8")


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
    result = handle_validate(registry_dir=registry_dir)
    render_result(ctx.obj["output"], result)
