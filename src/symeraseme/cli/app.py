"""Typer CLI application with rich-formatted output."""

from __future__ import annotations

import os

import typer

from symeraseme.cli.console import (
    OutputFormat,
    console,
    print_panel,
    print_success,
    print_table,
    render_error,
    render_result,
)
from symeraseme.registry.sync import handle_registry_sync
from symeraseme.services.account import (
    handle_account_add,
    handle_account_list,
    handle_account_remove,
)
from symeraseme.services.auto_confirm import handle_auto_confirm
from symeraseme.services.broker import handle_brokers_list, handle_brokers_show
from symeraseme.services.calendar import handle_calendar
from symeraseme.services.campaign import (
    handle_execute,
    handle_plan_create,
    handle_plan_show,
)
from symeraseme.services.captcha import handle_solve_captcha
from symeraseme.services.consent import handle_grant
from symeraseme.services.db import handle_db_init
from symeraseme.services.doctor import handle_doctor
from symeraseme.services.export import handle_export
from symeraseme.services.inbox import handle_poll_inbox
from symeraseme.services.manual_task import (
    handle_manual_tasks_complete,
    handle_manual_tasks_list,
    handle_manual_tasks_show,
)
from symeraseme.services.profile import (
    handle_init_profile,
    handle_render_template,
    handle_show_profile,
    handle_version,
)
from symeraseme.services.reply import (
    handle_classify_reply,
    handle_generate_rebuttal,
)
from symeraseme.services.reporting import (
    handle_generate_dashboard,
    handle_generate_report,
)
from symeraseme.services.request import (
    handle_events_show,
    handle_requests_list,
)
from symeraseme.services.scheduler import (
    handle_generate_scheduler,
    handle_schedule_install,
    handle_schedule_status,
    handle_schedule_uninstall,
)
from symeraseme.services.status import handle_status
from symeraseme.services.tick import handle_tick
from symeraseme.services.validate import handle_validate
from symeraseme.services.web_form import handle_run_web_form

app = typer.Typer(
    name="symeraseme",
    help="Automated data broker removal tool",
    no_args_is_help=True,
    epilog=(
        "Quick Start:\n"
        "  1. symeraseme init-profile                       Create your identity profile\n"
        "  2. symeraseme brokers list --jurisdiction EU      Browse the broker registry\n"
        "  3. symeraseme plan create --campaign initial \\\n"
        "     --jurisdiction EU --max 10                     Plan a removal campaign\n"
        "  4. symeraseme execute --campaign initial \\\n"
        "     --batch-size 5 --yes                           Send opt-out requests\n"
        "  5. symeraseme tick                                Process deadlines & reminders\n"
        "  6. symeraseme status                              Check campaign progress\n"
        "\n"
        "Run 'symeraseme <command> --help' for detailed options."
    ),
)
accounts_app = typer.Typer(
    name="accounts",
    help="Manage email accounts (OAuth2 setup, list, remove)",
    no_args_is_help=True,
)
app.add_typer(accounts_app, rich_help_panel="Account & Profile")
plan_app = typer.Typer(
    name="plan",
    help="Plan a removal campaign (scan registry, create events)",
    no_args_is_help=True,
)
app.add_typer(plan_app, rich_help_panel="Planning & Execution")
events_app = typer.Typer(
    name="events",
    help="View removal request event history",
    no_args_is_help=True,
)
app.add_typer(events_app, rich_help_panel="Inspection & Diagnostics")
requests_app = typer.Typer(
    name="requests",
    help="List and manage removal requests",
    no_args_is_help=True,
)
app.add_typer(requests_app, rich_help_panel="Inspection & Diagnostics")
manual_tasks_app = typer.Typer(
    name="manual-tasks",
    help="List and manage manual fallback tasks for web forms",
    no_args_is_help=True,
)
app.add_typer(manual_tasks_app, rich_help_panel="Web-form Automation")
schedule_app = typer.Typer(
    name="schedule",
    help="Manage scheduler configuration (install, uninstall, status)",
    no_args_is_help=True,
)
app.add_typer(schedule_app, rich_help_panel="Maintenance")
brokers_app = typer.Typer(
    name="brokers",
    help="Discover brokers in the registry (list, show)",
    no_args_is_help=True,
)
app.add_typer(brokers_app, rich_help_panel="Inspection & Diagnostics")
registry_app = typer.Typer(
    name="registry",
    help="Manage the broker registry (sync)",
    no_args_is_help=True,
)
app.add_typer(registry_app, rich_help_panel="Maintenance")

# ── commands ─────────────────────────────────────────────────────────────


@app.callback()
def main(ctx: typer.Context, output: OutputFormat = OutputFormat.text) -> None:
    ctx.ensure_object(dict)
    ctx.obj["output"] = output


@app.command(rich_help_panel="Inspection & Diagnostics")
def version() -> None:
    result = handle_version()
    console.print(result, markup=False, soft_wrap=True)


@app.command(rich_help_panel="Inspection & Diagnostics")
def doctor(ctx: typer.Context) -> None:
    """Run environment checks and report status."""
    result = handle_doctor(ctx.obj["output"])
    render_result(ctx.obj["output"], result)


@app.command(rich_help_panel="Account & Profile")
def init_profile(
    full_name: str = typer.Option(..., prompt="Full name"),
    email: str = typer.Option(..., prompt="Email address"),
) -> None:
    result = handle_init_profile(full_name, email)
    print_success(result)


@app.command(rich_help_panel="Account & Profile")
def show_profile() -> None:
    try:
        result = handle_show_profile()
    except typer.Exit:
        raise
    info = "\n".join(line.strip() for line in result.split("\n") if line.strip())
    print_panel("Profile", info)


@app.command(rich_help_panel="Account & Profile")
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
    render_result(ctx.obj["output"], result)


@plan_app.command(name="show")
def plan_show(
    ctx: typer.Context,
    campaign_id: str = typer.Option(None, "--campaign", help="Filter by campaign"),
    status: str = typer.Option(None, "--status", help="Filter by status"),
) -> None:
    result = handle_plan_show(campaign_id, status, ctx.obj["output"])
    render_result(ctx.obj["output"], result)


@app.command(rich_help_panel="Planning & Execution")
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
    render_result(ctx.obj["output"], result)


@app.command(rich_help_panel="Account & Profile")
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
) -> None:
    result = handle_requests_list(
        campaign_id,
        status,
        broker_id,
        ctx.obj["output"],
    )
    render_result(ctx.obj["output"], result)


@app.command(name="poll-inbox", rich_help_panel="Monitoring & Reports")
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
    render_result(ctx.obj["output"], result)


@app.command(rich_help_panel="Planning & Execution")
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
    render_result(ctx.obj["output"], result)


@app.command(name="classify-reply", rich_help_panel="Monitoring & Reports")
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
    result = handle_classify_reply(
        request_id,
        provider,
        model,
        save,
        ctx.obj["output"],
    )
    render_result(ctx.obj["output"], result)


@app.command(name="run-web-form", rich_help_panel="Web-form Automation")
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
    render_result(ctx.obj["output"], result)


@app.command(name="auto-confirm", rich_help_panel="Web-form Automation")
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
    render_result(ctx.obj["output"], result)


@app.command(name="generate-rebuttal", rich_help_panel="Monitoring & Reports")
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
    result = handle_generate_rebuttal(
        request_id,
        provider,
        model,
        save,
        ctx.obj["output"],
    )
    render_result(ctx.obj["output"], result)


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
    render_result(ctx.obj["output"], result)


@manual_tasks_app.command(name="show")
def manual_tasks_show(
    ctx: typer.Context,
    task_id: int = typer.Argument(..., help="Task ID to show"),
) -> None:
    result = handle_manual_tasks_show(task_id, ctx.obj["output"])
    render_result(ctx.obj["output"], result)


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
    render_result(ctx.obj["output"], result)


@app.command(name="solve-captcha", rich_help_panel="Web-form Automation")
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
    render_result(ctx.obj["output"], result)


@app.command(name="generate-scheduler", rich_help_panel="Maintenance")
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


@app.command(name="generate-dashboard", rich_help_panel="Monitoring & Reports")
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


@app.command(name="generate-report", rich_help_panel="Monitoring & Reports")
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


@app.command(rich_help_panel="Maintenance")
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
    render_result(ctx.obj["output"], result)


@brokers_app.command(name="show")
def brokers_show_cmd(
    ctx: typer.Context,
    broker_id: str = typer.Argument(help="Broker id (e.g. acxiom-eu, spokeo)"),
) -> None:
    """Show full details of one broker by id."""
    result = handle_brokers_show(broker_id, output_format=ctx.obj["output"])
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


@app.command(rich_help_panel="Planning & Execution")
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
    render_result(ctx.obj["output"], result)


@app.command(name="export", rich_help_panel="Maintenance")
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


@app.command(rich_help_panel="Monitoring & Reports")
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


@app.command(rich_help_panel="Maintenance")
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


if __name__ == "__main__":
    app()
