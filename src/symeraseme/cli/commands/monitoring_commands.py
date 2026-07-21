"""Monitoring & Reports commands."""

from __future__ import annotations

import logging
import os
import time
from datetime import UTC, datetime, timedelta
from typing import Any

import typer

from symeraseme.cli.console import render_result
from symeraseme.core.db_connection import get_connection, init_db
from symeraseme.core.result_types import CliResult
from symeraseme.llm.factory import list_available_providers
from symeraseme.services.inbox import handle_poll_inbox
from symeraseme.services.reply import (
    handle_classify_reply,
    handle_generate_rebuttal,
)
from symeraseme.services.reporting import (
    handle_generate_dashboard,
    handle_generate_report,
)

logger = logging.getLogger(__name__)


def poll_inbox(
    ctx: typer.Context,
    host: str = typer.Option("imap.gmail.com", "--host", help="IMAP server"),
    port: int = typer.Option(993, "--port", help="IMAP port"),
    username: str = typer.Option(
        ...,
        "--username",
        prompt=True,
        help="IMAP username",
    ),
    since_days: int = typer.Option(1, "--since", help="Look back N days"),
    ssl: bool = typer.Option(True, "--ssl/--no-ssl"),
    campaign_id: str = typer.Option(
        None,
        "--campaign",
        help="Campaign to match replies against",
    ),
    folders: str = typer.Option(
        None,
        "--folders",
        help="Comma-separated IMAP folders (default: INBOX). Use 'all' to discover all.",
    ),
    retries: int = typer.Option(
        3,
        "--retries",
        help="Number of retries on unhandled exceptions (TimeoutError, OSError)",
    ),
    retry_delay: int = typer.Option(
        5,
        "--retry-delay",
        help="Base delay in seconds between retries",
    ),
    watch: bool = typer.Option(
        False,
        "--watch",
        help="Run in background watch mode — poll periodically and notify on new mail",
    ),
    interval: int = typer.Option(
        900,
        "--interval",
        help="Polling interval in seconds for watch mode (default: 900 = 15 min)",
    ),
) -> None:
    """Poll IMAP inbox for broker replies and classify them.

    Examples:
        symeraseme poll-inbox --username you@gmail.com
        symeraseme poll-inbox --username you@gmail.com --since 7 --folders INBOX,Sent
        symeraseme poll-inbox --username you@gmail.com --watch --interval 600
    """
    password = os.environ.get("IMAP_PASSWORD", "")
    if not password:
        password = typer.prompt("IMAP password", hide_input=True)

    folder_list: list[str] | None = None
    if folders:
        if folders.strip().lower() == "all":
            from symeraseme.services.inbox import handle_list_folders

            result = handle_list_folders(host, port, username, ssl, password)
            if not result.success:
                render_result(ctx.obj["output"], result)
                return
            data = result.data
            folder_list = data.get("folders", ["INBOX"]) if isinstance(data, dict) else ["INBOX"]
        else:
            folder_list = [f.strip() for f in folders.split(",") if f.strip()]

    if watch:
        from symeraseme.services.inbox import handle_watch_inbox

        result = handle_watch_inbox(
            host=host,
            port=port,
            username=username,
            since_days=since_days,
            ssl=ssl,
            campaign_id=campaign_id,
            password=password,
            interval_seconds=interval,
            folders=folder_list,
        )
        render_result(ctx.obj["output"], result)
        return

    from symeraseme.cli.console import show_spinner

    last_error: Exception | None = None
    for attempt in range(retries + 1):
        try:
            with show_spinner(f"Polling inbox... (attempt {attempt + 1}/{retries + 1})"):
                result = handle_poll_inbox(
                    host,
                    port,
                    username,
                    since_days,
                    ssl,
                    campaign_id,
                    password,
                    folders=folder_list,
                )
            render_result(ctx.obj["output"], result)
            return
        except (TimeoutError, OSError) as exc:
            last_error = exc
            if attempt < retries:
                delay = retry_delay * (2**attempt)
                logger.info(
                    "Inbox poll failed (attempt %d/%d): %s. Retrying in %ds...",
                    attempt + 1,
                    retries + 1,
                    exc,
                    delay,
                )
                time.sleep(delay)
            else:
                logger.error("Inbox poll failed after %d attempts: %s", retries + 1, exc)

    if last_error is not None:
        raise typer.Exit(code=1) from last_error


def classify_reply(
    ctx: typer.Context,
    request_id: int = typer.Argument(
        ...,
        help="Request ID to classify the reply for",
    ),
    provider: str = typer.Option(
        None,
        "--provider",
        envvar="SYMERASEME_LLM_PROVIDER",
        help=f"LLM provider: {', '.join(list_available_providers())}",
    ),
    model: str = typer.Option(
        None,
        "--model",
        envvar="SYMERASEME_LLM_MODEL",
        help="Model name (provider-specific)",
    ),
    save: bool = typer.Option(
        True,
        "--save/--no-save",
        help="Save classification result to DB",
    ),
) -> None:
    """Classify a broker reply using an LLM.

    Examples:
        symeraseme classify-reply 42
        symeraseme classify-reply 42 --provider anthropic --model claude-sonnet-4-20250514
        symeraseme classify-reply 42 --no-save
    """
    from symeraseme.cli.console import show_spinner

    with show_spinner("Classifying reply via LLM..."):
        result = handle_classify_reply(
            request_id,
            provider,
            model,
            save,
        )
    render_result(ctx.obj["output"], result)


def generate_rebuttal_cmd(
    ctx: typer.Context,
    request_id: int = typer.Argument(
        ...,
        help="Request ID to generate rebuttal for",
    ),
    provider: str = typer.Option(
        None,
        "--provider",
        envvar="SYMERASEME_LLM_PROVIDER",
        help=f"LLM provider: {', '.join(list_available_providers())}",
    ),
    model: str = typer.Option(
        None,
        "--model",
        envvar="SYMERASEME_LLM_MODEL",
        help="Model name (provider-specific)",
    ),
    save: bool = typer.Option(
        True,
        "--save/--no-save",
        help="Save rebuttal to DB",
    ),
) -> None:
    """Generate a jurisdiction-aware rebuttal for a broker rejection.

    Examples:
        symeraseme generate-rebuttal 42
        symeraseme generate-rebuttal 42 --provider openai --model gpt-4
        symeraseme generate-rebuttal 42 --no-save
    """
    from symeraseme.cli.console import show_spinner

    with show_spinner("Generating rebuttal via LLM..."):
        result = handle_generate_rebuttal(
            request_id,
            provider,
            model,
            save,
        )
    render_result(ctx.obj["output"], result)


def generate_dashboard_cmd(
    ctx: typer.Context,
    output: str = typer.Option(
        "report.html",
        "--output",
        help="Output HTML file",
    ),
    auto_open: bool = typer.Option(
        False,
        "--open",
        help="Open in default browser",
    ),
    auto_refresh: int = typer.Option(
        0,
        "--auto-refresh",
        help="Auto-refresh interval in seconds (0 = none)",
    ),
) -> None:
    """Generate an interactive HTML dashboard for campaign analytics.

    Examples:
        symeraseme generate-dashboard
        symeraseme generate-dashboard --output my-report.html --open
        symeraseme generate-dashboard --auto-refresh 60
    """
    result = handle_generate_dashboard(
        output,
        auto_open,
        auto_refresh,
    )
    render_result(ctx.obj["output"], result)


def generate_report_cmd(
    ctx: typer.Context,
    campaign_id: str = typer.Option(
        None,
        "--campaign-id",
        help="Campaign ID to report on",
    ),
    format: str = typer.Option(
        "html",
        "--format",
        help="Output format: html, json, csv",
    ),
    output: str = typer.Option(
        "",
        "--output",
        help="Output file path (default: auto-generated)",
    ),
    all_campaigns: bool = typer.Option(
        False,
        "--all",
        help="Include all campaigns (not just specified one)",
    ),
) -> None:
    """Generate a campaign report in HTML, JSON, or CSV format.

    Examples:
        symeraseme generate-report
        symeraseme generate-report --campaign-id initial --format json
        symeraseme generate-report --all --output all-campaigns.csv
    """
    result = handle_generate_report(
        campaign_id,
        format,
        output,
        all_campaigns,
    )
    render_result(ctx.obj["output"], result)


def calendar(
    ctx: typer.Context,
    weeks: int = typer.Option(4, "--weeks", help="Horizon in weeks (default: 4)."),
    campaign: str = typer.Option(
        None,
        "--campaign",
        help="Restrict to one campaign id (default: all).",
    ),
) -> None:
    """Show upcoming deadlines and scheduled tick actions over the next N weeks."""
    campaign_id: str | None = campaign
    if weeks < 1:
        weeks = 1

    init_db()
    conn = get_connection()
    now = datetime.now(UTC)
    horizon = now + timedelta(weeks=weeks)
    now_iso = now.isoformat()
    horizon_iso = horizon.isoformat()

    rows = conn.execute(
        """SELECT r.id AS request_id,
                  r.broker_id,
                  r.campaign_id,
                  r.jurisdiction,
                  s.current_status,
                  s.sent_at,
                  s.deadline_at,
                  s.next_action_at,
                  s.reminders_sent,
                  s.escalation_level
           FROM removal_requests r
           JOIN request_state s ON s.request_id = r.id
           WHERE (? IS NULL OR r.campaign_id = ?)
             AND s.resolved_at IS NULL
             AND (
                 (s.deadline_at IS NOT NULL AND s.deadline_at <= ?)
              OR (s.next_action_at IS NOT NULL AND s.next_action_at <= ?)
             )
           ORDER BY COALESCE(s.next_action_at, s.deadline_at) ASC""",
        (campaign_id or None, campaign_id or None, horizon_iso, horizon_iso),
    ).fetchall()

    entries: list[dict] = []
    for row in rows:
        req = dict(row)
        deadline = req.get("deadline_at")
        next_action = req.get("next_action_at")
        marker_at = next_action or deadline
        kind = "next_action" if next_action else "deadline"
        marker_dt = _safe_parse(marker_at) if marker_at else None
        if marker_dt is None:
            continue
        days_from_now = (marker_dt - now).days
        entries.append(
            {
                "request_id": req["request_id"],
                "broker_id": req["broker_id"],
                "campaign_id": req["campaign_id"],
                "jurisdiction": req["jurisdiction"],
                "current_status": req["current_status"],
                "marker": kind,
                "marker_at": marker_at,
                "days_from_now": days_from_now,
                "overdue": days_from_now < 0,
                "deadline_at": deadline,
                "next_action_at": next_action,
                "escalation_level": req["escalation_level"],
                "reminders_sent": req["reminders_sent"],
            }
        )

    buckets: dict[str, list[dict]] = {}
    for e in entries:
        marker_dt = _safe_parse(e["marker_at"])
        if marker_dt is None:
            continue
        iso = marker_dt.isocalendar()
        week_key = f"{iso.year}-W{iso.week:02d}"
        buckets.setdefault(week_key, []).append(e)

    data: dict[str, Any] = {
        "schema_version": 1,
        "as_of": now_iso,
        "horizon_weeks": weeks,
        "horizon_until": horizon_iso,
        "scope": {"campaign_id": campaign_id or "all"},
        "totals": {
            "entries": len(entries),
            "overdue": sum(1 for e in entries if e["overdue"]),
            "weeks_with_actions": len(buckets),
        },
        "weeks": [{"week": week, "entries": items} for week, items in sorted(buckets.items())],
    }

    scope = f"campaign={campaign_id}" if campaign_id else "all campaigns"
    lines = [
        f"Calendar ({scope}) — next {weeks} weeks (until {horizon_iso[:10]})",
        f"  Total upcoming entries: {len(entries)}  Overdue: {data['totals']['overdue']}",
    ]
    if not entries:
        lines.append("")
        lines.append("Nothing scheduled in the horizon.")
        message = "\n".join(lines)
    else:
        for bucket in data["weeks"]:
            lines.append("")
            lines.append(f"Week {bucket['week']} ({len(bucket['entries'])} entries):")
            for e in bucket["entries"]:
                flag = " OVERDUE" if e["overdue"] else ""
                marker_short = (e["marker_at"] or "")[:16]
                lines.append(
                    f"  #{e['request_id']:<5} {e['broker_id']:<24} "
                    f"{e['current_status']:<20} "
                    f"{e['marker']:<11} @ {marker_short} "
                    f"({e['days_from_now']:+d}d){flag}"
                )
        message = "\n".join(lines)

    render_result(ctx.obj["output"], CliResult(data=data, message=message))


def _safe_parse(value: str | None) -> datetime | None:
    if not value:
        return None
    try:
        dt = datetime.fromisoformat(value.replace("Z", "+00:00"))
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=UTC)
        return dt
    except ValueError:
        return None
