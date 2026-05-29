"""Inspection & Diagnostics commands."""

from __future__ import annotations

import typer

from symeraseme.cli.console import console, render_result
from symeraseme.services.broker import handle_brokers_list, handle_brokers_show
from symeraseme.services.doctor import handle_doctor
from symeraseme.services.profile import handle_version
from symeraseme.services.request import (
    handle_events_show,
    handle_requests_list,
)

events_app = typer.Typer(
    name="events",
    help="View removal request event history",
    no_args_is_help=True,
)
requests_app = typer.Typer(
    name="requests",
    help="List and manage removal requests",
    no_args_is_help=True,
)
brokers_app = typer.Typer(
    name="brokers",
    help="Discover brokers in the registry (list, show)",
    no_args_is_help=True,
)


def version() -> None:
    result = handle_version()
    console.print(result, markup=False, soft_wrap=True)


def doctor(ctx: typer.Context) -> None:
    """Run environment checks and report status."""
    result = handle_doctor(ctx.obj["output"])
    render_result(ctx.obj["output"], result)


@events_app.command(name="show")
def events_show(
    ctx: typer.Context,
    request_id: int = typer.Argument(..., help="Request ID"),
) -> None:
    result = handle_events_show(request_id, ctx.obj["output"])
    render_result(ctx.obj["output"], result)


@requests_app.command(name="list")
def requests_list(
    ctx: typer.Context,
    campaign_id: str = typer.Option(
        None,
        "--campaign",
        help="Filter by campaign",
    ),
    status: str = typer.Option(None, "--status", help="Filter by status"),
    broker_id: str = typer.Option(
        None,
        "--broker",
        help="Filter by broker ID",
    ),
    page: int = typer.Option(
        None,
        "--page",
        help="Page number (1-based, requires --page-size, default 250)",
    ),
    page_size: int = typer.Option(
        250,
        "--page-size",
        help="Number of results per page (default: 250)",
    ),
) -> None:
    result = handle_requests_list(
        campaign_id,
        status,
        broker_id,
        page,
        page_size,
        ctx.obj["output"],
    )
    render_result(ctx.obj["output"], result)


@brokers_app.command(name="list")
def brokers_list_cmd(
    ctx: typer.Context,
    jurisdiction: str = typer.Option(None, help="Filter by jurisdiction (e.g. DE, US, EU)"),
    law: str = typer.Option(
        None,
        help="Filter by law (e.g. GDPR, CCPA, CPRA, LGPD, PIPEDA)",
    ),
    priority: str = typer.Option(None, help="Filter by priority: high, medium, low"),
    category: str = typer.Option(
        None,
        help="Filter by category: people-search, marketing, credit, analytics, "
        "background-check, social-media, other",
    ),
    include_disabled: bool = typer.Option(
        False,
        "--include-disabled",
        help="Include brokers marked disabled (default: skip them).",
    ),
) -> None:
    """List brokers in the registry, optionally filtered."""
    result = handle_brokers_list(
        jurisdiction=jurisdiction,
        law=law,
        priority=priority,
        category=category,
        include_disabled=include_disabled,
        output_format=ctx.obj["output"],
    )
    render_result(ctx.obj["output"], result)


@brokers_app.command(name="show")
def brokers_show_cmd(
    ctx: typer.Context,
    broker_id: str = typer.Argument(help="Broker id (e.g. acxiom-eu, spokeo)"),
) -> None:
    """Show full details of one broker by id."""
    result = handle_brokers_show(broker_id, output_format=ctx.obj["output"])
    render_result(ctx.obj["output"], result)
