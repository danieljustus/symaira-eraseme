"""Planning & Execution commands."""

from __future__ import annotations

import asyncio
from concurrent.futures import ThreadPoolExecutor
from typing import Any

import typer

from symeraseme.core.reports import get_campaign_status
from symeraseme.services.campaign import (
    handle_execute,
    handle_plan_create,
    handle_plan_show,
)

plan_app = typer.Typer(
    name="plan",
    help="Plan a removal campaign (scan registry, create events)",
    no_args_is_help=True,
)


def _web_form_runner(
    broker_id: str,
    *,
    headed: bool = False,
    screenshot_dir: str = "",
    dry_run: bool = False,
) -> dict[str, Any]:
    """Run a broker's web form, safe with or without a running event loop.

    ``run_web_form_for_broker`` is a coroutine. ``plan execute`` drives the
    SMTP async backend with ``asyncio.run`` inside ``handle_execute``, so this
    runner is invoked while an event loop is already running — a nested
    ``asyncio.run()`` would raise ``RuntimeError``. When no loop is running
    (Himalaya backend, concurrent workers, standalone callers) the coroutine
    is driven directly; otherwise it runs on a fresh loop in a worker thread.
    """
    from symeraseme.services.web_form import run_web_form_for_broker

    coroutine = run_web_form_for_broker(
        broker_id, headed=headed, screenshot_dir=screenshot_dir, dry_run=dry_run
    )
    try:
        asyncio.get_running_loop()
    except RuntimeError:
        # No event loop is running in this thread — drive the coroutine directly.
        return asyncio.run(coroutine)
    # Already inside a running event loop: run the coroutine on its own loop
    # in a worker thread and block for the result.
    with ThreadPoolExecutor(max_workers=1, thread_name_prefix="web-form") as pool:
        return pool.submit(asyncio.run, coroutine).result()


@plan_app.command(help="Scan the registry and create a removal campaign.")
def create(
    ctx: typer.Context,
    campaign_id: str = typer.Option(
        ...,
        "--campaign",
        help="Campaign identifier (e.g. initial-2026-Q2)",
    ),
    jurisdiction: str = typer.Option(
        None,
        "--jurisdiction",
        "-j",
        help="Filter by jurisdiction (e.g. EU, US, DE — country/region codes)",
    ),
    law: str = typer.Option(
        None,
        "--law",
        "-l",
        help="Filter by law (e.g. GDPR, CCPA, LGPD)",
    ),
    priority: str = typer.Option(
        None,
        help="Filter by priority (high, medium, low)",
    ),
    max_brokers: int = typer.Option(
        30,
        "--max",
        help="Maximum brokers to plan",
    ),
    status: str = typer.Option(
        "active",
        "--status",
        help="Broker lifecycle status (active, deprecated, merged, out-of-business)",
    ),
    include_inactive: bool = typer.Option(
        False,
        "--include-inactive",
        help="Include deprecated, merged, and out-of-business brokers",
    ),
) -> None:
    """Scan the broker registry and create a removal campaign.

    By default, --max caps the plan at 30 brokers. A warning is printed
    when more brokers match than the cap allows.

    Examples:
        symeraseme plan create --campaign initial --law GDPR --max 10
        symeraseme plan create --campaign ccpa-batch --jurisdiction US --priority high
        symeraseme plan create --campaign lgpd --law LGPD --jurisdiction BR
    """
    result = handle_plan_create(
        campaign_id,
        jurisdiction,
        law,
        priority,
        max_brokers,
        status=status,
        include_inactive=include_inactive,
    )
    from symeraseme.cli.console import render_result

    render_result(ctx.obj["output"], result)


@plan_app.command(name="show", help="Show removal requests for a campaign.")
def plan_show(
    ctx: typer.Context,
    campaign_id: str = typer.Option(None, "--campaign", help="Filter by campaign"),
    status: str = typer.Option(None, "--status", help="Filter by status"),
) -> None:
    """Show all removal requests for a campaign.

    Examples:
        symeraseme plan show
        symeraseme plan show --campaign initial
        symeraseme plan show --status PLANNED
    """
    result = handle_plan_show(campaign_id, status)
    from symeraseme.cli.console import render_result

    render_result(ctx.obj["output"], result)


@plan_app.command(help="Send removal requests for a campaign.")
def execute(
    ctx: typer.Context,
    campaign_id: str = typer.Option(
        ...,
        "--campaign",
        help="Campaign to execute",
    ),
    account: str = typer.Option(
        None,
        "--account",
        help="Himalaya account name",
    ),
    batch_size: int = typer.Option(
        5,
        "--batch-size",
        help="Number to send",
    ),
    dry_run: bool = typer.Option(False, "--dry-run", help="Simulate only"),
    yes: bool = typer.Option(
        False,
        "--yes",
        help="Skip consent prompt (destructive)",
    ),
    consent_token: str = typer.Option(
        None,
        "--consent",
        help="Pre-issued consent token",
    ),
    consent_file: str = typer.Option(
        None,
        "--consent-file",
        help="Read consent token from a file (supports /dev/stdin for pipe input)",
    ),
    backend: str = typer.Option(
        None,
        "--backend",
        help="Execution backend: smtp (batch) or himalaya (CLI)",
    ),
    concurrent: bool = typer.Option(
        False,
        "--concurrent",
        help="Process requests concurrently (SMTP backend only)",
    ),
    workers: int = typer.Option(
        3,
        "--workers",
        help="Max concurrent workers when --concurrent is set (3-10)",
    ),
) -> None:
    """Send removal requests for a campaign.

    Examples:
        symeraseme execute --campaign initial --batch-size 5 --yes
        symeraseme execute --campaign initial --consent-file /tmp/token
        echo $TOKEN | symeraseme execute --campaign initial --consent-file /dev/stdin
        symeraseme execute --campaign initial --concurrent --workers 5
    """
    from symeraseme.cli.console import render_result, show_spinner

    workers = max(3, min(10, workers))
    with show_spinner("Sending removal requests..."):
        result = handle_execute(
            campaign_id,
            account,
            batch_size,
            dry_run,
            yes,
            consent_token,
            consent_file,
            web_form_runner=_web_form_runner,
            backend=backend,
            concurrent=concurrent,
            workers=workers,
        )
    render_result(ctx.obj["output"], result)


@plan_app.command(help="Process deadlines, reminders, and escalations.")
def tick(
    ctx: typer.Context,
    dry_run: bool = typer.Option(
        False,
        "--dry-run",
        help="Show actions without executing",
    ),
    batch_size: int = typer.Option(
        None,
        "--batch-size",
        help="Limit tick to N requests per run",
    ),
) -> None:
    """Process deadlines, reminders, and escalations for active requests.

    Examples:
        symeraseme tick
        symeraseme tick --dry-run
        symeraseme tick --batch-size 10
    """
    from symeraseme.cli.console import render_result
    from symeraseme.core.db_connection import init_db
    from symeraseme.core.deadlines import apply_tick_actions, run_tick
    from symeraseme.core.result_types import CliResult

    init_db()
    actions = run_tick(dry_run=dry_run, batch_size=batch_size)

    data = {
        "total_actions": len(actions),
        "actions": [a.__dict__ for a in actions],
    }

    if not actions:
        message = "Tick: no actions needed."
    else:
        lines = [f"Tick: {len(actions)} action(s)"]
        for a in actions:
            dry_tag = " (DRY RUN)" if a.dry_run else ""
            lines.append(f"  #{a.request_id} [{a.action_type}] {a.description}{dry_tag}")
        if not dry_run:
            results = apply_tick_actions(actions)
            executed = sum(1 for r in results if r["executed"])
            lines.append(f"Executed {executed}/{len(results)} actions.")
        message = "\n".join(lines)

    render_result(ctx.obj["output"], CliResult(data=data, message=message))


@plan_app.command(help="Show aggregated removal-request status.")
def status(
    ctx: typer.Context,
    campaign: str = typer.Option(
        None,
        "--campaign",
        help="Restrict to one campaign id (default: aggregate across all).",
    ),
) -> None:
    """Show aggregated lifecycle status across removal requests.

    Examples:
        symeraseme status
        symeraseme status --campaign initial
    """
    from symeraseme.cli.console import render_result
    from symeraseme.core.result_types import CliResult

    summary = get_campaign_status(campaign_id=campaign)

    scope = f"campaign={campaign}" if campaign else "all campaigns"
    lines = [
        f"Status ({scope}) as of {summary['as_of']}",
        f"  Total: {summary['totals']['requests']}   "
        f"Resolved: {summary['totals']['resolved']}   "
        f"Open: {summary['totals']['open']}",
    ]
    if summary["by_status"]:
        lines.append("  By status:")
        for status, count in sorted(summary["by_status"].items(), key=lambda kv: -kv[1]):
            lines.append(f"    {status:<22} {count}")
    if summary["by_channel"]:
        lines.append("  By channel:")
        for channel, count in sorted(summary["by_channel"].items()):
            lines.append(f"    {channel:<22} {count}")
    lines.append("  Escalation:")
    lines.append(f"    none           {summary['escalation']['none']}")
    lines.append(f"    reminder sent  {summary['escalation']['reminder']}")
    lines.append(f"    dpa pending    {summary['escalation']['dpa_pending']}")
    lines.append("  Upcoming:")
    lines.append(f"    overdue              {summary['upcoming']['overdue']}")
    lines.append(f"    deadline within 7d   {summary['upcoming']['deadline_due_within_7d']}")
    lines.append(f"    deadline within 30d  {summary['upcoming']['deadline_due_within_30d']}")
    lines.append(f"    tick actions ready   {summary['upcoming']['tick_actions_ready']}")
    message = "\n".join(lines)

    render_result(ctx.obj["output"], CliResult(data=summary, message=message))
