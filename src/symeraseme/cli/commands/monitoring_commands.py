"""Monitoring & Reports commands."""

from __future__ import annotations

import os

import typer

from symeraseme.cli.console import render_result
from symeraseme.services.calendar import handle_calendar
from symeraseme.services.inbox import handle_poll_inbox
from symeraseme.services.reply import (
    handle_classify_reply,
    handle_generate_rebuttal,
)
from symeraseme.services.reporting import (
    handle_generate_dashboard,
    handle_generate_report,
)


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
) -> None:
    password = os.environ.get("IMAP_PASSWORD") or typer.prompt(
        "IMAP password",
        hide_input=True,
    )
    from symeraseme.cli.console import show_spinner

    with show_spinner("Polling inbox..."):
        result = handle_poll_inbox(
            host,
            port,
            username,
            since_days,
            ssl,
            campaign_id,
            password,
            ctx.obj["output"],
        )
    render_result(ctx.obj["output"], result)


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
        help="LLM provider: anthropic, openai, ollama",
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
    from symeraseme.cli.console import show_spinner

    with show_spinner("Classifying reply via LLM..."):
        result = handle_classify_reply(
            request_id,
            provider,
            model,
            save,
            ctx.obj["output"],
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
        help="LLM provider: anthropic, openai, ollama",
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
    from symeraseme.cli.console import show_spinner

    with show_spinner("Generating rebuttal via LLM..."):
        result = handle_generate_rebuttal(
            request_id,
            provider,
            model,
            save,
            ctx.obj["output"],
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
    result = handle_generate_dashboard(
        output,
        auto_open,
        auto_refresh,
        ctx.obj["output"],
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
    result = handle_generate_report(
        campaign_id,
        format,
        output,
        all_campaigns,
        ctx.obj["output"],
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
    result = handle_calendar(
        weeks=weeks,
        campaign_id=campaign,
        output_format=ctx.obj["output"],
    )
    render_result(ctx.obj["output"], result)
