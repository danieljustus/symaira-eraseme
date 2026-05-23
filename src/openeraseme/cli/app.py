"""Typer CLI application with rich-formatted output."""

from __future__ import annotations

import os
from enum import StrEnum

import typer

from openeraseme.cli.console import console, print_error, print_panel, print_success, print_table
from openeraseme.cli.types import CliResult
from openeraseme.registry.sync import handle_registry_sync
from openeraseme.services.account import (
    handle_account_add,
    handle_account_list,
    handle_account_remove,
)
from openeraseme.services.auto_confirm import handle_auto_confirm
from openeraseme.services.broker import handle_brokers_list, handle_brokers_show
from openeraseme.services.calendar import handle_calendar
from openeraseme.services.campaign import (
    handle_execute,
    handle_plan_create,
    handle_plan_show,
)
from openeraseme.services.captcha import handle_solve_captcha
from openeraseme.services.consent import handle_grant
from openeraseme.services.db import handle_db_init
from openeraseme.services.doctor import handle_doctor
from openeraseme.services.export import handle_export
from openeraseme.services.inbox import handle_poll_inbox
from openeraseme.services.manual_task import (
    handle_manual_tasks_complete,
    handle_manual_tasks_list,
    handle_manual_tasks_show,
)
from openeraseme.services.profile import (
    handle_init_profile,
    handle_render_template,
    handle_show_profile,
    handle_version,
)
from openeraseme.services.reply import (
    handle_classify_reply,
    handle_generate_rebuttal,
)
from openeraseme.services.reporting import (
    handle_generate_dashboard,
    handle_generate_report,
)
from openeraseme.services.request import (
    handle_events_show,
    handle_requests_list,
)
from openeraseme.services.scheduler import (
    handle_generate_scheduler,
    handle_schedule_install,
    handle_schedule_status,
    handle_schedule_uninstall,
)
from openeraseme.services.status import handle_status
from openeraseme.services.tick import handle_tick
from openeraseme.services.validate import handle_validate
from openeraseme.services.web_form import handle_run_web_form


class OutputFormat(StrEnum):
    text = "text"
    json = "json"


app = typer.Typer(
    name="openeraseme",
    help="Automated data broker removal tool",
    no_args_is_help=True,
)
accounts_app = typer.Typer(
    name="accounts",
    help="Manage email accounts (OAuth2 setup, list, remove)",
    no_args_is_help=True,
)
app.add_typer(accounts_app)
plan_app = typer.Typer(
    name="plan",
    help="Plan a removal campaign (scan registry, create events)",
    no_args_is_help=True,
)
app.add_typer(plan_app)
events_app = typer.Typer(
    name="events",
    help="View removal request event history",
    no_args_is_help=True,
)
app.add_typer(events_app)
requests_app = typer.Typer(
    name="requests",
    help="List and manage removal requests",
    no_args_is_help=True,
)
app.add_typer(requests_app)
manual_tasks_app = typer.Typer(
    name="manual-tasks",
    help="List and manage manual fallback tasks for web forms",
    no_args_is_help=True,
)
app.add_typer(manual_tasks_app)
schedule_app = typer.Typer(
    name="schedule",
    help="Manage scheduler configuration (install, uninstall, status)",
    no_args_is_help=True,
)
app.add_typer(schedule_app)
brokers_app = typer.Typer(
    name="brokers",
    help="Discover brokers in the registry (list, show)",
    no_args_is_help=True,
)
app.add_typer(brokers_app)
registry_app = typer.Typer(
    name="registry",
    help="Manage the broker registry (sync)",
    no_args_is_help=True,
)
app.add_typer(registry_app)


# ── helpers ──────────────────────────────────────────────────────────────


def _render(
    output_format: str,
    result: str | CliResult,
    result_obj: CliResult | None = None,
) -> None:
    """Print the result of a command handler, formatted appropriately.

    For JSON output the raw string is printed as-is (soft_wrap to avoid
    rich inserting line breaks into the serialized data).
    For text output the result is wrapped in a rich Panel when the content
    spans multiple lines or carries an error.

    Raises typer.Exit(1) when the result indicates failure so every command
    returns a non-zero exit code uniformly.
    """
    if isinstance(result, CliResult):
        result_obj = result
        result = result.message

    if output_format == "json":
        if result_obj is not None:
            import json as _json

            console.print(
                _json.dumps(result_obj.data, indent=2, default=str),
                markup=False,
                soft_wrap=True,
            )
        else:
            console.print(result, markup=False, soft_wrap=True)
    elif result_obj is not None and not result_obj.success:
        print_error(result_obj.message)
    elif "\n" not in result.strip():
        console.print(result, markup=False, soft_wrap=True)
    else:
        print_panel("Output", result.strip())

    if result_obj is not None and not result_obj.success:
        raise typer.Exit(1)


def _render_error(message: str) -> None:
    """Print an error message and exit."""
    print_error(message)
    raise typer.Exit(1)


# ── commands ─────────────────────────────────────────────────────────────


@app.callback()
def main(ctx: typer.Context, output: OutputFormat = OutputFormat.text) -> None:
    ctx.ensure_object(dict)
    ctx.obj["output"] = output


@app.command()
def version() -> None:
    result = handle_version()
    console.print(result, markup=False, soft_wrap=True)


@app.command()
def doctor(ctx: typer.Context) -> None:
    """Run environment checks and report status."""
    result = handle_doctor(ctx.obj["output"])
    _render(ctx.obj["output"], result)


@app.command()
def init_profile(
    full_name: str = typer.Option(..., prompt="Full name"),
    email: str = typer.Option(..., prompt="Email address"),
) -> None:
    result = handle_init_profile(full_name, email)
    print_success(result)


@app.command()
def show_profile() -> None:
    try:
        result = handle_show_profile()
    except typer.Exit:
        raise
    info = "\n".join(line.strip() for line in result.split("\n") if line.strip())
    print_panel("Profile", info)


@app.command()
def render_template(
    template: str = typer.Argument(
        help="Template name (e.g. gdpr-art17.de.md.j2)",
    ),
    broker_name: str = typer.Option("", help="Name of the data broker"),
    broker_website: str = typer.Option("", help="Broker website URL"),
) -> None:
    result = handle_render_template(template, broker_name, broker_website)
    console.print(result, markup=False, soft_wrap=True)


@accounts_app.command()
def add(
    provider: str = typer.Argument(help="Provider: gmail or outlook"),
    email: str = typer.Option(..., prompt=True, help="Email address"),
    client_id: str = typer.Option(..., prompt=True, help="OAuth2 client ID"),
    client_secret: str = typer.Option(
        ...,
        prompt=True,
        hide_input=True,
        help="OAuth2 client secret",
    ),
) -> None:
    result = handle_account_add(provider, email, client_id, client_secret)
    print_success(result)


@accounts_app.command(name="list")
def list_cmd() -> None:
    result = handle_account_list()
    if result.startswith("No"):
        console.print(result, markup=False, soft_wrap=True)
        return
    rows = []
    for line in result.strip().split("\n"):
        line = line.strip()
        if line:
            rows.append(line.split(None, 1))
    if rows:
        print_table("Accounts", ["Email", "Provider"], rows)
    else:
        console.print(result, markup=False, soft_wrap=True)


@accounts_app.command()
def remove(email: str = typer.Argument(help="Email address to remove")) -> None:
    result = handle_account_remove(email)
    console.print(result, markup=False, soft_wrap=True)


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
    result = handle_plan_create(
        campaign_id,
        jurisdiction,
        priority,
        max_brokers,
        ctx.obj["output"],
    )
    _render(ctx.obj["output"], result)


@plan_app.command(name="show")
def plan_show(
    ctx: typer.Context,
    campaign_id: str = typer.Option(None, "--campaign", help="Filter by campaign"),
    status: str = typer.Option(None, "--status", help="Filter by status"),
) -> None:
    result = handle_plan_show(campaign_id, status, ctx.obj["output"])
    _render(ctx.obj["output"], result)


@app.command()
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
    result = handle_execute(
        campaign_id,
        account,
        batch_size,
        dry_run,
        yes,
        consent_token,
        ctx.obj["output"],
    )
    _render(ctx.obj["output"], result)


@app.command()
def grant(
    ctx: typer.Context,
    command: str = typer.Argument(
        "execute",
        help="Command to authorize (e.g. execute)",
    ),
    ttl: int = typer.Option(86400, "--ttl", help="Token TTL in seconds"),
    revoke: str = typer.Option(
        None,
        "--revoke",
        help="Revoke a consent token",
    ),
    revoke_all: bool = typer.Option(
        False,
        "--revoke-all",
        help="Revoke all active tokens",
    ),
    list_tokens: bool = typer.Option(
        False,
        "--list",
        help="List active tokens",
    ),
    dry_run: bool = typer.Option(
        False,
        "--dry-run",
        help="Show token without creating it",
    ),
) -> None:
    result = handle_grant(
        command,
        ttl,
        revoke,
        revoke_all,
        list_tokens,
        dry_run,
        ctx.obj["output"],
    )
    _render(ctx.obj["output"], result)


@events_app.command(name="show")
def events_show(
    ctx: typer.Context,
    request_id: int = typer.Argument(..., help="Request ID"),
) -> None:
    result = handle_events_show(request_id, ctx.obj["output"])
    _render(ctx.obj["output"], result)


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
) -> None:
    result = handle_requests_list(
        campaign_id,
        status,
        broker_id,
        ctx.obj["output"],
    )
    _render(ctx.obj["output"], result)


@app.command(name="poll-inbox")
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
    _render(ctx.obj["output"], result)


@app.command()
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
    result = handle_tick(dry_run, batch_size, ctx.obj["output"])
    _render(ctx.obj["output"], result)


@app.command(name="classify-reply")
def classify_reply(
    ctx: typer.Context,
    request_id: int = typer.Argument(
        ...,
        help="Request ID to classify the reply for",
    ),
    provider: str = typer.Option(
        None,
        "--provider",
        envvar="OPENERASEME_LLM_PROVIDER",
        help="LLM provider: anthropic, openai, ollama",
    ),
    model: str = typer.Option(
        None,
        "--model",
        envvar="OPENERASEME_LLM_MODEL",
        help="Model name (provider-specific)",
    ),
    save: bool = typer.Option(
        True,
        "--save/--no-save",
        help="Save classification result to DB",
    ),
) -> None:
    result = handle_classify_reply(
        request_id,
        provider,
        model,
        save,
        ctx.obj["output"],
    )
    _render(ctx.obj["output"], result)


@app.command(name="run-web-form")
def run_web_form(
    ctx: typer.Context,
    broker_id: str = typer.Argument(
        ...,
        help="Broker ID from registry",
    ),
    headed: bool = typer.Option(
        False,
        "--headed",
        help="Show browser window",
    ),
    screenshot_dir: str = typer.Option(
        "",
        "--screenshots",
        help="Directory for screenshots",
    ),
    dry_run: bool = typer.Option(
        False,
        "--dry-run",
        help="Show form steps without executing",
    ),
) -> None:
    result = handle_run_web_form(
        broker_id,
        headed,
        screenshot_dir,
        dry_run,
        ctx.obj["output"],
    )
    _render(ctx.obj["output"], result)


@app.command(name="auto-confirm")
def auto_confirm_cmd(
    ctx: typer.Context,
    request_id: int = typer.Argument(
        ...,
        help="Request ID to auto-confirm",
    ),
    headed: bool = typer.Option(
        False,
        "--headed",
        help="Show browser window",
    ),
    screenshot_dir: str = typer.Option(
        "",
        "--screenshots",
        help="Directory for screenshots",
    ),
    dry_run: bool = typer.Option(
        False,
        "--dry-run",
        help="Simulate without clicking",
    ),
) -> None:
    result = handle_auto_confirm(
        request_id,
        headed,
        screenshot_dir,
        dry_run,
        ctx.obj["output"],
    )
    _render(ctx.obj["output"], result)


@app.command(name="generate-rebuttal")
def generate_rebuttal_cmd(
    ctx: typer.Context,
    request_id: int = typer.Argument(
        ...,
        help="Request ID to generate rebuttal for",
    ),
    provider: str = typer.Option(
        None,
        "--provider",
        envvar="OPENERASEME_LLM_PROVIDER",
        help="LLM provider: anthropic, openai, ollama",
    ),
    model: str = typer.Option(
        None,
        "--model",
        envvar="OPENERASEME_LLM_MODEL",
        help="Model name (provider-specific)",
    ),
    save: bool = typer.Option(
        True,
        "--save/--no-save",
        help="Save rebuttal to DB",
    ),
) -> None:
    result = handle_generate_rebuttal(
        request_id,
        provider,
        model,
        save,
        ctx.obj["output"],
    )
    _render(ctx.obj["output"], result)


@manual_tasks_app.command(name="list")
def manual_tasks_list(
    ctx: typer.Context,
    status: str = typer.Option(
        None,
        "--status",
        help="Filter by status (pending, completed, cancelled)",
    ),
    request_id: int = typer.Option(
        None,
        "--request-id",
        help="Filter by request ID",
    ),
) -> None:
    result = handle_manual_tasks_list(
        status,
        request_id,
        ctx.obj["output"],
    )
    _render(ctx.obj["output"], result)


@manual_tasks_app.command(name="show")
def manual_tasks_show(
    ctx: typer.Context,
    task_id: int = typer.Argument(..., help="Task ID to show"),
) -> None:
    result = handle_manual_tasks_show(task_id, ctx.obj["output"])
    _render(ctx.obj["output"], result)


@manual_tasks_app.command(name="complete")
def manual_tasks_complete(
    ctx: typer.Context,
    task_id: int = typer.Argument(
        ...,
        help="Task ID to mark as completed",
    ),
    notes: str = typer.Option(
        "",
        "--notes",
        help="Optional completion notes",
    ),
) -> None:
    result = handle_manual_tasks_complete(
        task_id,
        notes,
        ctx.obj["output"],
    )
    _render(ctx.obj["output"], result)


@app.command(name="solve-captcha")
def solve_captcha_cmd(
    ctx: typer.Context,
    provider: str = typer.Option(
        "capsolver",
        "--provider",
        help="Captcha provider: capsolver or twocaptcha",
    ),
    api_key: str = typer.Option(
        None,
        "--api-key",
        envvar="CAPSOLVER_API_KEY",
        help="API key (or set CAPSOLVER_API_KEY)",
    ),
    site_key: str = typer.Option(
        ...,
        "--site-key",
        prompt=True,
        help="reCAPTCHA site key",
    ),
    page_url: str = typer.Option(
        ...,
        "--page-url",
        prompt=True,
        help="Page URL where captcha appears",
    ),
    dry_run: bool = typer.Option(
        False,
        "--dry-run",
        help="Show captcha parameters without solving",
    ),
) -> None:
    result = handle_solve_captcha(
        provider,
        api_key,
        site_key,
        page_url,
        dry_run,
        ctx.obj["output"],
    )
    _render(ctx.obj["output"], result)


@app.command(name="generate-scheduler")
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
    openeraseme_bin: str = typer.Option(
        "",
        "--bin",
        help="Path to openeraseme binary",
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
        openeraseme_bin,
        venv_activate,
        dry_run,
        ctx.obj["output"],
    )
    _render(ctx.obj["output"], result)


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
    _render(ctx.obj["output"], result)


@schedule_app.command(name="uninstall")
def schedule_uninstall(
    platform: str = typer.Option(
        "",
        "--platform",
        help="Target platform: cron, launchd, systemd (auto-detect)",
    ),
) -> None:
    result = handle_schedule_uninstall(platform)
    _render("text", result)


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
    _render(ctx.obj["output"], result)


@app.command(name="generate-dashboard")
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
    _render(ctx.obj["output"], result)


@app.command(name="generate-report")
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
    _render(ctx.obj["output"], result)


@app.command()
def db_init() -> None:
    result = handle_db_init()
    print_success(result)


@brokers_app.command(name="list")
def brokers_list_cmd(
    ctx: typer.Context,
    jurisdiction: str = typer.Option(None, help="Filter by jurisdiction (e.g. DE, US, EU)"),
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
        priority=priority,
        category=category,
        include_disabled=include_disabled,
        output_format=ctx.obj["output"],
    )
    _render(ctx.obj["output"], result)


@brokers_app.command(name="show")
def brokers_show_cmd(
    ctx: typer.Context,
    broker_id: str = typer.Argument(help="Broker id (e.g. acxiom-eu, spokeo)"),
) -> None:
    """Show full details of one broker by id."""
    result = handle_brokers_show(broker_id, output_format=ctx.obj["output"])
    _render(ctx.obj["output"], result)


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
    _render(ctx.obj["output"], result)


@app.command()
def status(
    ctx: typer.Context,
    campaign: str = typer.Option(
        None,
        "--campaign",
        help="Restrict to one campaign id (default: aggregate across all).",
    ),
) -> None:
    """Show aggregated lifecycle status across removal requests."""
    result = handle_status(campaign_id=campaign, output_format=ctx.obj["output"])
    _render(ctx.obj["output"], result)


@app.command(name="export")
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
        _render_error(f"Unsupported --format {fmt!r}. Use 'json' or 'csv'.")
    result = handle_export(
        output_file=output_file,
        fmt=fmt,
        campaign_id=campaign,
        output_format=ctx.obj["output"],
    )
    # When writing to a file in text mode we get a one-line summary; print raw.
    # Otherwise raw payload for piping.
    console.print(result, markup=False, soft_wrap=True)


@app.command()
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
    _render(ctx.obj["output"], result)


@app.command()
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
    _render(ctx.obj["output"], result)


if __name__ == "__main__":
    app()
