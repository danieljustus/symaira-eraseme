"""Planning & Execution commands."""

from __future__ import annotations

import typer

from symeraseme.services.campaign import (
    handle_execute,
    handle_plan_create,
    handle_plan_show,
)
from symeraseme.services.status import handle_status
from symeraseme.services.tick import handle_tick

plan_app = typer.Typer(
    name="plan",
    help="Plan a removal campaign (scan registry, create events)",
    no_args_is_help=True,
)


@plan_app.command()
def create(
    ctx: typer.Context,
    campaign_id: str = typer.Option(
        ...,
        "--campaign",
        help="Campaign identifier (e.g. initial-2026-Q2)",
    ),
    jurisdiction: str = typer.Option(
        None,
        help="Filter by jurisdiction (e.g. DE, US)",
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
) -> None:
    """Scan the broker registry and create a removal campaign.

    Examples:
        symeraseme plan create --campaign initial --jurisdiction GDPR --max 10
        symeraseme plan create --campaign ccpa-batch --jurisdiction US --priority high
    """
    result = handle_plan_create(
        campaign_id,
        jurisdiction,
        priority,
        max_brokers,
        ctx.obj["output"],
    )
    from symeraseme.cli.console import render_result
    render_result(ctx.obj["output"], result)


@plan_app.command(name="show")
def plan_show(
    ctx: typer.Context,
    campaign_id: str = typer.Option(None, "--campaign", help="Filter by campaign"),
    status: str = typer.Option(None, "--status", help="Filter by status"),
) -> None:
    result = handle_plan_show(campaign_id, status, ctx.obj["output"])
    from symeraseme.cli.console import render_result
    render_result(ctx.obj["output"], result)


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
) -> None:
    """Send removal requests for a campaign.

    Examples:
        symeraseme execute --campaign initial --batch-size 5 --yes
        symeraseme execute --campaign initial --account gmail --dry-run
    """
    result = handle_execute(
        campaign_id,
        account,
        batch_size,
        dry_run,
        yes,
        consent_token,
        ctx.obj["output"],
    )
    from symeraseme.cli.console import render_result
    render_result(ctx.obj["output"], result)


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
    result = handle_tick(dry_run, batch_size, ctx.obj["output"])
    from symeraseme.cli.console import render_result
    render_result(ctx.obj["output"], result)


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
    result = handle_status(campaign_id=campaign, output_format=ctx.obj["output"])
    from symeraseme.cli.console import render_result
    render_result(ctx.obj["output"], result)
